//! The normalised agent event model (docs/03 §3).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::approval::{ApprovalId, ApprovalRequest, Decision, DecisionSource};
use crate::usage::Usage;

/// Which adapter produced a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    /// Claude Code.
    Claude,
    /// OpenAI Codex CLI.
    Codex,
    /// Anything else — heuristics only, labelled as such.
    Generic,
}

/// Coarse lifecycle state. `AwaitingInput` is the state the whole product
/// is organised around.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    /// Process spawned, no events yet.
    Starting,
    /// Waiting for the user to type.
    Idle,
    /// Model turn in progress.
    Thinking,
    /// A tool call is executing.
    ToolRunning,
    /// Blocked on a human decision.
    AwaitingInput,
    /// Exited cleanly.
    Stopped,
    /// Exited abnormally or lost.
    Crashed,
}

/// Everything an adapter emits collapses to one of these.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// The agent reported its own session id.
    SessionStarted {
        /// Vendor session id (Claude `session_id`, Codex thread id).
        agent_session_id: String,
        /// Model id if known.
        model: Option<String>,
        /// Working directory.
        cwd: PathBuf,
    },
    /// Lifecycle transition.
    StateChanged {
        /// Previous state.
        from: AgentState,
        /// New state.
        to: AgentState,
    },
    /// Assistant prose, possibly partial.
    AssistantText {
        /// Text chunk.
        text: String,
        /// `true` while more chunks are expected.
        streaming: bool,
    },
    /// Extended-thinking summary, when the vendor exposes one.
    Thinking {
        /// Summary text.
        summary: Option<String>,
    },
    /// A tool call began.
    ToolCallStart {
        /// Vendor tool-use id.
        id: String,
        /// Tool name.
        name: String,
        /// Raw input as the vendor gave it.
        input: serde_json::Value,
    },
    /// A tool call finished.
    ToolCallEnd {
        /// Vendor tool-use id.
        id: String,
        /// Success flag.
        ok: bool,
        /// Output text, if any.
        output: Option<String>,
        /// Wall-clock duration.
        duration_ms: u64,
    },
    /// A file on disk changed.
    FileChanged {
        /// Path.
        path: PathBuf,
        /// Unified diff text if the vendor supplied one.
        diff: Option<String>,
    },
    /// The inbox's source event.
    ApprovalNeeded(ApprovalRequest),
    /// An approval was answered, by whom.
    ApprovalResolved {
        /// Approval id.
        id: ApprovalId,
        /// Decision taken.
        decision: Decision,
        /// Who took it.
        by: DecisionSource,
    },
    /// Token/cost accounting.
    Usage(Usage),
    /// Rate-limit window status.
    RateLimit {
        /// Window name (`five_hour`, `seven_day`, `spend_limit`).
        window: String,
        /// Percentage used.
        used_pct: f32,
        /// Reset time as RFC 3339, if known.
        resets_at: Option<String>,
    },
    /// Subagent spawned.
    SubagentStart {
        /// Subagent id.
        id: String,
        /// Subagent type/name.
        kind: String,
    },
    /// Subagent finished.
    SubagentStop {
        /// Subagent id.
        id: String,
    },
    /// Agent-initiated message for the user.
    Notification {
        /// Title, if any.
        title: Option<String>,
        /// Body.
        body: String,
    },
    /// Something went wrong.
    Error {
        /// Category.
        kind: ErrorKind,
        /// Message.
        message: String,
        /// Whether the agent is retrying.
        retrying: bool,
    },
    /// The session ended.
    SessionEnded {
        /// Vendor-supplied reason.
        reason: String,
    },
    /// An event we received but could not classify. Emitted with a warning
    /// so a vendor schema change is visible, never silently dropped.
    Unknown {
        /// Vendor event name.
        name: String,
        /// Raw payload.
        payload: serde_json::Value,
    },
}

/// Error categories the UI distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Provider/API error.
    Api,
    /// Tool execution error.
    Tool,
    /// Adapter/transport error.
    Adapter,
    /// Anything else.
    Other,
}
