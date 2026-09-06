//! `config.toml` (docs/09). Every field has the documented default;
//! unknown keys are errors, because a misspelt key that silently does
//! nothing is the failure mode the reference forbids.

use serde::{Deserialize, Serialize};

macro_rules! lower_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[allow(missing_docs)]
        pub enum $name { $(#[serde(rename = $s)] $variant),+ }
        impl $name {
            /// The accepted spellings, in declaration order.
            pub const OPTIONS: &'static [&'static str] = &[$($s),+];
            /// The wire spelling.
            #[must_use]
            pub fn as_str(self) -> &'static str { match self { $(Self::$variant => $s),+ } }
        }
    };
}

lower_enum!(ThinStrokes { Auto = "auto", Always = "always", Never = "never" });
lower_enum!(Decorations { Native = "native", None = "none" });
lower_enum!(TabBar { Native = "native", None = "none" });
lower_enum!(CursorStyle { Block = "block", Bar = "bar", Underline = "underline" });
lower_enum!(Inject { Auto = "auto", Manual = "manual", Off = "off" });
lower_enum!(InputMode { Warp = "warp", Classic = "classic" });
lower_enum!(KeymapProfile { Tmux = "tmux", Macos = "macos", Both = "both" });
lower_enum!(ResumeBy { Id = "id" });
lower_enum!(CodexTransport { AppServer = "app-server", Exec = "exec", HooksOnly = "hooks-only" });
lower_enum!(AgentFinished { Always = "always", WhenUnfocused = "when_unfocused", Never = "never" });
lower_enum!(EgressMode { None = "none", Redacted = "redacted", Full = "full" });
lower_enum!(Redaction { Always = "always", CloudOnly = "cloud_only" });

macro_rules! section {
    ($(#[$m:meta])* $name:ident { $($(#[$fm:meta])* $field:ident : $ty:ty = $default:expr),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(default, deny_unknown_fields)]
        #[allow(missing_docs)]
        pub struct $name { $($(#[$fm])* pub $field: $ty),+ }
        impl Default for $name {
            fn default() -> Self { Self { $($field: $default),+ } }
        }
    };
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

section!(Font {
    family: String = "Berkeley Mono".into(),
    fallback: Vec<String> = strings(&["SF Mono", "Menlo"]),
    size: f64 = 13.0,
    line_height: f64 = 1.2,
    cell_width: f64 = 1.0,
    ligatures: bool = true,
    ligature_disable: Vec<String> = strings(&["fl", "fi", "st"]),
    thin_strokes: ThinStrokes = ThinStrokes::Auto,
    bold_is_bright: bool = false,
});

section!(ThemeRef {
    name: String = "warp-dark".into(),
    light: String = "vambiant-light".into(),
    follow_system: bool = false,
});

section!(Input {
    mode: InputMode = InputMode::Warp,
});

section!(Padding {
    x: u32 = 8,
    y: u32 = 6
});
section!(Quake {
    enabled: bool = false,
    hotkey: String = "cmd+`".into()
});

section!(Window {
    padding: Padding = Padding::default(),
    opacity: f64 = 1.0,
    blur: u32 = 0,
    decorations: Decorations = Decorations::Native,
    tab_bar: TabBar = TabBar::Native,
    restore_session: bool = true,
    quake: Quake = Quake::default(),
});

section!(Cursor {
    style: CursorStyle = CursorStyle::Block,
    blink: bool = true,
    blink_interval_ms: u32 = 600,
});

section!(Osc {
    clipboard_write: bool = true,
    clipboard_read: bool = false,
    window_ops: bool = false,
    title_report: bool = false,
    hyperlinks: bool = true,
});

section!(Terminal {
    scrollback_lines: u32 = 100_000,
    shell: String = String::new(),
    term: String = "vambiant-term".into(),
    kitty_keyboard: bool = true,
    confirm_close_with_running_process: bool = true,
    copy_on_select: bool = false,
    osc: Osc = Osc::default(),
});

section!(Blocks {
    dividers: bool = true,
    failed_tint: bool = true,
    sticky_header: bool = true,
});

section!(ShellIntegration {
    enabled: bool = true,
    shells: Vec<String> = strings(&["zsh", "fish", "bash"]),
    inject: Inject = Inject::Auto,
    warn_on_conflict: bool = true,
    ssh_wrap: bool = false,
    sudo_wrap: bool = false,
});

section!(Mux {
    keymap_profile: KeymapProfile = KeymapProfile::Tmux,
    prefix: String = "ctrl+b".into(),
    detach_on_close: bool = true,
    default_layout: String = "single".into(),
});

section!(Claude {
    binary: String = "claude".into(),
    install_hooks: bool = true,
    install_statusline: bool = true,
    resume_by: ResumeBy = ResumeBy::Id,
    extra_args: Vec<String> = Vec::new(),
});

section!(Codex {
    binary: String = "codex".into(),
    transport: CodexTransport = CodexTransport::AppServer,
    extra_args: Vec<String> = Vec::new(),
});

section!(Generic {
    enabled: bool = true,
    packs: Vec<String> = strings(&["aider", "gemini-cli", "opencode", "cursor-cli"]),
});

section!(Agents {
    auto_detect: bool = true,
    adopt_external: bool = true,
    claude: Claude = Claude::default(),
    codex: Codex = Codex::default(),
    generic: Generic = Generic::default(),
});

section!(Notifications {
    awaiting_input: bool = true,
    agent_finished: AgentFinished = AgentFinished::WhenUnfocused,
    agent_crashed: bool = true,
    long_command_ms: u32 = 30_000,
    coalesce_window_ms: u32 = 10_000,
});

section!(Routes {
    suggest: String = "local-fast".into(),
    classify: String = "none".into(),
    ask: String = "claude-strong".into(),
    explain: String = "claude-strong".into(),
    search: String = "local-embed".into(),
});

section!(Budget {
    daily_usd: f64 = 5.0,
    monthly_usd: f64 = 60.0,
    hard_stop: bool = true,
});

section!(Context {
    include_git: bool = true,
    include_last_blocks: u32 = 3,
    include_env: bool = false,
    include_help_text: bool = true,
    max_bytes: u32 = 8000,
});

section!(Ai {
    enabled: bool = true,
    inline_suggest: bool = true,
    suggest_debounce_ms: u32 = 40,
    suggest_budget_ms: u32 = 120,
    explain_on_failure: bool = true,
    routes: Routes = Routes::default(),
    budget: Budget = Budget::default(),
    context: Context = Context::default(),
});

section!(Privacy {
    egress_mode: EgressMode = EgressMode::Redacted,
    redaction: Redaction = Redaction::Always,
    show_payload_first_use: bool = true,
    egress_log_days: u32 = 365,
    telemetry: bool = false,
    update_check: bool = false,
});

section!(Storage {
    block_retention_days: u32 = 90,
    event_retention_days: u32 = 90,
    scrollback_max_mb: u32 = 2048,
    prune_on_start: bool = true,
});

section!(Api {
    enabled: bool = true,
    bind: String = "127.0.0.1".into(),
    port: u16 = 7433,
});

section!(
    /// The whole of `config.toml`.
    Config {
        font: Font = Font::default(),
        theme: ThemeRef = ThemeRef::default(),
        window: Window = Window::default(),
        cursor: Cursor = Cursor::default(),
        terminal: Terminal = Terminal::default(),
        shell_integration: ShellIntegration = ShellIntegration::default(),
        blocks: Blocks = Blocks::default(),
        input: Input = Input::default(),
        mux: Mux = Mux::default(),
        agents: Agents = Agents::default(),
        notifications: Notifications = Notifications::default(),
        ai: Ai = Ai::default(),
        privacy: Privacy = Privacy::default(),
        storage: Storage = Storage::default(),
        api: Api = Api::default(),
    }
);

impl Config {
    /// Rules the schema types cannot express. Each error names the key.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.privacy.telemetry {
            errors
                .push("privacy.telemetry: must be false — there is no telemetry to turn on".into());
        }
        if self.api.bind != "127.0.0.1" && self.api.bind != "localhost" && self.api.bind != "::1" {
            errors.push(format!(
                "api.bind: {:?} is not loopback; the API only ever binds locally",
                self.api.bind
            ));
        }
        if !(1.0..=200.0).contains(&self.font.size) {
            errors.push(format!("font.size: {} is outside 1–200", self.font.size));
        }
        if !(0.5..=3.0).contains(&self.font.line_height) {
            errors.push(format!(
                "font.line_height: {} is outside 0.5–3.0",
                self.font.line_height
            ));
        }
        if !(0.0..=1.0).contains(&self.window.opacity) {
            errors.push(format!(
                "window.opacity: {} is outside 0–1",
                self.window.opacity
            ));
        }
        if self.ai.suggest_budget_ms == 0 {
            errors.push("ai.suggest_budget_ms: must be at least 1".into());
        }
        if self.api.port == 0 {
            errors.push("api.port: 0 is not a port".into());
        }
        if crate::keymap::parse_chord(&self.mux.prefix).is_err() {
            errors.push(format!(
                "mux.prefix: {:?} is not a key chord",
                self.mux.prefix
            ));
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_reference() {
        let c = Config::default();
        assert_eq!(c.font.family, "Berkeley Mono");
        assert!((c.font.size - 13.0).abs() < f64::EPSILON);
        assert_eq!(c.mux.keymap_profile, KeymapProfile::Tmux);
        assert_eq!(c.agents.codex.transport, CodexTransport::AppServer);
        assert!(!c.privacy.telemetry);
        assert_eq!(c.api.port, 7433);
        assert!(c.validate().is_empty());
    }

    #[test]
    fn unknown_keys_are_errors_naming_the_key() {
        let err = toml::from_str::<Config>("[font]\nsiez = 12\n").unwrap_err();
        assert!(err.to_string().contains("siez"), "{err}");
    }

    #[test]
    fn enums_reject_unlisted_spellings() {
        let err = toml::from_str::<Config>("[agents.codex]\ntransport = \"grpc\"\n").unwrap_err();
        assert!(err.to_string().contains("app-server"), "{err}");
        let ok: Config = toml::from_str("[agents.codex]\ntransport = \"hooks-only\"\n").unwrap();
        assert_eq!(ok.agents.codex.transport, CodexTransport::HooksOnly);
    }

    #[test]
    fn validation_names_the_offending_key() {
        let c: Config =
            toml::from_str("[privacy]\ntelemetry = true\n[api]\nbind = \"0.0.0.0\"\n").unwrap();
        let errors = c.validate();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].starts_with("privacy.telemetry"));
        assert!(errors[1].starts_with("api.bind"));
    }

    #[test]
    fn round_trips_through_toml() {
        let c = Config::default();
        let text = toml::to_string(&c).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back, c);
    }
}
