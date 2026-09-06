//! Block model shared by the shell and agent timelines.

/// A contiguous region of terminal history with a known meaning.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Block {
    /// What kind of region this is.
    pub kind: BlockKind,
    /// How sure we are about the boundaries.
    pub confidence: Confidence,
    /// First scrollback line (inclusive).
    pub start_line: u64,
    /// Last scrollback line (inclusive), `None` while still open.
    pub end_line: Option<u64>,
    /// Wall-clock time from the command's `C` to its `D`, when both were seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// The row the command's output starts on (`C`), so the command line's
    /// rows — wrapped or multi-line — are known exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_line: Option<u64>,
}

/// Kinds of block. Agent-event blocks are added in M5 from `vt-proto`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
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
    /// Output that arrived at an idle prompt with no command running and no
    /// keystrokes behind it: a background job (`&`, a process that outlived
    /// its command). Always [`Confidence::Heuristic`] — the terminal cannot
    /// know which process wrote it.
    Background,
}

/// Provenance of a block boundary. Surfaced in the UI verbatim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Delimited by well-formed shell-integration marks.
    Marked,
    /// Inferred by [`crate::heuristic`]; shown as a guess.
    Heuristic,
}
