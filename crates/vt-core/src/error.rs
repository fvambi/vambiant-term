//! Error type for terminal-core operations.

/// Errors carry enough context for a user to act on (CLAUDE.md § Style).
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// A resize was requested with a zero dimension.
    #[error("cannot resize terminal to {cols}x{rows}: both dimensions must be non-zero")]
    InvalidSize {
        /// Requested columns.
        cols: u16,
        /// Requested rows.
        rows: u16,
    },
    /// The backend refused an operation; `what` names the call.
    #[error("terminal backend error in {what}: {detail}")]
    Backend {
        /// Backend call that failed.
        what: &'static str,
        /// Backend-supplied detail.
        detail: String,
    },
}
