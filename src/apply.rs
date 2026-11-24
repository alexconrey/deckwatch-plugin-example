//! Per-Deployment injection: the result of the `apply()` export.
//!
//! SDK features: `PluginContext` (`annotations`, `plugin_outputs`,
//! `provisioned_resources`); `PluginResult` (`env_vars`, `sidecars`,
//! `init_containers`, `kubernetes_resources`, `outputs`,
//! `wants_service_account`, `deployment_annotations`, `deployment_labels`,
//! and events through `info`/`warn`); the three `EnvVarSpec` constructors;
//! `SidecarSpec` with `VolumeSpec` (`emptyDir`, `configMap`, `secret`),
//! `VolumeMountSpec` (`read_only`, `sub_path`), `primary_volume_mounts` and
//! split resource requests and limits; `KubernetesObjectRef`.
//!
//! deckwatch calls `apply()` on Deployment create and update, and from its
//! reconcile loop for applications with a provisioned resource. It uses
//! different fields on each path; see the SDK README, "Host behaviour".

use deckwatch_plugin_sdk::{
    EnvVarSpec, KubernetesObjectRef, PluginContext, PluginResult, SidecarSpec, VolumeMountSpec,
    VolumeSpec, WantsServiceAccount,
};
use serde_json::{json, Value};

use crate::config::Config;
use crate::metadata::{cache_state, codes, CACHE_RESOURCE_ID, NAME};

/// Deployments opt in with this annotation on `metadata.annotations`.
/// deckwatch runs every loaded plugin on every Deployment, so the opt-in is
/// up to the plugin.
pub const OPT_IN: &str = "example.deckwatch.io/enabled";

/// Annotation and label this plugin stamps on opted-in Deployments.
pub const GREETING_ANNOTATION: &str = "example.deckwatch.io/greeting";
pub const MANAGED_LABEL: &str = "example.deckwatch.io/managed";

/// The registered name of deckwatch-plugin-aws, whose outputs `apply` reads.
pub const AWS_PLUGIN: &str = "aws";

/// The `emptyDir` the agent, its init container and the app container share.
pub const SHARED_VOLUME: &str = "example-shared";
pub const SHARED_DIR: &str = "/var/run/example";

/// Key in the plugin's ConfigMap, read by the `EXAMPLE_MOTD` env var.
pub const MOTD_KEY: &str = "motd";

/// The result of `apply()` for one Deployment.
pub fn apply(ctx: &PluginContext, cfg: &Config) -> PluginResult {
    let mut result = PluginResult::default();
    if ctx.annotations.get(OPT_IN).map(String::as_str) != Some("true") {
        // Nothing to do: an empty result changes nothing.
        return result;
    }
    let name = object_name(&ctx.deployment_name, "");

    for problem in &cfg.problems {
        let event = result.warn(
            codes::CONFIG_INVALID,
            format!("{}: {}", problem.key, problem.message),
        );
        event.hint = Some(format!("Fix {} in the plugin's settings", problem.key));
    }

    // Env vars for the first container. deckwatch skips any variable the
    // container already has, so the operator's own values win.
    result
        .env_vars
        .push(EnvVarSpec::value("EXAMPLE_GREETING", &cfg.greeting));
    // From the ConfigMap below. deckwatch creates it after the Deployment, so
    // the first pod can wait briefly until it exists.
    result.env_vars.push(EnvVarSpec::config_map_key_ref(
        "EXAMPLE_MOTD",
        &name,
        MOTD_KEY,
    ));
    // From a Secret the operator manages. deckwatch emits the reference with
    // `optional: false`, so only reference a Secret that exists.
    if let Some(secret) = &cfg.token_secret {
        result
            .env_vars
            .push(EnvVarSpec::secret_key_ref("EXAMPLE_TOKEN", secret, "token"));
    }
    for (key, value) in &cfg.extra_env {
        result.env_vars.push(EnvVarSpec::value(key, value));
    }

    if cfg.sidecar_enabled {
        result.sidecars.push(agent_sidecar(cfg, &name));
        // Added only with the sidecar: it mounts the sidecar's volume.
        result.init_containers.push(init_container());
    }

    // Applied with server-side apply after the Deployment is saved. Set
    // metadata.namespace: deckwatch doesn't fill it in.
    result
        .kubernetes_resources
        .push(config_map(ctx, cfg, &name));

    // deckwatch creates or patches this ServiceAccount and binds the pod to
    // it, unless the pod already names a ServiceAccount other than "default".
    // The IRSA role comes from settings, or from deckwatch-plugin-aws's output
    // when that plugin runs first (OPTIONAL_DEPENDS_ON).
    let role_arn = cfg.role_arn.clone().or_else(|| {
        ctx.plugin_outputs
            .get(AWS_PLUGIN)
            .and_then(|outputs| outputs.get("role_arn"))
            .cloned()
    });
    result.wants_service_account = Some(WantsServiceAccount {
        name: name.clone(),
        irsa_role_arn: role_arn.unwrap_or_default(),
    });

    // As of deckwatch fc03ec2 these two only take effect on the reconcile
    // loop (deckwatch#244), not on Deployment create and update.
    result
        .deployment_annotations
        .insert(GREETING_ANNOTATION.into(), cfg.greeting.clone());
    result
        .deployment_labels
        .insert(MANAGED_LABEL.into(), "true".into());

    // For plugins that run after this one: ctx.plugin_outputs["example"].
    result.outputs.insert("configmap".into(), name.clone());
    result
        .outputs
        .insert("greeting".into(), cfg.greeting.clone());

    // State of this plugin's provisioned cache, keyed "<plugin>/<resource>".
    // deckwatch fills this only on the reconcile loop, and already injects
    // the state keys as env vars, so here it only feeds an output.
    let cache_key = format!("{NAME}/{CACHE_RESOURCE_ID}");
    if let Some(cache) = ctx.provisioned_resources.get(&cache_key) {
        if let (Some(host), Some(port)) =
            (cache.get(cache_state::HOST), cache.get(cache_state::PORT))
        {
            result
                .outputs
                .insert("cache_address".into(), format!("{host}:{port}"));
        }
    }

    let summary = format!(
        "{} env var(s), {} sidecar(s), {} init container(s) for {}/{}",
        result.env_vars.len(),
        result.sidecars.len(),
        result.init_containers.len(),
        ctx.namespace,
        ctx.deployment_name,
    );
    result.info(codes::APPLIED, summary).object =
        Some(KubernetesObjectRef::new("v1", "ConfigMap", &name));
    result
}

/// A Kubernetes object name from `base` and `suffix`, cut to 63 characters
/// (the limit for names that also become label values, like a Job's).
pub fn object_name(base: &str, suffix: &str) -> String {
    let mut name: String = format!("{base}-example{suffix}").chars().take(63).collect();
    while name.ends_with('-') {
        name.pop();
    }
    name
}

fn agent_sidecar(cfg: &Config, config_map: &str) -> SidecarSpec {
    let [cpu_request, cpu_limit, memory_request, memory_limit] = cfg.sidecar_size.resources();
    let mut volumes = vec![
        VolumeSpec {
            name: SHARED_VOLUME.into(),
            volume_type: "emptyDir".into(),
            source_name: None,
            size_limit: Some("64Mi".into()),
        },
        VolumeSpec {
            name: "example-config".into(),
            volume_type: "configMap".into(),
            source_name: Some(config_map.into()),
            size_limit: None,
        },
    ];
    let mut volume_mounts = vec![
        VolumeMountSpec {
            name: SHARED_VOLUME.into(),
            mount_path: SHARED_DIR.into(),
            read_only: false,
            sub_path: None,
        },
        // One key of the ConfigMap, as a single file.
        VolumeMountSpec {
            name: "example-config".into(),
            mount_path: "/etc/example/motd".into(),
            read_only: true,
            sub_path: Some(MOTD_KEY.into()),
        },
    ];
    if let Some(secret) = &cfg.token_secret {
        volumes.push(VolumeSpec {
            name: "example-token".into(),
            volume_type: "secret".into(),
            source_name: Some(secret.clone()),
            size_limit: None,
        });
        volume_mounts.push(VolumeMountSpec {
            name: "example-token".into(),
            mount_path: "/etc/example/token".into(),
            read_only: true,
            sub_path: None,
        });
    }
    SidecarSpec {
        name: "example-agent".into(),
        image: cfg.sidecar_image.clone(),
        env: vec![EnvVarSpec::value("EXAMPLE_SHARED_DIR", SHARED_DIR)],
        // No `port`: the default image (pause) doesn't listen. Set one for an
        // agent that does; it becomes a containerPort with no name.
        cpu_request: Some(cpu_request.into()),
        cpu_limit: Some(cpu_limit.into()),
        memory_request: Some(memory_request.into()),
        memory_limit: Some(memory_limit.into()),
        volumes,
        volume_mounts,
        // The app container sees the shared directory read-only.
        primary_volume_mounts: vec![VolumeMountSpec {
            name: SHARED_VOLUME.into(),
            mount_path: SHARED_DIR.into(),
            read_only: true,
            sub_path: None,
        }],
        // `cpu` and `memory` are deprecated; the split fields above replace them.
        ..Default::default()
    }
}

/// busybox's default command (`sh`) exits at once without a terminal, so this
/// init container completes immediately. As of deckwatch fc03ec2 init
/// containers are appended, so they run after the workload's own
/// (deckwatch#249). deckwatch doesn't add an init container's own `volumes`,
/// so this one mounts the volume the agent sidecar declares.
fn init_container() -> SidecarSpec {
    SidecarSpec {
        name: "example-init".into(),
        image: "busybox:1.37".into(),
        env: vec![EnvVarSpec::value("EXAMPLE_SHARED_DIR", SHARED_DIR)],
        cpu_request: Some("10m".into()),
        cpu_limit: Some("50m".into()),
        memory_request: Some("8Mi".into()),
        memory_limit: Some("16Mi".into()),
        volume_mounts: vec![VolumeMountSpec {
            name: SHARED_VOLUME.into(),
            mount_path: SHARED_DIR.into(),
            read_only: false,
            sub_path: None,
        }],
        ..Default::default()
    }
}

fn config_map(ctx: &PluginContext, cfg: &Config, name: &str) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "ConfigMap",
        "metadata": {
            "name": name,
            "namespace": ctx.namespace,
            "labels": { "app.kubernetes.io/managed-by": "deckwatch-plugin-example" },
        },
        "data": { MOTD_KEY: format!("{}, {}!", cfg.greeting, ctx.deployment_name) },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::keys;
    use deckwatch_plugin_sdk::EventSeverity;
    use std::collections::HashMap;

    fn ctx(annotations: &[(&str, &str)]) -> PluginContext {
        PluginContext {
            namespace: "shop".into(),
            deployment_name: "api".into(),
            annotations: annotations
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            // New SDK fields won't break this: every SDK struct derives Default.
            ..Default::default()
        }
    }

    fn opted_in() -> PluginContext {
        ctx(&[(OPT_IN, "true")])
    }

    fn cfg(pairs: &[(&str, &str)]) -> Config {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|k| map.get(k).cloned())
    }

    fn env<'a>(result: &'a PluginResult, name: &str) -> Option<&'a EnvVarSpec> {
        result.env_vars.iter().find(|e| e.name == name)
    }

    #[test]
    fn does_nothing_without_the_opt_in() {
        for annotations in [&[][..], &[(OPT_IN, "false")][..], &[(OPT_IN, "yes")][..]] {
            let result = apply(&ctx(annotations), &Config::defaults());
            let json = serde_json::to_value(&result).unwrap();
            let empty = serde_json::to_value(PluginResult::default()).unwrap();
            assert_eq!(json, empty, "{annotations:?}");
        }
    }

    #[test]
    fn injects_env_vars_from_all_three_sources() {
        let result = apply(
            &opted_in(),
            &cfg(&[
                (keys::TOKEN_SECRET, "api-token"),
                (keys::EXTRA_ENV, r#"{"TEAM":"platform"}"#),
            ]),
        );
        assert_eq!(env(&result, "EXAMPLE_GREETING").unwrap().value, "Hello");
        let motd = env(&result, "EXAMPLE_MOTD")
            .unwrap()
            .value_from
            .clone()
            .unwrap();
        let cm = motd.config_map_key_ref.unwrap();
        assert_eq!(
            (cm.name.as_str(), cm.key.as_str()),
            ("api-example", MOTD_KEY)
        );
        let token = env(&result, "EXAMPLE_TOKEN")
            .unwrap()
            .value_from
            .clone()
            .unwrap();
        let secret = token.secret_key_ref.unwrap();
        assert_eq!(
            (secret.name.as_str(), secret.key.as_str()),
            ("api-token", "token")
        );
        assert_eq!(env(&result, "TEAM").unwrap().value, "platform");
    }

    #[test]
    fn no_secret_reference_unless_configured() {
        let result = apply(&opted_in(), &Config::defaults());
        assert!(env(&result, "EXAMPLE_TOKEN").is_none());
        let agent = &result.sidecars[0];
        assert!(agent.volumes.iter().all(|v| v.volume_type != "secret"));
    }

    #[test]
    fn agent_sidecar_uses_volumes_mounts_and_split_resources() {
        let result = apply(
            &opted_in(),
            &cfg(&[(keys::SIDECAR_SIZE, "medium"), (keys::TOKEN_SECRET, "t")]),
        );
        let agent = &result.sidecars[0];
        assert_eq!(agent.name, "example-agent");
        assert_eq!(agent.cpu_request.as_deref(), Some("50m"));
        assert_eq!(agent.memory_limit.as_deref(), Some("128Mi"));
        assert!(agent.cpu.is_none() && agent.memory.is_none());
        let types: Vec<_> = agent
            .volumes
            .iter()
            .map(|v| v.volume_type.as_str())
            .collect();
        assert_eq!(types, ["emptyDir", "configMap", "secret"]);
        let motd = agent
            .volume_mounts
            .iter()
            .find(|m| m.sub_path.is_some())
            .unwrap();
        assert!(motd.read_only);
        assert_eq!(agent.primary_volume_mounts[0].mount_path, SHARED_DIR);
        assert!(agent.primary_volume_mounts[0].read_only);
    }

    #[test]
    fn init_container_only_comes_with_the_sidecar() {
        let on = apply(&opted_in(), &Config::defaults());
        assert_eq!(on.init_containers.len(), 1);
        let mounted = &on.init_containers[0].volume_mounts[0].name;
        assert!(on.sidecars[0].volumes.iter().any(|v| &v.name == mounted));

        let off = apply(&opted_in(), &cfg(&[(keys::SIDECAR_ENABLED, "false")]));
        assert!(off.sidecars.is_empty());
        assert!(off.init_containers.is_empty());
    }

    #[test]
    fn config_map_names_its_namespace() {
        let result = apply(&opted_in(), &Config::defaults());
        let cm = &result.kubernetes_resources[0];
        assert_eq!(cm["kind"], "ConfigMap");
        assert_eq!(cm["metadata"]["namespace"], "shop");
        assert_eq!(cm["data"][MOTD_KEY], "Hello, api!");
    }

    #[test]
    fn service_account_role_prefers_settings_then_aws_output() {
        let mut context = opted_in();
        context.plugin_outputs.insert(
            AWS_PLUGIN.into(),
            HashMap::from([("role_arn".into(), "arn:from-aws".into())]),
        );
        let from_aws = apply(&context, &Config::defaults());
        assert_eq!(
            from_aws.wants_service_account.unwrap().irsa_role_arn,
            "arn:from-aws"
        );

        let from_settings = apply(&context, &cfg(&[(keys::ROLE_ARN, "arn:from-settings")]));
        let sa = from_settings.wants_service_account.unwrap();
        assert_eq!(sa.name, "api-example");
        assert_eq!(sa.irsa_role_arn, "arn:from-settings");

        let neither = apply(&opted_in(), &Config::defaults());
        assert_eq!(neither.wants_service_account.unwrap().irsa_role_arn, "");
    }

    #[test]
    fn outputs_include_the_provisioned_cache_address() {
        let mut context = opted_in();
        context.provisioned_resources.insert(
            format!("{NAME}/{CACHE_RESOURCE_ID}"),
            HashMap::from([
                (cache_state::HOST.into(), "127.0.0.1".into()),
                (cache_state::PORT.into(), "6379".into()),
            ]),
        );
        let result = apply(&context, &Config::defaults());
        assert_eq!(result.outputs["cache_address"], "127.0.0.1:6379");
        assert_eq!(result.outputs["configmap"], "api-example");
    }

    #[test]
    fn stamps_annotation_and_label() {
        let result = apply(&opted_in(), &cfg(&[(keys::GREETING, "Hi")]));
        assert_eq!(result.deployment_annotations[GREETING_ANNOTATION], "Hi");
        assert_eq!(result.deployment_labels[MANAGED_LABEL], "true");
    }

    #[test]
    fn reports_events_with_legacy_copies() {
        let result = apply(&opted_in(), &cfg(&[(keys::EXTRA_ENV, "TEAM=platform")]));
        let warning = result
            .events
            .iter()
            .find(|e| e.code == codes::CONFIG_INVALID)
            .unwrap();
        assert_eq!(warning.severity, EventSeverity::Warning);
        assert!(warning.hint.as_deref().unwrap().contains(keys::EXTRA_ENV));
        // Older hosts read `warnings`; the helper copied the event there.
        assert_eq!(result.warnings, [warning.legacy_text()]);

        let applied = result
            .events
            .iter()
            .find(|e| e.code == codes::APPLIED)
            .unwrap();
        assert_eq!(applied.severity, EventSeverity::Info);
        assert_eq!(
            applied.object,
            Some(KubernetesObjectRef::new("v1", "ConfigMap", "api-example"))
        );
        // Info events have no legacy field.
        assert!(result.errors.is_empty());
    }

    #[test]
    fn object_names_fit_in_63_characters() {
        let long = "a".repeat(80);
        let name = object_name(&long, "-warmup");
        assert_eq!(name.len(), 63);
        assert!(!name.ends_with('-'));
        assert_eq!(object_name("api", ""), "api-example");
    }
}
