//! The per-session thread: owns the adopted PTY and the terminal core,
//! flushes damage to viewers at display cadence, persists lifecycle changes.

use std::io::Write;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use vt_blocks::{Segmented, Segmenter};
use vt_core::backend::GhosttyCore;
use vt_core::cell::{CellSnapshot, GridSize};
use vt_core::damage::DamageSet;
use vt_core::key::KeyEvent;
use vt_core::{TermEvent, TerminalCore};
use vt_proto::agent::AgentState;
use vt_proto::session::{SessionInfo, notification};
use vt_pty::{Pty, WinSize};

use crate::holder::{Frame, Held, read_frame};
use crate::observe::Observer;
use crate::registry::{Registry, now};
use crate::wire;

/// Minimum interval between output deltas (one frame at 120 Hz).
const FLUSH_INTERVAL: Duration = Duration::from_millis(8);

/// Commands other threads send to a session.
pub enum SessionCmd {
    /// Raw bytes to the PTY.
    Input(Vec<u8>),
    /// A key event to encode with the terminal's current modes.
    Key(KeyEvent),
    /// New grid size.
    Resize(u16, u16),
    /// Grid snapshot request.
    Snapshot(Sender<CellSnapshot>),
    /// Send a signal to the child.
    Signal(i32),
    /// Rename.
    Rename(String),
}

enum Wake {
    /// Output buffered by the holder before we attached (grid rebuild).
    Replay(Vec<u8>),
    Bytes(Vec<u8>),
    Eof,
    Exited(Option<i32>),
    Cmd(SessionCmd),
}

/// Start the session thread over a held PTY; returns the command channel.
pub fn start(
    registry: Arc<Registry>,
    info: Arc<Mutex<SessionInfo>>,
    held: Held,
) -> Result<Sender<SessionCmd>, String> {
    let snapshot = info.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let (cols, rows) = snapshot.size.unwrap_or((80, 24));
    let Held {
        pty,
        control,
        already_exited,
        ..
    } = held;
    {
        let mut i = info.lock().unwrap_or_else(PoisonError::into_inner);
        i.pid = Some(pty.pid());
        i.state = AgentState::Idle;
    }
    let (wake_tx, wake_rx) = mpsc::channel::<Wake>();
    let (cmd_tx, cmd_rx) = mpsc::channel::<SessionCmd>();

    // Reader thread: frames from the holder (replay, output, exit).
    let mut control_reader = control;
    let wake_frames = wake_tx.clone();
    if let Some(exit) = already_exited {
        let _ = wake_frames.send(Wake::Exited(exit.code));
    }
    thread::Builder::new()
        .name(format!("hold-read-{}", snapshot.id.0))
        .spawn(move || {
            loop {
                match read_frame(&mut control_reader) {
                    Ok(Some(Frame::Replay(bytes))) => {
                        if wake_frames.send(Wake::Replay(bytes)).is_err() {
                            break;
                        }
                    }
                    Ok(Some(Frame::Output(bytes))) => {
                        if wake_frames.send(Wake::Bytes(bytes)).is_err() {
                            break;
                        }
                    }
                    Ok(Some(Frame::Exit(exit))) => {
                        let _ = wake_frames.send(Wake::Exited(exit.code));
                        let _ = wake_frames.send(Wake::Eof);
                        break;
                    }
                    Ok(None) | Err(_) => {
                        let _ = wake_frames.send(Wake::Eof);
                        break;
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    // Command forwarder: multiplexes commands into the same wake channel.
    let wake_cmds = wake_tx;
    thread::Builder::new()
        .name(format!("cmd-{}", snapshot.id.0))
        .spawn(move || {
            for c in cmd_rx {
                if wake_cmds.send(Wake::Cmd(c)).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;

    thread::Builder::new()
        .name(format!("session-{}", snapshot.id.0))
        .spawn(move || run(&registry, &info, &pty, &wake_rx, GridSize { cols, rows }))
        .map_err(|e| e.to_string())?;
    Ok(cmd_tx)
}

#[allow(clippy::too_many_lines)]
fn run(
    registry: &Arc<Registry>,
    info: &Arc<Mutex<SessionInfo>>,
    pty: &Pty,
    wake: &Receiver<Wake>,
    size: GridSize,
) {
    let id = info
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .id
        .clone();
    let mut core = match GhosttyCore::new(size) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("vtermd: session {}: cannot create terminal core: {e}", id.0);
            return;
        }
    };
    let mut writer = match pty.writer() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("vtermd: session {}: {e}", id.0);
            return;
        }
    };
    let fresh = !info
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .readopted;
    let mut observer = Observer::new(registry, &id, info);
    let mut segmenter = Segmenter::default();
    let mut pending = DamageSet::Full;
    let mut last_flush = Instant::now()
        .checked_sub(FLUSH_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut seq: u64 = 0;
    let mut eof = false;
    let mut exit_code: Option<Option<i32>> = None;
    let mut eof_at: Option<Instant> = None;

    loop {
        let timeout = if pending.is_clean() {
            Duration::from_millis(250)
        } else {
            FLUSH_INTERVAL
        };
        match wake.recv_timeout(timeout) {
            Ok(Wake::Replay(bytes)) => {
                // Rebuild the grid from what the holder buffered. Query
                // responses and events generated by the replay are stale —
                // but for a session we just spawned the replay is simply
                // its first output, which the observer must see.
                if !bytes.is_empty() {
                    if fresh {
                        observer.on_output(registry, &id, &bytes);
                    }
                    core.advance(&bytes);
                    let _ = core.take_responses();
                    // For a fresh session the replay is its first output, so
                    // its shell marks are real; for a re-adopted holder they
                    // are already-seen history and must not re-emit blocks.
                    for ev in core.take_events() {
                        if fresh && let TermEvent::ShellMark { mark, row } = &ev {
                            segment(registry, &id, &mut segmenter, mark, *row);
                        }
                    }
                    let _ = core.take_damage();
                    pending = DamageSet::Full;
                }
            }
            Ok(Wake::Bytes(bytes)) => {
                observer.on_output(registry, &id, &bytes);
                core.advance(&bytes);
                let responses = core.take_responses();
                if !responses.is_empty() {
                    let _ = writer.write_all(&responses);
                }
                merge_damage(&mut pending, core.take_damage());
                for ev in core.take_events() {
                    if let TermEvent::ShellMark { mark, row } = &ev {
                        segment(registry, &id, &mut segmenter, mark, *row);
                    }
                    publish_event(registry, &id, info, &ev);
                }
            }
            Ok(Wake::Cmd(cmd)) => match cmd {
                SessionCmd::Input(bytes) => {
                    let _ = writer.write_all(&bytes);
                }
                SessionCmd::Key(key) => {
                    let bytes = core.encode_key(&key);
                    if !bytes.is_empty() {
                        let _ = writer.write_all(&bytes);
                    }
                }
                SessionCmd::Resize(cols, rows) => {
                    if core.resize(GridSize { cols, rows }).is_ok() {
                        let _ = pty.resize(WinSize::cells(cols, rows));
                        info.lock().unwrap_or_else(PoisonError::into_inner).size =
                            Some((cols, rows));
                        pending = DamageSet::Full;
                    }
                }
                SessionCmd::Snapshot(reply_to) => {
                    let _ = reply_to.send(core.snapshot());
                }
                SessionCmd::Signal(sig) => {
                    let _ = pty.signal(sig);
                }
                SessionCmd::Rename(name) => {
                    info.lock().unwrap_or_else(PoisonError::into_inner).name = name;
                    persist(registry, info);
                    if let Some(server) = registry.server() {
                        let i = info.lock().unwrap_or_else(PoisonError::into_inner).clone();
                        server
                            .broadcast(notification::SESSION_CHANGED, serde_json::to_value(i).ok());
                    }
                }
            },
            Ok(Wake::Eof) => {
                eof = true;
                eof_at.get_or_insert_with(Instant::now);
            }
            Ok(Wake::Exited(reported)) => exit_code = Some(reported),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        observer.on_tick(registry, &id, &mut core);
        if !pending.is_clean() && last_flush.elapsed() >= FLUSH_INTERVAL {
            seq += 1;
            let delta = wire::delta(&id, &core.snapshot(), &pending, seq);
            pending = DamageSet::Lines(Vec::new());
            last_flush = Instant::now();
            if let Some(server) = registry.server() {
                server.publish(
                    &id.0,
                    notification::SESSION_OUTPUT,
                    serde_json::to_value(delta).ok(),
                );
            }
        }

        // The session ends when the PTY drained and the holder reported the
        // exit — or, if the holder is silent, two seconds after EOF.
        let holder_silent = eof_at.is_some_and(|t| t.elapsed() > Duration::from_secs(2));
        if eof && (exit_code.is_some() || holder_silent) {
            let final_code = exit_code.flatten();
            {
                let mut i = info.lock().unwrap_or_else(PoisonError::into_inner);
                i.state = AgentState::Stopped;
                i.pid = None;
            }
            if let Ok(store) = registry.store().lock() {
                let last = wire::text(&core.snapshot(), None);
                let _ = store.end_session_with_output(&id, final_code, &now(), Some(&last));
            }
            if let Some(server) = registry.server() {
                server.broadcast(
                    notification::SESSION_EXITED,
                    Some(serde_json::json!({ "id": id.0, "exit_code": final_code })),
                );
                let i = info.lock().unwrap_or_else(PoisonError::into_inner).clone();
                server.broadcast(notification::SESSION_CHANGED, serde_json::to_value(i).ok());
            }
            registry.remove(&id);
            break;
        }
    }
}

fn merge_damage(pending: &mut DamageSet, new: DamageSet) {
    match (&mut *pending, new) {
        (DamageSet::Full, _) | (_, DamageSet::Full) => *pending = DamageSet::Full,
        (DamageSet::Lines(acc), DamageSet::Lines(more)) => {
            for l in more {
                match acc.iter_mut().find(|a| a.row == l.row) {
                    Some(a) => {
                        a.left = a.left.min(l.left);
                        a.right = a.right.max(l.right);
                    }
                    None => acc.push(l),
                }
            }
        }
    }
}

/// Feed one mark to the segmenter; persist and broadcast a closed block,
/// and warn once when the marks turn out to be corrupted.
fn segment(
    registry: &Registry,
    id: &vt_proto::session::SessionId,
    segmenter: &mut Segmenter,
    mark: &vt_core::ShellMark,
    row: u64,
) {
    match segmenter.on_mark(mark, row) {
        Segmented::Pending => {}
        Segmented::Closed(block) => {
            let now = crate::registry::now();
            if let Ok(store) = registry.store().lock() {
                let _ = store.append_block(id, &now, &block);
            }
            if let Some(server) = registry.server() {
                server.broadcast(
                    notification::SESSION_BLOCK,
                    serde_json::to_value(&block)
                        .ok()
                        .map(|b| serde_json::json!({ "id": id.0, "block": b })),
                );
            }
        }
        Segmented::Corrupted(why) => {
            eprintln!("vtermd: session {}: {why}", id.0);
            if let Some(server) = registry.server() {
                server.broadcast(
                    notification::SESSION_EVENT,
                    Some(serde_json::json!({
                        "id": id.0,
                        "event": { "kind": "blocks_degraded", "reason": why }
                    })),
                );
            }
        }
    }
}

fn publish_event(
    registry: &Registry,
    id: &vt_proto::session::SessionId,
    info: &Arc<Mutex<SessionInfo>>,
    ev: &TermEvent,
) {
    if let TermEvent::Pwd(p) = ev {
        info.lock().unwrap_or_else(PoisonError::into_inner).cwd = p.into();
        persist(registry, info);
    }
    if let Some(server) = registry.server() {
        let payload = match ev {
            TermEvent::Bell => serde_json::json!({ "kind": "bell" }),
            TermEvent::Title(t) => serde_json::json!({ "kind": "title", "title": t }),
            TermEvent::Pwd(p) => serde_json::json!({ "kind": "pwd", "path": p }),
            TermEvent::ClipboardWrite { target, contents } => serde_json::json!({
                "kind": "clipboard_write",
                "target": format!("{target:?}").to_lowercase(),
                "bytes": contents.len(),
            }),
            // Marks drive the segmenter (see `segment`), not the event feed.
            TermEvent::ShellMark { .. } => return,
        };
        server.broadcast(
            notification::SESSION_EVENT,
            Some(serde_json::json!({ "id": id.0, "event": payload })),
        );
    }
}

fn persist(registry: &Registry, info: &Arc<Mutex<SessionInfo>>) {
    let i = info.lock().unwrap_or_else(PoisonError::into_inner).clone();
    if let Ok(store) = registry.store().lock()
        && let Ok(Some(mut rec)) = store.session(&i.id)
    {
        rec.info = i;
        let _ = store.upsert_session(&rec);
    }
}
