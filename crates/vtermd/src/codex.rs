//! Codex sessions: a per-session `codex app-server` on a Unix socket, the
//! user's TUI attached to it with `--remote`, and this daemon observing as a
//! second client (docs/03 §5; mechanism verified 2026-09-05, docs/10 §7).
//!
//! The observer resumes every loaded thread so it receives the turn stream
//! and approval requests. Approvals go to the inbox; an inbox decision is
//! answered on the RPC connection, and if the user answers in the TUI first
//! the server says `serverRequest/resolved` and the inbox item is withdrawn.

use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use vt_agent::codex::{AppServer, AppServerHandle};
use vt_proto::agent::{AgentEvent, ErrorKind};
use vt_proto::session::SessionId;

use crate::agents::Agents;

/// How long the observer waits for the app-server socket to come up.
const CONNECT_WAIT: Duration = Duration::from_secs(30);
/// Retry interval for `thread/resume` on a thread whose rollout is not yet
/// written (the server answers "no rollout found" until the first turn).
const RESUME_RETRY: Duration = Duration::from_millis(1500);

/// The app-server socket for a session.
pub fn socket_for(runtime_dir: &Path, session_id: &str) -> PathBuf {
    runtime_dir.join(format!("codex-{session_id}.sock"))
}

fn codex_binary() -> PathBuf {
    std::env::var_os("VTERMD_CODEX").map_or_else(|| PathBuf::from("codex"), PathBuf::from)
}

/// Start the session's app-server, detached so it outlives this daemon like
/// the holder does. `VTERMD_CODEX_SOCKET` (tests) names an already running
/// server instead. Returns the socket to attach to.
pub fn start_app_server(
    runtime_dir: &Path,
    session_id: &str,
    cwd: &Path,
) -> Result<PathBuf, String> {
    if let Some(external) = std::env::var_os("VTERMD_CODEX_SOCKET") {
        return Ok(PathBuf::from(external));
    }
    let socket = socket_for(runtime_dir, session_id);
    let _ = std::fs::remove_file(&socket);
    let mut cmd = Command::new(codex_binary());
    cmd.arg("app-server")
        .arg("--listen")
        .arg(format!("unix://{}", socket.display()))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe and touches no Rust state.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("cannot start `codex app-server` for session {session_id}: {e}"))?;
    let _ = std::fs::write(socket.with_extension("pid"), child.id().to_string());
    std::mem::forget(child); // reaped by the daemon's waitpid loop
    Ok(socket)
}

/// Stop the session's app-server (if this daemon or a previous one started
/// one) and remove its socket.
pub fn stop_app_server(runtime_dir: &Path, session_id: &str) {
    let socket = socket_for(runtime_dir, session_id);
    let pid_file = socket.with_extension("pid");
    if let Some(pid) = std::fs::read_to_string(&pid_file)
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
    {
        // SAFETY: plain kill(2) on a pid we recorded ourselves.
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
    let _ = std::fs::remove_file(&pid_file);
    let _ = std::fs::remove_file(&socket);
}

/// Start the observer thread for a session.
pub fn observe(agents: Arc<Agents>, session: SessionId, socket: PathBuf) {
    let name = format!("codex-observe-{}", session.0);
    let _ = thread::Builder::new()
        .name(name)
        .spawn(move || run(&agents, &session, &socket));
}

fn connect(socket: &Path) -> Result<AppServer, String> {
    let start = Instant::now();
    loop {
        match AppServer::connect(socket, "vambiant-term") {
            Ok(s) => return Ok(s),
            Err(e) if start.elapsed() < CONNECT_WAIT => {
                let _ = e;
                thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(format!("{}: {e}", socket.display())),
        }
    }
}

struct Threads {
    /// Threads we are subscribed to.
    subscribed: Vec<String>,
    /// Threads seen but not yet resumed, with the outstanding request id.
    pending: HashMap<String, Option<u64>>,
}

impl Threads {
    fn note(&mut self, thread: &str) {
        if !self.subscribed.iter().any(|t| t == thread) && !self.pending.contains_key(thread) {
            self.pending.insert(thread.to_string(), None);
        }
    }

    /// Send `thread/resume` for every noted thread without a request in flight.
    fn resume_pending(&mut self, handle: &AppServerHandle) {
        for (thread, req) in &mut self.pending {
            if req.is_none() {
                *req = handle
                    .request("thread/resume", &json!({ "threadId": thread }))
                    .ok();
            }
        }
    }

    /// Handle a response to one of our resume requests. Returns the thread
    /// to retry later when the server refused (rollout not written yet).
    fn on_response(&mut self, message: &Value) -> Option<String> {
        let id = message.get("id").and_then(Value::as_u64)?;
        let thread = self
            .pending
            .iter()
            .find(|(_, r)| **r == Some(id))
            .map(|(t, _)| t.clone())?;
        if message.get("error").is_some() {
            self.pending.insert(thread.clone(), None);
            Some(thread)
        } else {
            self.pending.remove(&thread);
            self.subscribed.push(thread);
            None
        }
    }
}

/// Retry a refused resume after [`RESUME_RETRY`] without blocking the reader.
fn retry_resume(threads: &Arc<Mutex<Threads>>, handle: &AppServerHandle) {
    let threads = Arc::clone(threads);
    let handle = handle.clone();
    let _ = thread::Builder::new()
        .name("codex-resume-retry".into())
        .spawn(move || {
            thread::sleep(RESUME_RETRY);
            threads
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .resume_pending(&handle);
        });
}

fn run(agents: &Arc<Agents>, session: &SessionId, socket: &Path) {
    let mut server = match connect(socket) {
        Ok(s) => s,
        Err(e) => {
            agents.record(
                session,
                &AgentEvent::Error {
                    kind: ErrorKind::Adapter,
                    message: format!(
                        "cannot reach codex app-server: {e}; only the terminal is observed"
                    ),
                    retrying: false,
                },
            );
            return;
        }
    };
    let handle = server.handle();
    let threads = Arc::new(Mutex::new(Threads {
        subscribed: Vec::new(),
        pending: HashMap::new(),
    }));
    let list_id = handle.request("thread/loaded/list", &json!({})).ok();
    let mut counter: u64 = 0;
    loop {
        let message = match server.next_message() {
            Ok(Some(m)) => m,
            Ok(None) => {
                agents.record(
                    session,
                    &AgentEvent::Error {
                        kind: ErrorKind::Adapter,
                        message: "codex app-server connection closed; structured events stopped"
                            .into(),
                        retrying: false,
                    },
                );
                return;
            }
            Err(e) => {
                agents.record(
                    session,
                    &AgentEvent::Error {
                        kind: ErrorKind::Adapter,
                        message: format!(
                            "codex app-server read failed: {e}; structured events stopped"
                        ),
                        retrying: false,
                    },
                );
                return;
            }
        };
        counter += 1;
        {
            let mut th = threads.lock().unwrap_or_else(PoisonError::into_inner);
            if message.get("method").is_none() {
                if message.get("id").and_then(Value::as_u64) == list_id {
                    for t in message
                        .pointer("/result/data")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                    {
                        th.note(t);
                    }
                    th.resume_pending(&handle);
                } else if th.on_response(&message).is_some() {
                    retry_resume(&threads, &handle);
                }
                continue;
            }
            for ptr in ["/params/threadId", "/params/thread/id"] {
                if let Some(t) = message.pointer(ptr).and_then(Value::as_str) {
                    th.note(t);
                }
            }
            th.resume_pending(&handle);
        }
        let ingested = vt_agent::codex::ingest(&message, counter);
        if let Some(pending) = agents.apply_ingested(session, ingested) {
            // Answer on this connection once the inbox decides; the observer
            // must keep reading meanwhile (the TUI may answer first).
            let handle = handle.clone();
            let _ = thread::Builder::new()
                .name("codex-answer".into())
                .spawn(move || {
                    if let Some(decision) = Agents::wait_decision(&pending)
                        && let Err(e) = handle.answer_approval(&pending.id(), &decision)
                    {
                        eprintln!(
                            "vtermd: cannot answer codex approval {}: {e}",
                            pending.id().0
                        );
                    }
                });
        }
    }
}
