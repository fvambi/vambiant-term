//! JSON-RPC methods (names in `vt_proto::session::method`).

use std::sync::{Arc, Mutex, PoisonError};

use base64::Engine as _;
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

fn internal(e: &serde_json::Error) -> RpcError {
    RpcError::new(RpcError::INTERNAL, e.to_string())
}

fn gone() -> RpcError {
    RpcError::new(RpcError::NO_SUCH_SESSION, "session thread has exited")
}
