//! Codex adapter (docs/03 §5): `codex app-server` JSON-RPC over
//! WebSocket-over-Unix, as verified in M0 and M3 (docs/10 §7).
//!
//! The daemon is a *second* client on the session's app-server; the user's
//! TUI is the first (`codex --remote unix://…`). After `thread/resume` on a
//! loaded thread the second client receives the whole turn stream including
//! approval requests, and whichever client answers first wins; the other
//! sees `serverRequest/resolved`. Hooks are not used: Codex cannot be told to
//! trust them without the interactive `/hooks` browser.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};
use vt_proto::agent::{AgentEvent, AgentKind, AgentState};
use vt_proto::approval::{ApprovalId, ApprovalRequest, Decision};
use vt_proto::session::Capabilities;

use crate::hooks::{Ingested, Warning};
use crate::ws::{WebSocket, Writer};

/// Capabilities of a Codex session observed through app-server.
pub const CAPABILITIES: Capabilities = Capabilities {
    structured_events: true,
    permission_control: true,
    cost_reporting: true,
    interrupt: true,
    fork: true,
    resume: true,
};

/// The adapter kind.
pub const KIND: AgentKind = AgentKind::Codex;

/// The sending half of an app-server connection: cloneable, usable from any
/// thread (the inbox answers approvals while the observer keeps reading).
#[derive(Clone, Debug)]
pub struct AppServerHandle {
    writer: Arc<Mutex<Writer>>,
    next_id: Arc<AtomicU64>,
}

impl AppServerHandle {
    fn send(&self, v: &Value) -> std::io::Result<()> {
        self.writer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send_text(&v.to_string())
    }

    /// Send a request; returns its id. Messages omit `"jsonrpc"` on this wire.
    pub fn request(&self, method: &str, params: &Value) -> std::io::Result<u64> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.send(&json!({ "method": method, "id": id, "params": params }))?;
        Ok(id)
    }

    /// Send a notification.
    pub fn notify(&self, method: &str, params: &Value) -> std::io::Result<()> {
        self.send(&json!({ "method": method, "params": params }))
    }

    /// Answer a server-initiated request.
    pub fn respond(&self, id: &Value, result: &Value) -> std::io::Result<()> {
        self.send(&json!({ "id": id, "result": result }))
    }

    /// Answer an approval request raised by [`ingest`].
    pub fn answer_approval(
        &self,
        approval: &ApprovalId,
        decision: &Decision,
    ) -> std::io::Result<()> {
        let Some(rpc_id) = rpc_id_of(approval) else {
            return Err(std::io::Error::other(format!(
                "approval {} carries no app-server request id",
                approval.0
            )));
        };
        self.respond(&rpc_id, &approval_result(decision))
    }

    /// Close the connection.
    pub fn close(&self) {
        self.writer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .close();
    }
}

/// A JSON-RPC connection to an app-server (reading half).
#[derive(Debug)]
pub struct AppServer {
    ws: WebSocket,
    handle: AppServerHandle,
}

impl AppServer {
    /// Connect and run the mandatory `initialize` / `initialized` handshake.
    pub fn connect(socket: &Path, client_name: &str) -> std::io::Result<Self> {
        let ws = WebSocket::connect(socket, "/")?;
        let handle = AppServerHandle {
            writer: ws.writer(),
            next_id: Arc::new(AtomicU64::new(1)),
        };
        let mut s = Self { ws, handle };
        let id = s.handle.request(
            "initialize",
            &json!({ "clientInfo": { "name": client_name, "version": env!("CARGO_PKG_VERSION") }, "capabilities": { "experimentalApi": true } }),
        )?;
        loop {
            match s.next_message()? {
                Some(v) if v.get("id") == Some(&json!(id)) => break,
                Some(_) => {}
                None => return Err(std::io::Error::other("app-server closed during initialize")),
            }
        }
        s.handle.notify("initialized", &json!({}))?;
        Ok(s)
    }

    /// The sending half.
    pub fn handle(&self) -> AppServerHandle {
        self.handle.clone()
    }

    /// Next message (request, notification or response); `None` when closed.
    pub fn next_message(&mut self) -> std::io::Result<Option<Value>> {
        match self.ws.recv_text()? {
            Some(text) => Ok(Some(serde_json::from_str(&text).unwrap_or(Value::Null))),
            None => Ok(None),
        }
    }
}

/// The app-server result for an inbox decision (`ReviewDecision`).
pub fn approval_result(decision: &Decision) -> Value {
    match decision {
        Decision::Allow { .. } => json!({ "decision": "accept" }),
        Decision::Deny { .. } => json!({ "decision": "decline" }),
    }
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_owned)
}

fn u(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

fn approval_id(thread: &str, rpc_id: &Value) -> ApprovalId {
    ApprovalId(format!("{thread}:rpc-{rpc_id}"))
}

/// Translate one `codex exec --json` line (dotted `type`, verified shapes in
/// `tests/fixtures/codex/exec/`, 0.153.2) into events.
#[allow(clippy::too_many_lines)]
pub fn ingest_exec_line(v: &Value, counter: u64) -> Ingested {
    let mut out = Ingested::default();
    let item = v.get("item").cloned().unwrap_or(Value::Null);
    match v.get("type").and_then(Value::as_str) {
        Some("thread.started") => {
            let id = s(v, "thread_id").unwrap_or_default();
            out.agent_session_id = Some(id.clone());
            out.events.push(AgentEvent::SessionStarted {
                agent_session_id: id,
                model: None,
                cwd: std::path::PathBuf::new(),
            });
        }
        Some("turn.started") => out.state = Some(AgentState::Thinking),
        Some("turn.completed") => {
            if let Some(usage) = v.get("usage") {
                out.events.push(AgentEvent::Usage(vt_proto::usage::Usage {
                    input: u(usage, "input_tokens"),
                    output: u(usage, "output_tokens"),
                    cache_read: u(usage, "cached_input_tokens"),
                    cache_write: u(usage, "cache_write_input_tokens"),
                    cost_usd: None,
                    context_used_pct: None,
                }));
            }
            out.state = Some(AgentState::Idle);
            out.events.push(AgentEvent::Notification {
                title: Some("turn".into()),
                body: "turn completed".into(),
            });
        }
        Some("turn.failed") => out.events.push(AgentEvent::Error {
            kind: vt_proto::agent::ErrorKind::Api,
            message: v
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("turn failed")
                .to_string(),
            retrying: false,
        }),
        Some(kind @ ("item.started" | "item.completed")) => {
            let id = s(&item, "id").unwrap_or_else(|| format!("item-{counter}"));
            match s(&item, "type").as_deref() {
                Some("command_execution") if kind == "item.started" => {
                    out.events.push(AgentEvent::ToolCallStart {
                        id,
                        name: "shell".into(),
                        input: json!({ "command": s(&item, "command") }),
                    });
                }
                Some("command_execution") => out.events.push(AgentEvent::ToolCallEnd {
                    id,
                    ok: item.get("exit_code").and_then(Value::as_i64) == Some(0),
                    output: s(&item, "aggregated_output"),
                    duration_ms: 0,
                }),
                Some("agent_message") if kind == "item.completed" => {
                    out.events.push(AgentEvent::AssistantText {
                        text: s(&item, "text").unwrap_or_default(),
                        streaming: false,
                    });
                }
                Some("file_change") if kind == "item.completed" => {
                    for change in item
                        .get("changes")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default()
                    {
                        out.events.push(AgentEvent::FileChanged {
                            path: std::path::PathBuf::from(s(&change, "path").unwrap_or_default()),
                            diff: s(&change, "diff"),
                        });
                    }
                }
                Some("error") => out.events.push(AgentEvent::Error {
                    kind: vt_proto::agent::ErrorKind::Other,
                    message: s(&item, "message").unwrap_or_default(),
                    retrying: false,
                }),
                Some("collab_tool_call") if kind == "item.started" => {
                    out.events.push(AgentEvent::SubagentStart {
                        id: s(&item, "sender_thread_id").unwrap_or(id),
                        kind: s(&item, "tool").unwrap_or_default(),
                    });
                }
                Some("collab_tool_call") => out.events.push(AgentEvent::SubagentStop {
                    id: s(&item, "sender_thread_id").unwrap_or(id),
                }),
                Some("reasoning" | "web_search" | "mcp_tool_call" | "todo_list") | None => {}
                Some(other) => {
                    out.warnings.push(Warning(format!(
                        "unknown codex exec item `{other}`; passing through as Unknown"
                    )));
                    out.events.push(AgentEvent::Unknown {
                        name: format!("{kind}/{other}"),
                        payload: v.clone(),
                    });
                }
            }
        }
        Some("error") => out.events.push(AgentEvent::Error {
            kind: vt_proto::agent::ErrorKind::Api,
            message: s(v, "message").unwrap_or_default(),
            retrying: false,
        }),
        Some(other) => {
            out.warnings.push(Warning(format!(
                "unknown codex exec line `{other}`; passing through as Unknown"
            )));
            out.events.push(AgentEvent::Unknown {
                name: other.to_string(),
                payload: v.clone(),
            });
        }
        None => {}
    }
    out
}

/// Translate one app-server message into normalised events. Server requests
/// (approvals) become `ApprovalNeeded` with the JSON-RPC id inside the
/// approval id so the answer can be routed back. Lines from `codex exec
/// --json` (dotted `type`, no `method`) are handled too.
#[allow(clippy::too_many_lines)]
pub fn ingest(message: &Value, counter: u64) -> Ingested {
    let mut out = Ingested::default();
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        if message.get("type").is_some() && message.get("id").is_none() {
            return ingest_exec_line(message, counter);
        }
        return out; // a response to one of our own requests
    };
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            let rpc_id = message.get("id").cloned().unwrap_or(json!(counter));
            let thread = s(&params, "threadId").unwrap_or_default();
            out.agent_session_id = Some(thread.clone());
            let (tool, input) = if method.starts_with("item/commandExecution") {
                (
                    "shell".to_string(),
                    json!({ "command": s(&params, "command"), "cwd": s(&params, "cwd") }),
                )
            } else {
                (
                    "apply_patch".to_string(),
                    params.get("changes").cloned().unwrap_or(Value::Null),
                )
            };
            out.events.push(AgentEvent::ApprovalNeeded(ApprovalRequest {
                id: approval_id(&thread, &rpc_id),
                tool,
                input,
                reason: s(&params, "reason"),
                source: method.to_string(),
            }));
            out.state = Some(AgentState::AwaitingInput);
        }
        "serverRequest/resolved" => {
            if let (Some(thread), Some(rid)) = (s(&params, "threadId"), params.get("requestId")) {
                out.withdrawn.push(approval_id(&thread, rid));
            }
        }
        "thread/started" => {
            if let Some(t) = params.get("thread") {
                out.agent_session_id = s(t, "id");
                out.events.push(AgentEvent::SessionStarted {
                    agent_session_id: s(t, "id").unwrap_or_default(),
                    model: s(t, "model"),
                    cwd: std::path::PathBuf::from(s(t, "cwd").unwrap_or_default()),
                });
            }
        }
        "thread/status/changed" => {
            let status = params.get("status").cloned().unwrap_or(Value::Null);
            let waiting = status
                .get("activeFlags")
                .and_then(Value::as_array)
                .is_some_and(|f| f.iter().any(|x| x.as_str() == Some("waitingOnApproval")));
            out.state = match s(&status, "type").as_deref() {
                Some("active") if waiting => Some(AgentState::AwaitingInput),
                Some("active") => Some(AgentState::Thinking),
                Some("idle") => Some(AgentState::Idle),
                _ => None,
            };
        }
        "turn/started" => {
            out.state = Some(AgentState::Thinking);
            out.events.push(AgentEvent::Notification {
                title: Some("turn".into()),
                body: "turn started".into(),
            });
        }
        "item/started" | "item/completed" => {
            let item = params.get("item").cloned().unwrap_or(Value::Null);
            let id = s(&item, "id").unwrap_or_else(|| format!("item-{counter}"));
            match s(&item, "type").as_deref() {
                Some("commandExecution" | "command_execution") => {
                    if method == "item/started" {
                        out.events.push(AgentEvent::ToolCallStart {
                            id,
                            name: "shell".into(),
                            input: json!({ "command": s(&item, "command") }),
                        });
                    } else {
                        out.events.push(AgentEvent::ToolCallEnd {
                            id,
                            ok: item.get("exitCode").and_then(Value::as_i64) == Some(0),
                            output: s(&item, "aggregatedOutput"),
                            duration_ms: 0,
                        });
                    }
                }
                Some("agentMessage" | "agent_message") if method == "item/completed" => {
                    out.events.push(AgentEvent::AssistantText {
                        text: s(&item, "text").unwrap_or_default(),
                        streaming: false,
                    });
                }
                Some("fileChange" | "file_change") if method == "item/completed" => {
                    for change in item
                        .get("changes")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default()
                    {
                        out.events.push(AgentEvent::FileChanged {
                            path: std::path::PathBuf::from(s(&change, "path").unwrap_or_default()),
                            diff: s(&change, "diff"),
                        });
                    }
                }
                Some("error") => out.events.push(AgentEvent::Error {
                    kind: vt_proto::agent::ErrorKind::Api,
                    message: s(&item, "message").unwrap_or_default(),
                    retrying: false,
                }),
                _ => {}
            }
        }
        "thread/tokenUsage/updated" => {
            let usage = params.get("tokenUsage").cloned().unwrap_or(Value::Null);
            let total = usage.get("total").cloned().unwrap_or(Value::Null);
            let window = u(&usage, "modelContextWindow");
            #[allow(clippy::cast_precision_loss)]
            let pct = (window > 0)
                .then(|| (u(&total, "totalTokens") as f32 / window as f32 * 100.0).min(100.0));
            out.events.push(AgentEvent::Usage(vt_proto::usage::Usage {
                input: u(&total, "inputTokens"),
                output: u(&total, "outputTokens"),
                cache_read: u(&total, "cachedInputTokens"),
                cache_write: u(&total, "cacheWriteInputTokens"),
                cost_usd: None,
                context_used_pct: pct,
            }));
        }
        "turn/completed" => {
            out.state = Some(AgentState::Idle);
            out.events.push(AgentEvent::Notification {
                title: Some("turn".into()),
                body: "turn completed".into(),
            });
        }
        "thread/closed" => out.events.push(AgentEvent::SessionEnded {
            reason: "thread closed".into(),
        }),
        "account/rateLimits/updated"
        | "item/agentMessage/delta"
        | "item/commandExecution/outputDelta"
        | "mcpServer/startupStatus/updated"
        | "configWarning"
        | "remoteControl/status/changed"
        | "item/reasoning/summaryTextDelta"
        | "item/reasoning/textDelta"
        | "turn/diff/updated"
        | "turn/plan/updated"
        | "thread/goal/cleared"
        | "thread/goal/updated"
        | "thread/name/updated"
        | "deprecationNotice" => {}
        other => {
            out.warnings.push(Warning(format!(
                "unknown app-server message `{other}`; passing through as Unknown"
            )));
            out.events.push(AgentEvent::Unknown {
                name: other.to_string(),
                payload: message.clone(),
            });
        }
    }
    out
}

/// The JSON-RPC id embedded in an approval id built by [`ingest`].
pub fn rpc_id_of(approval: &ApprovalId) -> Option<Value> {
    let (_, rest) = approval.0.rsplit_once(":rpc-")?;
    serde_json::from_str(rest)
        .ok()
        .or_else(|| Some(Value::String(rest.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_request_becomes_approval_needed_with_routable_id() {
        let msg = json!({ "method": "item/commandExecution/requestApproval", "id": 0, "params": {
            "threadId": "t1", "turnId": "u1", "itemId": "exec-1", "reason": "outside workspace",
            "command": "touch /tmp/x", "cwd": "/w", "commandActions": [] } });
        let got = ingest(&msg, 5);
        match &got.events[0] {
            AgentEvent::ApprovalNeeded(req) => {
                assert_eq!(req.id.0, "t1:rpc-0");
                assert_eq!(req.tool, "shell");
                assert_eq!(req.reason.as_deref(), Some("outside workspace"));
                assert_eq!(rpc_id_of(&req.id), Some(json!(0)));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(got.agent_session_id.as_deref(), Some("t1"));
        assert_eq!(got.state, Some(AgentState::AwaitingInput));
        let resolved = json!({ "method": "serverRequest/resolved", "params": { "threadId": "t1", "requestId": 0 } });
        assert_eq!(
            ingest(&resolved, 6).withdrawn,
            vec![ApprovalId("t1:rpc-0".into())]
        );
        assert_eq!(
            approval_result(&Decision::Deny {
                reason: "no".into()
            }),
            json!({ "decision": "decline" })
        );
    }

    #[test]
    fn exec_json_lines_become_events() {
        let started = json!({"type":"thread.started","thread_id":"t9"});
        let got = ingest(&started, 1);
        assert_eq!(got.agent_session_id.as_deref(), Some("t9"));
        let cmd = json!({"type":"item.started","item":{"id":"item_3","type":"command_execution","command":"/bin/zsh -lc 'cat big.txt'","status":"in_progress"}});
        assert!(
            matches!(&ingest(&cmd, 2).events[0], AgentEvent::ToolCallStart { name, .. } if name == "shell")
        );
        let done = json!({"type":"turn.completed","usage":{"input_tokens":51_958,"cached_input_tokens":41_216,"cache_write_input_tokens":0,"output_tokens":95}});
        let got = ingest(&done, 3);
        assert!(matches!(&got.events[0], AgentEvent::Usage(u) if u.cache_read == 41_216));
        assert_eq!(got.state, Some(AgentState::Idle));
        let err = json!({"type":"item.completed","item":{"id":"item_0","type":"error","message":"hooks bypassed"}});
        assert!(matches!(
            &ingest(&err, 4).events[0],
            AgentEvent::Error { .. }
        ));
        let weird = json!({"type":"thread.renamed"});
        assert_eq!(ingest(&weird, 5).warnings.len(), 1);
    }

    #[test]
    fn status_usage_items_and_unknowns() {
        let waiting = json!({ "method": "thread/status/changed", "params": { "threadId": "t1", "status": { "type": "active", "activeFlags": ["waitingOnApproval"] } } });
        assert_eq!(ingest(&waiting, 1).state, Some(AgentState::AwaitingInput));
        let idle = json!({ "method": "thread/status/changed", "params": { "threadId": "t1", "status": { "type": "idle" } } });
        assert_eq!(ingest(&idle, 1).state, Some(AgentState::Idle));
        let usage = json!({ "method": "thread/tokenUsage/updated", "params": { "threadId": "t1", "tokenUsage": {
            "total": { "totalTokens": 129_200, "inputTokens": 17978, "cachedInputTokens": 12160, "cacheWriteInputTokens": 0, "outputTokens": 5 },
            "modelContextWindow": 258_400 } } });
        match &ingest(&usage, 1).events[0] {
            AgentEvent::Usage(u) => {
                assert_eq!(u.input, 17978);
                assert_eq!(u.cache_read, 12160);
                assert_eq!(u.context_used_pct, Some(50.0));
            }
            other => panic!("{other:?}"),
        }
        let started = json!({ "method": "item/started", "params": { "item": { "id": "i1", "type": "commandExecution", "command": "ls" } } });
        assert!(
            matches!(&ingest(&started, 1).events[0], AgentEvent::ToolCallStart { name, .. } if name == "shell")
        );
        let done = json!({ "method": "item/completed", "params": { "item": { "id": "i1", "type": "commandExecution", "exitCode": 0, "aggregatedOutput": "a\n" } } });
        assert!(matches!(
            &ingest(&done, 1).events[0],
            AgentEvent::ToolCallEnd { ok: true, .. }
        ));
        let msg = json!({ "method": "item/completed", "params": { "item": { "id": "m", "type": "agentMessage", "text": "hi" } } });
        assert_eq!(
            ingest(&msg, 1).events,
            vec![AgentEvent::AssistantText {
                text: "hi".into(),
                streaming: false
            }]
        );
        let weird = json!({ "method": "thread/brandNew", "params": {} });
        let got = ingest(&weird, 1);
        assert_eq!(got.warnings.len(), 1);
        assert!(matches!(&got.events[0], AgentEvent::Unknown { .. }));
        assert!(
            ingest(&json!({ "id": 3, "result": {} }), 1)
                .events
                .is_empty()
        );
    }
}
