//! What the plugin declares: the result of the `metadata()` export.
//!
//! SDK features: every `PluginMetadata` field; `ConfigField` with each
//! `ConfigFieldType` a plugin should send (`String`, `Secret`, `Bool`,
//! `Select`, `KeyValue`) and its `default`, `required`, `options` and
//! `env_source`; `McpTuningField`; `PluginResource`.
//!
//! deckwatch calls `metadata()` once, when it loads the plugin, with only the
//! plugin's static config (no `CURRENT_TIMESTAMP`, no inherited environment).

use deckwatch_plugin_sdk::{
    ConfigField, ConfigFieldType, McpTuningField, PluginMetadata, PluginResource,
};

use crate::config::{Size, DEFAULT_GREETING, DEFAULT_SIDECAR_IMAGE};

/// The plugin's display name. deckwatch keys `outputs` and its tracking
/// annotations by the name the plugin is *registered* under, so register it
/// as `example` too.
pub const NAME: &str = "example";

/// Capability IDs: `provides` for this plugin, and one it runs after if
/// present. deckwatch-plugin-aws provides `aws:iam-role` and outputs
/// `role_arn`, which [`crate::apply`] uses for the ServiceAccount.
pub const PROVIDES: &str = "example:greeting";
pub const OPTIONAL_DEPENDS_ON: &str = "aws:iam-role";

/// deckwatch 0.7.0 (not yet tagged; checked against `main` at `604a36f`) is
/// the first version that understands the `key_value` fields below and
/// reads `events`. Sidecar volumes, split requests and limits, init
/// containers and this check itself need v0.6.4+. A deckwatch that predates
/// `key_value` can't parse this metadata at all, so on those this check never
/// runs; see [`config_schema`].
pub const MIN_DECKWATCH_VERSION: &str = "0.7.0";

/// Config keys. The settings form saves values under these, and
/// `extism_pdk::config::get` reads them.
pub mod keys {
    pub const GREETING: &str = "GREETING";
    pub const API_TOKEN: &str = "API_TOKEN";
    pub const SIDECAR_ENABLED: &str = "SIDECAR_ENABLED";
    pub const SIDECAR_SIZE: &str = "SIDECAR_SIZE";
    pub const SIDECAR_IMAGE: &str = "SIDECAR_IMAGE";
    pub const EXTRA_ENV: &str = "EXTRA_ENV";
    pub const TOKEN_SECRET: &str = "TOKEN_SECRET";
    pub const WEBHOOK_URL: &str = "WEBHOOK_URL";
    /// Named after the deckwatch environment variable it comes from: listed
    /// in `inherit_env_keys`, a variable reaches the plugin under its own
    /// name.
    pub const ROLE_ARN: &str = "EXAMPLE_ROLE_ARN";
}

/// Event codes: `<plugin-name>.<code>`. deckwatch reserves `plugin.`.
pub mod codes {
    pub const CONFIG_INVALID: &str = "example.config_invalid";
    pub const CACHE_CREATING: &str = "example.cache_creating";
    pub const CACHE_READY: &str = "example.cache_ready";
    pub const CACHE_FAILED: &str = "example.cache_failed";
    pub const CACHE_DATA_DISCARDED: &str = "example.cache_data_discarded";
    pub const TAGS_INVALID: &str = "example.tags_invalid";
    pub const UNKNOWN_RESOURCE: &str = "example.unknown_resource";
    pub const WEBHOOK_FAILED: &str = "example.webhook_failed";

    /// Every code above, for tests.
    pub const ALL: [&str; 8] = [
        CONFIG_INVALID,
        CACHE_CREATING,
        CACHE_READY,
        CACHE_FAILED,
        CACHE_DATA_DISCARDED,
        TAGS_INVALID,
        UNKNOWN_RESOURCE,
        WEBHOOK_FAILED,
    ];
}

/// The provisionable resource: its `id`, form fields and `state` keys.
pub const CACHE_RESOURCE_ID: &str = "cache";

pub mod cache_fields {
    pub const SIZE: &str = "SIZE";
    pub const TAGS: &str = "TAGS";
}

/// `state` keys. deckwatch injects each as an env var into every Deployment
/// and CronJob of the application.
pub mod cache_state {
    pub const HOST: &str = "EXAMPLE_CACHE_HOST";
    pub const PORT: &str = "EXAMPLE_CACHE_PORT";
    /// The `_STATUS` suffix matters: deckwatch re-polls a resource only while
    /// a `*_STATUS` key holds an in-progress value such as `creating`.
    pub const STATUS: &str = "EXAMPLE_CACHE_STATUS";
}

/// The result of `metadata()`.
pub fn metadata() -> PluginMetadata {
    PluginMetadata {
        name: NAME.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description: "Reference plugin: injects env vars, a sidecar and an init container into \
                      opted-in Deployments, and provisions a cache for applications."
            .into(),
        provides: vec![PROVIDES.into()],
        // A hard dependency makes deckwatch skip this plugin unless a loaded
        // plugin provides the capability. This example doesn't need one.
        depends_on: vec![],
        optional_depends_on: vec![OPTIONAL_DEPENDS_ON.into()],
        config_schema: config_schema(),
        resources: vec![cache_resource()],
        mcp_tuning_fields: vec![McpTuningField {
            key: "GREETING_STYLE".into(),
            label: "Greeting style".into(),
            description: "How AI agents should phrase greetings this plugin injects.".into(),
            placeholder: "friendly, one sentence".into(),
            default: Some("friendly".into()),
        }],
        min_deckwatch_version: Some(MIN_DECKWATCH_VERSION.into()),
    }
}

fn field(key: &str, label: &str, description: &str, field_type: ConfigFieldType) -> ConfigField {
    ConfigField {
        key: key.into(),
        label: label.into(),
        description: description.into(),
        field_type,
        ..Default::default()
    }
}

fn size_options() -> Vec<String> {
    Size::OPTIONS.iter().map(|s| s.to_string()).collect()
}

/// The settings form: one field of each type a plugin should send.
///
/// **`key_value` needs deckwatch 0.7.0** (not yet tagged; on `main` since
/// `278de71`). The v0.6.7 release and older reject this plugin's whole
/// metadata, so the plugin loads with no settings form and no cache resource
/// (`apply` still runs). Drop the `key_value` fields, and lower
/// [`MIN_DECKWATCH_VERSION`], if you target v0.6.7.
pub fn config_schema() -> Vec<ConfigField> {
    vec![
        ConfigField {
            default: Some(DEFAULT_GREETING.into()),
            ..field(
                keys::GREETING,
                "Greeting",
                "Injected into opted-in Deployments as EXAMPLE_GREETING.",
                ConfigFieldType::String,
            )
        },
        field(
            keys::API_TOKEN,
            "Webhook token",
            "Sent as a Bearer token when calling WEBHOOK_URL.",
            ConfigFieldType::Secret,
        ),
        ConfigField {
            default: Some("true".into()),
            ..field(
                keys::SIDECAR_ENABLED,
                "Agent sidecar",
                "Inject the agent sidecar and its init container.",
                ConfigFieldType::Bool,
            )
        },
        ConfigField {
            default: Some("small".into()),
            required: true,
            options: size_options(),
            ..field(
                keys::SIDECAR_SIZE,
                "Agent size",
                "Resource requests and limits for the agent sidecar.",
                ConfigFieldType::Select,
            )
        },
        ConfigField {
            default: Some(DEFAULT_SIDECAR_IMAGE.into()),
            ..field(
                keys::SIDECAR_IMAGE,
                "Agent image",
                "Image for the agent sidecar.",
                ConfigFieldType::String,
            )
        },
        field(
            keys::EXTRA_ENV,
            "Extra environment",
            "More environment variables for opted-in Deployments, as key/value pairs.",
            ConfigFieldType::KeyValue,
        ),
        field(
            keys::TOKEN_SECRET,
            "Token Secret",
            "Name of a Secret with a `token` key, injected as EXAMPLE_TOKEN. The pod won't \
             start if the Secret or key is missing.",
            ConfigFieldType::String,
        ),
        field(
            keys::WEBHOOK_URL,
            "Webhook URL",
            "Called after a cache is provisioned. Add its host to the plugin's allowed_hosts: \
             if the call can't be made at all, provisioning fails. HTTP error statuses only \
             add a warning.",
            ConfigFieldType::String,
        ),
        // `env_source` only changes the form: the field shows read-only with
        // the variable's name. The value arrives only if the operator lists
        // EXAMPLE_ROLE_ARN in the plugin's `inherit_env_keys`.
        ConfigField {
            env_source: Some(keys::ROLE_ARN.into()),
            ..field(
                keys::ROLE_ARN,
                "IAM role ARN",
                "IRSA role for the plugin's ServiceAccount.",
                ConfigFieldType::String,
            )
        },
    ]
}

/// The cache, provisioned per application from its detail page.
pub fn cache_resource() -> PluginResource {
    PluginResource {
        id: CACHE_RESOURCE_ID.into(),
        label: "Example cache".into(),
        icon: "mdi-database".into(),
        description: "A Redis sidecar in every Deployment and CronJob of the application.".into(),
        singleton: true,
        fields: vec![
            ConfigField {
                default: Some("small".into()),
                required: true,
                options: size_options(),
                ..field(
                    cache_fields::SIZE,
                    "Size",
                    "Resource requests and limits for the cache sidecar.",
                    ConfigFieldType::Select,
                )
            },
            field(
                cache_fields::TAGS,
                "Tags",
                "Labels for the cache's Kubernetes objects.",
                ConfigFieldType::KeyValue,
            ),
        ],
        output_keys: vec![
            cache_state::HOST.into(),
            cache_state::PORT.into(),
            cache_state::STATUS.into(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckwatch_plugin_sdk::telemetry;

    #[test]
    fn declares_every_metadata_field() {
        let m = metadata();
        assert_eq!(m.name, NAME);
        assert_eq!(m.version, env!("CARGO_PKG_VERSION"));
        assert!(!m.description.is_empty());
        assert_eq!(m.provides, [PROVIDES]);
        assert!(m.depends_on.is_empty());
        assert_eq!(m.optional_depends_on, [OPTIONAL_DEPENDS_ON]);
        assert!(!m.config_schema.is_empty());
        assert_eq!(m.resources.len(), 1);
        assert_eq!(m.mcp_tuning_fields.len(), 1);
        assert_eq!(
            m.min_deckwatch_version.as_deref(),
            Some(MIN_DECKWATCH_VERSION)
        );
    }

    #[test]
    fn schema_uses_every_field_type_a_plugin_sends() {
        let types: Vec<_> = config_schema().into_iter().map(|f| f.field_type).collect();
        for t in [
            ConfigFieldType::String,
            ConfigFieldType::Secret,
            ConfigFieldType::Bool,
            ConfigFieldType::Select,
            ConfigFieldType::KeyValue,
        ] {
            assert!(types.contains(&t), "{t:?} missing");
        }
        // Unknown is what a host reads for a type it doesn't know; plugins
        // never send it.
        assert!(!types.contains(&ConfigFieldType::Unknown));
    }

    #[test]
    fn keys_are_valid_and_unique() {
        let mut seen = std::collections::HashSet::new();
        let resource_keys = cache_resource().fields.into_iter().map(|f| f.key);
        let tuning_keys = metadata().mcp_tuning_fields.into_iter().map(|f| f.key);
        for key in config_schema()
            .into_iter()
            .map(|f| f.key)
            .chain(resource_keys)
            .chain(tuning_keys)
        {
            assert!(
                key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{key}"
            );
            assert!(seen.insert(key.clone()), "duplicate key {key}");
        }
    }

    #[test]
    fn select_fields_have_their_default_among_the_options() {
        let fields = config_schema().into_iter().chain(cache_resource().fields);
        for f in fields.filter(|f| f.field_type == ConfigFieldType::Select) {
            let default = f.default.clone().unwrap_or_default();
            assert!(f.options.contains(&default), "{}", f.key);
        }
    }

    #[test]
    fn env_source_matches_its_key() {
        for f in config_schema() {
            if let Some(var) = &f.env_source {
                assert_eq!(
                    &f.key, var,
                    "inherit_env_keys passes a variable under its own name"
                );
            }
        }
    }

    #[test]
    fn min_deckwatch_version_has_no_v_prefix() {
        // deckwatch compares only the integer parts, so "v0.7.0" would read as 7.0.0.
        assert!(MIN_DECKWATCH_VERSION
            .split('.')
            .all(|p| p.parse::<u32>().is_ok()));
    }

    #[test]
    fn event_codes_are_namespaced_and_not_reserved() {
        for code in codes::ALL {
            assert!(code.starts_with(&format!("{NAME}.")), "{code}");
            assert!(
                !code.starts_with(telemetry::code::RESERVED_PREFIX),
                "{code}"
            );
        }
    }

    #[test]
    fn cache_output_keys_include_a_status_key() {
        let keys = cache_resource().output_keys;
        assert!(keys.iter().any(|k| k.ends_with("_STATUS")));
    }

    #[test]
    fn metadata_round_trips_through_json() {
        let json = serde_json::to_value(metadata()).unwrap();
        assert_eq!(json["config_schema"][5]["field_type"], "key_value");
        let back: PluginMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(back.name, NAME);
    }
}
