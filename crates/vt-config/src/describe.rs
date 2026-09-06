//! Field metadata for the settings UI: what each key is, what values it
//! takes, and whether the running build actually applies it. The test at
//! the bottom keeps this list and the schema in lock-step.

use serde::Serialize;

/// Value kind, which decides the control.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Kind {
    #[allow(missing_docs)]
    Bool,
    #[allow(missing_docs)]
    Int { min: i64, max: i64 },
    #[allow(missing_docs)]
    Float { min: f64, max: f64, step: f64 },
    #[allow(missing_docs)]
    Text,
    /// A list of strings.
    List,
    #[allow(missing_docs)]
    Enum { options: Vec<&'static str> },
    /// A key chord, `cmd+shift+a`.
    Hotkey,
}

/// Whether the current build reads the value.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "applied", rename_all = "snake_case")]
pub enum Applied {
    /// Read and honoured now.
    Now,
    /// Stored and validated, honoured from the named milestone.
    Later {
        #[allow(missing_docs)]
        milestone: &'static str,
    },
}

/// One key.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Field {
    /// Dotted path, `font.size`.
    pub path: &'static str,
    #[allow(missing_docs)]
    pub doc: &'static str,
    #[serde(flatten)]
    #[allow(missing_docs)]
    pub kind: Kind,
    #[serde(flatten)]
    #[allow(missing_docs)]
    pub applied: Applied,
}

use Applied::{Later, Now};
use Kind::{Bool, Enum, Float, Hotkey, Int, List, Text};

fn e(options: &'static [&'static str]) -> Kind {
    Enum {
        options: options.to_vec(),
    }
}

macro_rules! f {
    ($path:literal, $doc:literal, $kind:expr, $applied:expr) => {
        Field {
            path: $path,
            doc: $doc,
            kind: $kind,
            applied: $applied,
        }
    };
}

/// Every key of `config.toml`, in the reference's order.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn fields() -> Vec<Field> {
    use crate::schema::{
        AgentFinished, CodexTransport, CursorStyle, Decorations, EgressMode, Inject, InputMode,
        KeymapProfile, Redaction, ResumeBy, TabBar, ThinStrokes,
    };
    let ms = Int {
        min: 0,
        max: 600_000,
    };
    let days = Int {
        min: 0,
        max: 36_500,
    };
    vec![
        f!("font.family", "Primary monospace family.", Text, Now),
        f!(
            "font.fallback",
            "Families tried before CoreText's own cascade.",
            List,
            Now
        ),
        f!(
            "font.size",
            "Point size.",
            Float {
                min: 6.0,
                max: 72.0,
                step: 0.5
            },
            Now
        ),
        f!(
            "font.line_height",
            "Cell height as a multiple of the font's natural line.",
            Float {
                min: 0.8,
                max: 2.0,
                step: 0.05
            },
            Now
        ),
        f!(
            "font.cell_width",
            "Cell width multiplier.",
            Float {
                min: 0.8,
                max: 1.5,
                step: 0.05
            },
            Later { milestone: "M5" }
        ),
        f!(
            "font.ligatures",
            "Shape ligatures across cells.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "font.ligature_disable",
            "Ligatures never shaped (terminal-safe defaults).",
            List,
            Later { milestone: "M5" }
        ),
        f!(
            "font.thin_strokes",
            "Font smoothing on Retina panels.",
            e(ThinStrokes::OPTIONS),
            Later { milestone: "M5" }
        ),
        f!(
            "font.bold_is_bright",
            "Render bold text in the bright palette.",
            Bool,
            Now
        ),
        f!(
            "theme.name",
            "Theme used in dark mode (built-in or themes/*.toml).",
            Text,
            Now
        ),
        f!(
            "theme.light",
            "Theme used when macOS is in light mode.",
            Text,
            Now
        ),
        f!(
            "theme.follow_system",
            "Switch between the two with the system appearance.",
            Bool,
            Now
        ),
        f!(
            "window.padding.x",
            "Horizontal padding around the grid, points.",
            Int { min: 0, max: 64 },
            Now
        ),
        f!(
            "window.padding.y",
            "Vertical padding around the grid, points.",
            Int { min: 0, max: 64 },
            Now
        ),
        f!(
            "window.opacity",
            "Window opacity.",
            Float {
                min: 0.0,
                max: 1.0,
                step: 0.05
            },
            Later { milestone: "M5" }
        ),
        f!(
            "window.blur",
            "Background blur radius when translucent.",
            Int { min: 0, max: 64 },
            Later { milestone: "M5" }
        ),
        f!(
            "window.decorations",
            "Native title bar or none.",
            e(Decorations::OPTIONS),
            Later { milestone: "M5" }
        ),
        f!(
            "window.tab_bar",
            "Native tab bar or none.",
            e(TabBar::OPTIONS),
            Later { milestone: "M5" }
        ),
        f!(
            "window.restore_session",
            "Reopen the previous windows on launch.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "window.quake.enabled",
            "Drop-down window on a global hotkey.",
            Bool,
            Later { milestone: "M8" }
        ),
        f!(
            "window.quake.hotkey",
            "The global hotkey.",
            Hotkey,
            Later { milestone: "M8" }
        ),
        f!(
            "cursor.style",
            "Cursor shape.",
            e(CursorStyle::OPTIONS),
            Now
        ),
        f!("cursor.blink", "Blink the cursor.", Bool, Now),
        f!(
            "cursor.blink_interval_ms",
            "Blink half-period.",
            Int {
                min: 100,
                max: 5000
            },
            Now
        ),
        f!(
            "terminal.scrollback_lines",
            "Lines kept per session.",
            Int {
                min: 0,
                max: 10_000_000
            },
            Later { milestone: "M5" }
        ),
        f!(
            "terminal.shell",
            "Program to run; empty means your login shell.",
            Text,
            Now
        ),
        f!(
            "terminal.term",
            "TERM value (falls back to xterm-256color).",
            Text,
            Later { milestone: "M5" }
        ),
        f!(
            "terminal.kitty_keyboard",
            "Offer the kitty keyboard protocol to programs.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "terminal.confirm_close_with_running_process",
            "Ask before closing a pane whose process is busy.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "terminal.osc.clipboard_write",
            "Programs may write the clipboard (OSC 52).",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "terminal.osc.clipboard_read",
            "Programs may read the clipboard — an exfiltration primitive.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "terminal.osc.window_ops",
            "Programs may move or resize the window.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "terminal.osc.title_report",
            "Programs may read the window title back.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "terminal.osc.hyperlinks",
            "Clickable OSC 8 links.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "shell_integration.enabled",
            "Prompt marks for blocks and jump-to-prompt.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "shell_integration.shells",
            "Shells the snippets are injected into.",
            List,
            Later { milestone: "M5" }
        ),
        f!(
            "shell_integration.inject",
            "How the snippets get loaded.",
            e(Inject::OPTIONS),
            Later { milestone: "M5" }
        ),
        f!(
            "shell_integration.warn_on_conflict",
            "Warn when a prompt framework corrupts the marks.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "shell_integration.ssh_wrap",
            "Carry integration through ssh.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "shell_integration.sudo_wrap",
            "Carry integration through sudo.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "input.mode",
            "warp: the app's editor at the bottom of the pane, the shell's prompt hidden; classic: the shell's own editor.",
            e(InputMode::OPTIONS),
            Later { milestone: "M5.5" }
        ),
        f!(
            "blocks.dividers",
            "Hairline above each command block.",
            Bool,
            Now
        ),
        f!(
            "blocks.failed_tint",
            "Tint the rows of a command that exited non-zero.",
            Bool,
            Now
        ),
        f!(
            "blocks.sticky_header",
            "Keep a scrolled-off command line pinned at the top of the pane.",
            Bool,
            Now
        ),
        f!(
            "mux.keymap_profile",
            "Which keymap profiles are active.",
            e(KeymapProfile::OPTIONS),
            Now
        ),
        f!("mux.prefix", "The tmux-style prefix chord.", Hotkey, Now),
        f!(
            "mux.detach_on_close",
            "Closing a window detaches its sessions; it never kills them.",
            Bool,
            Now
        ),
        f!(
            "mux.default_layout",
            "Layout for a new window.",
            Text,
            Later { milestone: "M5" }
        ),
        f!(
            "agents.auto_detect",
            "Recognise agent CLIs started in a plain shell.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "agents.adopt_external",
            "Adopt agents started outside Vambiant Term.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!("agents.claude.binary", "Claude Code executable.", Text, Now),
        f!(
            "agents.claude.install_hooks",
            "Provision the hook receiver for new sessions.",
            Bool,
            Now
        ),
        f!(
            "agents.claude.install_statusline",
            "Provision the status-line feed.",
            Bool,
            Now
        ),
        f!(
            "agents.claude.resume_by",
            "Resume strategy (never --continue).",
            e(ResumeBy::OPTIONS),
            Later { milestone: "M6" }
        ),
        f!(
            "agents.claude.extra_args",
            "Arguments appended to every launch.",
            List,
            Now
        ),
        f!("agents.codex.binary", "Codex executable.", Text, Now),
        f!(
            "agents.codex.transport",
            "How Codex is observed.",
            e(CodexTransport::OPTIONS),
            Later { milestone: "M5" }
        ),
        f!(
            "agents.codex.extra_args",
            "Arguments appended to every launch.",
            List,
            Now
        ),
        f!(
            "agents.generic.enabled",
            "Heuristic observation of unknown agents.",
            Bool,
            Now
        ),
        f!(
            "agents.generic.packs",
            "Prompt-pattern packs loaded from the state dir.",
            List,
            Now
        ),
        f!(
            "notifications.awaiting_input",
            "Notify when an agent waits for you.",
            Bool,
            Now
        ),
        f!(
            "notifications.agent_finished",
            "Notify when an agent finishes.",
            e(AgentFinished::OPTIONS),
            Later { milestone: "M5" }
        ),
        f!(
            "notifications.agent_crashed",
            "Notify when an agent crashes.",
            Bool,
            Later { milestone: "M5" }
        ),
        f!(
            "notifications.long_command_ms",
            "Notify when a command runs longer than this.",
            ms.clone(),
            Later { milestone: "M5" }
        ),
        f!(
            "notifications.coalesce_window_ms",
            "Merge notifications closer than this.",
            ms.clone(),
            Now
        ),
        f!(
            "ai.enabled",
            "Master switch for every model feature.",
            Bool,
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.inline_suggest",
            "Ghost-text suggestions at the prompt.",
            Bool,
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.suggest_debounce_ms",
            "Idle time before asking for a suggestion.",
            ms.clone(),
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.suggest_budget_ms",
            "A suggestion later than this is discarded.",
            ms.clone(),
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.explain_on_failure",
            "Offer an explanation when a command fails.",
            Bool,
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.routes.suggest",
            "Provider id for suggestions.",
            Text,
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.routes.classify",
            "Provider id for the safety second opinion.",
            Text,
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.routes.ask",
            "Provider id for ⌘K.",
            Text,
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.routes.explain",
            "Provider id for explanations.",
            Text,
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.routes.search",
            "Provider id for embeddings.",
            Text,
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.budget.daily_usd",
            "Daily spend cap.",
            Float {
                min: 0.0,
                max: 10_000.0,
                step: 0.5
            },
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.budget.monthly_usd",
            "Monthly spend cap.",
            Float {
                min: 0.0,
                max: 100_000.0,
                step: 1.0
            },
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.budget.hard_stop",
            "Refuse loudly at the cap; never degrade silently.",
            Bool,
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.context.include_git",
            "Send branch and status with prompts.",
            Bool,
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.context.include_last_blocks",
            "Recent command blocks sent with prompts.",
            Int { min: 0, max: 50 },
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.context.include_env",
            "Send environment variables — the most common leak vector.",
            Bool,
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.context.include_help_text",
            "Send --help output for the command.",
            Bool,
            Later { milestone: "M-AI" }
        ),
        f!(
            "ai.context.max_bytes",
            "Cap on context bytes per request.",
            Int {
                min: 0,
                max: 1_000_000
            },
            Later { milestone: "M-AI" }
        ),
        f!(
            "privacy.egress_mode",
            "What may leave the machine.",
            e(EgressMode::OPTIONS),
            Later { milestone: "M-SEC" }
        ),
        f!(
            "privacy.redaction",
            "When secrets are redacted (always is the only sane value).",
            e(Redaction::OPTIONS),
            Later { milestone: "M-SEC" }
        ),
        f!(
            "privacy.show_payload_first_use",
            "Show the outgoing payload the first time a feature sends one.",
            Bool,
            Later { milestone: "M-SEC" }
        ),
        f!(
            "privacy.egress_log_days",
            "Retention of the egress log.",
            days.clone(),
            Now
        ),
        f!(
            "privacy.telemetry",
            "Always false. Not a setting, a statement.",
            Bool,
            Now
        ),
        f!(
            "privacy.update_check",
            "Check for updates on launch.",
            Bool,
            Later { milestone: "M9" }
        ),
        f!(
            "storage.block_retention_days",
            "Retention of command blocks.",
            days.clone(),
            Now
        ),
        f!(
            "storage.event_retention_days",
            "Retention of agent events.",
            days,
            Now
        ),
        f!(
            "storage.scrollback_max_mb",
            "Cap on scrollback storage.",
            Int {
                min: 0,
                max: 1_000_000
            },
            Later { milestone: "M5" }
        ),
        f!(
            "storage.prune_on_start",
            "Sweep expired data when the daemon starts.",
            Bool,
            Now
        ),
        f!(
            "api.enabled",
            "Local HTTP API.",
            Bool,
            Later { milestone: "M8" }
        ),
        f!(
            "api.bind",
            "Bind address; loopback only, enforced.",
            Text,
            Later { milestone: "M8" }
        ),
        f!(
            "api.port",
            "Port.",
            Int {
                min: 1,
                max: 65_535
            },
            Later { milestone: "M8" }
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaves(value: &toml::Value, prefix: &str, out: &mut Vec<String>) {
        match value {
            toml::Value::Table(t) => {
                for (k, v) in t {
                    let path = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    leaves(v, &path, out);
                }
            }
            _ => out.push(prefix.to_owned()),
        }
    }

    #[test]
    fn every_schema_key_is_described_exactly_once() {
        let value = toml::Value::try_from(crate::schema::Config::default()).unwrap();
        let mut schema = Vec::new();
        leaves(&value, "", &mut schema);
        let mut described: Vec<String> = fields().iter().map(|f| f.path.to_owned()).collect();
        schema.sort();
        let mut sorted = described.clone();
        sorted.sort();
        assert_eq!(sorted, schema, "describe.rs and schema.rs disagree");
        described.dedup();
        assert_eq!(described.len(), fields().len(), "duplicate path");
    }

    #[test]
    fn enum_kinds_list_the_serde_spellings() {
        let f = fields();
        let transport = f
            .iter()
            .find(|f| f.path == "agents.codex.transport")
            .unwrap();
        assert_eq!(
            transport.kind,
            Kind::Enum {
                options: vec!["app-server", "exec", "hooks-only"]
            }
        );
    }
}
