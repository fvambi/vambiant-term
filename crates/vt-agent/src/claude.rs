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
