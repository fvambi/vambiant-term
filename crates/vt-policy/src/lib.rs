//! Command safety classification and the autonomy policy engine (ADR-0009).
//!
//! Two invariants that no configuration can change:
//! * the **never-auto floor** in [`floor`] is hard-coded — no config file,
//!   global or per-repo, can widen it (CLAUDE.md non-negotiable 3);
//! * unparseable input classifies as **unsafe**, never benign.
//!
//! Every autonomous behaviour ships defaulted off and is opt-in per
//! workspace, per rule, with an audit log and one-key undo (docs/00 §7).
//! Per-repo policy files can only *narrow* the global policy.

mod checks;
pub mod classify;
pub mod floor;
mod paths;
pub mod rules;
pub mod shell;
pub mod workspace;

pub use classify::{Context, Finding, SafetyClass, Verdict, classify};
pub use floor::FloorReason;
pub use rules::{Decide, Decision, Outcome, Policy, PolicyError, ToolRequest, evaluate};
