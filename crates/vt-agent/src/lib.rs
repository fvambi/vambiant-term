//! Agent adapters: turn vendor surfaces into normalised `AgentEvent`s.
//!
//! Governing rule (ADR-0006): **never build on a transcript file.** Live
//! observability comes only from hooks, structured stream events, the
//! status line feed and `codex app-server`. Both vendors' hook systems are
//! convergent enough that [`hooks`] is one abstraction serving both.
//!
//! Contract (docs/08 §5): an unknown event name or field yields a logged
//! warning and a degraded-but-correct event — never a panic, never a
//! silent drop.

pub mod adapter;
pub mod claude;
pub mod codex;
pub mod generic;
pub mod hooks;
pub mod watchdog;

pub use adapter::{AdapterInput, AgentAdapter};
