//! Claude Code adapter (docs/03 §4).
//!
//! Provisioning writes a **session-scoped settings file** (never the user's
//! `~/.claude/settings.json`) whose hooks are `http` handlers pointing at the
//! daemon's loopback receiver, plus a `statusLine` command that relays its
//! stdin JSON to the same receiver. Verified shapes: docs/10 §6.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use vt_proto::agent::AgentKind;
use vt_proto::session::Capabilities;

/// Every hook event name Claude Code 2.1.26x knows (docs/10 §6).
pub const HOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "Setup",
    "SessionEnd",
    "UserPromptSubmit",
    "UserPromptExpansion",
    "Stop",
    "StopFailure",
    "PreToolUse",
    "PermissionRequest",
    "PermissionDenied",
    "PostToolUse",
    "PostToolUseFailure",
    "PostToolBatch",
    "SubagentStart",
    "SubagentStop",
    "TaskCreated",
    "TaskCompleted",
    "TeammateIdle",
    "InstructionsLoaded",
    "ConfigChange",
    "CwdChanged",
    "DirectoryAdded",
    "FileChanged",
    "WorktreeCreate",
    "WorktreeRemove",
    "Notification",
    "MessageDisplay",
    "PreCompact",
    "PostCompact",
    "PreModelSwitch",
    "PostModelSwitch",
    "Elicitation",
    "ElicitationResult",
];

/// What `provision` produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provisioning {
    /// The settings file to pass with `--settings`.
    pub settings_path: PathBuf,
    /// Arguments to prepend to the user's `claude` arguments.
    pub extra_args: Vec<String>,
    /// Environment to add.
    pub env: Vec<(String, String)>,
}

/// Capabilities of a provisioned Claude session.
pub const CAPABILITIES: Capabilities = Capabilities {
    structured_events: true,
    permission_control: true,
    cost_reporting: true,
    interrupt: true,
    fork: true,
    resume: true,
};

/// The adapter kind.
pub const KIND: AgentKind = AgentKind::Claude;

/// Build the settings document. `receiver` is `http://127.0.0.1:<port>`,
/// `token` binds every request to one session, `vterm` is the CLI path used
/// as the status line relay.
pub fn settings_document(receiver: &str, token: &str, vterm: &Path) -> Value {
    let hook_url = format!("{receiver}/hook/{token}");
    let mut hooks = serde_json::Map::new();
    for event in HOOK_EVENTS {
        hooks.insert(
            (*event).to_string(),
            json!([{ "matcher": "*", "hooks": [{ "type": "http", "url": hook_url }] }]),
        );
    }
    json!({
        "hooks": hooks,
        "allowedHttpHookUrls": [format!("{receiver}/*")],
        "statusLine": {
            "type": "command",
            "command": format!("{} statusline --receiver {receiver} --token {token}", vterm.display())
        },
        "subagentStatusLine": {
            "type": "command",
            "command": format!("{} statusline --receiver {receiver} --token {token} --subagent", vterm.display())
        }
    })
}

/// Write the settings file for a session and return what to spawn with.
pub fn provision(
    dir: &Path,
    receiver: &str,
    token: &str,
    vterm: &Path,
) -> std::io::Result<Provisioning> {
    std::fs::create_dir_all(dir)?;
    let settings_path = dir.join("claude-settings.json");
    let doc = settings_document(receiver, token, vterm);
    std::fs::write(&settings_path, serde_json::to_vec_pretty(&doc)?)?;
    Ok(Provisioning {
        extra_args: vec!["--settings".into(), settings_path.display().to_string()],
        env: vec![("VAMBIANT_TERM_AGENT".into(), "claude".into())],
        settings_path,
    })
}

/// Status line stdin JSON → usage figures (docs/10 §6: `cost.*`,
/// `context_window.*`, `model.*`; `rate_limits` may be absent).
#[allow(clippy::cast_possible_truncation)]
pub fn status_to_usage(status: &Value) -> vt_proto::usage::Usage {
    let cur = status
        .get("context_window")
        .and_then(|c| c.get("current_usage"));
    let get = |v: Option<&Value>, k: &str| {
        v.and_then(|x| x.get(k))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    vt_proto::usage::Usage {
        input: get(cur, "input_tokens"),
        output: get(cur, "output_tokens"),
        cache_read: get(cur, "cache_read_input_tokens"),
        cache_write: get(cur, "cache_creation_input_tokens"),
        cost_usd: status
            .get("cost")
            .and_then(|c| c.get("total_cost_usd"))
            .and_then(Value::as_f64),
        context_used_pct: status
            .get("context_window")
            .and_then(|c| c.get("used_percentage"))
            .and_then(Value::as_f64)
            .map(|f| f as f32), // percentage: f32 is plenty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_cover_every_event_and_allowlist_the_receiver() {
        let doc = settings_document(
            "http://127.0.0.1:4711",
            "tok",
            Path::new("/usr/local/bin/vterm"),
        );
        let hooks = doc["hooks"].as_object().unwrap();
        assert_eq!(hooks.len(), HOOK_EVENTS.len());
        assert_eq!(hooks["PermissionRequest"][0]["hooks"][0]["type"], "http");
        assert_eq!(
            hooks["PermissionRequest"][0]["hooks"][0]["url"],
            "http://127.0.0.1:4711/hook/tok"
        );
        assert_eq!(doc["allowedHttpHookUrls"][0], "http://127.0.0.1:4711/*");
        assert!(
            doc["statusLine"]["command"]
                .as_str()
                .unwrap()
                .contains("statusline --receiver http://127.0.0.1:4711 --token tok")
        );
    }

    #[test]
    fn provision_writes_the_file_and_returns_args() {
        let dir = std::env::temp_dir().join(format!("vt-claude-prov-{}", std::process::id()));
        let p = provision(&dir, "http://127.0.0.1:1", "t", Path::new("vterm")).unwrap();
        assert!(p.settings_path.exists());
        assert_eq!(p.extra_args[0], "--settings");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn status_line_maps_to_usage() {
        let status = serde_json::json!({
            "model": {"id": "claude-haiku-4-5-20251001"},
            "cost": {"total_cost_usd": 0.0123, "total_duration_ms": 1000},
            "context_window": {"used_percentage": 12.5, "current_usage": {"input_tokens": 100, "output_tokens": 20, "cache_read_input_tokens": 5, "cache_creation_input_tokens": 7}}
        });
        let u = status_to_usage(&status);
        assert_eq!(
            (u.input, u.output, u.cache_read, u.cache_write),
            (100, 20, 5, 7)
        );
        assert_eq!(u.cost_usd, Some(0.0123));
        assert_eq!(u.context_used_pct, Some(12.5));
        // Missing sections are simply zero/None, never an error.
        let u = status_to_usage(&serde_json::json!({}));
        assert_eq!(u.cost_usd, None);
    }
}

/// Days since 1970-01-01 → (year, month, day); Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 { y + 1 } else { y },
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

/// Unix seconds → RFC 3339 UTC.
pub fn rfc3339_from_epoch(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn content_items(v: &Value) -> Vec<Value> {
    v.pointer("/message/content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|i| i.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Translate one `--output-format stream-json` line (verified shapes:
/// `tests/fixtures/claude/stream-json/`, Claude Code 2.1.260) into events.
/// Hook lines (`hook_event_name`) take the hook path instead.
#[allow(clippy::too_many_lines)]
pub fn ingest_stream(v: &Value, counter: u64) -> crate::hooks::Ingested {
    use crate::hooks::{Ingested, Warning};
    use vt_proto::agent::{AgentEvent, AgentState, ErrorKind};
    if v.get("hook_event_name").is_some() {
        return crate::hooks::ingest(v, counter);
    }
    let mut out = Ingested {
        agent_session_id: v
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        ..Ingested::default()
    };
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    match (
        v.get("type").and_then(Value::as_str),
        v.get("subtype").and_then(Value::as_str),
    ) {
        (Some("system"), Some("init")) => {
            out.events.push(AgentEvent::SessionStarted {
                agent_session_id: s("session_id").unwrap_or_default(),
                model: s("model"),
                cwd: std::path::PathBuf::from(s("cwd").unwrap_or_default()),
            });
        }
        (Some("system"), Some("permission_denied")) => {
            out.events.push(AgentEvent::Notification {
                title: Some("permission_denied".into()),
                body: format!(
                    "{} denied ({})",
                    s("tool_name").unwrap_or_default(),
                    s("decision_reason_type").unwrap_or_default()
                ),
            });
        }
        (Some("system"), Some("model_fallback" | "notification" | "informational" | "status")) => {
            out.events.push(AgentEvent::Notification {
                title: v.get("subtype").and_then(Value::as_str).map(str::to_owned),
                body: s("message")
                    .or_else(|| s("text"))
                    .unwrap_or_else(|| v.to_string()),
            });
        }
        (
            Some("system"),
            Some(
                "hook_started"
                | "hook_response"
                | "thinking_tokens"
                | "background_tasks_changed"
                | "task_started"
                | "task_progress"
                | "task_updated"
                | "task_notification",
            ),
        )
        | (Some("stream_event" | "attachment" | "command_lifecycle"), _) => {}
        (Some("assistant"), _) => {
            out.state = Some(AgentState::Thinking);
            for item in content_items(v) {
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => out.events.push(AgentEvent::AssistantText {
                        text: text_of(&item),
                        streaming: false,
                    }),
                    Some("tool_use") => out.events.push(AgentEvent::ToolCallStart {
                        id: item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        name: item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        input: item.get("input").cloned().unwrap_or(Value::Null),
                    }),
                    Some("thinking") => {
                        let t = item.get("thinking").and_then(Value::as_str).unwrap_or("");
                        if !t.is_empty() {
                            out.events.push(AgentEvent::Thinking {
                                summary: Some(t.chars().take(200).collect()),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        (Some("user"), _) => {
            for item in content_items(v) {
                if item.get("type").and_then(Value::as_str) == Some("tool_result") {
                    let text = text_of(item.get("content").unwrap_or(&Value::Null));
                    out.events.push(AgentEvent::ToolCallEnd {
                        id: item
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        ok: !item
                            .get("is_error")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        output: Some(text.chars().take(2000).collect()),
                        duration_ms: 0,
                    });
                }
            }
        }
        (Some("result"), subtype) => {
            let u = v.get("usage").cloned().unwrap_or(Value::Null);
            let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
            out.events.push(AgentEvent::Usage(vt_proto::usage::Usage {
                input: n("input_tokens"),
                output: n("output_tokens"),
                cache_read: n("cache_read_input_tokens"),
                cache_write: n("cache_creation_input_tokens"),
                cost_usd: v.get("total_cost_usd").and_then(Value::as_f64),
                context_used_pct: None,
            }));
            if v.get("is_error").and_then(Value::as_bool).unwrap_or(false) {
                out.events.push(AgentEvent::Error {
                    kind: ErrorKind::Api,
                    message: s("result").unwrap_or_else(|| subtype.unwrap_or("error").into()),
                    retrying: false,
                });
            }
            out.state = Some(AgentState::Idle);
            out.events.push(AgentEvent::Notification {
                title: Some("result".into()),
                body: subtype.unwrap_or("done").to_string(),
            });
        }
        (Some("rate_limit_event"), _) => {
            if let Some(windows) = v
                .pointer("/rate_limit_info/unifiedWindows")
                .and_then(Value::as_object)
            {
                for (name, w) in windows {
                    #[allow(clippy::cast_possible_truncation)]
                    let used = (w.get("utilization").and_then(Value::as_f64).unwrap_or(0.0) * 100.0)
                        as f32;
                    out.events.push(AgentEvent::RateLimit {
                        window: name.clone(),
                        used_pct: used,
                        resets_at: w
                            .get("resetsAt")
                            .and_then(Value::as_i64)
                            .map(rfc3339_from_epoch),
                    });
                }
            }
        }
        (other, sub) => {
            let name = format!(
                "{}{}",
                other.unwrap_or("?"),
                sub.map(|x| format!("/{x}")).unwrap_or_default()
            );
            out.warnings.push(Warning(format!(
                "unknown stream-json line `{name}`; passing through as Unknown"
            )));
            out.events.push(AgentEvent::Unknown {
                name,
                payload: v.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use vt_proto::agent::AgentEvent;

    #[test]
    fn epoch_to_rfc3339() {
        assert_eq!(rfc3339_from_epoch(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_from_epoch(1_788_543_600), "2026-09-04T17:40:00Z");
    }

    #[test]
    fn stream_lines_become_events() {
        let init = json!({"type":"system","subtype":"init","cwd":"/w","session_id":"s1","model":"m","permissionMode":"dontAsk"});
        assert!(
            matches!(&ingest_stream(&init, 1).events[0], AgentEvent::SessionStarted { model: Some(m), .. } if m == "m")
        );
        let tool = json!({"type":"assistant","session_id":"s1","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Write","input":{"file_path":"/w/x"}}]}});
        assert!(
            matches!(&ingest_stream(&tool, 2).events[0], AgentEvent::ToolCallStart { name, .. } if name == "Write")
        );
        let result = json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"ok","is_error":false}]}});
        assert!(matches!(
            &ingest_stream(&result, 3).events[0],
            AgentEvent::ToolCallEnd { ok: true, .. }
        ));
        let done = json!({"type":"result","subtype":"success","session_id":"s1","total_cost_usd":0.03,"usage":{"input_tokens":42,"output_tokens":987,"cache_read_input_tokens":123_305,"cache_creation_input_tokens":9218}});
        let got = ingest_stream(&done, 4);
        assert!(
            matches!(&got.events[0], AgentEvent::Usage(u) if u.cost_usd == Some(0.03) && u.cache_read == 123_305)
        );
        assert_eq!(got.state, Some(vt_proto::agent::AgentState::Idle));
        let rl = json!({"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"five_hour":{"utilization":0.7,"resetsAt":1_788_543_600}}}});
        assert!(
            matches!(&ingest_stream(&rl, 5).events[0], AgentEvent::RateLimit { window, resets_at: Some(t), .. } if window == "five_hour" && t == "2026-09-04T17:40:00Z")
        );
        let weird = json!({"type":"system","subtype":"brand_new"});
        let got = ingest_stream(&weird, 6);
        assert_eq!(got.warnings.len(), 1);
        assert!(matches!(&got.events[0], AgentEvent::Unknown { .. }));
        assert!(
            ingest_stream(&json!({"type":"system","subtype":"hook_started"}), 7)
                .events
                .is_empty()
        );
    }
}
