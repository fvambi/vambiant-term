//! Spawning the child under a fresh PTY.
//!
//! Responsibilities (M1): `openpty`, fork/exec via `login -fp <user>` (or a
//! configured shell), `-q` when `~/.hushlogin` exists, `TERM`/`COLORTERM`
//! environment, `drain_on_exit` so a fast-exiting child's output is not
//! lost, and the controlling-terminal dance (`setsid`, `TIOCSCTTY`).

use std::path::PathBuf;

/// What to run and where.
#[derive(Clone, Debug)]
pub struct SpawnSpec {
    /// Program and arguments; empty means the user's login shell.
    pub argv: Vec<String>,
    /// Working directory.
    pub cwd: PathBuf,
    /// Extra environment entries layered over the inherited one.
    pub env: Vec<(String, String)>,
    /// Human-readable session name, used in error messages.
    pub session: String,
}
