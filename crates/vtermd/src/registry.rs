//! Session table and the handle every other module uses.

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use vt_core::cell::CellSnapshot;
use vt_ipc::Server;
use vt_proto::agent::{AgentKind, AgentState};
use vt_proto::session::{Capabilities, NewSession, SessionId, SessionInfo};
use vt_store::Store;
use vt_store::sessions::SessionRecord;

use crate::session::{self, SessionCmd};

/// RFC 3339 UTC timestamp without a dependency: seconds since the epoch is
/// enough precision for session records.
pub fn now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // Civil-from-days (Howard Hinnant), UTC.
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// A live session as the daemon sees it.
#[derive(Clone)]
pub struct SessionHandle {
    /// Public record (kept current by the session thread).
    pub info: Arc<Mutex<SessionInfo>>,
    /// Command channel into the session thread.
    pub cmd: Sender<SessionCmd>,
}

/// All sessions.
pub struct Registry {
    sessions: Mutex<HashMap<SessionId, SessionHandle>>,
    store: Arc<Mutex<Store>>,
    server: OnceLock<Arc<Server>>,
    counter: Mutex<u64>,
}

impl Registry {
    /// Empty registry over `store`.
    pub fn new(store: Arc<Mutex<Store>>) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            store,
            server: OnceLock::new(),
            counter: Mutex::new(0),
        }
    }

    /// Wire the IPC server so session threads can broadcast.
    pub fn set_server(&self, server: Arc<Server>) {
        let _ = self.server.set(server);
    }

    /// The IPC server, once started.
    pub fn server(&self) -> Option<Arc<Server>> {
        self.server.get().cloned()
    }

    /// Persisted store.
    pub fn store(&self) -> &Arc<Mutex<Store>> {
        &self.store
    }

    fn next_id(&self) -> SessionId {
        let mut c = self.counter.lock().unwrap_or_else(PoisonError::into_inner);
        *c += 1;
        let pid = std::process::id();
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        SessionId(format!("{:x}{:x}{:x}", t & 0xffff_ffff, pid & 0xffff, *c))
    }

    /// Spawn a new session and its thread.
    pub fn create(self: &Arc<Self>, req: &NewSession) -> Result<SessionInfo, String> {
        let id = self.next_id();
        let (cols, rows) = req.size.unwrap_or((80, 24));
        let name = req
            .name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| {
                let base = req
                    .argv
                    .first()
                    .map_or("shell", |p| p.rsplit('/').next().unwrap_or(p));
                format!("{base}-{}", &id.0[id.0.len().saturating_sub(4)..])
            });
        let cwd = req
            .cwd
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| "/".into());
        let info = SessionInfo {
            id: id.clone(),
            name,
            agent: req.agent.unwrap_or(AgentKind::Generic),
            state: AgentState::Starting,
            capabilities: Capabilities::default(),
            cwd: cwd.clone(),
            pid: None,
            size: Some((cols, rows)),
            orphaned: false,
            created_at: now(),
        };
        let shared = Arc::new(Mutex::new(info.clone()));
        let cmd = session::spawn(Arc::clone(self), Arc::clone(&shared), req)?;
        let handle = SessionHandle {
            info: Arc::clone(&shared),
            cmd,
        };
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.clone(), handle);
        let record = SessionRecord {
            info: shared
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            argv: Vec::new(),
            env: Vec::new(),
            pty_path: None,
            exit_code: None,
        };
        if let Err(e) = self
            .store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .upsert_session(&record)
        {
            eprintln!("vtermd: cannot persist session {}: {e}", id.0);
        }
        Ok(shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }

    /// Look up by id, or by unique name.
    pub fn find(&self, key: &str) -> Option<SessionHandle> {
        let sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(h) = sessions.get(&SessionId(key.to_owned())) {
            return Some(h.clone());
        }
        let mut by_name = sessions
            .values()
            .filter(|h| h.info.lock().unwrap_or_else(PoisonError::into_inner).name == key);
        let first = by_name.next().cloned();
        if by_name.next().is_some() {
            None
        } else {
            first
        }
    }

    /// Live sessions, plus persisted ones that are not running (ended, orphaned).
    pub fn list(&self) -> Vec<SessionInfo> {
        let mut out: Vec<SessionInfo> = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|h| {
                h.info
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
            })
            .collect();
        let live: std::collections::HashSet<SessionId> = out.iter().map(|i| i.id.clone()).collect();
        if let Ok(all) = self
            .store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .all_sessions()
        {
            for rec in all {
                if !live.contains(&rec.info.id) {
                    out.push(rec.info);
                }
            }
        }
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        out
    }

    /// Remove a session that has ended.
    pub fn remove(&self, id: &SessionId) {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
    }

    /// Ask a session for its current grid.
    pub fn snapshot(handle: &SessionHandle) -> Option<CellSnapshot> {
        let (tx, rx) = std::sync::mpsc::channel();
        handle.cmd.send(SessionCmd::Snapshot(tx)).ok()?;
        rx.recv_timeout(std::time::Duration::from_secs(2)).ok()
    }
}
