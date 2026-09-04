//! Durable state in `~/.local/state/vambiant-term/state.db` (SQLite, WAL).
//!
//! Our own event log is the record of truth for agent history — we never
//! depend on vendor transcript files (ADR-0006). Migrations exist from the
//! first schema; every version upgrades from the previous with real data
//! (docs/08 §8). Retention is a first-class setting and the sweep runs on
//! daemon start.

pub mod egress;
pub mod events;
pub mod migrations;
pub mod retention;
pub mod schema;
pub mod sessions;
