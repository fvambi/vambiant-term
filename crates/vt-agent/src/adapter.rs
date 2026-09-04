//! The [`AgentAdapter`] trait (docs/03 §2).

use vt_proto::agent::{AgentEvent, AgentKind};
use vt_proto::approval::{ApprovalId, Decision};
use vt_proto::session::Capabilities;

/// Something an adapter can ingest.
#[derive(Clone, Debug)]
pub enum AdapterInput {
    /// A hook POST body (Claude `http` handler or Codex hook).
    HookPayload(serde_json::Value),
    /// One line of `stream-json` / `codex exec --json` output.
    StreamLine(String),
    /// A JSON-RPC notification or server request from `codex app-server`.
    Rpc(serde_json::Value),
    /// A status line stdin document.
    StatusLine(serde_json::Value),
    /// Raw PTY bytes (generic adapter heuristics only).
    PtyChunk(Vec<u8>),
}

/// One adapter per agent kind. Object-safe so the daemon can hold a
/// `Box<dyn AgentAdapter>` per session.
pub trait AgentAdapter: Send + Sync {
    /// Which vendor.
    fn kind(&self) -> AgentKind;
    /// What this adapter can actually do — surfaced in the UI verbatim.
    fn capabilities(&self) -> Capabilities;
    /// Consume one input, emit zero or more normalised events.
    fn ingest(&mut self, input: AdapterInput) -> Vec<AgentEvent>;
    /// Answer a pending decision. Every deferred decision is watched by
    /// [`crate::watchdog`] because vendor prompts never time out.
    fn decide(&self, req: &ApprovalId, decision: Decision) -> Result<(), AdapterError>;
}

/// Adapter failures that are the user's to act on.
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    /// The approval id is unknown or already answered.
    #[error("approval {0:?} is not pending for this session")]
    UnknownApproval(ApprovalId),
}
