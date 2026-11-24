//! Reading the operator's settings.
//!
//! deckwatch passes the plugin's settings (what the form saved, keyed by
//! `ConfigField::key`) as extism config. `exports` reads them with
//! `extism_pdk::config::get`; [`Config::from_lookup`] takes the lookup as a
//! function, so tests can supply their own values.
//!
//! SDK features: how each `ConfigFieldType` arrives (`bool` as `"true"` or
//! `"false"`, `select` as the chosen option, `key_value` as a JSON object of
//! strings) and `parse_key_value_field`.
//!
//! deckwatch doesn't pass a field's `default` to the plugin (the form only
//! pre-fills it), so a missing key means "use the default". It also doesn't
//! enforce `required`. Only the settings form does, so check values here too.

use std::collections::BTreeMap;

use deckwatch_plugin_sdk::parse_key_value_field;

use crate::metadata::keys;

/// Used when `GREETING` isn't set.
pub const DEFAULT_GREETING: &str = "Hello";

/// Used when `SIDECAR_IMAGE` isn't set: the Kubernetes pause image, which
/// does nothing and never exits.
pub const DEFAULT_SIDECAR_IMAGE: &str = "registry.k8s.io/pause:3.10";

/// A `select` setting's options, mapped to resource requests and limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Small,
    Medium,
    Large,
}

impl Size {
    /// The `select` options, in the order the form shows them.
    pub const OPTIONS: [&'static str; 3] = ["small", "medium", "large"];

    pub fn parse(option: &str) -> Option<Size> {
        match option {
            "small" => Some(Size::Small),
            "medium" => Some(Size::Medium),
            "large" => Some(Size::Large),
            _ => None,
        }
    }

    /// CPU request, CPU limit, memory request, memory limit.
    pub fn resources(self) -> [&'static str; 4] {
        match self {
            Size::Small => ["10m", "50m", "16Mi", "32Mi"],
            Size::Medium => ["50m", "200m", "64Mi", "128Mi"],
            Size::Large => ["200m", "500m", "256Mi", "512Mi"],
        }
    }
}

/// A setting that was set but can't be used. `apply` reports each one as a
/// warning event, and the default applies instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub key: &'static str,
    pub message: String,
}

/// The plugin's settings, with defaults applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub greeting: String,
    /// A `secret` field. As of deckwatch `fc03ec2`, values saved through the
    /// settings form reach the plugin still encrypted when deckwatch has
    /// `DECKWATCH_ENCRYPTION_KEY` set (alexconrey/deckwatch#251).
    pub api_token: Option<String>,
    pub sidecar_enabled: bool,
    pub sidecar_size: Size,
    pub sidecar_image: String,
    /// A `key_value` field: extra environment variables.
    pub extra_env: BTreeMap<String, String>,
    pub token_secret: Option<String>,
    pub webhook_url: Option<String>,
    pub role_arn: Option<String>,
    pub problems: Vec<Problem>,
}

impl Config {
    /// Builds the settings from `lookup`, which returns a config value by key.
    /// Empty and whitespace-only values count as unset.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Config {
        let get = |key: &str| {
            lookup(key)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let mut problems = Vec::new();

        let sidecar_enabled = match get(keys::SIDECAR_ENABLED).as_deref() {
            None | Some("true") => true,
            Some("false") => false,
            Some(other) => {
                problems.push(Problem {
                    key: keys::SIDECAR_ENABLED,
                    message: format!("expected \"true\" or \"false\", got {other:?}; using true"),
                });
                true
            }
        };

        let sidecar_size = match get(keys::SIDECAR_SIZE) {
            None => Size::Small,
            Some(option) => Size::parse(&option).unwrap_or_else(|| {
                problems.push(Problem {
                    key: keys::SIDECAR_SIZE,
                    message: format!(
                        "{option:?} is not one of {}; using small",
                        Size::OPTIONS.join(", ")
                    ),
                });
                Size::Small
            }),
        };

        let extra_env = match get(keys::EXTRA_ENV) {
            None => BTreeMap::new(),
            Some(raw) => parse_key_value_field(&raw).unwrap_or_else(|err| {
                problems.push(Problem {
                    key: keys::EXTRA_ENV,
                    message: err,
                });
                BTreeMap::new()
            }),
        };

        Config {
            greeting: get(keys::GREETING).unwrap_or_else(|| DEFAULT_GREETING.into()),
            api_token: get(keys::API_TOKEN),
            sidecar_enabled,
            sidecar_size,
            sidecar_image: get(keys::SIDECAR_IMAGE).unwrap_or_else(|| DEFAULT_SIDECAR_IMAGE.into()),
            extra_env,
            token_secret: get(keys::TOKEN_SECRET),
            webhook_url: get(keys::WEBHOOK_URL),
            role_arn: get(keys::ROLE_ARN),
            problems,
        }
    }

    /// The settings when nothing is configured.
    pub fn defaults() -> Config {
        Config::from_lookup(|_| None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn from(pairs: &[(&str, &str)]) -> Config {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|k| map.get(k).cloned())
    }

    #[test]
    fn defaults_apply_when_nothing_is_set() {
        let c = Config::defaults();
        assert_eq!(c.greeting, DEFAULT_GREETING);
        assert!(c.sidecar_enabled);
        assert_eq!(c.sidecar_size, Size::Small);
        assert_eq!(c.sidecar_image, DEFAULT_SIDECAR_IMAGE);
        assert!(c.extra_env.is_empty());
        assert!(c.problems.is_empty());
    }

    #[test]
    fn empty_values_count_as_unset() {
        let c = from(&[(keys::GREETING, "  "), (keys::WEBHOOK_URL, "")]);
        assert_eq!(c.greeting, DEFAULT_GREETING);
        assert_eq!(c.webhook_url, None);
    }

    #[test]
    fn reads_every_field_type() {
        let c = from(&[
            (keys::GREETING, "Hi"),
            (keys::API_TOKEN, "s3cret"),
            (keys::SIDECAR_ENABLED, "false"),
            (keys::SIDECAR_SIZE, "large"),
            (keys::EXTRA_ENV, r#"{"TEAM":"platform","TIER":"gold"}"#),
            (keys::ROLE_ARN, "arn:aws:iam::123456789012:role/example"),
        ]);
        assert_eq!(c.greeting, "Hi");
        assert_eq!(c.api_token.as_deref(), Some("s3cret"));
        assert!(!c.sidecar_enabled);
        assert_eq!(c.sidecar_size, Size::Large);
        assert_eq!(
            c.extra_env.get("TEAM").map(String::as_str),
            Some("platform")
        );
        assert_eq!(
            c.role_arn.as_deref(),
            Some("arn:aws:iam::123456789012:role/example")
        );
        assert!(c.problems.is_empty());
    }

    #[test]
    fn unusable_values_fall_back_and_are_reported() {
        let c = from(&[
            (keys::SIDECAR_ENABLED, "yes"),
            (keys::SIDECAR_SIZE, "huge"),
            (keys::EXTRA_ENV, "TEAM=platform"),
        ]);
        assert!(c.sidecar_enabled);
        assert_eq!(c.sidecar_size, Size::Small);
        assert!(c.extra_env.is_empty());
        let reported: Vec<_> = c.problems.iter().map(|p| p.key).collect();
        assert_eq!(
            reported,
            [keys::SIDECAR_ENABLED, keys::SIDECAR_SIZE, keys::EXTRA_ENV]
        );
    }

    #[test]
    fn every_size_option_parses() {
        for option in Size::OPTIONS {
            assert!(Size::parse(option).is_some(), "{option}");
        }
    }
}
