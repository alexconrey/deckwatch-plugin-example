# deckwatch-plugin-example

A reference [deckwatch](https://github.com/alexconrey/deckwatch) plugin that uses **every feature of [deckwatch-plugin-sdk](https://github.com/alexconrey/deckwatch-plugin-sdk) v0.10.0**. Read it alongside the [SDK README](https://github.com/alexconrey/deckwatch-plugin-sdk#readme): this repository shows each feature in working, tested code, and the SDK README documents the contract.

| | |
|---|---|
| SDK | `deckwatch-plugin-sdk` `v0.10.0` |
| Rust | 1.99, pinned in `rust-toolchain.toml` |
| deckwatch | **0.7.0 or later** (not yet tagged; checked against `main` at `58ff03c`), the first version that understands the two `key_value` settings and reads plugin events. The rest works on v0.6.4+, but v0.6.7 and older can't parse this plugin's metadata. See [compatibility](#compatibility) |

## What it does

- **`apply()`**, for Deployments annotated `example.deckwatch.io/enabled: "true"`:
  - injects env vars;
  - adds an agent sidecar and an init container that share a volume with the app;
  - creates a ConfigMap and a ServiceAccount;
  - stamps an annotation and a label;
  - reports configuration problems as warning events.
- **`provision()`**, for the **Example cache** resource on an application's page:
  - adds a Redis sidecar to every Deployment and CronJob in the application, and publishes its address as env vars;
  - creates a ServiceAccount and a warm-up Job;
  - calls an optional webhook, forwarding the trace context.
- **`deprovision()`** asks deckwatch to delete that Job and ServiceAccount.
- **`metadata()`** declares:
  - a settings form with one field of each type;
  - an MCP tuning field;
  - the cache resource;
  - capability ordering;
  - the minimum deckwatch version.

## SDK features and where they're used

| SDK feature | Where |
|---|---|
| `PluginMetadata`: every field | [`metadata::metadata`](src/metadata.rs) |
| `ConfigField` of each type a plugin sends (`String`, `Secret`, `Bool`, `Select`, `KeyValue`), with `default`, `required`, `options`, `env_source` | [`metadata::config_schema`](src/metadata.rs) |
| Reading settings: `bool`, `select` and `key_value` values; `parse_key_value_field` | [`config::Config::from_lookup`](src/config.rs) |
| `McpTuningField` | [`metadata::metadata`](src/metadata.rs) |
| `PluginResource`: form fields, `singleton`, `output_keys` | [`metadata::cache_resource`](src/metadata.rs) |
| `provides`, `optional_depends_on`, and reading another plugin's `plugin_outputs` | [`metadata`](src/metadata.rs), [`apply::apply`](src/apply.rs) |
| `min_deckwatch_version` (no `v` prefix) | [`metadata`](src/metadata.rs) |
| `EnvVarSpec::value`, `::config_map_key_ref`, `::secret_key_ref` | [`apply::apply`](src/apply.rs) |
| `SidecarSpec` with `VolumeSpec` (`emptyDir`, `configMap`, `secret`), `VolumeMountSpec` (`read_only`, `sub_path`), `primary_volume_mounts`, split requests and limits | [`apply::agent_sidecar`](src/apply.rs) |
| `init_containers` (apply and provision) | [`apply::init_container`](src/apply.rs), [`resources::provision`](src/resources.rs) |
| `kubernetes_resources` (with `metadata.namespace` set) | [`apply::config_map`](src/apply.rs), [`resources::provision`](src/resources.rs) |
| `wants_service_account` with an IRSA role | [`apply::apply`](src/apply.rs) |
| `deployment_annotations`, `deployment_labels` | [`apply::apply`](src/apply.rs) |
| `outputs` for later plugins | [`apply::apply`](src/apply.rs) |
| `provisioned_resources` (`"<plugin>/<resource>"`) | [`apply::apply`](src/apply.rs) |
| Events: `info`, `warn`, `error`, with `resource_id`, `object`, `retryable`, `hint`; the legacy `errors`/`warnings`/`message` copies | [`apply`](src/apply.rs), [`resources`](src/resources.rs), [`exports`](src/exports.rs) |
| `ResourceProvisionResult`: `state`, `sidecars`, `deployment_annotations`, `status` (`Pending`, `Available`, `Failed`), `retry_after_secs` | [`resources::provision`](src/resources.rs) |
| A `*_STATUS` state key, which is what makes deckwatch re-poll | [`metadata::cache_state`](src/metadata.rs), [`resources::provision`](src/resources.rs) |
| `ProvisioningStatus` is `#[non_exhaustive]`: a `match` with a `_` arm | [`resources::status_name`](src/resources.rs) |
| `ResourceDeprovisionResult`: `message`, `delete_kubernetes_resources` with `KubernetesObjectRef::job` and `::service_account` | [`resources::deprovision`](src/resources.rs) |
| `trace_context` and `trace_headers()` on an outbound call | [`resources::webhook_request`](src/resources.rs) |
| `telemetry::code::RESERVED_PREFIX`, `telemetry::TRACEPARENT_HEADER` | tests in [`metadata`](src/metadata.rs) and [`resources`](src/resources.rs) |
| `now()` (the host's clock) | [`exports::provision`](src/exports.rs) |
| extism PDK: `config::get`, `http::request`, log macros | [`exports`](src/exports.rs) |
| Building SDK structs with `..Default::default()` | throughout, including tests |

Not used, on purpose:
- **`ConfigFieldType::Unknown`** is what a host reads for a type it doesn't know; plugins never send it.
- **`SidecarSpec.cpu` and `memory`** are deprecated in favour of the split request/limit fields.
- **`service_account_name`** is the plain alternative to `wants_service_account`.
- **The `telemetry::attribute` names** label the host's own spans.

## Try it in deckwatch

1. **Register the plugin** (Settings → Plugins). Name it `example`: deckwatch keys `outputs` and its tracking annotations by the registered name.

   ```json
   {
     "name": "example",
     "enabled": true,
     "source": {
       "type": "github",
       "repo": "alexconrey/deckwatch-plugin-example",
       "ref": "v0.2.0",
       "path": "plugin.wasm",
       "use_release": true
     },
     "sha256": "<from the release's SHA256SUMS>",
     "allowed_hosts": ["hooks.example.com"],
     "config": {
       "GREETING": "Hi",
       "SIDECAR_SIZE": "small",
       "EXTRA_ENV": "{\"TEAM\":\"platform\"}",
       "WEBHOOK_URL": "https://hooks.example.com/deckwatch"
     },
     "inherit_env_keys": ["EXAMPLE_ROLE_ARN"]
   }
   ```

   - `allowed_hosts` is only needed for the webhook.
   - `inherit_env_keys` passes deckwatch's `EXAMPLE_ROLE_ARN` environment variable to the plugin under the same name. That's what the field's `env_source` refers to.
   - The settings form fills in `config` for you; the JSON shows the raw values. deckwatch 0.7.0 shows `EXTRA_ENV`, a `key_value` setting, as a text input of the JSON, and refuses to save a value that isn't a JSON object of strings.

2. **Opt a Deployment in.** Add the annotation to its `metadata.annotations`:

   ```bash
   kubectl annotate deployment my-app example.deckwatch.io/enabled=true -n my-namespace
   ```

   The next create or update through deckwatch's REST API (`POST /api/namespaces/{ns}/deployments` or `PUT /api/namespaces/{ns}/deployments/{name}`) runs `apply()`. deckwatch adds the env vars, the agent sidecar, its init container and the ServiceAccount name to the Deployment, then creates the ConfigMap and the ServiceAccount. The annotation and label come later ([below](#what-deckwatch-does-with-it-today)). MCP `create_deployment` and `update_deployment`, and YAML edits, don't run plugins ([deckwatch#283](https://github.com/alexconrey/deckwatch/issues/283)).

3. **Provision the cache.** On the application's page, add **Example cache**, choosing a size and optional tags. deckwatch 0.7.0 shows `TAGS` as a text input and doesn't check it, so enter a JSON object such as `{"team":"shop"}`. The Redis sidecar is added to every Deployment and CronJob labelled `deckwatch.io/application=<app>` on the next reconcile, and `EXAMPLE_CACHE_HOST`, `EXAMPLE_CACHE_PORT` and `EXAMPLE_CACHE_STATUS` appear as env vars.

## What deckwatch does with it today

Checked against deckwatch 0.7.0 (not yet tagged; `main` at `58ff03c`). The [SDK README](https://github.com/alexconrey/deckwatch-plugin-sdk#host-behaviour) has the details, including how the v0.6.7 release differs.

- **Events** are logged and returned in the create/update and provisioning responses. deckwatch drops the legacy copies the helpers write, so nothing is reported twice. For Deployments labelled `deckwatch.io/application`, and for the cache, deckwatch also records them on the plugin's status (shown on the application page), which drives Kubernetes Events and notifications.
  - **Configuration problems** are warnings: they mark the plugin **degraded** for the application, with a `PluginApplyWarning` Kubernetes Event on the affected Deployment.
  - **Invalid `TAGS`** is an error: it marks the cache **failing**, with a `PluginProvisionFailed` Kubernetes Event on the application's Deployments and a `plugin_failing` notification (at most one per 10 minutes). The other error codes, `example.unknown_resource` and `example.cache_failed`, show the pattern but don't occur here: deckwatch only provisions resources the plugin declares, and `exports` always reports the sidecar cache as ready.
  - The info events (`example.cache_creating`, `example.cache_ready`) never become Kubernetes Events. A provision or refresh with no warnings or errors gets a `PluginProvisioned` event instead.
- **`trace_context`** arrives while deckwatch exports OTLP traces, and the webhook forwards it as `traceparent`.
- **Deprovisioning** deletes the warm-up Job and the cache's ServiceAccount listed in `delete_kubernetes_resources`. The deprovision `message` comes back in the DELETE response, appears in the UI, and becomes the note of a `PluginDeprovisioned` Kubernetes Event.
- **The field sweep:** when an application's last resource (here, the cache) is removed, deckwatch removes everything its reconcile loop applied: the cache sidecar, its env vars and annotation, and `apply()`'s annotation and label.
- **Still open in deckwatch:**
  - `deployment_annotations` and `deployment_labels` from `apply()` are only applied by the reconcile loop, so the annotation and label only appear on the Deployments of an application with a provisioned resource ([deckwatch#244](https://github.com/alexconrey/deckwatch/issues/244));
  - nothing `apply()` injected is removed when the plugin is disabled or a Deployment opts out ([deckwatch#248](https://github.com/alexconrey/deckwatch/issues/248)), and the ConfigMap and ServiceAccount it created aren't deleted either;
  - init containers from `apply()` are appended after the workload's own, and those from `provision()` are never stored ([deckwatch#249](https://github.com/alexconrey/deckwatch/issues/249));
  - the `API_TOKEN` secret reaches the plugin encrypted when it was saved through the settings form and deckwatch has `DECKWATCH_ENCRYPTION_KEY` set ([deckwatch#251](https://github.com/alexconrey/deckwatch/issues/251));
  - over MCP `provision_resource`, pass `TAGS` as a JSON string: a non-string value makes deckwatch drop every field ([deckwatch#302](https://github.com/alexconrey/deckwatch/issues/302)).
- **The webhook:**
  - HTTP error statuses come back to the plugin and become a retryable warning.
  - A request that can't be made at all aborts the whole `provision()` call: deckwatch reports a `plugin.trap` error, and provisioning fails with HTTP 502. That covers a host not in `allowed_hosts`, a bad URL, and DNS or connection failures. extism handles these in the host, so the plugin can't catch them.
  - **No timeout.** extism has no HTTP timeout of its own, and deckwatch sets no time limit on plugin calls ([deckwatch#301](https://github.com/alexconrey/deckwatch/issues/301)), so a webhook that accepts the connection but never answers holds up the provisioning request indefinitely.
- **Log lines** from the PDK macros at `info` and above appear in deckwatch's logs by default since deckwatch `36e6637` (target `extism::pdk`), inside a span that names the plugin. `apply()`'s `debug!` lines also need `extism::pdk=debug` in `RUST_LOG`, which replaces deckwatch's whole default filter.

## Compatibility

- **deckwatch 0.7.0+** (not yet tagged): everything. The plugin declares `min_deckwatch_version: "0.7.0"`.
- **deckwatch v0.6.7 and older can't parse the plugin's metadata**, because they don't know `key_value`.
  - They log that only at `DEBUG`, and load the plugin with empty metadata: no settings form, no cache resource, and no version check.
  - `apply()` still runs, with the `config` values stored in the plugin's entry. Its events are ignored; only their legacy copies in `warnings` are logged.
  - To target v0.6.4–v0.6.7, remove the `key_value` fields from [`metadata.rs`](src/metadata.rs) and lower `MIN_DECKWATCH_VERSION` to `"0.6.4"`.
- **`main` builds from `278de71` up to, but not including, `f836fec`** parse the metadata but still call themselves 0.6.7, so the version check refuses to load the plugin.

## Layout

| Path | What |
|---|---|
| `src/lib.rs` | Module overview |
| `src/metadata.rs` | `metadata()`: settings form, resource, capabilities, keys, event codes |
| `src/config.rs` | Reading settings, with defaults and validation |
| `src/apply.rs` | `apply()` |
| `src/resources.rs` | `provision()`, `deprovision()`, the webhook |
| `src/exports.rs` | The four `#[plugin_fn]` exports (wasm32 only), the only code that calls the extism host |
| `.cargo/config.toml` | Makes `wasm32-unknown-unknown` the default build target |
| `rust-toolchain.toml` | Pins Rust 1.99 with the wasm32 target |

## Develop

```bash
make check   # fmt, clippy (host and wasm32), tests: what CI runs
make test    # unit tests on the host
make build   # dist/plugin.wasm
```

- **The plugin's logic is plain Rust, so `cargo test` runs on the host.** Only `src/exports.rs` touches the extism host, and it's compiled for wasm32 only. `.cargo/config.toml` makes wasm32 the default target, so host commands pass `--target "$(rustc -vV | sed -n 's/host: //p')"`; the Makefile does this for you.
- **`crate-type = ["cdylib"]`:** don't add `"rlib"` (see the SDK README).

## Release

Bump `version` in `Cargo.toml`, merge, then push a matching tag (`vX.Y.Z`).

`.github/workflows/release.yml` then:
- checks the tag against `Cargo.toml`;
- builds with the pinned toolchain and `--locked`;
- creates the GitHub Release with `plugin.wasm` and `SHA256SUMS`.

Put that checksum in the plugin's `sha256` setting so deckwatch rejects any other binary.

## Using this as a template

1. Rename the package in `Cargo.toml` and `metadata::NAME`, and register the plugin under the same name.
2. Change the opt-in annotation (`apply::OPT_IN`), the config keys (`metadata::keys`) and the event codes (`metadata::codes`, keeping the `<name>.` prefix).
3. Keep the split between plain logic and `exports.rs`, so everything stays testable on the host.
4. Pin the SDK by tag, never by branch.

## Private repositories

If your plugin repository is private, add a git token under `git_token_secrets` in deckwatch's settings, and reference it by name with `"token_secret": "<name>"` in the plugin's entry. deckwatch sends it as a bearer token when fetching the binary.
