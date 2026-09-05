//! The per-session thread: owns the PTY and the terminal core, flushes damage
//! to viewers at display cadence, persists lifecycle changes.

use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use vt_core::backend::GhosttyCore;
use vt_core::cell::{CellSnapshot, GridSize};
use vt_core::damage::DamageSet;
use vt_core::key::KeyEvent;
use vt_core::{TermEvent, TerminalCore};
use vt_proto::agent::AgentState;
use vt_proto::session::{NewSession, SessionInfo, notification};
use vt_pty::{Pty, SpawnSpec, WinSize};

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
    Bytes(Vec<u8>),
    Eof,
    Cmd(SessionCmd),
}

/// Spawn the PTY and the session thread; returns the command channel.
pub fn spawn(
    registry: Arc<Registry>,
    info: Arc<Mutex<SessionInfo>>,
    req: &NewSession,
) -> Result<Sender<SessionCmd>, String> {
    let snapshot = info.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let (cols, rows) = snapshot.size.unwrap_or((80, 24));
    let spec = SpawnSpec {
        argv: req.argv.clone(),
        cwd: Some(snapshot.cwd.clone()),
        env: {
            let mut env = req.env.clone();
            env.push(("VAMBIANT_TERM_SESSION".into(), snapshot.id.0.clone()));
            env.push(("TERM".into(), "xterm-256color".into()));
            env
        },
        session: snapshot.name.clone(),
    };
    let pty = Pty::spawn(&spec, WinSize::cells(cols, rows)).map_err(|e| e.to_string())?;
    {
        let mut i = info.lock().unwrap_or_else(PoisonError::into_inner);
        i.pid = Some(pty.pid());
        i.state = AgentState::Idle;
    }
    let (wake_tx, wake_rx) = mpsc::channel::<Wake>();
    let (cmd_tx, cmd_rx) = mpsc::channel::<SessionCmd>();

    // Reader thread: blocking PTY reads, forwarded as-is.
    let mut reader = pty.reader().map_err(|e| e.to_string())?;
    let wake_bytes = wake_tx.clone();
    thread::Builder::new()
        .name(format!("pty-read-{}", snapshot.id.0))
        .spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        if wake_bytes.send(Wake::Bytes(buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                    _ => {
                        let _ = wake_bytes.send(Wake::Eof);
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
        .spawn(move || run(registry, info, pty, wake_rx, GridSize { cols, rows }))
        .map_err(|e| e.to_string())?;
    Ok(cmd_tx)
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
fn run(
    registry: Arc<Registry>,
    info: Arc<Mutex<SessionInfo>>,
    mut pty: Pty,
    wake: Receiver<Wake>,
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
    let mut pending = DamageSet::Lines(Vec::new());
    let mut last_flush = Instant::now()
        .checked_sub(FLUSH_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut seq: u64 = 0;
    let mut eof = false;

    loop {
        let timeout = if pending.is_clean() {
            Duration::from_millis(250)
        } else {
            FLUSH_INTERVAL
        };
        match wake.recv_timeout(timeout) {
            Ok(Wake::Bytes(bytes)) => {
                core.advance(&bytes);
                let responses = core.take_responses();
                if !responses.is_empty() {
                    let _ = writer.write_all(&responses);
                }
                merge_damage(&mut pending, core.take_damage());
                for ev in core.take_events() {
                    publish_event(&registry, &id, &info, &ev);
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
                SessionCmd::Snapshot(reply) => {
                    let _ = reply.send(core.snapshot());
                }
                SessionCmd::Signal(sig) => {
                    let _ = pty.signal(sig);
                }
                SessionCmd::Rename(name) => {
                    info.lock().unwrap_or_else(PoisonError::into_inner).name = name;
                    persist(&registry, &info);
                    if let Some(server) = registry.server() {
                        let i = info.lock().unwrap_or_else(PoisonError::into_inner).clone();
                        server
                            .broadcast(notification::SESSION_CHANGED, serde_json::to_value(i).ok());
                    }
                }
            },
            Ok(Wake::Eof) => eof = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

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

        if eof {
            // Drain-on-exit already happened in the reader; now reap.
            let status = (0..50).find_map(|_| {
                let s = pty.try_wait().ok().flatten();
                if s.is_none() {
                    thread::sleep(Duration::from_millis(20));
                }
                s
            });
            let exit_code = status.and_then(|s| s.code());
            {
                let mut i = info.lock().unwrap_or_else(PoisonError::into_inner);
                i.state = AgentState::Stopped;
                i.pid = None;
            }
            if let Ok(store) = registry.store().lock() {
                let _ = store.end_session(&id, exit_code, &now());
            }
            if let Some(server) = registry.server() {
                server.broadcast(
                    notification::SESSION_EXITED,
                    Some(serde_json::json!({ "id": id.0, "exit_code": exit_code })),
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
