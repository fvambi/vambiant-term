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
        /// Provision an agent adapter (claude); the program defaults to the agent binary.
        #[arg(long)]
        agent: Option<String>,
        /// Program and arguments.
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// The approval inbox.
    Inbox {
        /// What to do.
        #[command(subcommand)]
        cmd: InboxCmd,
    },
    /// Agent events of a session (from the daemon's own event log).
    Events {
        /// Session id or unique name.
        session: String,
        /// Only events after this sequence number.
        #[arg(long, default_value_t = 0)]
        after: i64,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Status-line relay for Claude Code (installed by the daemon; not for humans).
    #[command(hide = true)]
    Statusline {
        /// Receiver base URL.
        #[arg(long)]
        receiver: String,
        /// Session token.
        #[arg(long)]
        token: String,
        /// Subagent feed.
        #[arg(long)]
        subagent: bool,
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
    /// Start vtermd: through launchd when installed, else as a detached child.
    Start,
    /// Stop a launchd-managed vtermd (sessions become orphaned; see ADR-0004).
    Stop,
    /// Install the launchd LaunchAgent (KeepAlive) for the current user.
    Install {
        /// Path to the vtermd binary (default: next to this executable).
        #[arg(long)]
        vtermd: Option<PathBuf>,
    },
    /// Remove the launchd LaunchAgent.
    Uninstall,
}

/// `vterm inbox …`.
#[derive(Subcommand, Debug)]
pub enum InboxCmd {
    /// Pending approvals.
    List {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Allow a pending approval.
    Allow {
        /// Approval id (or `all`).
        id: String,
    },
    /// Deny a pending approval.
    Deny {
        /// Approval id (or `all`).
        id: String,
        /// Reason the agent sees.
        #[arg(long, default_value = "denied from the Vambiant Term inbox")]
        reason: String,
    },
    /// Edit the tool input, then allow (Claude Code `updatedInput`).
    Edit {
        /// Approval id.
        id: String,
        /// Replacement input as JSON; without it `$VISUAL`/`$EDITOR` opens.
        #[arg(long)]
        input: Option<String>,
    },
}
