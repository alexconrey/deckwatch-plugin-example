use deckwatch_plugin_sdk::{EnvVarSpec, PluginContext, PluginResult, SidecarSpec};
use extism_pdk::*;

/// Entry point called by deckwatch for every deployment create/update.
///
/// Return [`PluginResult::default()`] (empty vecs) to inject nothing.
/// Any field deckwatch already sees on the deployment is left untouched —
/// user-supplied env vars and existing sidecar containers are never
/// overwritten by a plugin.
#[plugin_fn]
pub fn apply(Json(ctx): Json<PluginContext>) -> FnResult<Json<PluginResult>> {
    let mut result = PluginResult::default();

    // ── Example 1: external service provider ─────────────────────────────
    // Inject DATABASE_URL when the operator opts the deployment in via
    // annotation: `myorg.io/db: "enabled"`
    if ctx
        .annotations
        .get("myorg.io/db")
        .map(|v| v == "enabled")
        .unwrap_or(false)
    {
        result.env_vars.push(EnvVarSpec {
            name: "DATABASE_URL".into(),
            value: format!(
                "postgres://db.myorg.internal:5432/{}",
                ctx.deployment_name
            ),
        });
        result.env_vars.push(EnvVarSpec {
            name: "DB_POOL_SIZE".into(),
            value: "10".into(),
        });
    }

    // ── Example 2: internal cluster sidecar provider ──────────────────────
    // Inject a service-mesh agent sidecar when opted in via:
    // `myorg.io/mesh: "enabled"`
    if ctx
        .annotations
        .get("myorg.io/mesh")
        .map(|v| v == "enabled")
        .unwrap_or(false)
    {
        result.sidecars.push(SidecarSpec {
            name: "mesh-agent".into(),
            image: "registry.myorg.internal/mesh-agent:v2".into(),
            env: vec![
                EnvVarSpec {
                    name: "MESH_NAMESPACE".into(),
                    value: ctx.namespace.clone(),
                },
                EnvVarSpec {
                    name: "MESH_SERVICE".into(),
                    value: ctx.deployment_name.clone(),
                },
            ],
            port: Some(15000),
            cpu: Some("50m".into()),
            memory: Some("64Mi".into()),
        });
    }

    Ok(Json(result))
}

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
        }
    }

    #[test]
    fn no_annotations_returns_empty() {
        let result = super::apply_inner(&ctx(&[]));
        assert!(result.env_vars.is_empty());
        assert!(result.sidecars.is_empty());
    }

    #[test]
    fn db_annotation_injects_env() {
        let result = super::apply_inner(&ctx(&[("myorg.io/db", "enabled")]));
        assert!(result.env_vars.iter().any(|e| e.name == "DATABASE_URL"));
    }

    #[test]
    fn mesh_annotation_injects_sidecar() {
        let result = super::apply_inner(&ctx(&[("myorg.io/mesh", "enabled")]));
        assert!(result.sidecars.iter().any(|s| s.name == "mesh-agent"));
    }
}

// Pure function extracted so unit tests can call it without the extism ABI.
#[cfg_attr(test, allow(dead_code))]
fn apply_inner(ctx: &PluginContext) -> PluginResult {
    let mut result = PluginResult::default();

    if ctx
        .annotations
        .get("myorg.io/db")
        .map(|v| v == "enabled")
        .unwrap_or(false)
    {
        result.env_vars.push(EnvVarSpec {
            name: "DATABASE_URL".into(),
            value: format!(
                "postgres://db.myorg.internal:5432/{}",
                ctx.deployment_name
            ),
        });
        result.env_vars.push(EnvVarSpec {
            name: "DB_POOL_SIZE".into(),
            value: "10".into(),
        });
    }

    if ctx
        .annotations
        .get("myorg.io/mesh")
        .map(|v| v == "enabled")
        .unwrap_or(false)
    {
        result.sidecars.push(SidecarSpec {
            name: "mesh-agent".into(),
            image: "registry.myorg.internal/mesh-agent:v2".into(),
            env: vec![
                EnvVarSpec {
                    name: "MESH_NAMESPACE".into(),
                    value: ctx.namespace.clone(),
                },
                EnvVarSpec {
                    name: "MESH_SERVICE".into(),
                    value: ctx.deployment_name.clone(),
                },
            ],
            port: Some(15000),
            cpu: Some("50m".into()),
            memory: Some("64Mi".into()),
        });
    }

    result
}
