//! Anthropic Messages API (`POST /v1/messages`), streaming. Also the path
//! for llama.cpp's native `/v1/messages`. docs/04 §1, §6.

use crate::http::Http;
use crate::provider::{
    Assembler, Chunk, Completion, Content, Message, Provider, ProviderCaps, ProviderError, Request,
    Role, SamplingSupport, StopReason, Usage,
};
use crate::sse::Events;

/// One Anthropic-shaped profile: api.anthropic.com or a llama.cpp
/// server speaking `/v1/messages`.
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub struct Anthropic {
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub models: Vec<String>,
    /// Models whose sampling parameters must stay at their defaults.
    pub default_only_sampling: Vec<String>,
    pub http: Http,
}

/// The `anthropic-version` header value this client was written against.
pub const API_VERSION: &str = "2023-06-01";

impl Anthropic {
    fn key(&self) -> Result<&str, ProviderError> {
        self.api_key
            .as_deref()
            .ok_or_else(|| ProviderError::MissingKey {
                profile: self.name.clone(),
            })
    }

    /// The wire body. Sampling params are dropped where the caps say
    /// `DefaultOnly`, never forwarded blindly (docs/04 §5).
    pub fn body(&self, req: &Request) -> serde_json::Value {
        let caps = self
            .caps(&req.model)
            .unwrap_or_else(|| self.caps_for(&req.model));
        let messages: Vec<serde_json::Value> = req
            .messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
                    "content": m.content,
                })
            })
            .collect();
        let mut body = serde_json::json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "messages": messages,
            "stream": true,
        });
        if let Some(s) = &req.system {
            body["system"] = serde_json::Value::String(s.clone());
        }
        if !req.tools.is_empty() {
            body["tools"] = serde_json::to_value(&req.tools).unwrap_or_default();
        }
        if !req.stop.is_empty() {
            body["stop_sequences"] = serde_json::to_value(&req.stop).unwrap_or_default();
        }
        if caps.temperature == SamplingSupport::Full
            && let Some(t) = req.temperature
        {
            body["temperature"] = serde_json::json!(t);
        }
        if caps.top_p == SamplingSupport::Full
            && let Some(p) = req.top_p
        {
            body["top_p"] = serde_json::json!(p);
        }
        body
    }

    fn caps_for(&self, model: &str) -> ProviderCaps {
        let default_only = self.default_only_sampling.iter().any(|m| m == model);
        let s = if default_only {
            SamplingSupport::DefaultOnly
        } else {
            SamplingSupport::Full
        };
        ProviderCaps {
            temperature: s,
            top_p: s,
            top_k: s,
            streaming: true,
            prompt_caching: true,
            tools: true,
            fim: false,
            count_tokens: true,
        }
    }

    /// One SSE event → zero or more chunks, updating cumulative usage and
    /// the tool call currently receiving JSON fragments. Unknown event
    /// types produce nothing, by contract.
    fn chunks_for(
        &self,
        kind: &str,
        v: &serde_json::Value,
        usage: &mut Usage,
        current_tool: &mut Option<String>,
    ) -> Result<Vec<Chunk>, ProviderError> {
        Ok(match kind {
            "message_start" => {
                let u = &v["message"]["usage"];
                usage.input_tokens = u["input_tokens"].as_u64().unwrap_or(0);
                usage.cache_read_tokens = u["cache_read_input_tokens"].as_u64().unwrap_or(0);
                usage.cache_write_tokens = u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                vec![Chunk::Usage(*usage)]
            }
            "content_block_start" => {
                let cb = &v["content_block"];
                if cb["type"] == "tool_use" {
                    let id = cb["id"].as_str().unwrap_or_default().to_owned();
                    *current_tool = Some(id.clone());
                    vec![Chunk::ToolUseStart {
                        id,
                        name: cb["name"].as_str().unwrap_or_default().to_owned(),
                    }]
                } else if let Some(t) = cb["text"].as_str().filter(|t| !t.is_empty()) {
                    vec![Chunk::TextDelta(t.to_owned())]
                } else {
                    vec![]
                }
            }
            "content_block_delta" => {
                let d = &v["delta"];
                match d["type"].as_str().unwrap_or_default() {
                    "text_delta" => vec![Chunk::TextDelta(
                        d["text"].as_str().unwrap_or_default().to_owned(),
                    )],
                    "input_json_delta" => vec![Chunk::ToolInputDelta {
                        id: current_tool.clone().unwrap_or_default(),
                        json_fragment: d["partial_json"].as_str().unwrap_or_default().to_owned(),
                    }],
                    "thinking_delta" => {
                        vec![Chunk::ThinkingDelta(
                            d["thinking"].as_str().unwrap_or_default().to_owned(),
                        )]
                    }
                    _ => vec![], // signature_delta and whatever comes next: tolerated
                }
            }
            "content_block_stop" => {
                *current_tool = None;
                vec![]
            }
            "message_delta" => {
                if let Some(o) = v["usage"]["output_tokens"].as_u64() {
                    usage.output_tokens = o;
                }
                let mut out = vec![Chunk::Usage(*usage)];
                if let Some(s) = v["delta"]["stop_reason"].as_str() {
                    out.push(Chunk::Done(stop_reason(s)));
                }
                out
            }
            "error" => {
                return Err(ProviderError::Status {
                    provider: self.name.clone(),
                    status: 0,
                    body: v["error"]["message"]
                        .as_str()
                        .unwrap_or("stream error")
                        .to_owned(),
                });
            }
            _ => vec![], // unknown event types are tolerated by contract
        })
    }
}

impl Provider for Anthropic {
    fn name(&self) -> &str {
        &self.name
    }

    fn caps(&self, model: &str) -> Option<ProviderCaps> {
        self.models
            .iter()
            .any(|m| m == model)
            .then(|| self.caps_for(model))
    }

    fn stream(
        &self,
        req: &Request,
        on_chunk: &mut dyn FnMut(Chunk),
    ) -> Result<Completion, ProviderError> {
        let key = self.key()?;
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let body = self.body(req);
        let reader = self.http.post_json(
            &self.name,
            &url,
            &[
                ("x-api-key", key),
                ("anthropic-version", API_VERSION),
                ("accept", "text/event-stream"),
            ],
            &body,
        )?;
        let mut asm = Assembler::default();
        let mut usage = Usage::default();
        let mut current_tool: Option<String> = None;
        for event in Events::new(reader) {
            let event = event.map_err(|e| ProviderError::Transport {
                provider: self.name.clone(),
                url: url.clone(),
                detail: e.to_string(),
            })?;
            let v: serde_json::Value = match serde_json::from_str(&event.data) {
                Ok(v) => v,
                Err(_) if event.name == "ping" || event.data.is_empty() => continue,
                Err(e) => {
                    return Err(ProviderError::Protocol {
                        provider: self.name.clone(),
                        detail: format!("{} ({e})", event.name),
                    });
                }
            };
            let kind = v["type"].as_str().unwrap_or(event.name.as_str());
            let chunks = self.chunks_for(kind, &v, &mut usage, &mut current_tool)?;
            for c in chunks {
                asm.push(&c);
                on_chunk(c);
            }
        }
        Ok(asm.finish())
    }

    fn models(&self) -> Result<Vec<String>, ProviderError> {
        let key = self.key()?;
        let url = format!("{}/v1/models", self.base_url.trim_end_matches('/'));
        let v = self.http.get_json(
            &self.name,
            &url,
            &[("x-api-key", key), ("anthropic-version", API_VERSION)],
        )?;
        Ok(v["data"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default())
    }
}

fn stop_reason(s: &str) -> StopReason {
    match s {
        "end_turn" => StopReason::EndTurn,
        "max_tokens" => StopReason::MaxTokens,
        "tool_use" => StopReason::ToolUse,
        "stop_sequence" => StopReason::StopSequence,
        other => StopReason::Other(other.to_owned()),
    }
}

/// Convenience for tests and callers assembling a one-shot request.
pub fn one_shot(model: &str, system: Option<&str>, prompt: &str, max_tokens: u32) -> Request {
    Request {
        model: model.to_owned(),
        system: system.map(str::to_owned),
        messages: vec![Message::user(prompt)],
        tools: vec![],
        max_tokens,
        temperature: None,
        top_p: None,
        stop: vec![],
    }
}

#[allow(dead_code)]
fn _content_is_serializable(c: &Content) -> serde_json::Value {
    serde_json::to_value(c).unwrap_or_default()
}
