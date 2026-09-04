//! PTY errors, always naming the session they belong to.

/// `"failed to spawn pty"` is useless; `"failed to spawn pty for session
/// api-refactor: openpty: too many open files"` is not (CLAUDE.md § Style).
#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    /// `openpty` or the fork/exec that follows failed.
    #[error("failed to spawn pty for session {session}: {stage}: {source}")]
    Spawn {
        /// Session name.
        session: String,
        /// Which syscall/step failed.
        stage: &'static str,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// `TIOCSWINSZ` failed.
    #[error("failed to resize pty for session {session}: {source}")]
    Resize {
        /// Session name.
        session: String,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
}
