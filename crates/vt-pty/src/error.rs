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
    /// The program or an argument contained a NUL byte.
    #[error("cannot spawn session {session}: argument contains a NUL byte: {what}")]
    NulByte {
        /// Session name.
        session: String,
        /// Offending argument, truncated.
        what: String,
    },
    /// `TIOCSWINSZ` failed.
    #[error("failed to resize pty for session {session} to {cols}x{rows}: {source}")]
    Resize {
        /// Session name.
        session: String,
        /// Requested columns.
        cols: u16,
        /// Requested rows.
        rows: u16,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// `waitpid` or `kill` failed.
    #[error("failed to {op} child {pid} of session {session}: {source}")]
    Child {
        /// Session name.
        session: String,
        /// Child pid.
        pid: i32,
        /// Operation.
        op: &'static str,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
}
