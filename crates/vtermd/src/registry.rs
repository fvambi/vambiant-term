//! Session table and the handle every other module uses.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use vt_core::cell::CellSnapshot;
use vt_ipc::Server;
use vt_proto::agent::{AgentKind, AgentState};
use vt_proto::session::{Capabilities, NewSession, SessionId, SessionInfo};
use vt_store::Store;
use vt_store::sessions::SessionRecord;

use crate::holder::{self, ReadoptFailure};
use crate::session::{self, SessionCmd};

/// RFC 3339 UTC timestamp without a dependency: seconds since the epoch is
/// enough precision for session records.
/// Every generic-adapter session carries this label (docs/03 §6).
pub const GENERIC_LABEL: &str =
    "generic adapter: state is a heuristic guess from terminal output; approvals cannot be routed";

pub fn now() -> String {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = since_epoch.as_secs();
    let millis = since_epoch.subsec_millis();
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
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
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
    runtime_dir: PathBuf,
    state_dir: PathBuf,
    agents: OnceLock<Arc<crate::agents::Agents>>,
    config: OnceLock<Arc<crate::config::ConfigState>>,
    counter: Mutex<u64>,
}

impl Registry {
    /// Empty registry over `store`; holder sockets live in `runtime_dir`.
    pub fn new(store: Arc<Mutex<Store>>, runtime_dir: PathBuf, state_dir: PathBuf) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            store,
            server: OnceLock::new(),
            runtime_dir,
            state_dir,
            agents: OnceLock::new(),
            config: OnceLock::new(),
            counter: Mutex::new(0),
        }
    }

    /// Wire the agent layer (hook receiver, inbox).
    pub fn set_agents(&self, agents: Arc<crate::agents::Agents>) {
        let _ = self.agents.set(agents);
    }

    /// The agent layer, once wired.
    /// Wire the configuration once loaded.
    pub fn set_config(&self, config: Arc<crate::config::ConfigState>) {
        let _ = self.config.set(config);
    }

    /// The configuration, when wired.
    pub fn config(&self) -> Option<Arc<crate::config::ConfigState>> {
        self.config.get().cloned()
    }

    /// Current `config.toml` values, or the defaults before wiring.
    pub fn cfg(&self) -> vt_config::Config {
        self.config
            .get()
            .map_or_else(vt_config::Config::default, |c| c.config())
    }

    pub fn agents(&self) -> Option<Arc<crate::agents::Agents>> {
        self.agents.get().cloned()
    }

    /// Provision an agent session: settings file, extra args, env, token.
    fn provision(
        &self,
        id: &SessionId,
        kind: AgentKind,
        req: &NewSession,
    ) -> Result<(NewSession, Option<String>, Capabilities), String> {
        let mut spec = req.clone();
        match kind {
            AgentKind::Claude => {
                let agents = self.agents().ok_or("agent layer not ready")?;
                let receiver = agents
                    .receiver
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                if receiver.is_empty() {
                    return Err(
                        "hook receiver is not listening; cannot provision a Claude session".into(),
                    );
                }
                let token = crate::agents::Agents::new_token();
                let dir = self.state_dir.join("sessions").join(&id.0);
                let vterm = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.join("vterm")))
                    .filter(|p| p.exists())
                    .unwrap_or_else(|| PathBuf::from("vterm"));
                let prov = vt_agent::claude::provision(&dir, &receiver, &token, &vterm)
                    .map_err(|e| e.to_string())?;
                let claude_cfg = self.cfg().agents.claude;
                if spec.argv.is_empty() {
                    spec.argv.push(claude_cfg.binary.clone());
                }
                // `claude [args]` → `claude --settings <file> [args]`. Any other
                // program (a wrapper script, a fake agent in tests) gets the
                // path in the environment and must pass it on itself.
                let is_claude = spec.argv[0].rsplit('/').next() == Some("claude")
                    || spec.argv[0] == claude_cfg.binary;
                if is_claude {
                    let program = spec.argv.remove(0);
                    let mut argv = vec![program];
                    argv.extend(prov.extra_args.clone());
                    argv.extend(claude_cfg.extra_args.clone());
                    argv.extend(spec.argv);
                    spec.argv = argv;
                }
                spec.env.push((
                    "VAMBIANT_TERM_CLAUDE_SETTINGS".into(),
                    prov.settings_path.display().to_string(),
                ));
                spec.env.extend(prov.env);
                spec.env.push((
                    "VAMBIANT_TERM_HOOK_URL".into(),
                    format!("{receiver}/hook/{token}"),
                ));
                spec.env.push((
                    "VAMBIANT_TERM_STATUS_URL".into(),
                    format!("{receiver}/status/{token}"),
                ));
                agents.register(id.clone(), kind, token.clone());
                Ok((spec, Some(token), vt_agent::claude::CAPABILITIES))
            }
            AgentKind::Codex => {
                let agents = self.agents().ok_or("agent layer not ready")?;
                let cwd = req
                    .cwd
                    .clone()
                    .or_else(|| std::env::current_dir().ok())
                    .unwrap_or_else(|| "/".into());
                let codex_cfg = self.cfg().agents.codex;
                let socket = crate::codex::start_app_server(
                    &self.runtime_dir,
                    &id.0,
                    &cwd,
                    Path::new(&codex_cfg.binary),
                )?;
                if spec.argv.is_empty() {
                    spec.argv.push(codex_cfg.binary.clone());
                }
                // `codex [args]` → `codex --remote unix://<sock> [args]`; any
                // other program gets the socket in the environment only.
                let is_codex = spec.argv[0].rsplit('/').next() == Some("codex")
                    || spec.argv[0] == codex_cfg.binary;
                if is_codex {
                    let program = spec.argv.remove(0);
                    let mut argv = vec![
                        program,
                        "--remote".into(),
                        format!("unix://{}", socket.display()),
                    ];
                    argv.extend(codex_cfg.extra_args.clone());
                    argv.extend(spec.argv);
                    spec.argv = argv;
                }
                spec.env.push((
                    "VAMBIANT_TERM_CODEX_SOCKET".into(),
                    socket.display().to_string(),
                ));
                let token = crate::agents::Agents::new_token();
                agents.register(id.clone(), kind, token.clone());
                crate::codex::observe(agents, id.clone(), socket);
                Ok((spec, Some(token), vt_agent::codex::CAPABILITIES))
            }
            AgentKind::Generic => {
                self.inject_shell_integration(id, &mut spec);
                Ok((spec, None, Capabilities::default()))
            }
        }
    }

    /// For a plain shell session, write our OSC 133/7 snippet and point the
    /// shell at it (`[shell_integration]`). Best effort: a shell we do not
    /// recognise, or `inject = off`, leaves the session on heuristic blocks.
    fn inject_shell_integration(&self, id: &SessionId, spec: &mut NewSession) {
        let cfg = self.cfg().shell_integration;
        if !cfg.enabled || matches!(cfg.inject, vt_config::schema::Inject::Off) {
            return;
        }
        // The program: the explicit argv[0], else the user's login shell.
        let controls_argv = !spec.argv.is_empty();
        let program = spec
            .argv
            .first()
            .cloned()
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_default();
        let Some(shell) = vt_shell::Shell::from_program(&program) else {
            return;
        };
        // Only recognised as a shell to integrate; write under a per-session dir.
        let dir = self.state_dir.join("shell-integration").join(&id.0);
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        let user_zdotdir = std::env::var_os("ZDOTDIR").map_or(home, PathBuf::from);
        let inj = vt_shell::inject(shell, &dir, &user_zdotdir, controls_argv);
        for (path, contents) in &inj.files {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(path, contents) {
                eprintln!(
                    "vtermd: session {}: shell integration file {}: {e}",
                    id.0,
                    path.display()
                );
                return;
            }
        }
        // XDG_DATA_DIRS must prepend to the *real* current value, which the
        // injection could not read; re-derive it here.
        for (k, v) in inj.env {
            let value = if k == "XDG_DATA_DIRS" {
                let existing = std::env::var("XDG_DATA_DIRS")
                    .unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
                format!("{}:{existing}", dir.display())
            } else {
                v
            };
            spec.env.retain(|(ek, _)| ek != &k);
            spec.env.push((k, value));
        }
        if controls_argv && !inj.args.is_empty() {
            // bash `--rcfile <file>` goes right after the program.
            let program = spec.argv.remove(0);
            let mut argv = vec![program];
            argv.extend(inj.args);
            argv.extend(std::mem::take(&mut spec.argv));
            spec.argv = argv;
        }
        if let Some(why) = inj.limitation {
            eprintln!("vtermd: session {}: shell integration limited: {why}", id.0);
        }
    }

    /// Directory of user prompt packs for the generic adapter.
    pub fn packs_dir(&self) -> PathBuf {
        self.state_dir.join("packs")
    }

    /// Wire the IPC server so session threads can publish.
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

    /// Spawn a new session (through an fd holder) and its thread.
    pub fn create(self: &Arc<Self>, req: &NewSession) -> Result<SessionInfo, String> {
        // `[terminal] shell` replaces the login shell for plain sessions;
        // agent sessions have their own program.
        let mut req = req.clone();
        if req.argv.is_empty() && req.agent.is_none() {
            let shell = self.cfg().terminal.shell;
            if !shell.trim().is_empty() {
                req.argv = shell.split_whitespace().map(str::to_owned).collect();
            }
        }
        let req = &req;
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
        let kind = req.agent.unwrap_or(AgentKind::Generic);
        let (spec, token, capabilities) = self.provision(&id, kind, req)?;
        let info = SessionInfo {
            id: id.clone(),
            name: name.clone(),
            agent: kind,
            state: AgentState::Starting,
            capabilities,
            degraded: (kind == AgentKind::Generic).then(|| GENERIC_LABEL.to_string()),
            cwd: cwd.clone(),
            pid: None,
            size: Some((cols, rows)),
            orphaned: false,
            readopted: false,
            created_at: now(),
        };
        // Persist before spawning: the child's first hook may arrive before
        // the thread below runs, and agent_events has a foreign key on sessions.
        let provisional = SessionRecord {
            info: info.clone(),
            argv: spec.argv.clone(),
            env: spec.env.clone(),
            pty_path: None,
            exit_code: None,
            hold_socket: None,
            agent_token: token.clone(),
        };
        if let Err(e) = self
            .store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .upsert_session(&provisional)
        {
            eprintln!("vtermd: cannot persist session {}: {e}", id.0);
        }
        let held = holder::spawn(&self.runtime_dir, &id.0, &name, &spec, (cols, rows), &cwd)?;
        let hold_socket = held.socket.display().to_string();
        let shared = Arc::new(Mutex::new(info));
        let cmd = session::start(Arc::clone(self), Arc::clone(&shared), held)?;
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
            argv: spec.argv.clone(),
            env: spec.env.clone(),
            pty_path: None,
            exit_code: None,
            hold_socket: Some(hold_socket),
            agent_token: token,
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

    /// Re-adopt every session the previous daemon left running. Sessions
    /// whose holder recorded an exit are closed with that code; sessions whose
    /// holder cannot be reached are marked orphaned — never dropped.
    pub fn readopt_all(self: &Arc<Self>) {
        let live = match self
            .store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .live_sessions()
        {
            Ok(l) => l,
            Err(e) => {
                eprintln!("vtermd: cannot read previous sessions: {e}");
                return;
            }
        };
        for rec in live {
            let id = rec.info.id.clone();
            let name = rec.info.name.clone();
            let outcome = match rec.hold_socket.as_deref() {
                Some(sock) => holder::readopt(Path::new(sock), &name),
                None => Err(ReadoptFailure::Unreachable("no holder recorded".into())),
            };
            let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
            match outcome {
                Ok(held) => {
                    let mut info = rec.info.clone();
                    info.readopted = true;
                    info.orphaned = false;
                    if info.agent == AgentKind::Generic {
                        info.degraded = Some(GENERIC_LABEL.to_string());
                    }
                    if let (Some(token), Some(agents)) = (rec.agent_token.clone(), self.agents()) {
                        agents.register(id.clone(), info.agent, token);
                        if info.agent == AgentKind::Codex {
                            info.capabilities = vt_agent::codex::CAPABILITIES;
                            let socket = std::env::var_os("VTERMD_CODEX_SOCKET").map_or_else(
                                || crate::codex::socket_for(&self.runtime_dir, &id.0),
                                PathBuf::from,
                            );
                            crate::codex::observe(agents, id.clone(), socket);
                        }
                        if info.agent == AgentKind::Claude {
                            info.capabilities = vt_agent::claude::CAPABILITIES;
                        }
                    }
                    let shared = Arc::new(Mutex::new(info));
                    match session::start(Arc::clone(self), Arc::clone(&shared), held) {
                        Ok(cmd) => {
                            self.sessions
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .insert(id.clone(), SessionHandle { info: shared, cmd });
                            eprintln!(
                                "vtermd: re-adopted session {} ({name}) from its holder",
                                id.0
                            );
                        }
                        Err(e) => {
                            eprintln!(
                                "vtermd: session {} ({name}) could not be restarted: {e}; marking orphaned",
                                id.0
                            );
                            let _ = store.mark_orphaned(&id);
                        }
                    }
                }
                Err(ReadoptFailure::Exited(exit)) => {
                    eprintln!(
                        "vtermd: session {} ({name}) exited while no daemon was attached (code {:?})",
                        id.0, exit.code
                    );
                    let _ = store.end_session(&id, exit.code, &now());
                }
                Err(ReadoptFailure::Unreachable(why)) => {
                    eprintln!("vtermd: session {} ({name}) is orphaned: {why}", id.0);
                    let _ = store.mark_orphaned(&id);
                }
            }
        }
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
        if let Some(agents) = self.agents() {
            agents.unregister(id);
        }
        let is_codex = self.find(&id.0).is_some_and(|h| {
            h.info.lock().unwrap_or_else(PoisonError::into_inner).agent == AgentKind::Codex
        });
        if is_codex {
            crate::codex::stop_app_server(&self.runtime_dir, &id.0);
        }
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

    /// Plain text of absolute rows `from..=to`, from the session thread.
    pub fn text(handle: &SessionHandle, from: u64, to: u64) -> Option<String> {
        Self::export(handle, from, to, vt_core::core::TextFormat::Plain)
    }

    /// Absolute rows `from..=to` in `format`, from the session thread.
    pub fn export(
        handle: &SessionHandle,
        from: u64,
        to: u64,
        format: vt_core::core::TextFormat,
    ) -> Option<String> {
        let (tx, rx) = std::sync::mpsc::channel();
        handle
            .cmd
            .send(SessionCmd::Export(from, to, format, tx))
            .ok()?;
        rx.recv_timeout(std::time::Duration::from_secs(2)).ok()
    }
}
