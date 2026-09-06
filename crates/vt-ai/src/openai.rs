//! OpenAI Chat Completions and everything that speaks it: OpenAI itself,
//! OpenRouter, Groq, vLLM, LM Studio, Ollama's `/v1`. Only the safe subset
//! from docs/04 §2 is sent; `tools` best-effort.

use crate::http::Http;
use crate::provider::{
    Assembler, Chunk, Completion, Content, Provider, ProviderCaps, ProviderError, Request, Role,
    SamplingSupport, StopReason, Usage,
};
use crate::sse::Events;

/// One chat-completions profile: OpenAI, or any compatible endpoint.
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub struct OpenAiChat {
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub models: Vec<String>,
    /// Endpoints that require a key (openai.com); local ones do not.
    pub key_required: bool,
    pub http: Http,
}

impl OpenAiChat {
    fn auth_header(&self) -> Result<Option<String>, ProviderError> {
        match (&self.api_key, self.key_required) {
            (Some(k), _) => Ok(Some(format!("Bearer {k}"))),
            (None, false) => Ok(None),
            (None, true) => Err(ProviderError::MissingKey {
                profile: self.name.clone(),
            }),
        }
    }

    /// Messages-shaped request → chat-completions body. Tool results become
    /// `tool` role messages; tool uses become `tool_calls` on the assistant.
    pub fn body(&self, req: &Request) -> serde_json::Value {
        let mut messages: Vec<serde_json::Value> = Vec::new();
        if let Some(s) = &req.system {
            messages.push(serde_json::json!({ "role": "system", "content": s }));
        }
        for m in &req.messages {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            let mut text = String::new();
            let mut tool_calls = Vec::new();
            for c in &m.content {
                match c {
                    Content::Text { text: t } => text.push_str(t),
                    Content::ToolUse { id, name, input } => tool_calls.push(serde_json::json!({
                        "id": id, "type": "function",
                        "function": { "name": name, "arguments": input.to_string() }
                    })),
                    Content::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => messages.push(serde_json::json!({
                        "role": "tool", "tool_call_id": tool_use_id, "content": content
                    })),
                }
            }
            if !text.is_empty() || !tool_calls.is_empty() {
                let mut msg = serde_json::json!({ "role": role, "content": text });
                if !tool_calls.is_empty() {
                    msg["tool_calls"] = serde_json::Value::Array(tool_calls);
                }
                messages.push(msg);
            }
        }
        let mut body = serde_json::json!({
            "model": req.model,
            "messages": messages,
            "max_tokens": req.max_tokens,
            "stream": true,
        });
        if let Some(t) = req.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(p) = req.top_p {
            body["top_p"] = serde_json::json!(p);
        }
        if !req.stop.is_empty() {
            body["stop"] = serde_json::to_value(&req.stop).unwrap_or_default();
        }
        if !req.tools.is_empty() {
            body["tools"] = serde_json::Value::Array(
                req.tools
                    .iter()
                    .map(|t| {
                        serde_json::json!({ "type": "function", "function": {
                            "name": t.name, "description": t.description, "parameters": t.input_schema
                        }})
                    })
                    .collect(),
            );
        }
        body
    }
}

/// Chunks for one `tool_calls` delta. Calls arrive by index; the id is
/// only on the first fragment, so `tool_ids` remembers index → id.
fn tool_call_chunks(call: &serde_json::Value, tool_ids: &mut Vec<String>) -> Vec<Chunk> {
    let mut out = Vec::new();
    let index = usize::try_from(call["index"].as_u64().unwrap_or(0)).unwrap_or(0);
    if let Some(id) = call["id"].as_str() {
        if tool_ids.len() <= index {
            tool_ids.resize(index + 1, String::new());
        }
        id.clone_into(&mut tool_ids[index]);
        out.push(Chunk::ToolUseStart {
            id: id.to_owned(),
            name: call["function"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        });
    }
    if let Some(args) = call["function"]["arguments"]
        .as_str()
        .filter(|a| !a.is_empty())
    {
        out.push(Chunk::ToolInputDelta {
            id: tool_ids.get(index).cloned().unwrap_or_default(),
            json_fragment: args.to_owned(),
        });
    }
    out
}

impl Provider for OpenAiChat {
    fn name(&self) -> &str {
        &self.name
    }

    fn caps(&self, model: &str) -> Option<ProviderCaps> {
        self.models
            .iter()
            .any(|m| m == model)
            .then_some(ProviderCaps {
                temperature: SamplingSupport::Full,
                top_p: SamplingSupport::Full,
                top_k: SamplingSupport::Unsupported,
                streaming: true,
                prompt_caching: false,
                tools: true,
                fim: false,
                count_tokens: false,
            })
    }

    fn stream(
        &self,
        req: &Request,
        on_chunk: &mut dyn FnMut(Chunk),
    ) -> Result<Completion, ProviderError> {
        let auth = self.auth_header()?;
        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );
        let mut headers: Vec<(&str, &str)> = vec![("accept", "text/event-stream")];
        if let Some(a) = &auth {
            headers.push(("authorization", a.as_str()));
        }
        let reader = self
            .http
            .post_json(&self.name, &url, &headers, &self.body(req))?;
        let mut asm = Assembler::default();
        let mut usage = Usage::default();
        // tool_calls arrive by index; map index → id once seen.
        let mut tool_ids: Vec<String> = Vec::new();
        let mut finished = false;
        for event in Events::new(reader) {
            let event = event.map_err(|e| ProviderError::Transport {
                provider: self.name.clone(),
                url: url.clone(),
                detail: e.to_string(),
            })?;
            if event.data.trim() == "[DONE]" {
                break;
            }
            let v: serde_json::Value =
                serde_json::from_str(&event.data).map_err(|e| ProviderError::Protocol {
                    provider: self.name.clone(),
                    detail: e.to_string(),
                })?;
            if let Some(err) = v.get("error") {
                return Err(ProviderError::Status {
                    provider: self.name.clone(),
                    status: 0,
                    body: err["message"].as_str().unwrap_or("stream error").to_owned(),
                });
            }
            if let Some(u) = v.get("usage").filter(|u| !u.is_null()) {
                usage.input_tokens = u["prompt_tokens"].as_u64().unwrap_or(usage.input_tokens);
                usage.output_tokens = u["completion_tokens"]
                    .as_u64()
                    .unwrap_or(usage.output_tokens);
                let c = Chunk::Usage(usage);
                asm.push(&c);
                on_chunk(c);
            }
            let Some(choice) = v["choices"].as_array().and_then(|a| a.first()) else {
                continue;
            };
            let delta = &choice["delta"];
            if let Some(t) = delta["content"].as_str().filter(|t| !t.is_empty()) {
                let c = Chunk::TextDelta(t.to_owned());
                asm.push(&c);
                on_chunk(c);
            }
            if let Some(calls) = delta["tool_calls"].as_array() {
                for call in calls {
                    for c in tool_call_chunks(call, &mut tool_ids) {
                        asm.push(&c);
                        on_chunk(c);
                    }
                }
            }
            if let Some(reason) = choice["finish_reason"].as_str() {
                let stop = match reason {
                    "stop" => StopReason::EndTurn,
                    "length" => StopReason::MaxTokens,
                    "tool_calls" => StopReason::ToolUse,
                    other => StopReason::Other(other.to_owned()),
                };
                let c = Chunk::Done(stop);
                asm.push(&c);
                on_chunk(c);
                finished = true;
            }
        }
        if !finished {
            let c = Chunk::Done(StopReason::EndTurn);
            asm.push(&c);
            on_chunk(c);
        }
        Ok(asm.finish())
    }

    fn models(&self) -> Result<Vec<String>, ProviderError> {
        let auth = self.auth_header()?;
        let url = format!("{}/v1/models", self.base_url.trim_end_matches('/'));
        let mut headers: Vec<(&str, &str)> = Vec::new();
        if let Some(a) = &auth {
            headers.push(("authorization", a.as_str()));
        }
        let v = self.http.get_json(&self.name, &url, &headers)?;
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
