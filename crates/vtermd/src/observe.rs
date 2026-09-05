//! Per-session observation that needs the byte stream or the grid: the
//! generic adapter's heuristics (docs/03 §6) and Claude `stream-json`
//! lines. Runs on the session thread; every verdict goes through
//! [`Agents`] so it is recorded and labelled like any other event.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use vt_agent::generic::Detector;
use vt_core::TerminalCore;
use vt_core::cell::PromptMark;
use vt_proto::agent::{AgentEvent, AgentKind, AgentState};
use vt_proto::session::{SessionId, SessionInfo};

use crate::registry::Registry;
use crate::wire;

/// Output must be quiet this long before the screen is read for a guess.
const IDLE_AFTER: Duration = Duration::from_millis(700);
/// A stdout line longer than this without a newline is not a JSON event.
const MAX_LINE: usize = 256 * 1024;

pub enum Observer {
    /// Claude / Codex headless: parse JSON lines out of the PTY stream.
    Stream { carry: Vec<u8> },
    /// Generic: heuristics over modes, idleness and screen text.
    Heuristic {
        detector: Detector,
        last_output: Instant,
        flowing: bool,
    },
    /// `[agents.generic] enabled = false`: watch nothing.
    Off,
}

impl Observer {
    pub fn new(registry: &Arc<Registry>, id: &SessionId, info: &Arc<Mutex<SessionInfo>>) -> Self {
        let kind = info.lock().unwrap_or_else(PoisonError::into_inner).agent;
        match kind {
            // Both vendors print one JSON object per line in headless mode
            // (`claude -p --output-format stream-json`, `codex exec --json`);
            // interactive TUIs never emit a line starting with `{"type":`.
            AgentKind::Claude | AgentKind::Codex => Self::Stream { carry: Vec::new() },
            AgentKind::Generic => {
                let generic = registry.cfg().agents.generic;
                if !generic.enabled {
                    return Self::Off;
                }
                let (mut packs, problems) = vt_agent::generic::load_packs(&registry.packs_dir());
                for p in problems {
                    eprintln!("vtermd: session {}: prompt pack skipped: {p}", id.0);
                }
                // `[agents.generic] packs` names the user packs that load; the
                // built-in one always does.
                packs.retain(|p| generic.packs.contains(&p.name));
                Self::Heuristic {
                    detector: Detector::new(packs),
                    last_output: Instant::now(),
                    flowing: false,
                }
            }
        }
    }

    pub fn on_output(&mut self, registry: &Registry, id: &SessionId, bytes: &[u8]) {
        match self {
            Self::Stream { carry } => {
                carry.extend_from_slice(bytes);
                while let Some(nl) = carry.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = carry.drain(..=nl).collect();
                    let text = String::from_utf8_lossy(&line);
                    let text = text.trim_end_matches(['\n', '\r']);
                    if text.starts_with("{\"type\":")
                        && let Some(agents) = registry.agents()
                    {
                        agents.ingest_stream_line(id, text.to_string());
                    }
                }
                if carry.len() > MAX_LINE {
                    carry.clear();
                }
            }
            Self::Heuristic {
                detector,
                last_output,
                flowing,
            } => {
                *last_output = Instant::now();
                let transitions = detector.on_output(bytes);
                if let Some(agents) = registry.agents() {
                    for t in transitions {
                        agents.record(
                            id,
                            &AgentEvent::Notification {
                                title: Some("terminal-mode".into()),
                                body: t.to_string(),
                            },
                        );
                    }
                    if !*flowing {
                        detector.on_activity();
                        agents.heuristic(id, AgentState::Thinking, "output flowing", None);
                    }
                }
                *flowing = true;
            }
            Self::Off => {}
        }
    }

    pub fn on_tick(&mut self, registry: &Registry, id: &SessionId, core: &mut dyn TerminalCore) {
        let Self::Heuristic {
            detector,
            last_output,
            flowing,
        } = self
        else {
            return;
        };
        if !*flowing || last_output.elapsed() < IDLE_AFTER {
            return;
        }
        *flowing = false;
        let snap = core.snapshot();
        let prompt_mark = wire::last_used_row(&snap).is_some_and(|(_, m)| m == PromptMark::Prompt);
        let tail: Vec<String> = wire::text(&snap, Some(6))
            .lines()
            .map(str::to_owned)
            .collect();
        if let Some(guess) = detector.on_idle(&tail, prompt_mark)
            && let Some(agents) = registry.agents()
        {
            agents.heuristic(id, guess.state, &guess.why, guess.question.as_deref());
        }
    }
}
