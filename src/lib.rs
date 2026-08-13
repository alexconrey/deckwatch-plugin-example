//! # deckwatch-plugin-example
//!
//! A minimal reference plugin demonstrating both patterns the deckwatch
//! plugin SDK supports:
//!
//! ## Pattern 1 — Infrastructure tab (recommended for stateful resources)
//!
//! Declare `resources` in `metadata()`. Deckwatch shows an "Add Debug Sidecar"
//! button on the Application detail page. Clicking it calls `provision()`,
//! which injects a sidecar into every deployment in the application and
//! returns state (env vars) that are also injected by the reconciler.
//!
//! ```text
//! metadata() → resources: [debug_resource()]
//! provision() → state + sidecars + deployment_annotations
//! deprovision() → message (sidecar removed on next reconcile)
//! ```
//!
//! ## Pattern 2 — Annotation-based opt-in (apply)
//!
//! Still valid for simple per-deployment injection. The plugin is associated
//! with an application and `apply()` is called on every deployment update.
//! Use this for lightweight env var injection where no persistent state is
//! needed.
//!
//! ```text
//! apply(ctx) → env_vars / sidecars based on ctx.annotations
//! ```

// Pure types — unconditionally imported so they can be used in host-target tests.
use deckwatch_plugin_sdk::{
    ConfigField, ConfigFieldType, EnvVarSpec, PluginContext, PluginResource, PluginResult,
    SidecarSpec,
};

#[cfg(target_arch = "wasm32")]
use deckwatch_plugin_sdk::{
    PluginMetadata, ResourceDeprovisionRequest, ResourceDeprovisionResult,
    ResourceProvisionRequest, ResourceProvisionResult,
};
#[cfg(target_arch = "wasm32")]
use extism_pdk::*;

// ── Resource declarations (not cfg-gated — must be testable on host) ─────────

/// Declares the "Debug Sidecar" resource shown in the Infrastructure tab.
pub fn debug_resource() -> PluginResource {
    PluginResource {
        id: "debug".into(),
        label: "Debug Sidecar".into(),
        icon: "mdi-bug-outline".into(),
        description: "Injects a lightweight Alpine sidecar that runs sleep infinity. Useful for debugging — exec into it with kubectl.".into(),
        singleton: true,
        fields: vec![
            ConfigField {
                key: "image".into(),
                label: "Sidecar Image".into(),
                description: "Container image to use for the debug sidecar.".into(),
                field_type: ConfigFieldType::String,
                default: Some("alpine:3.20".into()),
                required: false,
                options: vec![],
                env_source: None,
            },
        ],
        output_keys: vec!["DEBUG_ENABLED".into(), "DEBUG_NAMESPACE".into()],
    }
}

// ── WASM entry points ─────────────────────────────────────────────────────────

/// Called once at plugin load time. Declares capabilities, config schema,
/// and the resources this plugin can provision via the Infrastructure tab.
#[cfg(target_arch = "wasm32")]
#[plugin_fn]
pub fn metadata() -> FnResult<Json<PluginMetadata>> {
    Ok(Json(PluginMetadata {
        name: "example".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description:
            "Reference plugin demonstrating PluginResource + provision() and apply() patterns."
                .into(),
        provides: vec!["example:debug-sidecar".into()],
        depends_on: vec![],
        optional_depends_on: vec![],
        config_schema: vec![ConfigField {
            key: "LOG_LEVEL".into(),
            label: "Log Level".into(),
            description: "Log verbosity for the example plugin (debug, info, warn, error).".into(),
            field_type: ConfigFieldType::Select,
            default: Some("info".into()),
            required: false,
            options: vec!["debug".into(), "info".into(), "warn".into(), "error".into()],
            env_source: None,
        }],
        resources: vec![debug_resource()],
    }))
}

/// Pattern 1 — provision a debug sidecar via the Infrastructure tab.
///
/// Called when the operator clicks "+ Add Debug Sidecar" in the Application
/// detail page. Returns a sidecar that is injected into ALL deployments in
/// the application by the deckwatch reconciler within ~30 seconds.
#[cfg(target_arch = "wasm32")]
#[plugin_fn]
pub fn provision(
    Json(req): Json<ResourceProvisionRequest>,
) -> FnResult<Json<ResourceProvisionResult>> {
    let mut result = ResourceProvisionResult::default();

    if req.resource_id != "debug" {
        result
            .errors
            .push(format!("unknown resource_id: {}", req.resource_id));
        return Ok(Json(result));
    }

    let image = req
        .fields
        .get("image")
        .cloned()
        .unwrap_or_else(|| "alpine:3.20".into());

    // State: env vars injected into all application deployments.
    result.state.insert("DEBUG_ENABLED".into(), "true".into());
    result
        .state
        .insert("DEBUG_NAMESPACE".into(), req.namespace.clone());
    result
        .state
        .insert("DEBUG_APPLICATION".into(), req.application_name.clone());

    // Sidecar: injected into spec.template.spec.containers of every deployment.
    result.sidecars.push(SidecarSpec {
        name: "debug-sidecar".into(),
        image,
        env: vec![
            EnvVarSpec::value("DEBUG_NAMESPACE", &req.namespace),
            EnvVarSpec::value("DEBUG_APPLICATION", &req.application_name),
        ],
        port: None,
        cpu: Some("10m".into()),
        memory: Some("16Mi".into()),
    });

    // Annotations stamped on all application deployments.
    result
        .deployment_annotations
        .insert("deckwatch.io/example-debug".into(), "enabled".into());

    result.errors.extend(vec![]); // no errors
    Ok(Json(result))
}

/// Clean up the debug sidecar resource.
/// The sidecar is removed from deployments on the next reconciler cycle.
#[cfg(target_arch = "wasm32")]
#[plugin_fn]
pub fn deprovision(
    Json(req): Json<ResourceDeprovisionRequest>,
) -> FnResult<Json<ResourceDeprovisionResult>> {
    Ok(Json(ResourceDeprovisionResult {
        errors: vec![],
        message: format!(
            "Debug sidecar for '{}' will be removed from all deployments on the next reconcile.",
            req.application_name
        ),
    }))
}

/// Pattern 2 — annotation-based opt-in via apply().
///
/// Called by deckwatch on every deployment create/update when the plugin is
/// associated with the application. Use this for lightweight per-deployment
/// injection where no persistent state is needed.
///
/// Opt-in annotations:
/// - `example.deckwatch.io/env: "enabled"` — injects EXAMPLE_VERSION env var
/// - `example.deckwatch.io/sidecar: "enabled"` — injects a logger sidecar
#[cfg(target_arch = "wasm32")]
#[plugin_fn]
pub fn apply(Json(ctx): Json<PluginContext>) -> FnResult<Json<PluginResult>> {
    Ok(Json(apply_inner(&ctx)))
}

// ── Pure helper — testable on host target ─────────────────────────────────────

pub fn apply_inner(ctx: &PluginContext) -> PluginResult {
    let mut result = PluginResult::default();

    if ctx
        .annotations
        .get("example.deckwatch.io/env")
        .map(|v| v == "enabled")
        .unwrap_or(false)
    {
        result.env_vars.push(EnvVarSpec::value(
            "EXAMPLE_VERSION",
            env!("CARGO_PKG_VERSION"),
        ));
        result
            .env_vars
            .push(EnvVarSpec::value("EXAMPLE_NAMESPACE", &ctx.namespace));
    }

    if ctx
        .annotations
        .get("example.deckwatch.io/sidecar")
        .map(|v| v == "enabled")
        .unwrap_or(false)
    {
        result.sidecars.push(SidecarSpec {
            name: "example-logger".into(),
            image: "busybox:1.36".into(),
            env: vec![EnvVarSpec::value("LOG_NAMESPACE", &ctx.namespace)],
            port: None,
            cpu: Some("5m".into()),
            memory: Some("8Mi".into()),
        });
    }

    result
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn ctx(annotations: &[(&str, &str)]) -> PluginContext {
        PluginContext {
            namespace: "production".into(),
            deployment_name: "my-service".into(),
            annotations: annotations
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            labels: HashMap::new(),
            plugin_outputs: HashMap::new(),
        }
    }

    // ── apply() tests ─────────────────────────────────────────────────────

    #[test]
    fn no_annotations_returns_empty() {
        let result = apply_inner(&ctx(&[]));
        assert!(result.env_vars.is_empty());
        assert!(result.sidecars.is_empty());
    }

    #[test]
    fn env_annotation_injects_env_vars() {
        let result = apply_inner(&ctx(&[("example.deckwatch.io/env", "enabled")]));
        assert!(result.env_vars.iter().any(|e| e.name == "EXAMPLE_VERSION"));
        assert!(result
            .env_vars
            .iter()
            .any(|e| e.name == "EXAMPLE_NAMESPACE"));
    }

    #[test]
    fn sidecar_annotation_injects_sidecar() {
        let result = apply_inner(&ctx(&[("example.deckwatch.io/sidecar", "enabled")]));
        assert!(result.sidecars.iter().any(|s| s.name == "example-logger"));
    }

    // ── resource declaration tests ────────────────────────────────────────

    #[test]
    fn debug_resource_has_correct_id() {
        assert_eq!(debug_resource().id, "debug");
    }

    #[test]
    fn debug_resource_is_singleton() {
        assert!(debug_resource().singleton);
    }

    #[test]
    fn debug_resource_declares_output_keys() {
        let r = debug_resource();
        assert!(r.output_keys.iter().any(|k| k == "DEBUG_ENABLED"));
    }
}
