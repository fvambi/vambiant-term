//! The provider trait and the Messages-shaped types every adapter maps to
//! (ADR-0005, docs/04 §1, §5, §6).

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

/// Per-model capability table.
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
    /// Tool use.
    pub tools: bool,
    /// Fill-in-the-middle completion (local models).
    pub fim: bool,
    /// Server-side token counting endpoint.
    pub count_tokens: bool,
}

/// Who said it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)] // the two API roles
pub enum Role {
    User,
    Assistant,
}

/// One content block, Anthropic-shaped.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub enum Content {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

/// One turn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub struct Message {
    pub role: Role,
    pub content: Vec<Content>,
}

impl Message {
    /// A user turn with one text block.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![Content::Text { text: text.into() }],
        }
    }

    /// An assistant turn with one text block.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![Content::Text { text: text.into() }],
        }
    }
}

/// A tool the model may call; `input_schema` is JSON Schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// A completion request. Sampling fields are dropped by adapters whose
/// capability table says so — never forwarded blindly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub struct Request {
    pub model: String,
    #[serde(default)]
    pub system: Option<String>,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub tools: Vec<Tool>,
    pub max_tokens: u32,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub stop: Vec<String>,
}

/// Token accounting for one request, cumulative.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

/// Why the model stopped.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    StopSequence,
    Other(String),
}

/// One streamed increment (docs/04 §6).
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub enum Chunk {
    TextDelta(String),
    /// A tool call started; `input` accumulates from [`Chunk::ToolInputDelta`].
    ToolUseStart {
        id: String,
        name: String,
    },
    /// Partial JSON of the current tool call's input. Accumulate; never parse per event.
    ToolInputDelta {
        id: String,
        json_fragment: String,
    },
    ThinkingDelta(String),
    Usage(Usage),
    Done(StopReason),
}

/// The full result once the stream ends.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub struct Completion {
    pub content: Vec<Content>,
    pub usage: Usage,
    pub stop: StopReason,
}

/// What can go wrong talking to a provider. Messages carry what the user
/// needs to act on and never a credential.
#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub enum ProviderError {
    #[error("no API key for profile `{profile}`: run `vterm ai key set {profile}`")]
    MissingKey { profile: String },
    #[error("{provider} at {url}: {detail}")]
    Transport {
        provider: String,
        url: String,
        detail: String,
    },
    #[error("{provider} returned HTTP {status}: {body}")]
    Status {
        provider: String,
        status: u16,
        body: String,
    },
    #[error("{provider} sent an unreadable event: {detail}")]
    Protocol { provider: String, detail: String },
    #[error("model `{model}` is not one of profile `{profile}`'s models")]
    UnknownModel { profile: String, model: String },
}

/// A model provider. Synchronous by design: the daemon runs each request
/// on its own thread and streams chunks through `on_chunk`.
pub trait Provider: Send + Sync {
    /// Stable profile name from `providers.toml`.
    fn name(&self) -> &str;
    /// Capabilities for a model id belonging to this provider.
    fn caps(&self, model: &str) -> Option<ProviderCaps>;
    /// Streams a completion; returns the assembled result when it ends.
    fn stream(
        &self,
        req: &Request,
        on_chunk: &mut dyn FnMut(Chunk),
    ) -> Result<Completion, ProviderError>;
    /// Model ids the endpoint reports, when it has a listing endpoint.
    fn models(&self) -> Result<Vec<String>, ProviderError>;
}

/// Accumulates chunks into a [`Completion`]: text runs and tool calls
/// with their JSON input assembled from fragments.
#[derive(Default)]
pub struct Assembler {
    content: Vec<Content>,
    tool_json: Vec<(String, String, String)>, // id, name, json so far
    usage: Usage,
    stop: Option<StopReason>,
}

impl Assembler {
    /// Folds one chunk in.
    pub fn push(&mut self, chunk: &Chunk) {
        match chunk {
            Chunk::TextDelta(t) => {
                if let Some(Content::Text { text }) = self.content.last_mut() {
                    text.push_str(t);
                } else {
                    self.content.push(Content::Text { text: t.clone() });
                }
            }
            Chunk::ToolUseStart { id, name } => {
                self.tool_json
                    .push((id.clone(), name.clone(), String::new()));
            }
            Chunk::ToolInputDelta { id, json_fragment } => {
                if let Some(t) = self.tool_json.iter_mut().find(|t| &t.0 == id) {
                    t.2.push_str(json_fragment);
                }
            }
            Chunk::ThinkingDelta(_) => {}
            Chunk::Usage(u) => self.usage = *u,
            Chunk::Done(s) => self.stop = Some(s.clone()),
        }
    }

    /// Parses each tool call's accumulated JSON and returns the result.
    pub fn finish(mut self) -> Completion {
        for (id, name, json) in self.tool_json.drain(..) {
            let input = if json.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(&json).unwrap_or(serde_json::Value::String(json))
            };
            self.content.push(Content::ToolUse { id, name, input });
        }
        Completion {
            content: self.content,
            usage: self.usage,
            stop: self.stop.unwrap_or(StopReason::EndTurn),
        }
    }
}
