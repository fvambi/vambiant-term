//! Session records and the daemon's session RPC vocabulary.
//!
//! Method names are constants so every client and the daemon agree; the
//! parameter and result types live next to them. Unknown fields on the wire
//! are ignored (`serde` default), never fatal.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::agent::{AgentKind, AgentState};

/// Stable daemon-side session id.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

/// How much the adapter can actually see and do. Shown in the UI verbatim;
/// a degraded session must look degraded (ADR-0006).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Real events, not scraped text.
    pub structured_events: bool,
    /// We can answer, not just observe.
    pub permission_control: bool,
    /// Cost/usage feed available.
    pub cost_reporting: bool,
    /// Interrupt supported.
    pub interrupt: bool,
    /// Fork supported.
    pub fork: bool,
    /// Resume supported.
    pub resume: bool,
}

/// A session as listed by `vterm ls --json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Id.
    pub id: SessionId,
    /// Human name.
    pub name: String,
    /// Adapter kind.
    pub agent: AgentKind,
    /// Current state.
    pub state: AgentState,
    /// Capability level.
    pub capabilities: Capabilities,
    /// Working directory.
    pub cwd: PathBuf,
    /// Child pid, when the daemon owns a live PTY.
    #[serde(default)]
    pub pid: Option<u32>,
    /// Grid size.
    #[serde(default)]
    pub size: Option<(u16, u16)>,
    /// `true` when the daemon could not re-adopt the PTY after a restart.
    /// Never dropped silently (ADR-0004).
    pub orphaned: bool,
    /// `true` when this daemon re-adopted the session from its fd holder
    /// after a restart: the grid was rebuilt from buffered output and may be
    /// incomplete until the program redraws (docs/02 §8).
    #[serde(default)]
    pub readopted: bool,
    /// Why structured observation is unavailable right now (hooks never
    /// arrived, app-server unreachable). `None` while the adapter is live.
    /// Shown verbatim: a degraded session must look degraded.
    #[serde(default)]
    pub degraded: Option<String>,
    /// Creation time, RFC 3339.
    #[serde(default)]
    pub created_at: String,
}

/// RPC method names (`vtermd` ← clients).
pub mod method {
    /// `-> { version, pid, socket }`.
    pub const DAEMON_STATUS: &str = "daemon.status";
    /// `-> [SessionInfo]`.
    pub const SESSION_LIST: &str = "session.list";
    /// [`super::NewSession`] `-> SessionInfo`.
    pub const SESSION_NEW: &str = "session.new";
    /// `{ id } -> SessionInfo`.
    pub const SESSION_GET: &str = "session.get";
    /// `{ id, name } -> SessionInfo`.
    pub const SESSION_RENAME: &str = "session.rename";
    /// `{ id, signal? } -> {}` — SIGHUP by default, then reap.
    pub const SESSION_KILL: &str = "session.kill";
    /// `{ id, cols, rows } -> {}`.
    pub const SESSION_RESIZE: &str = "session.resize";
    /// `{ id, bytes: base64 } -> {}` — raw bytes to the PTY.
    pub const SESSION_INPUT: &str = "session.input";
    /// `{ id, key: KeyEvent } -> {}` — a key event encoded by the core.
    pub const SESSION_KEY: &str = "session.key";
    /// `{ id } -> Snapshot` and subscribes this connection to
    /// [`notification::SESSION_OUTPUT`] for the session.
    pub const SESSION_ATTACH: &str = "session.attach";
    /// `{ id } -> {}`.
    pub const SESSION_DETACH: &str = "session.detach";
    /// `{ id, lines? } -> { text }` — last `lines` of the visible grid as text.
    pub const SESSION_LOGS: &str = "session.logs";
    /// `{ id, after? }` → the session's command blocks (OSC 133/633).
    pub const SESSION_BLOCKS: &str = "session.blocks";
    /// `{id, to: "top"|"bottom"|"lines"|"row", n?}` → `{top, total}`.
    /// Moves the session's viewport; every attached viewer follows.
    pub const SESSION_SCROLL: &str = "session.scroll";
    /// `{id, from, to, format?: "plain"|"html"}` (absolute rows, inclusive)
    /// → `{text}`; soft wraps joined. Empty when the rows were pruned.
    pub const SESSION_TEXT: &str = "session.text";
    /// `{id}` → `{top, total}`. Erases the scrollback (not the live grid)
    /// and moves every viewer to the live end.
    pub const SESSION_CLEAR: &str = "session.clear";
    /// `{id, query, regex?, case_sensitive?, from?, to?, limit?}` →
    /// `[{row, col, len}]` over absolute rows, oldest first.
    pub const SESSION_FIND: &str = "session.find";
    /// `{prompt, feature?: "ask", session?}` → `{text, profile, model,
    /// usage, cost_usd_estimate, redactions}`. Every text part is redacted
    /// first; a redaction failure refuses the request (ADR-0007).
    pub const AI_ASK: &str = "ai.ask";
    /// `{}` → `{providers_path, profiles: [{name, kind, base_url, model,
    /// key, reachable, models|error}], routes}`.
    pub const AI_DOCTOR: &str = "ai.doctor";
    /// `{ command, session?, cwd? } -> { verdict, floor?, decision }`:
    /// the safety classification of a command line (docs/05 §5).
    pub const POLICY_CLASSIFY: &str = "policy.classify";
    /// `{prefix?, limit?}` → `[cmdline]`: distinct command lines across all
    /// sessions, most recent first (the Warp-mode editor's history).
    pub const HISTORY_SEARCH: &str = "history.search";
    /// `{id, seq, on}` → `{seq, bookmarked}`; broadcasts
    /// `session.block_changed`.
    pub const SESSION_BLOCK_BOOKMARK: &str = "session.block.bookmark";
    /// → everything in `vt_config::Loaded` plus field metadata and actions.
    pub const CONFIG_GET: &str = "config.get";
    /// `{ key, value }` → the new `Config`; errors name file and key.
    pub const CONFIG_SET: &str = "config.set";
    /// `{ chord, action? }` (null action removes) → resolved keymap.
    pub const CONFIG_KEYMAP_SET: &str = "config.keymap.set";
    /// `{ theme }` → path written under `themes/`.
    pub const CONFIG_THEME_SAVE: &str = "config.theme.save";
    /// Re-read the files now → same shape as `config.get`.
    pub const CONFIG_RELOAD: &str = "config.reload";
}

/// Notification names (`vtermd` → attached clients).
pub mod notification {
    /// [`super::OutputDelta`]: damage-batched cells for an attached session.
    pub const SESSION_OUTPUT: &str = "session.output";
    /// `{ id, state }` — lifecycle change.
    pub const SESSION_STATE: &str = "session.state";
    /// `{ id, event: TermEvent }` — bell, title, cwd, clipboard.
    pub const SESSION_EVENT: &str = "session.event";
    /// `{ id, block }` — a command block closed (OSC 133/633).
    pub const SESSION_BLOCK: &str = "session.block";
    /// `{id, seq, bookmarked}` — a stored block's user flags changed.
    pub const SESSION_BLOCK_CHANGED: &str = "session.block_changed";
    /// `{ request, id, delta }` — one text delta of a streaming `ai.ask`;
    /// `id` is the session it was asked for, or null.
    pub const AI_CHUNK: &str = "ai.chunk";
    /// `{ request, id, text, profile, model, usage, cost_usd_estimate, redactions, stop }`.
    pub const AI_DONE: &str = "ai.done";
    /// `{ request, id, message }` — the request was refused or failed.
    pub const AI_ERROR: &str = "ai.error";
    /// `{ request, id, tool_use, command, verdict, floor?, decision, applied, approval? }`
    /// — Agent Mode wants to run a command; `approval` is the inbox id
    /// when a human decides.
    pub const AI_TOOL_REQUEST: &str = "ai.tool_request";
    /// `{ request, id, tool_use, exit?, output?, denied?, reason? }`.
    pub const AI_TOOL_RESULT: &str = "ai.tool_result";
    /// `{ id, exit_code? , signal? }` — child exited.
    pub const SESSION_EXITED: &str = "session.exited";
    /// `SessionInfo` — a session was created, renamed or removed.
    pub const SESSION_CHANGED: &str = "session.changed";
    /// `{ config_error?, config_warnings, keymap_error?, theme_warnings }` —
    /// a config file changed on disk and was re-read.
    pub const CONFIG_CHANGED: &str = "config.changed";
}

/// Parameters of [`method::SESSION_NEW`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NewSession {
    /// Display name; generated when empty.
    #[serde(default)]
    pub name: Option<String>,
    /// Program and arguments; empty = login shell.
    #[serde(default)]
    pub argv: Vec<String>,
    /// Working directory.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    /// Extra environment.
    #[serde(default)]
    pub env: Vec<(String, String)>,
    /// Initial grid size; default 80×24.
    #[serde(default)]
    pub size: Option<(u16, u16)>,
    /// Which adapter to provision; `None` = plain shell (generic observation).
    #[serde(default)]
    pub agent: Option<AgentKind>,
}

/// One cell on the wire. Kept flat and small: it is sent per damaged cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireCell {
    /// Character (base code point).
    pub c: char,
    /// Foreground: `[kind, a, b, c]` — kind 0 default, 1 indexed (a), 2 rgb.
    pub fg: [u8; 4],
    /// Background, same encoding.
    pub bg: [u8; 4],
    /// Attribute bits as in `vt_core::cell::Attrs`.
    pub attrs: u16,
}

/// One damaged row.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireRow {
    /// Row index.
    pub row: u16,
    /// Full row contents.
    pub cells: Vec<WireCell>,
}

/// Full snapshot (attach) or delta (output notification).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputDelta {
    /// Session.
    pub id: SessionId,
    /// Grid size at the time of the delta.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
    /// `true` when `lines` holds every row (attach, resize, full damage).
    pub full: bool,
    /// Changed rows.
    pub lines: Vec<WireRow>,
    /// Cursor row, column, visibility.
    pub cursor: (u16, u16, bool),
    /// Monotonic sequence number per session; gaps mean a missed delta.
    pub seq: u64,
    /// Absolute row shown at the top of the grid (0 = oldest retained
    /// scrollback row; block rows use the same numbering).
    #[serde(default)]
    pub top: u64,
    /// Scrollback rows plus the visible grid.
    #[serde(default)]
    pub total: u64,
}
