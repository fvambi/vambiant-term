//! Provider layer: one Messages-shaped [`Provider`](provider::Provider)
//! trait over Anthropic, OpenAI, OpenAI-compatible endpoints and local
//! runtimes (ADR-0005).
//!
//! Capability tables, not blind pass-through: Anthropic returns HTTP 400
//! for non-default `temperature`/`top_p` on recent Opus models, so every
//! provider declares [`provider::SamplingSupport`] and the router strips
//! what a target cannot take. **No model id is hardcoded here** — ids live
//! in `providers.toml`.

pub mod anthropic;
pub mod compat;
pub mod cost;
pub mod local;
pub mod openai;
pub mod provider;
pub mod route;
