//! Provider layer: one Messages-shaped [`Provider`](provider::Provider)
//! trait over Anthropic, OpenAI, OpenAI-compatible endpoints and local
//! runtimes (ADR-0005).
//!
//! Capability tables, not blind pass-through: Anthropic returns HTTP 400
//! for non-default `temperature`/`top_p` on recent Opus models, so every
//! provider declares [`provider::SamplingSupport`] and the request builder
//! drops what a target cannot take. **No model id is hardcoded here** — ids
//! live in `providers.toml` ([`route::DEFAULT_FILE`] is the shipped data).
//!
//! Synchronous by design (the daemon runs a request per thread); docs/04
//! sketched async signatures, this is the same contract without a runtime.

pub mod anthropic;
pub mod compat;
pub mod cost;
pub mod http;
pub mod keychain;
pub mod local;
pub mod openai;
pub mod provider;
pub mod resilience;
pub mod route;
pub mod sse;

pub use provider::{Chunk, Completion, Content, Message, Provider, ProviderError, Request, Usage};
