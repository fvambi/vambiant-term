//! The [`AgentAdapter`] vocabulary (docs/03 §2).
//!
//! In M3 the daemon drives adapters through the free functions in
//! [`crate::hooks`], [`crate::claude`] and [`crate::codex`]; this trait is
//! the object-safe seam the daemon holds per session.

use vt_proto::agent::{AgentEvent, AgentKind};
use vt_proto::session::Capabilities;

use crate::hooks::Ingested;

/// Something an adapter can ingest.
#[derive(Clone, Debug)]
pub enum AdapterInput {
    /// A hook POST body (Claude `http` handler or Codex hook).
    HookPayload(serde_json::Value),
    /// A status line stdin document.
    StatusLine(serde_json::Value),
    /// One line of `stream-json` / `codex exec --json` output.
    StreamLine(String),
    /// A JSON-RPC notification or server request from `codex app-server`.
    Rpc(serde_json::Value),
    /// Raw PTY bytes (generic adapter heuristics only).
    PtyChunk(Vec<u8>),
}

/// One adapter per agent kind.
pub trait AgentAdapter: Send + Sync {
    /// Which vendor.
    fn kind(&self) -> AgentKind;
    /// What this adapter can actually do — surfaced in the UI verbatim.
    fn capabilities(&self) -> Capabilities;
    /// Consume one input, emit normalised events plus warnings.
    fn ingest(&mut self, input: AdapterInput) -> Ingested;
}

/// Claude Code adapter state.
#[derive(Debug, Default)]
pub struct ClaudeAdapter {
    counter: u64,
}

impl AgentAdapter for ClaudeAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Claude
    }

    fn capabilities(&self) -> Capabilities {
        crate::claude::CAPABILITIES
    }

    fn ingest(&mut self, input: AdapterInput) -> Ingested {
        self.counter += 1;
        match input {
            AdapterInput::HookPayload(v) => crate::hooks::ingest(&v, self.counter),
            AdapterInput::StatusLine(v) => Ingested {
                events: vec![AgentEvent::Usage(crate::claude::status_to_usage(&v))],
                warnings: Vec::new(),
                agent_session_id: v
                    .get("session_id")
                    .and_then(|s| s.as_str())
                    .map(str::to_owned),
            },
            AdapterInput::StreamLine(line) => {
                match serde_json::from_str::<serde_json::Value>(&line) {
                    Ok(v) => crate::hooks::ingest(&v, self.counter),
                    Err(e) => Ingested {
                        warnings: vec![crate::hooks::Warning(format!(
                            "unparseable stream line: {e}"
                        ))],
                        ..Ingested::default()
                    },
                }
            }
            AdapterInput::Rpc(_) | AdapterInput::PtyChunk(_) => Ingested::default(),
        }
    }
}

/// Generic adapter: observation only, honestly labelled.
#[derive(Debug, Default)]
pub struct GenericAdapter;

impl AgentAdapter for GenericAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Generic
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }

    fn ingest(&mut self, _input: AdapterInput) -> Ingested {
        Ingested::default()
    }
}
