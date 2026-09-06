//! JSON-RPC methods (names in `vt_proto::session::method`).

use std::sync::{Arc, Mutex, PoisonError};

use base64::Engine as _;
use vt_core::core::{Scroll, TextFormat};
use vt_core::key::KeyEvent;
use vt_ipc::{ConnId, Handler};
use vt_proto::jsonrpc::{Request, RpcError};
use vt_proto::session::{NewSession, method};
use vt_store::Store;

use crate::registry::Registry;
use crate::session::SessionCmd;
use crate::wire;

/// The daemon's request handler.
pub struct Rpc {
    registry: Arc<Registry>,
    store: Arc<Mutex<Store>>,
}

impl Rpc {
    /// Handler over a registry and its store.
    pub fn new(registry: Arc<Registry>, store: Arc<Mutex<Store>>) -> Self {
        Self { registry, store }
    }

    fn param<T: serde::de::DeserializeOwned>(req: &Request, key: &str) -> Result<T, RpcError> {
        req.params
            .as_ref()
            .and_then(|p| p.get(key))
            .cloned()
            .ok_or_else(|| {
                RpcError::new(
                    RpcError::INVALID_PARAMS,
                    format!("missing parameter `{key}` for {}", req.method),
                )
            })
            .and_then(|v| {
                serde_json::from_value(v).map_err(|e| {
                    RpcError::new(RpcError::INVALID_PARAMS, format!("bad `{key}`: {e}"))
                })
            })
    }

    /// A persisted session by id or unique name (running or not).
    fn stored_session(store: &Store, key: &str) -> Option<vt_store::sessions::SessionRecord> {
        let all = store.all_sessions().ok()?;
        if let Some(r) = all.iter().find(|r| r.info.id.0 == key) {
            return Some(r.clone());
        }
        let mut by_name = all.iter().filter(|r| r.info.name == key);
        let first = by_name.next().cloned();
        if by_name.next().is_some() {
            None
        } else {
            first
        }
    }

    fn session(&self, req: &Request) -> Result<crate::registry::SessionHandle, RpcError> {
        let key: String = Self::param(req, "id")?;
        self.registry.find(&key).ok_or_else(|| {
            RpcError::new(
                RpcError::NO_SUCH_SESSION,
                format!("no running session `{key}` (see `vterm ls`)"),
            )
        })
    }

    /// The session id for `id` (name or id), running or ended, so history
    /// queries work after the process is gone.
    fn session_id_for(&self, req: &Request) -> Result<vt_proto::session::SessionId, RpcError> {
        let key: String = Self::param(req, "id")?;
        match self.session(req) {
            Ok(h) => Ok(h
                .info
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .id
                .clone()),
            Err(not_running) => {
                let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
                Self::stored_session(&store, &key)
                    .map(|r| r.info.id)
                    .ok_or(not_running)
            }
        }
    }
}

impl Handler for Rpc {
    #[allow(clippy::too_many_lines)] // one arm per RPC method; splitting hides the table
    fn handle(&self, conn: ConnId, req: &Request) -> Result<serde_json::Value, RpcError> {
        match req.method.as_str() {
            method::DAEMON_STATUS => Ok(serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "pid": std::process::id(),
                "sessions": self.registry.list().len(),
                "state_db": self.store.lock().unwrap_or_else(PoisonError::into_inner).path().display().to_string(),
            })),
            method::SESSION_LIST => {
                serde_json::to_value(self.registry.list()).map_err(|e| internal(&e))
            }
            method::SESSION_NEW => {
                let params: NewSession = req
                    .params
                    .clone()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|e| RpcError::new(RpcError::INVALID_PARAMS, e.to_string()))?
                    .unwrap_or_default();
                let info = self
                    .registry
                    .create(&params)
                    .map_err(|e| RpcError::new(RpcError::INTERNAL, e))?;
                if let Some(server) = self.registry.server() {
                    server.broadcast(
                        vt_proto::session::notification::SESSION_CHANGED,
                        serde_json::to_value(&info).ok(),
                    );
                }
                serde_json::to_value(info).map_err(|e| internal(&e))
            }
            method::SESSION_GET => {
                let h = self.session(req)?;
                let info = h
                    .info
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                serde_json::to_value(info).map_err(|e| internal(&e))
            }
            method::SESSION_RENAME => {
                let h = self.session(req)?;
                let name: String = Self::param(req, "name")?;
                h.cmd.send(SessionCmd::Rename(name)).map_err(|_| gone())?;
                let info = h
                    .info
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                serde_json::to_value(info).map_err(|e| internal(&e))
            }
            method::SESSION_KILL => {
                let h = self.session(req)?;
                let signal: i32 = Self::param(req, "signal").unwrap_or(libc::SIGHUP);
                h.cmd.send(SessionCmd::Signal(signal)).map_err(|_| gone())?;
                Ok(serde_json::json!({}))
            }
            method::SESSION_RESIZE => {
                let h = self.session(req)?;
                let cols: u16 = Self::param(req, "cols")?;
                let rows: u16 = Self::param(req, "rows")?;
                if cols == 0 || rows == 0 {
                    return Err(RpcError::new(
                        RpcError::INVALID_PARAMS,
                        "cols and rows must be non-zero",
                    ));
                }
                h.cmd
                    .send(SessionCmd::Resize(cols, rows))
                    .map_err(|_| gone())?;
                Ok(serde_json::json!({}))
            }
            method::SESSION_INPUT => {
                let h = self.session(req)?;
                let b64: String = Self::param(req, "bytes")?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| {
                        RpcError::new(
                            RpcError::INVALID_PARAMS,
                            format!("bytes is not base64: {e}"),
                        )
                    })?;
                h.cmd.send(SessionCmd::Input(bytes)).map_err(|_| gone())?;
                Ok(serde_json::json!({}))
            }
            method::SESSION_KEY => {
                let h = self.session(req)?;
                let key: KeyEvent = Self::param(req, "key")?;
                h.cmd.send(SessionCmd::Key(key)).map_err(|_| gone())?;
                Ok(serde_json::json!({}))
            }
            method::SESSION_ATTACH => {
                let h = self.session(req)?;
                let snap = Registry::snapshot(&h).ok_or_else(gone)?;
                let id = h
                    .info
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .id
                    .clone();
                if let Some(server) = self.registry.server() {
                    server.subscribe(conn, &id.0);
                }
                serde_json::to_value(wire::full(&id, &snap, 0)).map_err(|e| internal(&e))
            }
            method::SESSION_DETACH => {
                let h = self.session(req)?;
                let id = h
                    .info
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .id
                    .clone();
                if let Some(server) = self.registry.server() {
                    server.unsubscribe(conn, &id.0);
                }
                Ok(serde_json::json!({}))
            }
            method::SESSION_LOGS => {
                let lines: Option<usize> = Self::param(req, "lines").ok();
                match self.session(req) {
                    Ok(h) => {
                        let snap = Registry::snapshot(&h).ok_or_else(gone)?;
                        Ok(serde_json::json!({ "text": wire::text(&snap, lines), "live": true }))
                    }
                    Err(not_running) => {
                        // Ended sessions keep their final grid in the store.
                        let key: String = Self::param(req, "id")?;
                        let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
                        let rec = Self::stored_session(&store, &key).ok_or(not_running)?;
                        let text = store
                            .last_output(&rec.info.id)
                            .map_err(|e| RpcError::new(RpcError::INTERNAL, e.to_string()))?
                            .unwrap_or_default();
                        let text = match lines {
                            Some(n) => {
                                let all: Vec<&str> = text.lines().collect();
                                all[all.len().saturating_sub(n)..].join("\n")
                            }
                            None => text,
                        };
                        Ok(serde_json::json!({ "text": text, "live": false }))
                    }
                }
            }
            method::SESSION_SCROLL => {
                let h = self.session(req)?;
                let to: String = Self::param(req, "to")?;
                let n: i64 = Self::param(req, "n").unwrap_or(0);
                let bad = |what: &str| RpcError::new(RpcError::INVALID_PARAMS, what.to_owned());
                let scroll = match to.as_str() {
                    "top" => Scroll::Top,
                    "bottom" => Scroll::Bottom,
                    "lines" => {
                        Scroll::Lines(i32::try_from(n).map_err(|_| bad("`n` out of range"))?)
                    }
                    "row" => Scroll::Row(u64::try_from(n).map_err(|_| bad("`n` must be >= 0"))?),
                    other => return Err(bad(&format!("unknown scroll target `{other}`"))),
                };
                h.cmd.send(SessionCmd::Scroll(scroll)).map_err(|_| gone())?;
                let snap = Registry::snapshot(&h).ok_or_else(gone)?;
                Ok(serde_json::json!({ "top": snap.viewport.top, "total": snap.viewport.total }))
            }
            method::SESSION_TEXT => {
                let h = self.session(req)?;
                let from: u64 = Self::param(req, "from")?;
                let to: u64 = Self::param(req, "to")?;
                let format: String = Self::param(req, "format").unwrap_or_else(|_| "plain".into());
                let format = match format.as_str() {
                    "plain" => TextFormat::Plain,
                    "rows" => TextFormat::Rows,
                    "html" => TextFormat::Html,
                    other => {
                        return Err(RpcError::new(
                            RpcError::INVALID_PARAMS,
                            format!("unknown text format `{other}` (plain|rows|html)"),
                        ));
                    }
                };
                let text = Registry::export(&h, from, to, format).ok_or_else(gone)?;
                Ok(serde_json::json!({ "text": text }))
            }
            method::SESSION_CLEAR => {
                let h = self.session(req)?;
                h.cmd.send(SessionCmd::Clear).map_err(|_| gone())?;
                let snap = Registry::snapshot(&h).ok_or_else(gone)?;
                Ok(serde_json::json!({ "top": snap.viewport.top, "total": snap.viewport.total }))
            }
            method::SESSION_FIND => {
                let h = self.session(req)?;
                let query: String = Self::param(req, "query")?;
                let is_regex: bool = Self::param(req, "regex").unwrap_or(false);
                let case_sensitive: bool = Self::param(req, "case_sensitive").unwrap_or(false);
                let snap = Registry::snapshot(&h).ok_or_else(gone)?;
                let from: u64 = Self::param(req, "from").unwrap_or(0);
                let to: u64 = Self::param(req, "to")
                    .unwrap_or_else(|_| snap.viewport.total.saturating_sub(1));
                let limit: usize = Self::param(req, "limit").unwrap_or(1000);
                let pattern = if is_regex {
                    query
                } else {
                    regex::escape(&query)
                };
                let re = regex::RegexBuilder::new(&pattern)
                    .case_insensitive(!case_sensitive)
                    .build()
                    .map_err(|e| {
                        RpcError::new(RpcError::INVALID_PARAMS, format!("bad `query`: {e}"))
                    })?;
                Ok(serde_json::json!(find_rows(&h, &re, from, to, limit)))
            }
            method::AI_ASK => {
                let prompt: String = Self::param(req, "prompt")?;
                let feature: String = Self::param(req, "feature").unwrap_or_else(|_| "ask".into());
                let session: Option<String> = Self::param(req, "session").ok();
                let history: Vec<crate::ai::HistoryTurn> =
                    Self::param(req, "history").unwrap_or_default();
                let stream: bool = Self::param(req, "stream").unwrap_or(false);
                let agent: bool = Self::param(req, "agent").unwrap_or(false);
                let run = if agent {
                    crate::agent_mode::run
                } else if stream {
                    crate::ai::ask_streaming
                } else {
                    crate::ai::ask
                };
                run(
                    &self.registry,
                    &self.store,
                    &prompt,
                    &feature,
                    session.as_deref(),
                    &history,
                )
            }
            method::AI_DOCTOR => crate::ai::doctor(&self.registry),
            method::AI_PAYLOAD_LAST => {
                let session: Option<String> = Self::param(req, "session").ok();
                let key = session
                    .as_deref()
                    .and_then(|s| self.registry.find(s))
                    .map(|h| {
                        h.info
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .id
                            .0
                            .clone()
                    })
                    .or(session)
                    .unwrap_or_else(|| "global".into());
                self.registry.last_payload(&key).ok_or_else(|| {
                    RpcError::new(
                        RpcError::INVALID_PARAMS,
                        format!("nothing has been sent for `{key}` since the daemon started"),
                    )
                })
            }
            method::EGRESS_TAIL => {
                let limit: usize = Self::param(req, "limit").unwrap_or(20);
                let with_payload: bool = Self::param(req, "payload").unwrap_or(false);
                let records = self
                    .store
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .recent_egress(limit)
                    .map_err(|e| RpcError::new(RpcError::INTERNAL, e.to_string()))?;
                Ok(serde_json::Value::Array(
                    records
                        .into_iter()
                        .map(|r| {
                            serde_json::json!({
                                "at": r.at, "provider": r.provider, "model": r.model, "purpose": r.purpose,
                                "bytes_sent": r.bytes_sent, "redactions": r.redactions,
                                "payload": if with_payload { r.payload } else { None },
                            })
                        })
                        .collect(),
                ))
            }
            method::POLICY_CLASSIFY => {
                let command: String = Self::param(req, "command")?;
                let session: Option<String> = Self::param(req, "session").ok();
                let cwd: Option<String> = Self::param(req, "cwd").ok();
                crate::policy::classify_rpc(
                    &self.registry,
                    &command,
                    session.as_deref(),
                    cwd.as_deref(),
                )
            }
            method::HISTORY_SEARCH => {
                let prefix: String = Self::param(req, "prefix").unwrap_or_default();
                let limit: usize = Self::param(req, "limit").unwrap_or(200);
                let items = self
                    .store
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .history(&prefix, limit.min(2000))
                    .map_err(|e| RpcError::new(RpcError::INTERNAL, e.to_string()))?;
                Ok(serde_json::json!(items))
            }
            method::SESSION_BLOCK_BOOKMARK => {
                let sid = self.session_id_for(req)?;
                let seq: i64 = Self::param(req, "seq")?;
                let on: bool = Self::param(req, "on")?;
                let found = self
                    .store
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .set_block_bookmark(seq, on)
                    .map_err(|e| RpcError::new(RpcError::INTERNAL, e.to_string()))?;
                if !found {
                    return Err(RpcError::new(
                        RpcError::INVALID_PARAMS,
                        format!("no block with seq {seq}"),
                    ));
                }
                if let Some(server) = self.registry.server() {
                    server.broadcast(
                        vt_proto::session::notification::SESSION_BLOCK_CHANGED,
                        Some(serde_json::json!({ "id": sid.0, "seq": seq, "bookmarked": on })),
                    );
                }
                Ok(serde_json::json!({ "seq": seq, "bookmarked": on }))
            }
            method::SESSION_BLOCKS => {
                let after: i64 = req
                    .params
                    .as_ref()
                    .and_then(|p| p.get("after"))
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                let sid = self.session_id_for(req)?;
                let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
                let blocks = store
                    .blocks(&sid, after, 10_000)
                    .map_err(|e| RpcError::new(RpcError::INTERNAL, e.to_string()))?;
                let out: Vec<_> = blocks
                    .into_iter()
                    .map(|b| {
                        serde_json::json!({ "seq": b.seq, "bookmarked": b.bookmarked, "block": b.block })
                    })
                    .collect();
                Ok(serde_json::json!(out))
            }
            method::CONFIG_GET => {
                let config = self
                    .registry
                    .config()
                    .ok_or_else(|| RpcError::new(RpcError::INTERNAL, "config not loaded"))?;
                Ok(config.describe())
            }
            method::CONFIG_RELOAD => {
                let config = self
                    .registry
                    .config()
                    .ok_or_else(|| RpcError::new(RpcError::INTERNAL, "config not loaded"))?;
                config.reload();
                Ok(config.describe())
            }
            method::CONFIG_SET => {
                let config = self
                    .registry
                    .config()
                    .ok_or_else(|| RpcError::new(RpcError::INTERNAL, "config not loaded"))?;
                let key: String = Self::param(req, "key")?;
                let value: serde_json::Value = Self::param(req, "value")?;
                let new = config
                    .set(&key, &value)
                    .map_err(|e| RpcError::new(RpcError::INVALID_PARAMS, e.to_string()))?;
                serde_json::to_value(new).map_err(|e| internal(&e))
            }
            method::CONFIG_KEYMAP_SET => {
                let config = self
                    .registry
                    .config()
                    .ok_or_else(|| RpcError::new(RpcError::INTERNAL, "config not loaded"))?;
                let chord: String = Self::param(req, "chord")?;
                let action: Option<String> = req
                    .params
                    .as_ref()
                    .and_then(|p| p.get("action"))
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                let resolved = config
                    .set_binding(&chord, action.as_deref())
                    .map_err(|e| RpcError::new(RpcError::INVALID_PARAMS, e.to_string()))?;
                serde_json::to_value(resolved).map_err(|e| internal(&e))
            }
            method::CONFIG_THEME_SAVE => {
                let config = self
                    .registry
                    .config()
                    .ok_or_else(|| RpcError::new(RpcError::INTERNAL, "config not loaded"))?;
                let theme: vt_config::theme::Theme = Self::param(req, "theme")?;
                let problems = theme.warnings();
                let path = config
                    .save_theme(&theme)
                    .map_err(|e| RpcError::new(RpcError::INVALID_PARAMS, e.to_string()))?;
                Ok(serde_json::json!({ "path": path, "warnings": problems }))
            }
            "inbox.list" => {
                let agents = self
                    .registry
                    .agents()
                    .ok_or_else(|| RpcError::new(RpcError::INTERNAL, "agent layer not ready"))?;
                serde_json::to_value(agents.inbox()).map_err(|e| internal(&e))
            }
            "inbox.decide" => {
                let agents = self
                    .registry
                    .agents()
                    .ok_or_else(|| RpcError::new(RpcError::INTERNAL, "agent layer not ready"))?;
                let id: String = Self::param(req, "id")?;
                let decision: vt_proto::approval::Decision = Self::param(req, "decision")?;
                if let vt_proto::approval::Decision::Allow {
                    updated_input: Some(_),
                } = &decision
                {
                    let session = agents
                        .inbox()
                        .into_iter()
                        .find(|i| i.id.0 == id)
                        .map(|i| i.session);
                    let is_codex =
                        session
                            .and_then(|s| self.registry.find(&s.0))
                            .is_some_and(|h| {
                                h.info.lock().unwrap_or_else(PoisonError::into_inner).agent
                                    == vt_proto::agent::AgentKind::Codex
                            });
                    if is_codex {
                        return Err(RpcError::new(
                            RpcError::INVALID_PARAMS,
                            "Codex approvals accept or decline only; deny with a reason telling it what to run instead",
                        ));
                    }
                }
                let by = vt_proto::approval::DecisionSource::Human;
                match agents.decide(&vt_proto::approval::ApprovalId(id.clone()), decision, by) {
                    Some(item) => serde_json::to_value(item).map_err(|e| internal(&e)),
                    None => Err(RpcError::new(
                        RpcError::INVALID_PARAMS,
                        format!("no pending approval `{id}` (see `vterm inbox list`)"),
                    )),
                }
            }
            "agent.events" => {
                let key: String = Self::param(req, "id")?;
                let sid = match self.session(req) {
                    Ok(h) => h
                        .info
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .id
                        .clone(),
                    Err(not_running) => {
                        let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
                        Self::stored_session(&store, &key)
                            .ok_or(not_running)?
                            .info
                            .id
                    }
                };
                let after: i64 = Self::param(req, "after").unwrap_or(0);
                let limit: usize = Self::param(req, "limit").unwrap_or(200);
                let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
                let events = store
                    .events(&sid, after, limit)
                    .map_err(|e| RpcError::new(RpcError::INTERNAL, e.to_string()))?;
                serde_json::to_value(events).map_err(|e| internal(&e))
            }
            other => Err(RpcError::new(
                RpcError::METHOD_NOT_FOUND,
                format!("unknown method `{other}`"),
            )),
        }
    }
}

/// Scans absolute rows `from..=to` in chunks through the session thread
/// and returns matches as `{row, col, len}` (columns in characters).
fn find_rows(
    h: &crate::registry::SessionHandle,
    re: &regex::Regex,
    from: u64,
    to: u64,
    limit: usize,
) -> Vec<serde_json::Value> {
    const CHUNK: u64 = 500;
    let mut out = Vec::new();
    let mut start = from.min(to);
    let end = from.max(to);
    while start <= end && out.len() < limit {
        let stop = start.saturating_add(CHUNK - 1).min(end);
        // One line per grid row: soft wraps must not shift the row numbers.
        let Some(text) = Registry::export(h, start, stop, TextFormat::Rows) else {
            break;
        };
        for (i, line) in text.lines().enumerate() {
            for m in re.find_iter(line) {
                out.push(serde_json::json!({
                    "row": start + i as u64,
                    "col": line[..m.start()].chars().count(),
                    "len": m.as_str().chars().count(),
                }));
                if out.len() >= limit {
                    return out;
                }
            }
        }
        if stop == end {
            break;
        }
        start = stop + 1;
    }
    out
}

fn internal(e: &serde_json::Error) -> RpcError {
    RpcError::new(RpcError::INTERNAL, e.to_string())
}

fn gone() -> RpcError {
    RpcError::new(RpcError::NO_SUCH_SESSION, "session thread has exited")
}
