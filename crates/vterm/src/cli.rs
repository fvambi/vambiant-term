//! Command-line surface (clap derive).

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Vambiant Term CLI — supervise agent sessions owned by `vtermd`.
#[derive(Parser, Debug)]
#[command(name = "vterm", version, about)]
pub struct Cli {
    /// Daemon socket (default: $TMPDIR/vambiant-term-<uid>/vtermd.sock).
    #[arg(long, global = true)]
    pub socket: Option<PathBuf>,
    /// Subcommand.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level commands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Daemon lifecycle.
    Daemon {
        /// What to do.
        #[command(subcommand)]
        cmd: DaemonCmd,
    },
    /// List sessions.
    Ls {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Start a new session (login shell unless a program is given after `--`).
    New {
        /// Display name.
        #[arg(long, short)]
        name: Option<String>,
        /// Working directory (default: current).
        #[arg(long)]
        cwd: Option<PathBuf>,
        /// Initial columns.
        #[arg(long, default_value_t = 80)]
        cols: u16,
        /// Initial rows.
        #[arg(long, default_value_t = 24)]
        rows: u16,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
        /// Program and arguments.
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// Attach to a session in this terminal (detach with Ctrl-\ then d).
    Attach {
        /// Session id or unique name.
        session: String,
    },
    /// Send a signal to a session's process (SIGHUP by default).
    Kill {
        /// Session id or unique name.
        session: String,
        /// Signal number.
        #[arg(long, default_value_t = 1)]
        signal: i32,
    },
    /// Rename a session.
    Rename {
        /// Session id or unique name.
        session: String,
        /// New name.
        name: String,
    },
    /// Print the visible grid of a session as text.
    Logs {
        /// Session id or unique name.
        session: String,
        /// Only the last N lines.
        #[arg(long)]
        lines: Option<usize>,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Write text to a session's input (no newline appended).
    Send {
        /// Session id or unique name.
        session: String,
        /// Text to send; escapes are not interpreted.
        text: String,
    },
}

/// `vterm daemon …`.
#[derive(Subcommand, Debug)]
pub enum DaemonCmd {
    /// Is the daemon reachable?
    Status {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Start vtermd (foreground child of this shell until launchd install lands).
    Start,
}
