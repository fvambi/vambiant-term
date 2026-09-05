//! Shared hook-handling abstraction for Claude Code and Codex.
//!
//! Both vendors POST (or pipe) one JSON object per event with the same core
//! fields (`hook_event_name`, `session_id`, `cwd`, `transcript_path`) and
//! convergent per-event fields, verified in M0 (docs/10 §6, §7). Parsing is
//! **lenient by contract**: an unknown event name or a missing field never
//! fails — it yields [`AgentEvent::Unknown`] plus a [`Warning`], so a vendor
//! schema change is a warning in the log, not an outage (ADR-0006).

use std::path::PathBuf;

use serde_json::Value;
use vt_proto::agent::{AgentEvent, ErrorKind};
use vt_proto::approval::{ApprovalId, ApprovalRequest};

/// A non-fatal parsing problem worth logging.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning(pub String);

/// The result of ingesting one hook payload.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ingested {
    /// Normalised events, in order.
    pub events: Vec<AgentEvent>,
    /// Anything that degraded.
    pub warnings: Vec<Warning>,
    /// The vendor's session id, when present.
    pub agent_session_id: Option<String>,
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// Build the approval id for a permission request so answers can be routed
/// back: vendor session + tool use id (or a counter when the vendor gives none).
pub fn approval_id(agent_session: &str, tool_use_id: Option<&str>, fallback: u64) -> ApprovalId {
    match tool_use_id {
        Some(t) => ApprovalId(format!("{agent_session}:{t}")),
        None => ApprovalId(format!("{agent_session}:req-{fallback}")),
    }
}

/// Translate one hook payload (either vendor) into normalised events.
///
/// `counter` disambiguates approvals that carry no tool-use id.
#[allow(clippy::too_many_lines)] // one arm per hook event; a table is the point
pub fn ingest(payload: &Value, counter: u64) -> Ingested {
    let mut out = Ingested {
        agent_session_id: str_field(payload, "session_id"),
        ..Ingested::default()
    };
    let Some(name) = payload.get("hook_event_name").and_then(Value::as_str) else {
        out.warnings
            .push(Warning("hook payload without hook_event_name".into()));
        out.events.push(AgentEvent::Unknown {
            name: "<missing>".into(),
            payload: payload.clone(),
        });
        return out;
    };
    let session = out.agent_session_id.clone().unwrap_or_default();
    let tool_name = str_field(payload, "tool_name");
    let tool_use_id = str_field(payload, "tool_use_id");
    let tool_input = payload.get("tool_input").cloned().unwrap_or(Value::Null);

    match name {
        "SessionStart" => out.events.push(AgentEvent::SessionStarted {
            agent_session_id: session,
            model: str_field(payload, "model"),
            cwd: PathBuf::from(str_field(payload, "cwd").unwrap_or_default()),
        }),
        "SessionEnd" => out.events.push(AgentEvent::SessionEnded {
            reason: str_field(payload, "reason").unwrap_or_else(|| "session_end".into()),
        }),
        "UserPromptSubmit" => {
            // Both vendors use `prompt`; the model is now busy.
            out.events.push(AgentEvent::Notification {
                title: Some("prompt".into()),
                body: str_field(payload, "prompt").unwrap_or_default(),
            });
        }
        "PreToolUse" => match (tool_name, tool_use_id) {
            (Some(name), Some(id)) => out.events.push(AgentEvent::ToolCallStart {
                id,
                name,
                input: tool_input,
            }),
            (name, id) => {
                out.warnings.push(Warning(format!(
                    "PreToolUse without tool_name/tool_use_id ({name:?}, {id:?})"
                )));
                out.events.push(AgentEvent::ToolCallStart {
                    id: id.unwrap_or_else(|| format!("unknown-{counter}")),
                    name: name.unwrap_or_else(|| "unknown".into()),
                    input: tool_input,
                });
            }
        },
        "PostToolUse" | "PostToolUseFailure" => {
            let ok = name == "PostToolUse";
            let output = payload
                .get("tool_response")
                .map(|r| match r {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .or_else(|| str_field(payload, "error"));
            out.events.push(AgentEvent::ToolCallEnd {
                id: tool_use_id.unwrap_or_else(|| format!("unknown-{counter}")),
                ok,
                output,
                duration_ms: payload
                    .get("duration_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            });
        }
        "PermissionRequest" => {
            let tool = tool_name.clone().unwrap_or_else(|| "unknown".into());
            let id = approval_id(&session, tool_use_id.as_deref(), counter);
            out.events.push(AgentEvent::ApprovalNeeded(ApprovalRequest {
                id,
                tool,
                input: tool_input,
                reason: str_field(payload, "reason"),
                source: "PermissionRequest".into(),
            }));
        }
        "Stop" | "StopFailure" => {
            if let Some(msg) = str_field(payload, "last_assistant_message") {
                out.events.push(AgentEvent::AssistantText {
                    text: msg,
                    streaming: false,
                });
            }
            if name == "StopFailure" {
                out.events.push(AgentEvent::Error {
                    kind: ErrorKind::Api,
                    message: str_field(payload, "error").unwrap_or_else(|| "turn failed".into()),
                    retrying: false,
                });
            }
        }
        "Notification" => out.events.push(AgentEvent::Notification {
            title: str_field(payload, "title"),
            body: str_field(payload, "message").unwrap_or_default(),
        }),
        "FileChanged" => out.events.push(AgentEvent::FileChanged {
            path: PathBuf::from(str_field(payload, "file_path").unwrap_or_default()),
            diff: None,
        }),
        "SubagentStart" => out.events.push(AgentEvent::SubagentStart {
            id: str_field(payload, "agent_id").unwrap_or_else(|| format!("subagent-{counter}")),
            kind: str_field(payload, "agent_type").unwrap_or_else(|| "default".into()),
        }),
        "SubagentStop" => out.events.push(AgentEvent::SubagentStop {
            id: str_field(payload, "agent_id").unwrap_or_else(|| format!("subagent-{counter}")),
        }),
        "Interrupt" => out.events.push(AgentEvent::Notification {
            title: Some("interrupted".into()),
            body: "turn interrupted".into(),
        }),
        // Observed but carrying nothing the timeline needs beyond the fact.
        "PreCompact"
        | "PostCompact"
        | "PreModelSwitch"
        | "PostModelSwitch"
        | "CwdChanged"
        | "DirectoryAdded"
        | "WorktreeCreate"
        | "WorktreeRemove"
        | "Setup"
        | "InstructionsLoaded"
        | "ConfigChange"
        | "MessageDisplay"
        | "PostToolBatch"
        | "UserPromptExpansion"
        | "TaskCreated"
        | "TaskCompleted"
        | "TeammateIdle"
        | "Elicitation"
        | "ElicitationResult"
        | "PermissionDenied" => out.events.push(AgentEvent::Notification {
            title: Some(name.to_ascii_lowercase()),
            body: summarise(payload),
        }),
        other => {
            out.warnings.push(Warning(format!(
                "unknown hook event `{other}`; passing through as Unknown"
            )));
            out.events.push(AgentEvent::Unknown {
                name: other.to_string(),
                payload: payload.clone(),
            });
        }
    }
    out
}

fn summarise(payload: &Value) -> String {
    [
        "message",
        "file_path",
        "cwd",
        "trigger",
        "new_model",
        "path",
        "task_id",
    ]
    .iter()
    .find_map(|k| str_field(payload, k))
    .unwrap_or_default()
}

/// The JSON a hook must answer with to allow, deny or leave the decision to
/// the vendor. Shapes verified in M0 for both `PreToolUse` and
/// `PermissionRequest` on Claude Code and `PreToolUse` on Codex.
pub mod answer {
    use serde_json::{Value, json};

    /// Allow, optionally rewriting the tool input (edit-then-allow).
    pub fn allow(event: &str, updated_input: Option<Value>) -> Value {
        if event == "PermissionRequest" {
            let mut decision = json!({ "behavior": "allow" });
            if let Some(u) = updated_input {
                decision["updatedInput"] = u;
            }
            return json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": decision } });
        }
        let mut hso = json!({ "hookEventName": event, "permissionDecision": "allow" });
        if let Some(u) = updated_input {
            hso["updatedInput"] = u;
        }
        json!({ "hookSpecificOutput": hso })
    }

    /// Deny with a reason the agent sees.
    pub fn deny(event: &str, reason: &str) -> Value {
        if event == "PermissionRequest" {
            return json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": { "behavior": "deny", "message": reason }
                }
            });
        }
        json!({
            "hookSpecificOutput": {
                "hookEventName": event,
                "permissionDecision": "deny",
                "permissionDecisionReason": reason
            }
        })
    }

    /// No decision: the vendor's own flow continues (interactive prompt).
    pub fn pass() -> Value {
        json!({})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_the_common_events() {
        let pre = json!({
            "hook_event_name": "PreToolUse", "session_id": "s1", "cwd": "/w",
            "tool_name": "Bash", "tool_use_id": "toolu_1", "tool_input": {"command": "ls"},
            "permission_mode": "default", "prompt_id": "p1", "transcript_path": "/t"
        });
        let got = ingest(&pre, 1);
        assert!(got.warnings.is_empty());
        assert_eq!(got.agent_session_id.as_deref(), Some("s1"));
        assert_eq!(
            got.events,
            vec![AgentEvent::ToolCallStart {
                id: "toolu_1".into(),
                name: "Bash".into(),
                input: json!({"command": "ls"})
            }]
        );

        let perm = json!({
            "hook_event_name": "PermissionRequest", "session_id": "s1", "cwd": "/w",
            "tool_name": "Write", "tool_input": {"file_path": "/w/x", "content": "y"},
            "permission_suggestions": []
        });
        let got = ingest(&perm, 7);
        match &got.events[0] {
            AgentEvent::ApprovalNeeded(req) => {
                assert_eq!(req.id, ApprovalId("s1:req-7".into()));
                assert_eq!(req.tool, "Write");
                assert_eq!(req.source, "PermissionRequest");
            }
            other => panic!("expected ApprovalNeeded, got {other:?}"),
        }

        let stop = json!({ "hook_event_name": "Stop", "session_id": "s1", "last_assistant_message": "done", "stop_hook_active": false });
        assert_eq!(
            ingest(&stop, 1).events,
            vec![AgentEvent::AssistantText {
                text: "done".into(),
                streaming: false
            }]
        );

        // Codex shape: model + turn_id, no prompt_id — must parse identically.
        let codex = json!({ "hook_event_name": "PostToolUse", "session_id": "c1", "model": "gpt-5.6-sol", "turn_id": "t",
            "tool_name": "shell", "tool_use_id": "call_1", "tool_input": {"command": "echo hi"}, "tool_response": {"output": "hi"} });
        match &ingest(&codex, 1).events[0] {
            AgentEvent::ToolCallEnd { id, ok, output, .. } => {
                assert_eq!(id, "call_1");
                assert!(ok);
                assert!(output.as_deref().unwrap().contains("hi"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unknown_events_and_missing_fields_degrade_never_fail() {
        let weird = json!({ "hook_event_name": "BrandNewEvent", "session_id": "s", "payload": 1 });
        let got = ingest(&weird, 1);
        assert_eq!(got.warnings.len(), 1);
        assert!(got.warnings[0].0.contains("BrandNewEvent"));
        assert!(
            matches!(&got.events[0], AgentEvent::Unknown { name, .. } if name == "BrandNewEvent")
        );

        let missing = json!({ "session_id": "s" });
        let got = ingest(&missing, 1);
        assert!(!got.warnings.is_empty());
        assert!(matches!(&got.events[0], AgentEvent::Unknown { .. }));

        let half = json!({ "hook_event_name": "PreToolUse", "session_id": "s" });
        let got = ingest(&half, 3);
        assert_eq!(got.warnings.len(), 1);
        assert!(
            matches!(&got.events[0], AgentEvent::ToolCallStart { id, name, .. } if id == "unknown-3" && name == "unknown")
        );

        // Extra unknown fields are ignored.
        let extra = json!({ "hook_event_name": "SessionEnd", "session_id": "s", "reason": "exit", "future_field": {"x": 1} });
        assert_eq!(
            ingest(&extra, 1).events,
            vec![AgentEvent::SessionEnded {
                reason: "exit".into()
            }]
        );
    }

    #[test]
    fn answer_shapes_match_the_verified_ones() {
        assert_eq!(
            answer::allow("PreToolUse", Some(json!({"command": "echo x"}))),
            json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": {"command": "echo x"}}})
        );
        assert_eq!(
            answer::deny("PermissionRequest", "no"),
            json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "deny", "message": "no"}}})
        );
        assert_eq!(answer::pass(), json!({}));
    }
}
