//! Session records as seen by every client.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::agent::{AgentKind, AgentState};

/// Stable daemon-side session id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    /// `true` when the daemon could not re-adopt the PTY after a restart.
    /// Never dropped silently (ADR-0004).
    pub orphaned: bool,
}
