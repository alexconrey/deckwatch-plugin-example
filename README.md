# deckwatch-plugin-example

A reference implementation of a [deckwatch](https://github.com/kore/deckwatch) plugin, demonstrating both injection patterns:

- **Env var injection** — inject environment variables into the primary container when a deployment is annotated with `myorg.io/db: "enabled"` (external service provider pattern)
- **Sidecar injection** — inject a sidecar container into the pod when annotated with `myorg.io/mesh: "enabled"` (internal cluster service pattern)

Use this repo as a starting point for your own organization-specific plugin.

---

## How it works

Deckwatch fetches this plugin's compiled WASM binary from GitHub Releases and executes it for every deployment create/update. The plugin receives the deployment's namespace, name, annotations, and labels, and returns what to inject. Nothing is injected unless the deployment explicitly opts in via annotation.

```
Deckwatch                          Plugin (WASM)
────────────────────────────────────────────────────────
deployment create/update request
  │
  ├─ build Deployment object with user-supplied values
  │
  ├─ load plugins from AppState (fetched at startup)
  │
  ├─ call plugin.apply(PluginContext) ──────────────────►  inspect annotations
  │                                                         return env_vars + sidecars
  │  ◄────────────────────────────────────────────────────
  │
  ├─ merge plugin results (user values always win)
  │
  └─ apply to Kubernetes API
```

---

## Repository layout

```
.
├── src/
│   └── lib.rs                  # Plugin logic — start here
├── .cargo/
│   └── config.toml             # Sets wasm32-unknown-unknown as default build target
├── .github/
│   └── workflows/
│       └── release.yml         # Builds and publishes plugin.wasm on git tag
├── dist/
│   └── .gitkeep                # Artifact directory (plugin.wasm goes here after build)
├── Cargo.toml
└── Makefile                    # make build / make test / make clean
```

---

## Using this as a template

### 1. Fork or clone

```bash
git clone https://github.com/alexconrey/deckwatch-plugin-example
cd deckwatch-plugin-example
```

### 2. Replace the annotation keys

The example uses `myorg.io/db` and `myorg.io/mesh`. Replace these with your organization's domain:

```rust
// src/lib.rs — change the annotation key strings
if ctx.annotations.get("yourorg.io/db").map(|v| v == "enabled").unwrap_or(false) {
```

### 3. Replace the injected values

Update `DATABASE_URL`, image references, env vars, and resource sizes to match your environment.

### 4. Update `Cargo.toml`

Change the package name and description. Point the `deckwatch-plugin-sdk` dependency at the tag you want to pin to.

### 5. Build and test

```bash
# Build the WASM binary
make build

# Run unit tests on the host (not WASM — see Testing section)
make test
```

---

## Building

Prerequisites: Rust stable + `wasm32-unknown-unknown` target.

```bash
rustup target add wasm32-unknown-unknown
make build
# outputs: dist/plugin.wasm
```

Or without Make:

```bash
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/deckwatch_plugin_example.wasm dist/plugin.wasm
```

---

## Testing

Unit tests run against the host Rust target because compiled WASM requires a runtime (wasmtime, browser, or WASI) to execute. The `apply_inner` function extracted in `src/lib.rs` is the testable pure function:

```bash
make test
# equivalent: cargo test --target $(rustc -vV | sed -n 's/host: //p')
```

Tests cover:
- No annotations → no injection
- `myorg.io/db: enabled` → `DATABASE_URL` and `DB_POOL_SIZE` injected
- `myorg.io/mesh: enabled` → `mesh-agent` sidecar injected

---

## Releasing

Push a version tag to trigger the GitHub Actions release workflow, which builds the WASM binary and attaches it as a release asset:

```bash
git tag v1.0.0
git push origin v1.0.0
```

The workflow produces `plugin.wasm` as a release asset at:
```
https://github.com/alexconrey/deckwatch-plugin-example/releases/download/v1.0.0/plugin.wasm
```

---

## Configuring in deckwatch

Once released, register the plugin in deckwatch's Settings → Plugins section:

```json
{
  "plugins": [
    {
      "name": "example-plugin",
      "enabled": true,
      "source": {
        "type": "github",
        "repo": "alexconrey/deckwatch-plugin-example",
        "ref": "v1.0.0",
        "path": "plugin.wasm",
        "use_release": true
      }
    }
  ]
}
```

The `name` field becomes the annotation key suffix. The plugin will track what it injected under:
- `deckwatch.plugin-env/example-plugin` — env vars it injected
- `deckwatch.plugin-sidecar/example-plugin` — sidecars it injected

---

## Opting deployments in

Add annotations to any deployment you want the plugin to act on. In deckwatch, open the deployment → YAML editor:

```yaml
metadata:
  annotations:
    myorg.io/db: "enabled"     # triggers env var injection
    myorg.io/mesh: "enabled"   # triggers sidecar injection
```

Or use `kubectl`:

```bash
kubectl annotate deployment my-app myorg.io/db=enabled -n my-namespace
```

---

## Private repositories

If your plugin repo is private, configure a `git_token_secrets` entry in deckwatch and reference it by name in the plugin config:

```json
{
  "plugins": [
    {
      "name": "example-plugin",
      "enabled": true,
      "source": { ... },
      "token_secret": "my-github-token"
    }
  ]
}
```

See [deckwatch PLUGINS.md](https://github.com/kore/deckwatch/blob/main/docs/PLUGINS.md) for full token configuration instructions.

---

## SDK reference

This plugin depends on [deckwatch-plugin-sdk](https://github.com/alexconrey/deckwatch-plugin-sdk) for the shared `PluginContext` and `PluginResult` types. See the SDK README for full type documentation and testing guidance.
