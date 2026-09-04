//! Wire types shared by `vtermd`, the app and `vterm` (serde).
//!
//! This is the vocabulary everything speaks: sessions, the normalised
//! [`AgentEvent`](agent::AgentEvent), approvals, usage. Adapters translate
//! *into* these types; the UI, inbox, policy engine and store consume only
//! them. Unknown fields on the wire are preserved, not rejected — a vendor
//! schema change must degrade to a warning, never a panic (ADR-0006).

pub mod agent;
pub mod approval;
pub mod jsonrpc;
pub mod session;
pub mod usage;
