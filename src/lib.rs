//! # deckwatch-plugin-example
//!
//! A reference plugin that uses every feature of
//! [deckwatch-plugin-sdk](https://github.com/alexconrey/deckwatch-plugin-sdk)
//! 0.10. Read it alongside the SDK README. Each module says which SDK features
//! it shows, and this repository's README maps every feature to the code that
//! uses it.
//!
//! | Module | What it holds |
//! |---|---|
//! | [`metadata`] | What the plugin declares: the result of `metadata()` |
//! | [`config`] | Reading the operator's settings |
//! | [`apply`] | Per-Deployment injection: the result of `apply()` |
//! | [`resources`] | The provisionable "cache": `provision()`, `deprovision()` and a webhook |
//! | `exports` | The four `#[plugin_fn]` exports (wasm32 only) |
//!
//! Only `exports` talks to the extism host (config, clock, HTTP, logs). It
//! passes what it reads to the plain functions in the other modules, so
//! `cargo test` covers all the plugin's logic on the host.

pub mod apply;
pub mod config;
pub mod metadata;
pub mod resources;

#[cfg(target_arch = "wasm32")]
mod exports;
