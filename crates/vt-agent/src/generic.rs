//! Generic adapter for agents without a supported surface (docs/03 §6).
//! Heuristics only, always labelled a guess; the policy engine hard-refuses
//! to auto-answer for it.
//!
//! Signals, in the order the spec ranks them: OSC 133 prompt marks (the
//! terminal core reports them per row), terminal mode transitions seen in
//! the byte stream, idle detection (the caller decides when output has been
//! quiet), and prompt-shape matching with regex packs kept as data files.

use std::path::Path;

use regex::Regex;
use serde::Deserialize;
use vt_proto::agent::AgentState;

/// The pack shipped with the binary.
pub const BUILTIN_PACK: &str = include_str!("../packs/generic.json");

/// What a matched line looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PromptKind {
    /// A yes/no or free-text question.
    Question,
    /// A numbered or highlighted menu.
    Menu,
    /// "Press enter to continue".
    Confirm,
    /// A shell prompt: the agent process is not asking anything.
    Shell,
}

#[derive(Deserialize)]
struct PatternFile {
    name: String,
    kind: PromptKind,
    regex: String,
}

#[derive(Deserialize)]
struct PackFile {
    name: String,
    patterns: Vec<PatternFile>,
}

/// One compiled pattern.
#[derive(Debug)]
pub struct Pattern {
    /// Name from the pack.
    pub name: String,
    /// Kind.
    pub kind: PromptKind,
    re: Regex,
}

/// A named set of patterns.
#[derive(Debug)]
pub struct Pack {
    /// Pack name.
    pub name: String,
    /// Patterns in pack order.
    pub patterns: Vec<Pattern>,
}

impl Pack {
    /// Parse a pack document; a bad regex names the pattern in the error.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let file: PackFile =
            serde_json::from_str(text).map_err(|e| format!("pack is not valid JSON: {e}"))?;
        let mut patterns = Vec::with_capacity(file.patterns.len());
        for p in file.patterns {
            let re = Regex::new(&p.regex)
                .map_err(|e| format!("pack `{}` pattern `{}`: {e}", file.name, p.name))?;
            patterns.push(Pattern {
                name: p.name,
                kind: p.kind,
                re,
            });
        }
        Ok(Self {
            name: file.name,
            patterns,
        })
    }

    /// The built-in pack.
    ///
    /// # Panics
    /// Only if the checked-in `packs/generic.json` is invalid, which the
    /// unit tests rule out.
    pub fn builtin() -> Self {
        Self::from_json(BUILTIN_PACK).expect("built-in pack is valid")
    }
}

/// Load every `*.json` pack in `dir` (missing dir → empty). Broken packs are
/// reported, not fatal.
pub fn load_packs(dir: &Path) -> (Vec<Pack>, Vec<String>) {
    let mut packs = Vec::new();
    let mut problems = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (packs, problems);
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    for path in paths {
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| Pack::from_json(&t))
        {
            Ok(p) => packs.push(p),
            Err(e) => problems.push(format!("{}: {e}", path.display())),
        }
    }
    (packs, problems)
}

/// Terminal modes observed in the byte stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modes {
    /// Alternate screen active (`?1049h` / `?47h`).
    pub alt_screen: bool,
    /// Bracketed paste enabled (`?2004h`).
    pub bracketed_paste: bool,
    /// Kitty keyboard flags pushed (`CSI > … u`).
    pub kitty_keyboard: bool,
}

/// A state guess with the evidence behind it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Guess {
    /// Guessed state.
    pub state: AgentState,
    /// Why, in words a user can check against the screen.
    pub why: String,
    /// The question line, when one was recognised.
    pub question: Option<String>,
}

/// Stateful detector for one session.
#[derive(Debug)]
pub struct Detector {
    packs: Vec<Pack>,
    modes: Modes,
    carry: Vec<u8>,
    last: Option<(AgentState, String)>,
}

type ModeApply = fn(&mut Modes) -> Option<&'static str>;

const MODE_SEQS: &[(&[u8], ModeApply)] = &[
    (b"\x1b[?1049h", |m| {
        set(&mut m.alt_screen, true, "entered the alternate screen")
    }),
    (b"\x1b[?1049l", |m| {
        set(&mut m.alt_screen, false, "left the alternate screen")
    }),
    (b"\x1b[?47h", |m| {
        set(&mut m.alt_screen, true, "entered the alternate screen")
    }),
    (b"\x1b[?47l", |m| {
        set(&mut m.alt_screen, false, "left the alternate screen")
    }),
    (b"\x1b[?2004h", |m| {
        set(&mut m.bracketed_paste, true, "enabled bracketed paste")
    }),
    (b"\x1b[?2004l", |m| {
        set(&mut m.bracketed_paste, false, "disabled bracketed paste")
    }),
    (b"\x1b[>1u", |m| {
        set(&mut m.kitty_keyboard, true, "pushed kitty keyboard flags")
    }),
    (b"\x1b[<u", |m| {
        set(&mut m.kitty_keyboard, false, "popped kitty keyboard flags")
    }),
];

fn set(flag: &mut bool, to: bool, what: &'static str) -> Option<&'static str> {
    if *flag == to {
        None
    } else {
        *flag = to;
        Some(what)
    }
}

impl Detector {
    /// Detector over the given packs (the built-in one first).
    pub fn new(mut extra: Vec<Pack>) -> Self {
        let mut packs = vec![Pack::builtin()];
        packs.append(&mut extra);
        Self {
            packs,
            modes: Modes::default(),
            carry: Vec::new(),
            last: None,
        }
    }

    /// Modes seen so far.
    pub fn modes(&self) -> Modes {
        self.modes
    }

    /// Scan output bytes for mode transitions; returns descriptions of the
    /// ones that changed something. Sequences split across chunks are
    /// handled with a small carry.
    pub fn on_output(&mut self, bytes: &[u8]) -> Vec<&'static str> {
        let mut buf = std::mem::take(&mut self.carry);
        buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        let mut i = 0;
        while i < buf.len() {
            if buf[i] == 0x1b {
                let mut matched = 0;
                for (seq, apply) in MODE_SEQS {
                    if buf[i..].starts_with(seq) {
                        if let Some(what) = apply(&mut self.modes) {
                            out.push(what);
                        }
                        matched = seq.len();
                        break;
                    }
                }
                i += matched.max(1);
            } else {
                i += 1;
            }
        }
        // Keep a possible sequence prefix for the next chunk.
        let keep = buf.len().saturating_sub(12);
        if let Some(pos) = buf[keep..].iter().rposition(|b| *b == 0x1b) {
            self.carry = buf[keep + pos..].to_vec();
        }
        out
    }

    /// Output has been quiet: guess from the last screen lines. `prompt_mark`
    /// says the last used row carries an OSC 133 prompt mark. Returns `None`
    /// when the guess did not change since the last call.
    pub fn on_idle(&mut self, tail: &[String], prompt_mark: bool) -> Option<Guess> {
        let lines: Vec<&str> = tail
            .iter()
            .map(|l| l.trim_end())
            .filter(|l| !l.trim().is_empty())
            .collect();
        let guess = if prompt_mark {
            Guess {
                state: AgentState::Idle,
                why: "shell prompt (OSC 133 mark) after quiet output".into(),
                question: None,
            }
        } else {
            self.match_tail(&lines).unwrap_or_else(|| Guess {
                state: AgentState::Idle,
                why: if self.modes.alt_screen {
                    "quiet full-screen UI; nothing prompt-like recognised".into()
                } else {
                    "quiet; nothing prompt-like recognised".into()
                },
                question: None,
            })
        };
        let key = (guess.state, guess.why.clone());
        if self.last.as_ref() == Some(&key) {
            return None;
        }
        self.last = Some(key);
        Some(guess)
    }

    /// Output resumed: forget the last guess so the next idle re-reports.
    pub fn on_activity(&mut self) {
        self.last = None;
    }

    fn match_tail(&self, lines: &[&str]) -> Option<Guess> {
        let window: Vec<&str> = lines.iter().rev().take(4).copied().collect();
        for pack in &self.packs {
            for p in &pack.patterns {
                for line in &window {
                    if p.re.is_match(line) {
                        let (state, why) = match p.kind {
                            PromptKind::Question | PromptKind::Menu | PromptKind::Confirm => (
                                AgentState::AwaitingInput,
                                format!(
                                    "looks like a {} ({}/{}): {}",
                                    kind_name(p.kind),
                                    pack.name,
                                    p.name,
                                    line.trim()
                                ),
                            ),
                            PromptKind::Shell => (
                                AgentState::Idle,
                                format!("prompt-like last line ({}/{})", pack.name, p.name),
                            ),
                        };
                        let question = (state == AgentState::AwaitingInput).then(|| {
                            window
                                .iter()
                                .find(|l| l.trim_end().ends_with('?'))
                                .map_or_else(|| line.trim().to_string(), |q| q.trim().to_string())
                        });
                        return Some(Guess {
                            state,
                            why,
                            question,
                        });
                    }
                }
            }
        }
        None
    }
}

fn kind_name(k: PromptKind) -> &'static str {
    match k {
        PromptKind::Question => "question",
        PromptKind::Menu => "menu",
        PromptKind::Confirm => "confirmation",
        PromptKind::Shell => "shell prompt",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn builtin_pack_parses() {
        let p = Pack::builtin();
        assert_eq!(p.name, "generic");
        assert!(p.patterns.len() >= 5);
    }

    #[test]
    fn codex_and_claude_menus_are_questions() {
        let mut d = Detector::new(Vec::new());
        // Codex 0.153.2 approval prompt as captured live on 2026-09-05.
        let codex = lines(&[
            "  $ touch /tmp/vt-m0/codex/outside/live2.txt",
            "› 1. Yes, proceed (y)",
            "  2. Yes, and don't ask again for commands that start with `touch` (p)",
            "  3. No, and tell Codex what to do differently (esc)",
            "  Press enter to confirm or esc to cancel",
        ]);
        let g = d.on_idle(&codex, false).unwrap();
        assert_eq!(g.state, AgentState::AwaitingInput);
        assert!(g.why.contains("generic/"), "{}", g.why);
        // Same screen again: no new guess.
        assert!(d.on_idle(&codex, false).is_none());
        // Claude Code's prompt.
        let claude = lines(&["Do you want to proceed?", "❯ 1. Yes", "  2. No"]);
        let g = d.on_idle(&claude, false).unwrap();
        assert_eq!(g.state, AgentState::AwaitingInput);
        assert_eq!(g.question.as_deref(), Some("Do you want to proceed?"));
        // Plain y/N.
        let g = d
            .on_idle(&lines(&["Apply changes? (y/N) "]), false)
            .unwrap();
        assert_eq!(g.state, AgentState::AwaitingInput);
        assert_eq!(g.question.as_deref(), Some("Apply changes? (y/N)"));
    }

    #[test]
    fn prompt_marks_and_shell_prompts_are_idle() {
        let mut d = Detector::new(Vec::new());
        let g = d.on_idle(&lines(&["done", "$ "]), true).unwrap();
        assert_eq!(g.state, AgentState::Idle);
        assert!(g.why.contains("OSC 133"));
        let g = d
            .on_idle(&lines(&["build ok", "user@host ~ % "]), false)
            .unwrap();
        assert_eq!(g.state, AgentState::Idle);
        assert!(g.why.contains("prompt-like"));
        let g = d.on_idle(&lines(&["compiling 42 crates"]), false).unwrap();
        assert_eq!(g.state, AgentState::Idle);
        assert!(g.why.contains("nothing prompt-like"));
    }

    #[test]
    fn mode_transitions_survive_chunk_boundaries() {
        let mut d = Detector::new(Vec::new());
        assert_eq!(d.on_output(b"hello \x1b[?10"), Vec::<&str>::new());
        assert_eq!(
            d.on_output(b"49h\x1b[?2004h"),
            vec!["entered the alternate screen", "enabled bracketed paste"]
        );
        assert!(d.modes().alt_screen && d.modes().bracketed_paste);
        assert!(
            d.on_output(b"\x1b[?2004h").is_empty(),
            "no change, no report"
        );
        assert_eq!(
            d.on_output(b"\x1b[?1049l"),
            vec!["left the alternate screen"]
        );
    }

    #[test]
    fn broken_packs_are_reported_not_fatal() {
        let dir = std::env::temp_dir().join(format!("vt-packs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.json"),
            r#"{"name":"a","patterns":[{"name":"x","kind":"question","regex":"("}]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("b.json"),
            r#"{"name":"b","patterns":[{"name":"ok","kind":"confirm","regex":"^ok$"}]}"#,
        )
        .unwrap();
        let (packs, problems) = load_packs(&dir);
        assert_eq!(packs.len(), 1);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("pattern `x`"), "{problems:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
