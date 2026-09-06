//! Shell-integration marks (OSC 133 FinalTerm/iTerm2/Ghostty and OSC 633
//! VS Code) scanned out of the byte stream. The backend still receives
//! every byte — it needs the marks for its own semantic row tags — but only
//! this scanner knows the exit code in `D;<exit>` and the command line in
//! `633;E`, which no backend exposes.
//!
//! Parameters are parsed permissively: unknown keys are ignored, a
//! malformed sequence becomes [`ShellMark::Malformed`] rather than being
//! dropped, so corrupted integrations are visible (docs/10 §10).

/// One mark, in stream order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellMark {
    /// `133;A` — prompt starts. `params` are the `k=v` pairs after it.
    PromptStart {
        /// Raw `k=v` pairs, unknown keys included.
        params: Vec<(String, String)>,
    },
    /// `133;B` — user input starts.
    CommandStart,
    /// `133;C` — the command is executing; output follows.
    CommandExecuted,
    /// `133;D[;exit]` — the command finished.
    CommandFinished {
        /// Exit status when reported.
        exit: Option<i32>,
    },
    /// `633;E;<cmdline>[;nonce]` — the command text as the shell saw it.
    CommandLine {
        /// Unescaped command line.
        cmdline: String,
        /// Nonce, the only spoofing defence in this family.
        nonce: Option<String>,
    },
    /// A 133/633 sequence this scanner could not interpret.
    Malformed {
        /// The payload after `133;` / `633;`.
        payload: String,
    },
}

/// Incremental scanner; keeps state across `advance` chunks.
#[derive(Debug, Default)]
pub struct Scanner {
    state: State,
    payload: Vec<u8>,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    Escape,
    /// Inside an OSC; collecting until BEL or ESC \.
    Osc,
    /// Saw ESC inside an OSC: the next byte decides ST or not.
    OscEscape,
}

/// Longest payload kept; a runaway OSC is abandoned past this.
const MAX_PAYLOAD: usize = 64 * 1024;

impl Scanner {
    /// Scans `bytes`, returning each recognised mark with the byte offset
    /// just past its terminator (so the caller can feed the stream up to
    /// and including the mark before querying terminal state).
    pub fn scan(&mut self, bytes: &[u8]) -> Vec<(usize, ShellMark)> {
        let mut out = Vec::new();
        for (i, &b) in bytes.iter().enumerate() {
            match self.state {
                State::Ground => {
                    if b == 0x1B {
                        self.state = State::Escape;
                    }
                }
                State::Escape => {
                    self.state = if b == b']' {
                        self.payload.clear();
                        State::Osc
                    } else {
                        State::Ground
                    };
                }
                State::Osc => match b {
                    0x07 => self.finish(i + 1, &mut out),
                    0x1B => self.state = State::OscEscape,
                    _ => {
                        if self.payload.len() < MAX_PAYLOAD {
                            self.payload.push(b);
                        } else {
                            self.state = State::Ground;
                        }
                    }
                },
                State::OscEscape => {
                    if b == b'\\' {
                        self.finish(i + 1, &mut out);
                    } else {
                        // ESC inside an OSC that is not ST: the sequence is
                        // aborted and a new escape begins.
                        self.state = if b == b']' {
                            self.payload.clear();
                            State::Osc
                        } else {
                            State::Ground
                        };
                    }
                }
            }
        }
        out
    }

    fn finish(&mut self, end: usize, out: &mut Vec<(usize, ShellMark)>) {
        self.state = State::Ground;
        let payload = std::mem::take(&mut self.payload);
        if let Some(mark) = parse(&payload) {
            out.push((end, mark));
        }
    }
}

/// Parses one OSC payload (`133;A;cl=line`); non-shell OSCs give `None`.
#[must_use]
pub fn parse(payload: &[u8]) -> Option<ShellMark> {
    let text = String::from_utf8_lossy(payload);
    let (kind, rest) = match text.split_once(';') {
        Some((k, r)) => (k, r),
        None => (text.as_ref(), ""),
    };
    match kind {
        "133" => Some(parse_133(rest)),
        "633" => Some(parse_633(rest)),
        _ => None,
    }
}

fn params(rest: &str) -> Vec<(String, String)> {
    rest.split(';')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (k.to_owned(), v.to_owned()),
            None => (p.to_owned(), String::new()),
        })
        .collect()
}

fn parse_133(rest: &str) -> ShellMark {
    let (code, tail) = rest.split_once(';').unwrap_or((rest, ""));
    match code {
        // FinalTerm also defines P (prompt kind), I and N; treat them, like
        // A, as prompt starts so a prompt block still opens.
        "A" | "P" | "I" | "N" => ShellMark::PromptStart {
            params: params(tail),
        },
        "B" => ShellMark::CommandStart,
        "C" => ShellMark::CommandExecuted,
        "D" => ShellMark::CommandFinished {
            exit: tail.split(';').next().and_then(|s| s.trim().parse().ok()),
        },
        _ => ShellMark::Malformed {
            payload: rest.to_owned(),
        },
    }
}

fn parse_633(rest: &str) -> ShellMark {
    let (code, tail) = rest.split_once(';').unwrap_or((rest, ""));
    match code {
        "A" => ShellMark::PromptStart { params: Vec::new() },
        "B" => ShellMark::CommandStart,
        "C" => ShellMark::CommandExecuted,
        "D" => ShellMark::CommandFinished {
            exit: tail.split(';').next().and_then(|s| s.trim().parse().ok()),
        },
        "E" => {
            let (cmd, nonce) = match tail.rsplit_once(';') {
                Some((c, n)) if !n.is_empty() && !n.contains('\\') => (c, Some(n.to_owned())),
                _ => (tail, None),
            };
            ShellMark::CommandLine {
                cmdline: unescape_633(cmd),
                nonce,
            }
        }
        // P (property) carries cwd etc.; OSC 7 already covers cwd.
        "P" => ShellMark::PromptStart {
            params: params(tail),
        },
        _ => ShellMark::Malformed {
            payload: rest.to_owned(),
        },
    }
}

/// `633;E` escapes `;`, `\` and bytes ≤ 0x20 as `\xAB`.
fn unescape_633(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1] == b'x'
            && let Ok(v) = u8::from_str_radix(&s[i + 2..i + 4], 16)
        {
            out.push(char::from(v));
            i += 4;
            continue;
        }
        let ch = s[i..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_command_cycle_with_params_and_exit() {
        let mut s = Scanner::default();
        let bytes = b"\x1b]133;A;cl=line;aid=42\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07out\r\n\x1b]133;D;2\x1b\\";
        let marks = s.scan(bytes);
        assert_eq!(marks.len(), 4);
        assert_eq!(
            marks[0].1,
            ShellMark::PromptStart {
                params: vec![("cl".into(), "line".into()), ("aid".into(), "42".into())]
            }
        );
        assert_eq!(marks[0].0, b"\x1b]133;A;cl=line;aid=42\x07".len());
        assert_eq!(marks[1].1, ShellMark::CommandStart);
        assert_eq!(marks[2].1, ShellMark::CommandExecuted);
        assert_eq!(marks[3].1, ShellMark::CommandFinished { exit: Some(2) });
        assert_eq!(marks[3].0, bytes.len());
    }

    #[test]
    fn marks_survive_chunk_boundaries() {
        let mut s = Scanner::default();
        let whole = b"\x1b]133;D;0\x07";
        let mut all = Vec::new();
        for (i, chunk) in whole.chunks(3).enumerate() {
            for (off, m) in s.scan(chunk) {
                all.push((i, off, m));
            }
        }
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].2, ShellMark::CommandFinished { exit: Some(0) });
    }

    #[test]
    fn vscode_command_line_is_unescaped_with_nonce() {
        let mut s = Scanner::default();
        let m = s.scan(b"\x1b]633;E;echo\\x20a\\x3bb;n0nce\x07");
        assert_eq!(
            m[0].1,
            ShellMark::CommandLine {
                cmdline: "echo a;b".into(),
                nonce: Some("n0nce".into())
            }
        );
        let m = s.scan(b"\x1b]633;E;plain\x07");
        assert_eq!(
            m[0].1,
            ShellMark::CommandLine {
                cmdline: "plain".into(),
                nonce: None
            }
        );
        assert_eq!(
            s.scan(b"\x1b]633;D;1\x07")[0].1,
            ShellMark::CommandFinished { exit: Some(1) }
        );
    }

    #[test]
    fn other_oscs_are_ignored_and_bad_marks_are_reported() {
        let mut s = Scanner::default();
        assert!(
            s.scan(b"\x1b]0;title\x07\x1b]7;file:///tmp\x1b\\")
                .is_empty()
        );
        let m = s.scan(b"\x1b]133;Z;wat\x07\x1b]133;D;notanumber\x07");
        assert_eq!(
            m[0].1,
            ShellMark::Malformed {
                payload: "Z;wat".into()
            }
        );
        assert_eq!(m[1].1, ShellMark::CommandFinished { exit: None });
    }

    #[test]
    fn an_escape_inside_an_osc_aborts_it() {
        let mut s = Scanner::default();
        let m = s.scan(b"\x1b]133;A\x1b[0m\x1b]133;B\x07");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].1, ShellMark::CommandStart);
    }
}
