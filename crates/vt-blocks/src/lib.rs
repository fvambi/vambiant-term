//! Turns a terminal byte/mark stream into a stream of semantic [`Block`]s.
//!
//! Primary signal is OSC 133 (and VS Code's OSC 633 superset). When marks
//! are missing or malformed — Powerlevel10k and starship both corrupt them
//! (docs/10 §10) — segmentation falls back to `vt-agent`'s generic detector and the session
//! is flagged so the UI can say so. A wrong block boundary is labelled a
//! guess, never presented as fact (CLAUDE.md non-negotiable 4).

pub mod block;
pub mod segment;

pub use block::{Block, BlockKind, Confidence};
pub use segment::{Segmented, Segmenter};
