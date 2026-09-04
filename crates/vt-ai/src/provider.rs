//! The provider trait and its capability table.

use serde::{Deserialize, Serialize};

/// Whether a provider accepts a sampling parameter at all, and if so
/// whether non-default values are safe to send.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SamplingSupport {
    /// Parameter accepted with any value.
    Full,
    /// Parameter must be omitted or left at its default (Anthropic Opus 4.7+
    /// returns 400 otherwise).
    DefaultOnly,
    /// Parameter rejected outright.
    Unsupported,
}

/// Per-model capability table, resolved at startup by `vterm ai doctor`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCaps {
    /// `temperature` support.
    pub temperature: SamplingSupport,
    /// `top_p` support.
    pub top_p: SamplingSupport,
    /// `top_k` support.
    pub top_k: SamplingSupport,
    /// Streaming responses.
    pub streaming: bool,
    /// Prompt caching with breakpoints.
    pub prompt_caching: bool,
    /// Fill-in-the-middle completion (local models).
    pub fim: bool,
    /// Server-side token counting endpoint.
    pub count_tokens: bool,
}

/// A model provider. Implementations land in M-AI.
pub trait Provider: Send + Sync {
    /// Stable profile name from `providers.toml`.
    fn name(&self) -> &str;
    /// Capabilities for a model id belonging to this provider.
    fn caps(&self, model: &str) -> Option<ProviderCaps>;
}
