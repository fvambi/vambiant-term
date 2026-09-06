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
    /// Read or edit config.toml / keymap.toml through the daemon.
    Config {
        /// What to do.
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Print the resolved keymap and flag conflicts.
    Keys {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Ask the configured model a question with the terminal's context.
    Ask {
        /// The question (words are joined with spaces).
        #[arg(required = true)]
        prompt: Vec<String>,
        /// Session id or name whose cwd and last blocks are the context.
        #[arg(long)]
        session: Option<String>,
        /// Route (`ask`, `explain`); see config.toml [ai.routes].
        #[arg(long, default_value = "ask")]
        feature: String,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Safety classification of a command line (docs/05 §5), as the
    /// daemon would attach it to an approval request.
    Classify {
        /// The command line (words are joined with spaces).
        #[arg(required = true)]
        command: Vec<String>,
        /// Session id or name whose cwd is the context.
        #[arg(long)]
        session: Option<String>,
        /// Directory the command would run in (defaults to the session's cwd, else here).
        #[arg(long)]
        cwd: Option<String>,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Workflows: parameterised saved commands (Warp YAML, drop-in compatible).
    Workflow {
        /// What to do.
        #[command(subcommand)]
        cmd: WorkflowCmd,
    },
    /// Themes: import from Warp, Ghostty, Alacritty, iTerm2 or base16.
    Theme {
        /// What to do.
        #[command(subcommand)]
        cmd: ThemeCmd,
    },
    /// The egress log: what left the machine, redacted (docs/05 §4.2).
    Egress {
        /// What to do.
        #[command(subcommand)]
        cmd: EgressCmd,
    },
    /// Provider layer: keys, health, models.
    Ai {
        /// What to do.
        #[command(subcommand)]
        cmd: AiCmd,
    },
    /// Command blocks of a session (OSC 133/633 segmentation).
    Blocks {
        /// Session id or unique name.
        session: String,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
}

/// `vterm config …`.
#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    /// Where the files are and whether they parse.
    Path,
    /// The effective configuration.
    Show {
        /// Machine-readable output (includes field metadata).
        #[arg(long)]
        json: bool,
    },
    /// One value, e.g. `font.size`.
    Get {
        /// Dotted key.
        key: String,
    },
    /// Set one value; JSON values (numbers, true/false, `["lists"]`) are
    /// parsed, anything else is a string. Comments in the file are kept.
    Set {
        /// Dotted key.
        key: String,
        /// New value.
        value: String,
    },
    /// Bind a chord in keymap.toml (`--remove` to delete the override).
    Bind {
        /// Chord, e.g. `cmd+shift+a` or `prefix a`.
        chord: String,
        /// Action id (see `vterm keys`).
        action: Option<String>,
        /// Remove the override instead.
        #[arg(long)]
        remove: bool,
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
pub enum WorkflowCmd {
    /// Every workflow found for the current directory.
    List {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Print a workflow's command with arguments filled in (never runs it).
    Show {
        /// The workflow's name.
        name: String,
        /// `key=value` arguments.
        #[arg(long = "arg")]
        args: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ThemeCmd {
    /// Import a theme file into `themes/<name>.toml`.
    Import {
        /// The file (`.yaml` from ~/.warp/themes, a Ghostty config, `.toml`, `.itermcolors`, base16 `.yaml`).
        file: String,
        /// Name for the theme; defaults to the file name.
        #[arg(long)]
        name: Option<String>,
        /// Force a format: warp | ghostty | alacritty | iterm2 | base16.
        #[arg(long)]
        format: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum EgressCmd {
    /// Recent requests, newest first.
    Tail {
        /// How many.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Machine-readable output, payloads included.
        #[arg(long)]
        json: bool,
    },
    /// The last request sent on a session's behalf, exactly as it left.
    Last {
        /// Session id or name; omitted means requests made without a session.
        #[arg(long)]
        session: Option<String>,
    },
}

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

/// `vterm ai …`
#[derive(Subcommand, Debug)]
pub enum AiCmd {
    /// Print every profile with its key status and reachability.
    Doctor {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Store or remove a profile's API key in the Keychain.
    Key {
        /// What to do.
        #[command(subcommand)]
        cmd: KeyCmd,
    },
}

/// `vterm ai key …`
#[derive(Subcommand, Debug)]
pub enum KeyCmd {
    /// Read the key from stdin (never from an argument) and store it.
    Set {
        /// Profile name from providers.toml.
        profile: String,
    },
    /// Remove the stored key.
    Remove {
        /// Profile name from providers.toml.
        profile: String,
    },
}
