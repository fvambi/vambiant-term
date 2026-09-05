//! Server half: accept, authenticate, dispatch, publish.
//!
//! Every connection owns a writer thread fed by a bounded outbox. Responses
//! and notifications both go through it, so lines never interleave and a
//! publisher never blocks on a slow reader: a client that falls
//! [`OUTBOX_CAPACITY`] messages behind is disconnected and must reconnect
//! (an attach then yields a fresh full snapshot), which is the honest
//! alternative to silently dropping deltas.

use std::collections::HashSet;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use vt_proto::jsonrpc::{Message, Request, Response, RpcError};

use crate::auth;
use crate::error::IpcError;
use crate::transport::{Framed, ensure_runtime_dir};

/// Messages a connection may fall behind before it is dropped.
pub const OUTBOX_CAPACITY: usize = 512;

/// Identifies a connection for the lifetime of the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConnId(pub u64);

/// Application logic behind the socket. One instance serves every connection,
/// so it must be `Send + Sync`; per-connection state is keyed by [`ConnId`].
pub trait Handler: Send + Sync + 'static {
    /// Answer a request. Notifications (no id) are delivered too; the result
    /// is then discarded.
    fn handle(&self, conn: ConnId, request: &Request) -> Result<serde_json::Value, RpcError>;

    /// A connection went away; drop any per-connection state.
    fn disconnected(&self, _conn: ConnId) {}
}

struct Conn {
    id: ConnId,
    outbox: SyncSender<Vec<u8>>,
    /// Topics (session ids) this connection asked for.
    subscriptions: HashSet<String>,
}

type Conns = Arc<Mutex<Vec<Conn>>>;

/// A running listener.
#[derive(Debug)]
pub struct Server {
    path: PathBuf,
    conns: Conns,
}

impl std::fmt::Debug for Conn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conn")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

fn encode(method: &str, params: Option<serde_json::Value>) -> Option<Vec<u8>> {
    let msg = Message::Request(Request::notification(method, params));
    let mut buf = serde_json::to_vec(&msg).ok()?;
    buf.push(b'\n');
    Some(buf)
}

impl Server {
    /// Bind `path` (removing a stale socket file first) and serve `handler`
    /// on a background thread per connection.
    pub fn start(path: &Path, handler: Arc<dyn Handler>) -> Result<Self, IpcError> {
        let listen_err = |source| IpcError::Listen {
            path: path.to_path_buf(),
            source,
        };
        if let Some(dir) = path.parent() {
            ensure_runtime_dir(dir).map_err(listen_err)?;
        }
        if path.exists() {
            std::fs::remove_file(path).map_err(listen_err)?;
        }
        let listener = UnixListener::bind(path).map_err(listen_err)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(listen_err)?;
        let conns: Conns = Arc::new(Mutex::new(Vec::new()));
        let registry = Arc::clone(&conns);
        let next_id = AtomicU64::new(1);
        thread::Builder::new()
            .name("vt-ipc-accept".into())
            .spawn(move || {
                for incoming in listener.incoming() {
                    let Ok(stream) = incoming else { break };
                    if let Err(e) = auth::check_peer(&stream) {
                        // Refused peers get one error line so the failure is visible.
                        if let Ok(mut f) = Framed::new(stream) {
                            let _ = f.write(&Message::Response(Response::err(
                                None,
                                RpcError::new(RpcError::UNAUTHORIZED, e.to_string()),
                            )));
                            f.close();
                        }
                        continue;
                    }
                    let id = ConnId(next_id.fetch_add(1, Ordering::Relaxed));
                    let Ok(write_half) = stream.try_clone() else {
                        continue;
                    };
                    let (outbox, inbox) = sync_channel::<Vec<u8>>(OUTBOX_CAPACITY);
                    registry
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(Conn {
                            id,
                            outbox: outbox.clone(),
                            subscriptions: HashSet::new(),
                        });
                    // Writer: the only thread that touches the write half.
                    thread::Builder::new()
                        .name("vt-ipc-write".into())
                        .spawn(move || {
                            let mut w = write_half;
                            for line in inbox {
                                if w.write_all(&line).is_err() {
                                    break;
                                }
                            }
                            let _ = w.shutdown(std::net::Shutdown::Both);
                        })
                        .ok();
                    let handler = Arc::clone(&handler);
                    let registry = Arc::clone(&registry);
                    thread::Builder::new()
                        .name("vt-ipc-conn".into())
                        .spawn(move || serve_connection(stream, &handler, &registry, id, &outbox))
                        .ok();
                }
            })
            .map_err(listen_err)?;
        Ok(Self {
            path: path.to_path_buf(),
            conns,
        })
    }

    /// Subscribe a connection to a topic (a session id).
    pub fn subscribe(&self, conn: ConnId, topic: &str) {
        let mut conns = self.conns.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(c) = conns.iter_mut().find(|c| c.id == conn) {
            c.subscriptions.insert(topic.to_owned());
        }
    }

    /// Remove a subscription.
    pub fn unsubscribe(&self, conn: ConnId, topic: &str) {
        let mut conns = self.conns.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(c) = conns.iter_mut().find(|c| c.id == conn) {
            c.subscriptions.remove(topic);
        }
    }

    /// Send a notification to every connected client.
    pub fn broadcast(&self, method: &str, params: Option<serde_json::Value>) {
        if let Some(line) = encode(method, params) {
            self.deliver(&line, |_| true);
        }
    }

    /// Send a notification to the subscribers of `topic` only.
    pub fn publish(&self, topic: &str, method: &str, params: Option<serde_json::Value>) {
        if let Some(line) = encode(method, params) {
            self.deliver(&line, |c| c.subscriptions.contains(topic));
        }
    }

    fn deliver(&self, line: &[u8], select: impl Fn(&Conn) -> bool) {
        let mut conns = self.conns.lock().unwrap_or_else(PoisonError::into_inner);
        conns.retain(|c| {
            if !select(c) {
                return true;
            }
            match c.outbox.try_send(line.to_vec()) {
                Ok(()) => true,
                Err(TrySendError::Full(_)) => {
                    eprintln!(
                        "vt-ipc: connection {} fell {OUTBOX_CAPACITY} messages behind; disconnecting it",
                        c.id.0
                    );
                    false // dropping the sender ends the writer and closes the socket
                },
                Err(TrySendError::Disconnected(_)) => false,
            }
        });
    }

    /// Number of live connections.
    pub fn connections(&self) -> usize {
        self.conns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Socket path being served.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn serve_connection(
    stream: UnixStream,
    handler: &Arc<dyn Handler>,
    registry: &Mutex<Vec<Conn>>,
    id: ConnId,
    outbox: &SyncSender<Vec<u8>>,
) {
    let Ok(mut framed) = Framed::new(stream) else {
        return;
    };
    let send = |resp: Response| -> bool {
        let Ok(mut line) = serde_json::to_vec(&Message::Response(resp)) else {
            return false;
        };
        line.push(b'\n');
        // Our own response to our own client: blocking is correct here.
        outbox.send(line).is_ok()
    };
    loop {
        match framed.read() {
            Ok(Some(Message::Request(req))) => {
                let rid = req.id.clone();
                let result = handler.handle(id, &req);
                if req.is_notification() {
                    continue;
                }
                let resp = match result {
                    Ok(v) => Response::ok(rid, v),
                    Err(e) => Response::err(rid, e),
                };
                if !send(resp) {
                    break;
                }
            }
            Ok(Some(Message::Response(_))) => {} // clients do not answer us (yet)
            Err(IpcError::Protocol(detail)) => {
                if !send(Response::err(
                    None,
                    RpcError::new(RpcError::PARSE_ERROR, detail),
                )) {
                    break;
                }
            }
            // Clean EOF or a broken connection: either way we are done.
            Ok(None) | Err(_) => break,
        }
    }
    framed.close();
    registry
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|c| c.id != id);
    handler.disconnected(id);
}
