//! The provisionable "cache": the results of the `provision()` and
//! `deprovision()` exports, and the webhook `provision()` calls.
//!
//! SDK features: `ResourceProvisionRequest` (`fields`, `trace_context` with
//! `trace_headers()`); `ResourceProvisionResult` (`state`, `sidecars`,
//! `init_containers`, `deployment_annotations`, `kubernetes_resources`,
//! `status`, `retry_after_secs`, events with `resource_id`, `retryable` and
//! `hint`); `ProvisioningStatus` (non-exhaustive); `ResourceDeprovisionRequest`;
//! `ResourceDeprovisionResult` (`message`, `delete_kubernetes_resources`,
//! events); `KubernetesObjectRef`; `parse_key_value_field`.

use std::collections::BTreeMap;

use deckwatch_plugin_sdk::{
    parse_key_value_field, EnvVarSpec, KubernetesObjectRef, ProvisioningStatus,
    ResourceDeprovisionRequest, ResourceDeprovisionResult, ResourceProvisionRequest,
    ResourceProvisionResult, SidecarSpec, VolumeMountSpec, VolumeSpec,
};
use extism_pdk::HttpRequest;
use serde_json::{json, Value};

use crate::apply::object_name;
use crate::config::{Config, Size};
use crate::metadata::{cache_fields, cache_state, codes, CACHE_RESOURCE_ID};

/// The cache sidecar's port, on localhost in each pod.
pub const CACHE_PORT: u16 = 6379;

/// Annotation `provision()` asks deckwatch to stamp on the application's
/// Deployments (not its CronJobs).
pub const CACHE_ANNOTATION: &str = "example.deckwatch.io/cache";

/// What the backing system reports about the cache. A plugin backed by a
/// cloud API would ask that API; this one runs the cache as a sidecar, so
/// `exports` always passes `Ready`. The tests cover all three.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    Creating,
    Ready { host: String, port: u16 },
    Failed(String),
}

/// The result of `provision()` for the cache.
///
/// deckwatch calls `provision()` when an operator adds the resource, when
/// they refresh it, and every 30 s while its stored `state` looks pending.
/// Each call gets the `fields` from the form.
pub fn provision(req: &ResourceProvisionRequest, observed: &Observed) -> ResourceProvisionResult {
    let mut result = ResourceProvisionResult::default();
    if req.resource_id != CACHE_RESOURCE_ID {
        result.status = ProvisioningStatus::Failed;
        let event = result.error(
            codes::UNKNOWN_RESOURCE,
            format!("unknown resource {:?}", req.resource_id),
        );
        event.resource_id = Some(req.resource_id.clone());
        return result;
    }

    let size = req
        .fields
        .get(cache_fields::SIZE)
        .and_then(|s| Size::parse(s))
        .unwrap_or(Size::Small);
    let tags = match req.fields.get(cache_fields::TAGS) {
        None => BTreeMap::new(),
        Some(raw) => match parse_key_value_field(raw) {
            Ok(tags) => tags,
            Err(err) => {
                result.status = ProvisioningStatus::Failed;
                result
                    .state
                    .insert(cache_state::STATUS.into(), "failed".into());
                let event = result.error(codes::TAGS_INVALID, format!("TAGS: {err}"));
                event.resource_id = Some(req.resource_id.clone());
                event.hint = Some(r#"Enter tags as key/value pairs, e.g. {"team":"shop"}"#.into());
                return result;
            }
        },
    };

    match observed {
        Observed::Creating => {
            // deckwatch re-polls a resource only while its stored state looks
            // pending: a `*_STATUS` key with an in-progress value such as
            // "creating" (deckwatch#245). `status: Pending` alone doesn't
            // trigger a re-poll.
            result
                .state
                .insert(cache_state::STATUS.into(), "creating".into());
            result.status = ProvisioningStatus::Pending;
            // Skip re-polls for 30 s (honoured by the background re-poll only).
            result.retry_after_secs = Some(30);
            result
                .info(codes::CACHE_CREATING, "the cache is being created")
                .resource_id = Some(req.resource_id.clone());
        }
        Observed::Ready { host, port } => {
            // Each key becomes an env var in the first container of every
            // Deployment and CronJob of the application.
            result.state.insert(cache_state::HOST.into(), host.clone());
            result
                .state
                .insert(cache_state::PORT.into(), port.to_string());
            result
                .state
                .insert(cache_state::STATUS.into(), "available".into());
            result.sidecars.push(cache_sidecar(size, *port));
            // deckwatch (0.7.0 and v0.6.7) doesn't store init containers from
            // provision() (deckwatch#249), so this has no effect there.
            result.init_containers.push(cache_init());
            result
                .deployment_annotations
                .insert(CACHE_ANNOTATION.into(), "enabled".into());
            result
                .kubernetes_resources
                .push(service_account(req, &tags));
            result.kubernetes_resources.push(warmup_job(req, &tags));
            result.status = ProvisioningStatus::Available;
            let job = warmup_job_name(&req.application_name);
            let event = result.info(
                codes::CACHE_READY,
                format!("the cache is ready at {host}:{port}; warm-up Job {job} requested"),
            );
            event.resource_id = Some(req.resource_id.clone());
            event.object = Some(KubernetesObjectRef::job(job));
        }
        Observed::Failed(reason) => {
            result
                .state
                .insert(cache_state::STATUS.into(), "failed".into());
            result.status = ProvisioningStatus::Failed;
            let event = result.error(
                codes::CACHE_FAILED,
                format!("creating the cache failed: {reason}"),
            );
            event.resource_id = Some(req.resource_id.clone());
            event.retryable = true;
            event.hint = Some("Refresh the resource to try again".into());
        }
    }
    result
}

/// The result of `deprovision()` for the cache. deckwatch deletes the
/// resource's record afterwards, whatever this returns.
pub fn deprovision(req: &ResourceDeprovisionRequest) -> ResourceDeprovisionResult {
    let mut result = ResourceDeprovisionResult::default();
    if req.resource_id != CACHE_RESOURCE_ID {
        result
            .error(
                codes::UNKNOWN_RESOURCE,
                format!("unknown resource {:?}", req.resource_id),
            )
            .resource_id = Some(req.resource_id.clone());
        return result;
    }
    // deckwatch 0.7.0 returns this in the DELETE response and shows it in the
    // UI (v0.6.7 only logs it). When this was the application's last resource,
    // 0.7.0's field sweep removes the cache sidecar; v0.6.7 leaves it.
    result.message = "deckwatch stops injecting the cache sidecar".into();
    // Objects provision() created, for deckwatch to delete: only v1
    // ServiceAccounts and batch/v1 Jobs in the application's namespace.
    // deckwatch 0.7.0 deletes the Jobs first, then each ServiceAccount that
    // no workload still uses (never the namespace's default one). It doesn't
    // check ownership, so list only what this plugin created. v0.6.7 ignores
    // the field.
    result.delete_kubernetes_resources = vec![
        KubernetesObjectRef::job(warmup_job_name(&req.application_name)),
        KubernetesObjectRef::service_account(service_account_name(&req.application_name)),
    ];
    // ResourceDeprovisionResult has no `warnings`; warn() appends the text
    // to `message` for hosts that don't read `events`.
    result.warn(
        codes::CACHE_DATA_DISCARDED,
        "the cache's data is discarded with the sidecar",
    );
    result
}

/// The webhook call `provision()` makes when `WEBHOOK_URL` is set: the
/// request and its JSON body. `exports` sends it with `extism_pdk::http`,
/// which only reaches hosts in the plugin's `allowed_hosts`; any other host
/// aborts the whole `provision()` call.
pub fn webhook_request(
    req: &ResourceProvisionRequest,
    result: &ResourceProvisionResult,
    cfg: &Config,
    now: u64,
) -> Option<(HttpRequest, Vec<u8>)> {
    let url = cfg.webhook_url.as_ref()?;
    let mut request = HttpRequest::new(url.clone())
        .with_method("POST")
        .with_header("Content-Type", "application/json");
    if let Some(token) = &cfg.api_token {
        request = request.with_header("Authorization", format!("Bearer {token}"));
    }
    // Forward deckwatch's trace so the receiver's spans join it.
    // trace_headers() is empty unless trace_context is a valid W3C
    // traceparent. deckwatch 0.7.0 sends one while it exports OTLP traces.
    for (name, value) in req.trace_headers() {
        request = request.with_header(name, value);
    }
    let body = json!({
        "event": "cache.provisioned",
        "application": req.application_name,
        "namespace": req.namespace,
        "resource_id": req.resource_id,
        "status": status_name(&result.status),
        "sent_at": now,
    });
    Some((request, body.to_string().into_bytes()))
}

/// `ProvisioningStatus` is `#[non_exhaustive]`, so a `match` needs a `_` arm.
pub fn status_name(status: &ProvisioningStatus) -> &'static str {
    match status {
        ProvisioningStatus::Pending => "pending",
        ProvisioningStatus::Available => "available",
        ProvisioningStatus::Failed => "failed",
        _ => "unknown",
    }
}

pub fn service_account_name(application: &str) -> String {
    object_name(application, "-cache")
}

pub fn warmup_job_name(application: &str) -> String {
    object_name(application, "-cache-warmup")
}

fn cache_sidecar(size: Size, port: u16) -> SidecarSpec {
    let [cpu_request, cpu_limit, memory_request, memory_limit] = size.resources();
    SidecarSpec {
        name: "example-cache".into(),
        image: "redis:7.4-alpine".into(),
        port: Some(i32::from(port)),
        cpu_request: Some(cpu_request.into()),
        cpu_limit: Some(cpu_limit.into()),
        memory_request: Some(memory_request.into()),
        memory_limit: Some(memory_limit.into()),
        volumes: vec![VolumeSpec {
            name: "example-cache-data".into(),
            volume_type: "emptyDir".into(),
            source_name: None,
            size_limit: Some("256Mi".into()),
        }],
        volume_mounts: vec![VolumeMountSpec {
            name: "example-cache-data".into(),
            mount_path: "/data".into(),
            read_only: false,
            sub_path: None,
        }],
        ..Default::default()
    }
}

fn cache_init() -> SidecarSpec {
    SidecarSpec {
        name: "example-cache-init".into(),
        image: "busybox:1.37".into(),
        env: vec![EnvVarSpec::value("EXAMPLE_CACHE_DIR", "/data")],
        ..Default::default()
    }
}

fn labels(tags: &BTreeMap<String, String>) -> Value {
    let mut labels = serde_json::Map::new();
    for (key, value) in tags {
        labels.insert(key.clone(), Value::String(value.clone()));
    }
    labels.insert(
        "app.kubernetes.io/managed-by".into(),
        "deckwatch-plugin-example".into(),
    );
    Value::Object(labels)
}

fn service_account(req: &ResourceProvisionRequest, tags: &BTreeMap<String, String>) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "ServiceAccount",
        "metadata": {
            "name": service_account_name(&req.application_name),
            "namespace": req.namespace,
            "labels": labels(tags),
        },
    })
}

/// A Job that finishes at once (busybox's default command exits). It shows a
/// plugin-created object that `deprovision()` asks deckwatch to delete.
fn warmup_job(req: &ResourceProvisionRequest, tags: &BTreeMap<String, String>) -> Value {
    json!({
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {
            "name": warmup_job_name(&req.application_name),
            "namespace": req.namespace,
            "labels": labels(tags),
        },
        "spec": {
            "backoffLimit": 0,
            "template": {
                "spec": {
                    "serviceAccountName": service_account_name(&req.application_name),
                    "restartPolicy": "Never",
                    "containers": [{ "name": "warmup", "image": "busybox:1.37" }],
                },
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::keys;
    use deckwatch_plugin_sdk::{telemetry, EventSeverity};
    use std::collections::HashMap;

    const TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

    fn request(fields: &[(&str, &str)]) -> ResourceProvisionRequest {
        ResourceProvisionRequest {
            application_name: "shop".into(),
            namespace: "shop".into(),
            resource_id: CACHE_RESOURCE_ID.into(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Default::default()
        }
    }

    fn ready() -> Observed {
        Observed::Ready {
            host: "127.0.0.1".into(),
            port: CACHE_PORT,
        }
    }

    #[test]
    fn ready_cache_returns_state_sidecar_objects_and_available() {
        let result = provision(
            &request(&[
                (cache_fields::SIZE, "large"),
                (cache_fields::TAGS, r#"{"team":"shop"}"#),
            ]),
            &ready(),
        );
        assert!(matches!(result.status, ProvisioningStatus::Available));
        assert_eq!(result.state[cache_state::HOST], "127.0.0.1");
        assert_eq!(result.state[cache_state::PORT], "6379");
        assert_eq!(result.state[cache_state::STATUS], "available");
        let cache = &result.sidecars[0];
        assert_eq!(cache.port, Some(6379));
        assert_eq!(cache.cpu_limit.as_deref(), Some("500m"));
        assert_eq!(result.init_containers.len(), 1);
        assert_eq!(result.deployment_annotations[CACHE_ANNOTATION], "enabled");
        let kinds: Vec<_> = result
            .kubernetes_resources
            .iter()
            .map(|o| o["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["ServiceAccount", "Job"]);
        for object in &result.kubernetes_resources {
            assert_eq!(object["metadata"]["namespace"], "shop");
            assert_eq!(object["metadata"]["labels"]["team"], "shop");
        }
        assert_eq!(result.retry_after_secs, None);
        let event = &result.events[0];
        assert_eq!(event.code, codes::CACHE_READY);
        assert_eq!(event.resource_id.as_deref(), Some(CACHE_RESOURCE_ID));
        assert_eq!(
            event.object,
            Some(KubernetesObjectRef::job("shop-example-cache-warmup"))
        );
    }

    #[test]
    fn creating_cache_is_pending_with_a_status_key_and_retry_delay() {
        let result = provision(&request(&[]), &Observed::Creating);
        assert!(matches!(result.status, ProvisioningStatus::Pending));
        assert_eq!(result.state[cache_state::STATUS], "creating");
        assert_eq!(result.retry_after_secs, Some(30));
        assert!(result.sidecars.is_empty());
        assert_eq!(result.events[0].severity, EventSeverity::Info);
    }

    #[test]
    fn failed_cache_is_a_retryable_error() {
        let result = provision(&request(&[]), &Observed::Failed("quota exceeded".into()));
        assert!(matches!(result.status, ProvisioningStatus::Failed));
        let event = &result.events[0];
        assert_eq!(event.code, codes::CACHE_FAILED);
        assert!(event.retryable);
        assert!(event.hint.is_some());
        // Older hosts log `errors`; the helper copied the event there.
        assert_eq!(result.errors, [event.legacy_text()]);
    }

    #[test]
    fn invalid_tags_fail_before_creating_anything() {
        let result = provision(&request(&[(cache_fields::TAGS, "team=shop")]), &ready());
        assert!(matches!(result.status, ProvisioningStatus::Failed));
        assert!(result.sidecars.is_empty() && result.kubernetes_resources.is_empty());
        assert_eq!(result.events[0].code, codes::TAGS_INVALID);
    }

    #[test]
    fn unknown_resource_is_an_error() {
        let mut req = request(&[]);
        req.resource_id = "queue".into();
        let result = provision(&req, &ready());
        assert!(matches!(result.status, ProvisioningStatus::Failed));
        assert_eq!(result.events[0].resource_id.as_deref(), Some("queue"));

        let gone = deprovision(&ResourceDeprovisionRequest {
            resource_id: "queue".into(),
            ..Default::default()
        });
        assert_eq!(gone.events[0].code, codes::UNKNOWN_RESOURCE);
        assert!(gone.delete_kubernetes_resources.is_empty());
    }

    #[test]
    fn deprovision_asks_to_delete_what_provision_created() {
        let result = deprovision(&ResourceDeprovisionRequest {
            application_name: "shop".into(),
            namespace: "shop".into(),
            resource_id: CACHE_RESOURCE_ID.into(),
            state: HashMap::from([(cache_state::STATUS.into(), "available".into())]),
            ..Default::default()
        });
        assert_eq!(
            result.delete_kubernetes_resources,
            [
                KubernetesObjectRef::job("shop-example-cache-warmup"),
                KubernetesObjectRef::service_account("shop-example-cache"),
            ]
        );
        // warn() appended its legacy text to `message`.
        assert!(result.message.starts_with("deckwatch stops injecting"));
        assert!(result.message.contains(codes::CACHE_DATA_DISCARDED));
        assert_eq!(result.events[0].severity, EventSeverity::Warning);
    }

    #[test]
    fn deprovision_deletes_the_objects_provision_named() {
        let created = provision(&request(&[]), &ready());
        let names: Vec<_> = created
            .kubernetes_resources
            .iter()
            .map(|o| o["metadata"]["name"].as_str().unwrap().to_string())
            .collect();
        let deleted: Vec<_> = deprovision(&ResourceDeprovisionRequest {
            application_name: "shop".into(),
            resource_id: CACHE_RESOURCE_ID.into(),
            ..Default::default()
        })
        .delete_kubernetes_resources
        .into_iter()
        .map(|o| o.name)
        .collect();
        for name in names {
            assert!(deleted.contains(&name), "{name}");
        }
    }

    #[test]
    fn webhook_needs_a_url() {
        let req = request(&[]);
        let result = provision(&req, &ready());
        assert!(webhook_request(&req, &result, &Config::defaults(), 0).is_none());
    }

    #[test]
    fn webhook_forwards_the_trace_and_token() {
        let mut req = request(&[]);
        req.trace_context = Some(TRACEPARENT.into());
        let result = provision(&req, &ready());
        let cfg = Config::from_lookup(|k| match k {
            keys::WEBHOOK_URL => Some("https://hooks.example.com/deckwatch".into()),
            keys::API_TOKEN => Some("s3cret".into()),
            _ => None,
        });
        let (http, body) = webhook_request(&req, &result, &cfg, 1_700_000_000).unwrap();
        assert_eq!(http.url, "https://hooks.example.com/deckwatch");
        assert_eq!(http.method.as_deref(), Some("POST"));
        assert_eq!(http.headers[telemetry::TRACEPARENT_HEADER], TRACEPARENT);
        assert_eq!(http.headers["Authorization"], "Bearer s3cret");
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["status"], "available");
        assert_eq!(body["sent_at"], 1_700_000_000);
    }

    #[test]
    fn webhook_drops_an_invalid_trace_context() {
        let mut req = request(&[]);
        req.trace_context = Some("not-a-traceparent".into());
        let result = provision(&req, &ready());
        let cfg = Config::from_lookup(|k| {
            (k == keys::WEBHOOK_URL).then(|| "https://hooks.example.com".to_string())
        });
        let (http, _) = webhook_request(&req, &result, &cfg, 0).unwrap();
        assert!(!http.headers.contains_key(telemetry::TRACEPARENT_HEADER));
        assert!(!http.headers.contains_key("Authorization"));
    }

    #[test]
    fn every_status_has_a_name() {
        for (status, name) in [
            (ProvisioningStatus::Pending, "pending"),
            (ProvisioningStatus::Available, "available"),
            (ProvisioningStatus::Failed, "failed"),
        ] {
            assert_eq!(status_name(&status), name);
        }
    }
}
