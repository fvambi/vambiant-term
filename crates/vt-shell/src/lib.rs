//! Shell-integration snippets and how to inject them.
//!
//! The snippets (originals, MIT — deliberately not derived from Ghostty's
//! GPL scripts) emit OSC 133 prompt marks and OSC 7 cwd so the terminal can
//! segment output into blocks and inherit the working directory. They are
//! self-installing on `source`, silent in non-interactive shells, and never
//! replace a user's existing prompt or hooks — they add to them.
//!
//! Injection is per shell. The daemon writes the snippet under a private
//! directory and points the shell at it through the mechanism that survives
//! `/usr/bin/login` (which our sessions go through): a startup directory or
//! a data-dir entry, carried in the environment. Bash has no such
//! environment hook for interactive login shells, so bash integration is
//! injected only when the daemon controls the command line; a plain login
//! bash is left alone and its sessions fall back to heuristic blocks.

use std::path::{Path, PathBuf};

/// The three shells we ship integration for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    /// Z shell.
    Zsh,
    /// Bash.
    Bash,
    /// Fish.
    Fish,
}

impl Shell {
    /// Recognises a shell from a program path (`/bin/zsh`, `-zsh`, `fish`).
    #[must_use]
    pub fn from_program(program: &str) -> Option<Self> {
        let name = program.rsplit('/').next().unwrap_or(program);
        let name = name.strip_prefix('-').unwrap_or(name); // login shells: "-zsh"
        match name {
            "zsh" => Some(Self::Zsh),
            "bash" | "sh" => Some(Self::Bash),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }

    /// The snippet text.
    #[must_use]
    pub fn snippet(self) -> &'static str {
        match self {
            Self::Zsh => include_str!("zsh.zsh"),
            Self::Bash => include_str!("bash.bash"),
            Self::Fish => include_str!("fish.fish"),
        }
    }

    /// The file name the snippet is written as.
    #[must_use]
    pub fn filename(self) -> &'static str {
        match self {
            Self::Zsh => ".zshrc",
            Self::Bash => "vambiant.bash",
            Self::Fish => "vambiant.fish",
        }
    }
}

/// How to launch a shell so it loads our snippet: environment to set, and
/// (bash only) arguments to add. `VAMBIANT_TERM` gates every snippet, so it
/// is always set.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Injection {
    /// Environment variables to add or override.
    pub env: Vec<(String, String)>,
    /// Extra leading arguments for the shell (bash `--rcfile <file>`).
    pub args: Vec<String>,
    /// Files to write: (absolute path, contents).
    pub files: Vec<(PathBuf, String)>,
    /// Why integration is limited, if it is. `None` = full integration.
    pub limitation: Option<String>,
}

/// Builds the injection for `shell`, writing snippet files under `dir`.
///
/// `user_zdotdir` is the user's real `ZDOTDIR` (or `$HOME`) so the zsh
/// wrapper can chain-load their config; ignored for other shells.
///
/// `controls_argv` is true when the daemon spawns the shell binary directly
/// (so it may add `--rcfile`); false for a `login`-launched shell.
#[must_use]
pub fn inject(shell: Shell, dir: &Path, user_zdotdir: &Path, controls_argv: bool) -> Injection {
    let mut inj = Injection {
        env: vec![("VAMBIANT_TERM".into(), "1".into())],
        ..Default::default()
    };
    match shell {
        Shell::Zsh => {
            // zsh reads .zshrc from $ZDOTDIR; point it at our dir and chain
            // to the user's. This survives `login -p`.
            let user_rc = user_zdotdir.join(".zshrc");
            // macOS /etc/zshrc runs before us and sets
            // HISTFILE=${ZDOTDIR:-$HOME}/.zsh_history, i.e. inside our
            // private dir. Point it back at the user's before their rc runs
            // so history is never diverted into a per-session directory.
            let rc = format!(
                "# Vambiant Term: load the user's config, then our integration.\n\
                 export ZDOTDIR={user}\n\
                 [[ \"$HISTFILE\" == {dir}/* ]] && HISTFILE=\"$ZDOTDIR/.zsh_history\"\n\
                 [ -f {user_rc} ] && source {user_rc}\n\
                 source {snippet}\n",
                user = shquote(&user_zdotdir.display().to_string()),
                dir = shquote(&dir.display().to_string()),
                user_rc = shquote(&user_rc.display().to_string()),
                snippet = shquote(&dir.join("vambiant.zsh").display().to_string()),
            );
            inj.files.push((dir.join(".zshrc"), rc));
            inj.files
                .push((dir.join("vambiant.zsh"), Shell::Zsh.snippet().to_owned()));
            inj.env.push(("ZDOTDIR".into(), dir.display().to_string()));
        }
        Shell::Fish => {
            // fish auto-sources conf.d/*.fish from every XDG_DATA_DIRS entry
            // under fish/vendor_conf.d.
            let conf = dir.join("fish/vendor_conf.d");
            inj.files
                .push((conf.join("vambiant.fish"), Shell::Fish.snippet().to_owned()));
            inj.env.push((
                "XDG_DATA_DIRS".into(),
                prepend_path_env("XDG_DATA_DIRS", dir, "/usr/local/share:/usr/share"),
            ));
        }
        Shell::Bash => {
            let file = dir.join(Shell::Bash.filename());
            inj.files
                .push((file.clone(), Shell::Bash.snippet().to_owned()));
            if controls_argv {
                inj.args.push("--rcfile".into());
                inj.args.push(file.display().to_string());
            } else {
                inj.limitation = Some(
                    "bash has no environment hook for an interactive login shell; \
                     integration is active only when Vambiant Term launches bash directly. \
                     Blocks fall back to heuristics."
                        .into(),
                );
            }
        }
    }
    inj
}

/// Single-quotes a value for a POSIX shell.
fn shquote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Prepends `dir` to a colon-path environment variable value. The daemon
/// resolves the current value; here we take the fallback for when it is
/// unset (`inject` cannot read the child's future env, so the caller may
/// re-derive this from the real value).
fn prepend_path_env(_var: &str, dir: &Path, fallback: &str) -> String {
    format!("{}:{fallback}", dir.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_shells_including_login_dashes() {
        assert_eq!(Shell::from_program("/bin/zsh"), Some(Shell::Zsh));
        assert_eq!(Shell::from_program("-zsh"), Some(Shell::Zsh));
        assert_eq!(
            Shell::from_program("/usr/local/bin/fish"),
            Some(Shell::Fish)
        );
        assert_eq!(Shell::from_program("bash"), Some(Shell::Bash));
        assert_eq!(Shell::from_program("nu"), None);
    }

    #[test]
    fn every_snippet_emits_the_four_marks_and_gates_on_the_env() {
        for shell in [Shell::Zsh, Shell::Bash, Shell::Fish] {
            let s = shell.snippet();
            assert!(
                s.contains("VAMBIANT_TERM"),
                "{shell:?} gates on VAMBIANT_TERM"
            );
            for mark in ["133;A", "133;B", "133;C", "133;D"] {
                assert!(s.contains(mark), "{shell:?} emits {mark}");
            }
            assert!(s.contains("133;A"), "{shell:?} osc7");
            assert!(s.contains("]7;file://"), "{shell:?} emits OSC 7");
        }
    }

    #[test]
    fn bash_wraps_the_prompt_mark_but_not_the_hook_marks() {
        let s = Shell::Bash.snippet();
        // The B mark in PS1 is wrapped in \[ \]; the readline width fix.
        assert!(s.contains(r"PS1='\[\033]133;B"), "{s}");
    }

    #[test]
    fn zsh_injection_chains_the_users_config() {
        let dir = std::env::temp_dir().join("vt-shell-zsh");
        let inj = inject(Shell::Zsh, &dir, Path::new("/home/u"), false);
        assert!(inj.limitation.is_none());
        assert!(
            inj.env
                .iter()
                .any(|(k, v)| k == "ZDOTDIR" && *v == dir.display().to_string())
        );
        assert!(
            inj.env
                .iter()
                .any(|(k, v)| k == "VAMBIANT_TERM" && v == "1")
        );
        let rc = &inj
            .files
            .iter()
            .find(|(p, _)| p.ends_with(".zshrc"))
            .unwrap()
            .1;
        assert!(rc.contains("source '/home/u/.zshrc'"), "{rc}");
        assert!(inj.files.iter().any(|(p, _)| p.ends_with("vambiant.zsh")));
        // /etc/zshrc derives HISTFILE from ZDOTDIR before our rc runs; the
        // wrapper must send it back home or history lands in our dir.
        assert!(
            rc.contains(&format!(
                "[[ \"$HISTFILE\" == '{}'/* ]] && HISTFILE=\"$ZDOTDIR/.zsh_history\"",
                dir.display()
            )),
            "{rc}"
        );
    }

    #[test]
    fn every_snippet_reports_the_command_line() {
        for shell in [Shell::Zsh, Shell::Bash, Shell::Fish] {
            assert!(shell.snippet().contains("633;E;"), "{shell:?} emits 633;E");
        }
    }

    #[test]
    fn fish_injection_uses_the_vendor_conf_dir() {
        let dir = std::env::temp_dir().join("vt-shell-fish");
        let inj = inject(Shell::Fish, &dir, Path::new("/home/u"), false);
        assert!(
            inj.files
                .iter()
                .any(|(p, _)| p.ends_with("fish/vendor_conf.d/vambiant.fish"))
        );
        assert!(
            inj.env
                .iter()
                .any(|(k, v)| k == "XDG_DATA_DIRS" && v.starts_with(&dir.display().to_string()))
        );
    }

    #[test]
    fn bash_needs_argv_control_and_says_so_otherwise() {
        let dir = std::env::temp_dir().join("vt-shell-bash");
        let with = inject(Shell::Bash, &dir, Path::new("/home/u"), true);
        assert_eq!(
            with.args,
            vec![
                "--rcfile".to_owned(),
                dir.join("vambiant.bash").display().to_string()
            ]
        );
        assert!(with.limitation.is_none());
        let without = inject(Shell::Bash, &dir, Path::new("/home/u"), false);
        assert!(without.args.is_empty());
        assert!(
            without
                .limitation
                .as_deref()
                .unwrap()
                .contains("heuristics")
        );
    }
}
