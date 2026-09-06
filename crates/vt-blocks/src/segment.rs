//! The mark-driven segmenter: OSC 133/633 marks in, [`Block`]s out.
//!
//! State machine over one prompt→command→output→finished cycle. A `633;E`
//! command line attaches to the command block it precedes. Marks that
//! arrive out of order (a `D` with no `C`, a second `A` mid-command) are
//! counted: past a threshold the integration is deemed corrupted and the
//! caller is told to fall back to heuristics — Powerlevel10k and starship
//! are the reason (docs/10 §10). No block is ever emitted with the wrong
//! confidence: mark-built blocks are `Marked`, nothing else.

use vt_core::ShellMark;

use crate::block::{Block, BlockKind, Confidence};

/// What the caller should do with the session's blocks after a mark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segmented {
    /// Nothing to hand over yet.
    Pending,
    /// A block just closed (a finished command, or a prompt that gave way
    /// to a command).
    Closed(Block),
    /// The marks are too disordered to trust; the caller should switch this
    /// session to heuristic segmentation and warn once. Carries the reason.
    Corrupted(String),
}

/// Threshold of out-of-order marks before a session is declared corrupted.
const CORRUPT_LIMIT: u32 = 3;

/// Consumes marks for one session and yields blocks.
#[derive(Debug, Default)]
pub struct Segmenter {
    open: Option<Open>,
    pending_cmdline: Option<String>,
    disorder: u32,
    corrupted: bool,
}

#[derive(Debug)]
struct Open {
    kind: OpenKind,
    start: u64,
    cmdline: Option<String>,
    /// `C` seen: the command is running, output belongs to it.
    executed: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum OpenKind {
    Prompt,
    Command,
}

impl Segmenter {
    /// Feeds one mark seen at absolute `row`.
    pub fn on_mark(&mut self, mark: &ShellMark, row: u64) -> Segmented {
        if self.corrupted {
            return Segmented::Pending;
        }
        match mark {
            ShellMark::CommandLine { cmdline, .. } => {
                // Attaches to the command block that follows (VS Code emits
                // E just before C).
                if let Some(open) = self.open.as_mut().filter(|o| o.kind == OpenKind::Command) {
                    open.cmdline = Some(cmdline.clone());
                } else {
                    self.pending_cmdline = Some(cmdline.clone());
                }
                Segmented::Pending
            }
            ShellMark::PromptStart { .. } => {
                let closed = self.close(row.saturating_sub(1));
                self.open = Some(Open {
                    kind: OpenKind::Prompt,
                    start: row,
                    cmdline: None,
                    executed: false,
                });
                closed
            }
            ShellMark::CommandStart => {
                // Prompt ends, command begins. B without a preceding A is
                // tolerated once (some shells only emit A on the first
                // prompt) by opening a command at this row.
                let closed = self.close_prompt(row.saturating_sub(1));
                self.open = Some(Open {
                    kind: OpenKind::Command,
                    start: row,
                    cmdline: self.pending_cmdline.take(),
                    executed: false,
                });
                closed
            }
            ShellMark::CommandExecuted => {
                // C should follow B; if there is no open command, that is
                // disorder but not fatal — open one here.
                if self.open.as_ref().map(|o| &o.kind) != Some(&OpenKind::Command) {
                    if let Some(c) = self.note_disorder("C without a command start") {
                        return c;
                    }
                    self.open = Some(Open {
                        kind: OpenKind::Command,
                        start: row,
                        cmdline: self.pending_cmdline.take(),
                        executed: false,
                    });
                }
                if let Some(open) = self.open.as_mut() {
                    open.executed = true;
                }
                Segmented::Pending
            }
            ShellMark::CommandFinished { exit } => match self.open.take() {
                Some(open) if open.kind == OpenKind::Command => Segmented::Closed(Block {
                    kind: BlockKind::Command {
                        cmdline: open.cmdline,
                        exit: *exit,
                    },
                    confidence: Confidence::Marked,
                    start_line: open.start,
                    end_line: Some(row),
                }),
                other => {
                    self.open = other;
                    self.note_disorder("D without a command")
                        .unwrap_or(Segmented::Pending)
                }
            },
            ShellMark::Malformed { payload } => self
                .note_disorder(&format!("malformed mark: {payload}"))
                .unwrap_or(Segmented::Pending),
        }
    }

    fn close(&mut self, end: u64) -> Segmented {
        match self.open.take() {
            Some(open) => Segmented::Closed(Self::block_of(open, end)),
            None => Segmented::Pending,
        }
    }

    fn close_prompt(&mut self, end: u64) -> Segmented {
        match self.open.take() {
            Some(open) if open.kind == OpenKind::Prompt => {
                Segmented::Closed(Self::block_of(open, end))
            }
            other => {
                self.open = other;
                Segmented::Pending
            }
        }
    }

    /// True between a prompt's `A` and the command's `C`: the shell is
    /// waiting for input, so unsolicited output is not a command's.
    #[must_use]
    pub fn at_prompt(&self) -> bool {
        !self.corrupted
            && self
                .open
                .as_ref()
                .is_some_and(|o| o.kind == OpenKind::Prompt || !o.executed)
    }

    /// Output (with a newline, no marks, no recent keystrokes) landed on
    /// rows `first..=last` while the shell sat at its prompt: a background
    /// block, flagged heuristic. Pending when the shell was busy.
    pub fn on_output(&mut self, first: u64, last: u64) -> Segmented {
        if !self.at_prompt() || last <= first {
            return Segmented::Pending;
        }
        Segmented::Closed(Block {
            kind: BlockKind::Background,
            confidence: Confidence::Heuristic,
            start_line: first,
            end_line: Some(last),
        })
    }

    fn block_of(open: Open, end: u64) -> Block {
        let kind = match open.kind {
            OpenKind::Prompt => BlockKind::Prompt,
            OpenKind::Command => BlockKind::Command {
                cmdline: open.cmdline,
                exit: None,
            },
        };
        Block {
            kind,
            confidence: Confidence::Marked,
            start_line: open.start,
            end_line: Some(end.max(open.start)),
        }
    }

    fn note_disorder(&mut self, why: &str) -> Option<Segmented> {
        self.disorder += 1;
        if self.disorder >= CORRUPT_LIMIT {
            self.corrupted = true;
            return Some(Segmented::Corrupted(format!(
                "shell integration marks are out of order ({why}); \
                 falling back to heuristic blocks (Powerlevel10k/starship?)"
            )));
        }
        None
    }

    /// Whether this session has been declared corrupted.
    #[must_use]
    pub fn is_corrupted(&self) -> bool {
        self.corrupted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(line: &str) -> ShellMark {
        ShellMark::CommandLine {
            cmdline: line.into(),
            nonce: None,
        }
    }

    #[test]
    fn background_output_is_a_heuristic_block_only_at_the_prompt() {
        let mut seg = Segmenter::default();
        assert!(!seg.at_prompt(), "nothing open yet");
        assert_eq!(seg.on_output(0, 3), Segmented::Pending);
        seg.on_mark(&ShellMark::PromptStart { params: vec![] }, 4);
        assert!(seg.at_prompt());
        seg.on_mark(&ShellMark::CommandStart, 4);
        assert!(seg.at_prompt(), "typing is still the prompt phase");
        assert_eq!(
            seg.on_output(4, 6),
            Segmented::Closed(Block {
                kind: BlockKind::Background,
                confidence: Confidence::Heuristic,
                start_line: 4,
                end_line: Some(6),
            })
        );
        assert_eq!(seg.on_output(6, 6), Segmented::Pending, "no new row");
        seg.on_mark(&ShellMark::CommandExecuted, 6);
        assert!(!seg.at_prompt(), "a running command owns its output");
        assert_eq!(seg.on_output(6, 9), Segmented::Pending);
    }

    #[test]
    fn one_clean_command_cycle() {
        let mut seg = Segmenter::default();
        assert_eq!(
            seg.on_mark(&ShellMark::PromptStart { params: vec![] }, 10),
            Segmented::Pending
        );
        // Prompt closes when the command starts.
        assert_eq!(
            seg.on_mark(&cmd("ls -la"), 10),
            Segmented::Pending,
            "E before B is buffered"
        );
        let closed = seg.on_mark(&ShellMark::CommandStart, 10);
        assert_eq!(
            closed,
            Segmented::Closed(Block {
                kind: BlockKind::Prompt,
                confidence: Confidence::Marked,
                start_line: 10,
                end_line: Some(10),
            })
        );
        assert_eq!(
            seg.on_mark(&ShellMark::CommandExecuted, 10),
            Segmented::Pending
        );
        let done = seg.on_mark(&ShellMark::CommandFinished { exit: Some(0) }, 14);
        assert_eq!(
            done,
            Segmented::Closed(Block {
                kind: BlockKind::Command {
                    cmdline: Some("ls -la".into()),
                    exit: Some(0)
                },
                confidence: Confidence::Marked,
                start_line: 10,
                end_line: Some(14),
            })
        );
        assert!(!seg.is_corrupted());
    }

    #[test]
    fn disorder_past_the_limit_declares_corruption_once() {
        let mut seg = Segmenter::default();
        for _ in 0..2 {
            assert_eq!(
                seg.on_mark(&ShellMark::CommandFinished { exit: None }, 1),
                Segmented::Pending
            );
        }
        match seg.on_mark(&ShellMark::CommandFinished { exit: None }, 1) {
            Segmented::Corrupted(why) => assert!(why.contains("out of order")),
            other => panic!("expected corruption, got {other:?}"),
        }
        assert!(seg.is_corrupted());
        // Once corrupted it stops emitting mark blocks.
        assert_eq!(
            seg.on_mark(&ShellMark::PromptStart { params: vec![] }, 2),
            Segmented::Pending
        );
    }

    #[test]
    fn command_line_attaches_when_it_arrives_mid_command() {
        let mut seg = Segmenter::default();
        seg.on_mark(&ShellMark::CommandStart, 5);
        seg.on_mark(&cmd("git status"), 5);
        seg.on_mark(&ShellMark::CommandExecuted, 5);
        let done = seg.on_mark(&ShellMark::CommandFinished { exit: Some(1) }, 8);
        assert_eq!(
            done,
            Segmented::Closed(Block {
                kind: BlockKind::Command {
                    cmdline: Some("git status".into()),
                    exit: Some(1)
                },
                confidence: Confidence::Marked,
                start_line: 5,
                end_line: Some(8),
            })
        );
    }
}
