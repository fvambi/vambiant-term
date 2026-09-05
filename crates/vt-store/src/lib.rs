//! Durable state in `~/.local/state/vambiant-term/state.db` (SQLite, WAL).
//!
//! Our own event log is the record of truth for agent history — we never
//! depend on vendor transcript files (ADR-0006). Migrations exist from the
//! first schema; every version upgrades from the previous with real data
//! (docs/08 §8). Retention is a first-class setting and the sweep runs on
//! daemon start. A corrupt database is reported, never crash-looped.

pub mod egress;
pub mod error;
pub mod events;
pub mod migrations;
pub mod retention;
pub mod schema;
pub mod sessions;

pub use error::StoreError;
pub use schema::Store;
