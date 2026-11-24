//! The four `#[plugin_fn]` exports, compiled for wasm32 only.
//!
//! This is the only module that calls the extism host. It reads config,
//! the clock and the network, and passes the results to the plain functions
//! in the other modules.
//!
//! SDK and PDK features: `deckwatch_plugin_sdk::now()` (the host's clock;
//! `std::time::SystemTime::now()` panics in wasm32), `extism_pdk::config::get`,
//! `extism_pdk::http::request` (only to hosts in the plugin's
//! `allowed_hosts`), and the PDK log macros.
//!
//! Logging: deckwatch keeps PDK log lines since deckwatch `36e6637` (target
//! `extism::pdk`). Their only field is extism's per-call instance id, so
//! each message names the plugin. `apply` runs on every 10-second reconcile
//! for applications with a provisioned resource, so routine lines use
//! `debug!`.

use deckwatch_plugin_sdk::{
    now, PluginContext, PluginMetadata, PluginResult, ResourceDeprovisionRequest,
    ResourceDeprovisionResult, ResourceProvisionRequest, ResourceProvisionResult,
};
use extism_pdk::{config, debug, http, info, plugin_fn, FnResult, Json};

use crate::config::Config;
use crate::metadata::codes;
use crate::resources::{Observed, CACHE_PORT};

fn load_config() -> Config {
    Config::from_lookup(|key| config::get(key).ok().flatten())
}

#[plugin_fn]
pub fn metadata() -> FnResult<Json<PluginMetadata>> {
    Ok(Json(crate::metadata::metadata()))
}

#[plugin_fn]
pub fn apply(Json(ctx): Json<PluginContext>) -> FnResult<Json<PluginResult>> {
    debug!(
        "example: apply for {}/{}",
        ctx.namespace, ctx.deployment_name
    );
    Ok(Json(crate::apply::apply(&ctx, &load_config())))
}

#[plugin_fn]
pub fn provision(
    Json(req): Json<ResourceProvisionRequest>,
) -> FnResult<Json<ResourceProvisionResult>> {
    let cfg = load_config();
    // The cache is a sidecar, so it's ready as soon as deckwatch injects it.
    let observed = Observed::Ready {
        host: "127.0.0.1".into(),
        port: CACHE_PORT,
    };
    let mut result = crate::resources::provision(&req, &observed);

    // HTTP error statuses come back as values and become a warning. A call
    // that can't be made at all (host not in allowed_hosts, bad URL, DNS,
    // connection, timeout) traps in the extism host: the whole provision()
    // call fails and deckwatch reports provisioning as failed. Err below only
    // covers plugin-side problems, such as encoding the request.
    if let Some((request, body)) = crate::resources::webhook_request(&req, &result, &cfg, now()) {
        match http::request::<Vec<u8>>(&request, Some(body)) {
            Ok(response) if (200..300).contains(&response.status_code()) => {
                info!("example: webhook answered HTTP {}", response.status_code());
            }
            Ok(response) => {
                result
                    .warn(
                        codes::WEBHOOK_FAILED,
                        format!("the webhook answered HTTP {}", response.status_code()),
                    )
                    .retryable = true;
            }
            Err(err) => {
                let event = result.warn(
                    codes::WEBHOOK_FAILED,
                    format!("calling the webhook failed: {err}"),
                );
                event.retryable = true;
            }
        }
    }
    Ok(Json(result))
}

#[plugin_fn]
pub fn deprovision(
    Json(req): Json<ResourceDeprovisionRequest>,
) -> FnResult<Json<ResourceDeprovisionResult>> {
    info!(
        "example: deprovision {} for {}/{}",
        req.resource_id, req.namespace, req.application_name
    );
    Ok(Json(crate::resources::deprovision(&req)))
}
