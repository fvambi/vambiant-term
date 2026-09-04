//! Safety classes for typed and agent-proposed commands.

use serde::{Deserialize, Serialize};

/// Ordered from most to least benign. Comparison is meaningful.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyClass {
    /// Read-only, reversible.
    Safe,
    /// Writes inside the workspace.
    Mutating,
    /// Network egress, package installs, git history rewrites.
    Risky,
    /// Destructive or unbounded: `rm -rf /`, `curl | sh`, `eval "$CMD"`.
    Dangerous,
    /// Could not be parsed. Treated as [`SafetyClass::Dangerous`] or worse.
    Unparseable,
}
