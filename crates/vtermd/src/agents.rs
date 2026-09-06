//! Agent integration inside the daemon: the loopback hook receiver, the
//! approval queue with the hold-then-answer protocol, and session state.
//!
//! Flow for a permission request (docs/03 §4.6, as verified in M0):
//! 1. Claude Code POSTs `PermissionRequest` to `/hook/<token>`.
//! 2. The payload becomes `ApprovalNeeded`; the session flips to
//!    `AwaitingInput`, the inbox is notified, a desktop notification fires.
//! 3. The HTTP request is **held** for up to [`HOLD_TIMEOUT`] waiting for an
//!    inbox decision. Answered in time → the vendor-shaped allow/deny JSON.
//!    Not answered → `{}` so the agent's own prompt appears in its terminal;
//!    the inbox item stays, marked "prompt shown in terminal", and the
//!    watchdog keeps reminding.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use vt_agent::hooks::answer;
use vt_agent::watchdog::Watchdog;
use vt_agent::{AdapterInput, AgentAdapter, ClaudeAdapter, GenericAdapter};
use vt_ipc::http::{HttpHandler, HttpRequest, HttpResponse};
use vt_proto::agent::{AgentEvent, AgentKind, AgentState};
use vt_proto::approval::{ApprovalId, ApprovalRequest, Decision, DecisionSource};
use vt_proto::session::SessionId;

use crate::registry::{Registry, now};

/// How long a hook request is held waiting for an inbox decision before the
/// agent's own prompt takes over. Under Claude Code's 600 s hook timeout.
pub const HOLD_TIMEOUT: Duration = Duration::from_secs(90);

/// Reminder cadence for unanswered approvals (`VTERMD_REMINDER_SECS`).
fn reminder_interval() -> Duration {
    Duration::from_secs(
        std::env::var("VTERMD_REMINDER_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120),
    )
}

/// How long a Claude session may run without a single hook event before it
/// is labelled degraded (`VTERMD_HOOK_GRACE_SECS`).
fn hook_grace() -> Duration {
    Duration::from_secs(
        std::env::var("VTERMD_HOOK_GRACE_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30),
    )
}

/// A pending approval as the inbox sees it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct InboxItem {
    /// Approval id.
    pub id: ApprovalId,
    /// Session it belongs to.
    pub session: SessionId,
    /// Session display name.
    pub session_name: String,
    /// The request.
    pub request: ApprovalRequest,
    /// Vendor hook event that raised it (`PermissionRequest` / `PreToolUse`).
    pub hook_event: String,
    /// RFC 3339 arrival time.
    pub requested_at: String,
    /// Seconds waiting.
    pub waiting_secs: u64,
    /// `true` once the hold expired and the agent shows its own prompt: an
    /// inbox answer can no longer be delivered through the hook.
    pub prompt_shown: bool,
    /// Reminders raised so far (every [`reminder_interval`]).
    #[serde(default)]
    pub reminders: u32,
    /// The command's safety classification, when the request carries a
    /// command line (docs/06 §3: the verdict is inline).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<vt_policy::Verdict>,
    /// Why it can never be auto-approved, when it cannot (ADR-0009).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor: Option<vt_policy::FloorReason>,
}

/// An approval waiting for a decision.
pub struct PendingApproval {
    item: InboxItem,
    since: Instant,
    decision: Mutex<Option<Decision>>,
    decided: Condvar,
    hook_open: Mutex<bool>,
    withdrawn: Mutex<bool>,
    reminders: Mutex<u32>,
}

impl PendingApproval {
    /// The approval id.
    pub fn id(&self) -> ApprovalId {
        self.item.id.clone()
    }
}

/// Per-session agent state owned by the daemon.
pub struct AgentSession {
    /// The adapter.
    pub adapter: Mutex<Box<dyn AgentAdapter>>,
    /// Vendor session id once known.
    pub vendor_session_id: Mutex<Option<String>>,
    /// When the session was registered (for the no-events grace period).
    pub since: Instant,
    /// Whether the vendor has produced any structured input at all.
    pub heard_from: Mutex<bool>,
}

/// Everything agent-related the daemon shares.
pub struct Agents {
    registry: Arc<Registry>,
    by_token: Mutex<HashMap<String, (SessionId, Arc<AgentSession>)>>,
    pending: Mutex<HashMap<ApprovalId, Arc<PendingApproval>>>,
    watchdog: Mutex<Watchdog>,
    /// Last question text notified per session (generic adapter).
    last_question: Mutex<HashMap<SessionId, String>>,
    /// Loopback receiver base URL, e.g. `http://127.0.0.1:4711`.
    pub receiver: Mutex<String>,
}

impl Agents {
    /// Empty state over the registry.
    pub fn new(registry: Arc<Registry>) -> Self {
        Self {
            registry,
            by_token: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            watchdog: Mutex::new(Watchdog::default()),
            last_question: Mutex::new(HashMap::new()),
            receiver: Mutex::new(String::new()),
        }
    }

    /// Run the watchdog: reminders for unanswered approvals and the
    /// no-events grace period for Claude sessions. Ticks every second.
    pub fn start_watchdog(self: &Arc<Self>) {
        let agents = Arc::clone(self);
        let _ = std::thread::Builder::new()
            .name("agent-watchdog".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    agents.tick();
                }
            });
    }

    fn tick(&self) {
        let due = self
            .watchdog
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .due(reminder_interval());
        for id in due {
            let item = self
                .pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&id)
                .map(|p| {
                    *p.reminders.lock().unwrap_or_else(PoisonError::into_inner) += 1;
                    p.item.clone()
                });
            if let Some(item) = item {
                let waited = self
                    .pending
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&id)
                    .map_or(0, |p| p.since.elapsed().as_secs());
                notify_desktop(
                    &self.registry,
                    &item.session_name,
                    &item.request.tool,
                    Some(&format!("still waiting after {waited} s")),
                );
                self.broadcast_inbox();
            }
        }
        // Claude sessions that never call home are degraded, not idle.
        let sessions: Vec<(SessionId, Arc<AgentSession>)> = self
            .by_token
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        for (session, state) in sessions {
            if state
                .adapter
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .kind()
                != AgentKind::Claude
            {
                continue;
            }
            let heard = *state
                .heard_from
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !heard && state.since.elapsed() >= hook_grace() {
                self.degrade(
                    &session,
                    Some(format!(
                        "no hook events in {} s: hooks may be blocked (managed settings) or the program is not claude; only the terminal is observed",
                        hook_grace().as_secs()
                    )),
                );
            }
        }
    }

    /// A heuristic verdict from the generic adapter: state plus the evidence,
    /// recorded as a labelled guess. A recognised question also raises a
    /// desktop notification, once per distinct question text.
    pub fn heuristic(
        &self,
        session: &SessionId,
        state: AgentState,
        why: &str,
        question: Option<&str>,
    ) {
        self.set_state(session, state);
        self.record_event(
            session,
            &AgentEvent::Notification {
                title: Some("guess".into()),
                body: why.to_string(),
            },
        );
        if let Some(q) = question {
            let mut last = self
                .last_question
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if last.get(session).map(String::as_str) != Some(q) {
                last.insert(session.clone(), q.to_string());
                let name = self
                    .registry
                    .find(&session.0)
                    .map(|h| {
                        h.info
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .name
                            .clone()
                    })
                    .unwrap_or_default();
                notify_desktop(&self.registry, &name, "question (guess)", Some(q));
            }
        }
    }

    /// Feed one line of the agent's stdout (Claude `stream-json`) to its
    /// adapter. No-op for sessions without an adapter.
    pub fn ingest_stream_line(&self, session: &SessionId, line: String) {
        let Some(state) = self.by_session(session) else {
            return;
        };
        let ingested = state
            .adapter
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ingest(AdapterInput::StreamLine(line));
        if ingested.events.is_empty() && ingested.state.is_none() && ingested.warnings.is_empty() {
            return;
        }
        let _ = self.apply_ingested(session, ingested);
    }

    /// Label (or clear) a session's degraded state; broadcast when changed.
    pub fn degrade(&self, session: &SessionId, reason: Option<String>) {
        let Some(h) = self.registry.find(&session.0) else {
            return;
        };
        let changed = {
            let mut info = h.info.lock().unwrap_or_else(PoisonError::into_inner);
            if info.degraded == reason {
                false
            } else {
                info.degraded = reason;
                true
            }
        };
        if changed && let Some(server) = self.registry.server() {
            let i = h
                .info
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            server.broadcast(
                vt_proto::session::notification::SESSION_CHANGED,
                serde_json::to_value(i).ok(),
            );
        }
    }

    /// Register a session's token and adapter.
    pub fn register(
        &self,
        session: SessionId,
        kind: AgentKind,
        token: String,
    ) -> Arc<AgentSession> {
        let adapter: Box<dyn AgentAdapter> = match kind {
            AgentKind::Claude => Box::new(ClaudeAdapter::default()),
            AgentKind::Codex => Box::new(vt_agent::CodexAdapter::default()),
            AgentKind::Generic => Box::new(GenericAdapter),
        };
        let state = Arc::new(AgentSession {
            adapter: Mutex::new(adapter),
            vendor_session_id: Mutex::new(None),
            since: Instant::now(),
            heard_from: Mutex::new(false),
        });
        self.by_token
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(token, (session, Arc::clone(&state)));
        state
    }

    /// Forget a session's token (session ended).
    pub fn unregister(&self, session: &SessionId) {
        self.by_token
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|_, (s, _)| s != session);
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|_, p| p.item.session != *session);
    }

    fn by_session(&self, session: &SessionId) -> Option<Arc<AgentSession>> {
        self.by_token
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .find(|(s, _)| s == session)
            .map(|(_, st)| Arc::clone(st))
    }

    fn lookup(&self, token: &str) -> Option<(SessionId, Arc<AgentSession>)> {
        self.by_token
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(token)
            .cloned()
    }

    /// Fresh random-looking token (not a secret against local peers — the
    /// socket dir and loopback binding are the boundary — but unguessable
    /// enough to keep sessions from crossing).
    pub fn new_token() -> String {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let mut x = u64::try_from(t & u128::from(u64::MAX)).unwrap_or(1)
            ^ (u64::from(std::process::id()) << 32);
        let mut out = String::new();
        for _ in 0..4 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let _ = write!(out, "{x:016x}");
        }
        out
    }

    /// Pending approvals, oldest first.
    pub fn inbox(&self) -> Vec<InboxItem> {
        let mut items: Vec<InboxItem> = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|p| {
                let mut item = p.item.clone();
                item.waiting_secs = p.since.elapsed().as_secs();
                item.prompt_shown = !*p.hook_open.lock().unwrap_or_else(PoisonError::into_inner);
                item.reminders = *p.reminders.lock().unwrap_or_else(PoisonError::into_inner);
                item
            })
            .collect();
        items.sort_by(|a, b| a.requested_at.cmp(&b.requested_at));
        items
    }

    /// Answer a pending approval. Returns the item if it existed.
    pub fn decide(
        &self,
        id: &ApprovalId,
        decision: Decision,
        by: DecisionSource,
    ) -> Option<InboxItem> {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id)?;
        self.watchdog
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .disarm(id);
        *pending
            .decision
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(decision.clone());
        pending.decided.notify_all();
        let hook_open = *pending
            .hook_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let item = pending.item.clone();
        self.record_event(
            &item.session,
            &AgentEvent::ApprovalResolved {
                id: id.clone(),
                decision,
                by,
            },
        );
        if !hook_open {
            // The agent is showing its own prompt; the answer can only reach it
            // as keystrokes. y/n are what Claude Code's prompt accepts.
            let keys: &[u8] = match pending
                .decision
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                Some(Decision::Allow { .. }) => b"y",
                _ => b"n",
            };
            if let Some(h) = self.registry.find(&item.session.0) {
                let _ = h.cmd.send(crate::session::SessionCmd::Input(keys.to_vec()));
            }
        }
        self.set_state(&item.session, AgentState::ToolRunning);
        self.broadcast_inbox();
        Some(item)
    }

    /// Apply everything an adapter produced for a session: warnings to the
    /// log, withdrawn approvals out of the inbox, a reported state, then the
    /// events. Returns the approval to hold, if one was raised.
    pub fn apply_ingested(
        &self,
        session: &SessionId,
        ingested: vt_agent::hooks::Ingested,
    ) -> Option<Arc<PendingApproval>> {
        let state = self.by_session(session)?;
        {
            let mut heard = state
                .heard_from
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !*heard {
                *heard = true;
                self.degrade(session, None);
            }
        }
        for w in &ingested.warnings {
            eprintln!("vtermd: session {}: {}", session.0, w.0);
        }
        for id in &ingested.withdrawn {
            self.withdraw(id);
        }
        if let Some(sid) = ingested.agent_session_id {
            *state
                .vendor_session_id
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(sid);
        }
        let held = self.apply_events(session, &state, ingested.events);
        if let Some(reported) = ingested.state
            && held.is_none()
        {
            self.set_state(session, reported);
        }
        held
    }

    /// Drop a pending approval the agent resolved on its own surface. Anyone
    /// waiting on it wakes up with no decision.
    pub fn withdraw(&self, id: &ApprovalId) {
        let removed = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
        if let Some(p) = removed {
            self.watchdog
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .disarm(id);
            *p.withdrawn.lock().unwrap_or_else(PoisonError::into_inner) = true;
            p.decided.notify_all();
            self.record_event(
                &p.item.session,
                &AgentEvent::ApprovalResolved {
                    id: id.clone(),
                    decision: Decision::Deny {
                        reason: "answered in the agent's own prompt".into(),
                    },
                    by: DecisionSource::Agent,
                },
            );
            self.broadcast_inbox();
        }
    }

    /// Block until the inbox decides (`Some`) or the item is withdrawn
    /// (`None`). No timeout: the agent's own prompt is visible all along.
    pub fn wait_decision(pending: &Arc<PendingApproval>) -> Option<Decision> {
        let guard = pending
            .decision
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let guard = pending
            .decided
            .wait_while(guard, |d| {
                d.is_none()
                    && !*pending
                        .withdrawn
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
            })
            .unwrap_or_else(PoisonError::into_inner);
        guard.clone()
    }

    /// Record and broadcast an event on a session's behalf.
    pub fn record(&self, session: &SessionId, event: &AgentEvent) {
        self.record_event(session, event);
    }

    /// The session's registry handle, waiting briefly for it: the handle is
    /// inserted only after the child is spawned, and an agent that posts
    /// its first hook at startup can arrive before that (the inbox test
    /// on slow CI runners). Two seconds covers the spawn; a session that
    /// never registers yields `None` and the caller degrades.
    fn handle(&self, session: &SessionId) -> Option<crate::registry::SessionHandle> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(h) = self.registry.find(&session.0) {
                return Some(h);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The session's display name, from the store record (persisted before
    /// the spawn) when the handle never arrives.
    fn session_name(&self, session: &SessionId) -> String {
        if let Some(h) = self.handle(session) {
            return h
                .info
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .name
                .clone();
        }
        self.registry
            .store()
            .lock()
            .ok()
            .and_then(|s| s.session(session).ok().flatten())
            .map(|r| r.info.name)
            .unwrap_or_default()
    }

    /// The verdict for a request that carries a command line (`Bash`,
    /// Codex `shell`): classified from the session's cwd. The generic
    /// adapter puts every command on the floor.
    fn classify_request(
        &self,
        session: &SessionId,
        req: &vt_proto::approval::ApprovalRequest,
    ) -> (Option<vt_policy::Verdict>, Option<vt_policy::FloorReason>) {
        let Some(command) = req.input.get("command").and_then(|c| c.as_str()) else {
            return (None, None);
        };
        let (cwd, generic) = self.handle(session).map_or((None, false), |h| {
            let info = h.info.lock().unwrap_or_else(PoisonError::into_inner);
            (
                Some(info.cwd.clone()),
                info.agent == vt_proto::agent::AgentKind::Generic,
            )
        });
        let cwd = cwd.unwrap_or_else(|| std::path::PathBuf::from("/"));
        let c = crate::policy::classify(command, &cwd, generic);
        (Some(c.verdict), c.floor)
    }

    fn set_state(&self, session: &SessionId, state: AgentState) {
        if let Some(h) = self.handle(session) {
            let mut info = h.info.lock().unwrap_or_else(PoisonError::into_inner);
            if info.state != state {
                let from = info.state;
                info.state = state;
                drop(info);
                if let Some(server) = self.registry.server() {
                    server.broadcast(
                        vt_proto::session::notification::SESSION_STATE,
                        Some(serde_json::json!({ "id": session.0, "from": from, "to": state })),
                    );
                }
            }
        }
    }

    fn record_event(&self, session: &SessionId, event: &AgentEvent) {
        if let Ok(store) = self.registry.store().lock() {
            let _ = store.append_event(session, &now(), event);
        }
        if let Some(server) = self.registry.server() {
            server.broadcast(
                "agent.event",
                Some(serde_json::json!({ "id": session.0, "event": event })),
            );
        }
    }

    fn broadcast_inbox(&self) {
        if let Some(server) = self.registry.server() {
            server.broadcast("inbox.changed", serde_json::to_value(self.inbox()).ok());
        }
    }

    fn apply_events(
        &self,
        session: &SessionId,
        state: &AgentSession,
        events: Vec<AgentEvent>,
    ) -> Option<Arc<PendingApproval>> {
        let mut hold = None;
        for event in events {
            match &event {
                AgentEvent::SessionStarted {
                    agent_session_id, ..
                } => {
                    *state
                        .vendor_session_id
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner) = Some(agent_session_id.clone());
                    self.set_state(session, AgentState::Idle);
                }
                AgentEvent::ToolCallStart { .. } => {
                    self.set_state(session, AgentState::ToolRunning);
                }
                AgentEvent::ToolCallEnd { .. } => self.set_state(session, AgentState::Thinking),
                AgentEvent::AssistantText { .. } => self.set_state(session, AgentState::Idle),
                AgentEvent::SessionEnded { .. } => self.set_state(session, AgentState::Stopped),
                AgentEvent::Notification { title: Some(t), .. } if t == "prompt" => {
                    self.set_state(session, AgentState::Thinking);
                }
                AgentEvent::ApprovalNeeded(req) => {
                    let name = self.session_name(session);
                    let (verdict, floor) = self.classify_request(session, req);
                    let pending = Arc::new(PendingApproval {
                        item: InboxItem {
                            id: req.id.clone(),
                            session: session.clone(),
                            session_name: name.clone(),
                            request: req.clone(),
                            hook_event: req.source.clone(),
                            requested_at: now(),
                            waiting_secs: 0,
                            prompt_shown: false,
                            reminders: 0,
                            verdict,
                            floor,
                        },
                        since: Instant::now(),
                        decision: Mutex::new(None),
                        decided: Condvar::new(),
                        hook_open: Mutex::new(true),
                        withdrawn: Mutex::new(false),
                        reminders: Mutex::new(0),
                    });
                    self.watchdog
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .arm(req.id.clone());
                    self.pending
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(req.id.clone(), Arc::clone(&pending));
                    self.set_state(session, AgentState::AwaitingInput);
                    notify_desktop(&self.registry, &name, &req.tool, req.reason.as_deref());
                    hold = Some(pending);
                }
                _ => {}
            }
            self.record_event(session, &event);
        }
        if hold.is_some() {
            self.broadcast_inbox();
        }
        hold
    }

    /// Hold the hook until decided or the timeout; produce the reply body.
    fn hold(&self, pending: &Arc<PendingApproval>) -> serde_json::Value {
        let guard = pending
            .decision
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (guard, _) = pending
            .decided
            .wait_timeout_while(guard, HOLD_TIMEOUT, |d| d.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        let event = pending.item.hook_event.as_str();
        match guard.as_ref() {
            Some(Decision::Allow { updated_input }) => answer::allow(event, updated_input.clone()),
            Some(Decision::Deny { reason }) => answer::deny(event, reason),
            None => {
                *pending
                    .hook_open
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = false;
                self.broadcast_inbox();
                answer::pass()
            }
        }
    }
}

impl HttpHandler for Agents {
    fn handle(&self, req: HttpRequest) -> HttpResponse {
        if req.method != "POST" {
            return HttpResponse::text(405, "POST only");
        }
        let mut parts = req.path.trim_start_matches('/').splitn(2, '/');
        let kind = parts.next().unwrap_or("");
        let token = parts.next().unwrap_or("").split('?').next().unwrap_or("");
        let Some((session, state)) = self.lookup(token) else {
            return HttpResponse::text(404, "unknown session token");
        };
        let payload: serde_json::Value = match serde_json::from_slice(&req.body) {
            Ok(v) => v,
            Err(e) => return HttpResponse::text(400, format!("body is not JSON: {e}")),
        };
        let input = match kind {
            "hook" => AdapterInput::HookPayload(payload),
            "status" => AdapterInput::StatusLine(payload),
            _ => return HttpResponse::text(404, "unknown endpoint"),
        };
        let ingested = state
            .adapter
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ingest(input);
        match self.apply_ingested(&session, ingested) {
            Some(pending) => HttpResponse::json(&self.hold(&pending)),
            None => HttpResponse::json(&answer::pass()),
        }
    }
}

/// macOS Notification Center via `osascript` until the app (M4) does it
/// natively. Best effort; failures are silent.
fn notify_desktop(
    registry: &crate::registry::Registry,
    session: &str,
    tool: &str,
    reason: Option<&str>,
) {
    if !registry.cfg().notifications.awaiting_input {
        return;
    }
    let body = reason.map_or_else(
        || format!("{tool} needs approval"),
        |r| format!("{tool}: {r}"),
    );
    let script = format!(
        "display notification \"{}\" with title \"Vambiant Term\" subtitle \"{}\"",
        body.replace('"', "'").chars().take(200).collect::<String>(),
        session.replace('"', "'")
    );
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
