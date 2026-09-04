//! Block model shared by the shell and agent timelines.

/// A contiguous region of terminal history with a known meaning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// What kind of region this is.
    pub kind: BlockKind,
    /// How sure we are about the boundaries.
    pub confidence: Confidence,
    /// First scrollback line (inclusive).
    pub start_line: u64,
    /// Last scrollback line (inclusive), `None` while still open.
    pub end_line: Option<u64>,
}

/// Kinds of block. Agent-event blocks are added in M5 from `vt-proto`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// Shell prompt (OSC 133 `A`..`B`).
    Prompt,
    /// A command and its output (OSC 133 `C`..`D`), with exit code when known.
    Command {
        /// Command line as reported by OSC 633 `E`, if any.
        cmdline: Option<String>,
        /// Exit status from OSC 133 `D;<exit>`.
        exit: Option<i32>,
    },
}

/// Provenance of a block boundary. Surfaced in the UI verbatim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    /// Delimited by well-formed shell-integration marks.
    Marked,
    /// Inferred by [`crate::heuristic`]; shown as a guess.
    Heuristic,
}
