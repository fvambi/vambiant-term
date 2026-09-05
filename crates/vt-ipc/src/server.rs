//! Server half: accept, authenticate, dispatch, broadcast.

use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use vt_proto::jsonrpc::{Message, Request, Response, RpcError};

use crate::auth;
use crate::error::IpcError;
use crate::transport::{Framed, ensure_runtime_dir};

/// Application logic behind the socket. One instance serves every connection,
/// so it must be `Send + Sync`; per-connection state is the handler's business.
pub trait Handler: Send + Sync + 'static {
    /// Answer a request. Notifications (no id) are delivered too; the result
    /// is then discarded.
    fn handle(&self, request: &Request) -> Result<serde_json::Value, RpcError>;
}

/// A running listener.
#[derive(Debug)]
pub struct Server {
    path: PathBuf,
    clients: Arc<Mutex<Vec<UnixStream>>>,
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
        let clients: Arc<Mutex<Vec<UnixStream>>> = Arc::new(Mutex::new(Vec::new()));
        let registry = Arc::clone(&clients);
        thread::Builder::new()
            .name("vt-ipc-accept".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    let Ok(stream) = conn else { break };
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
                    if let Ok(clone) = stream.try_clone() {
                        registry
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push(clone);
                    }
                    let handler = Arc::clone(&handler);
                    let registry = Arc::clone(&registry);
                    thread::Builder::new()
                        .name("vt-ipc-conn".into())
                        .spawn(move || serve_connection(stream, &handler, &registry))
                        .ok();
                }
            })
            .map_err(listen_err)?;
        Ok(Self {
            path: path.to_path_buf(),
            clients,
        })
    }

    /// Send a notification to every connected client.
    pub fn broadcast(&self, method: &str, params: Option<serde_json::Value>) {
        let msg = Message::Request(Request::notification(method, params));
        let Ok(mut buf) = serde_json::to_vec(&msg) else {
            return;
        };
        buf.push(b'\n');
        let mut clients = self.clients.lock().unwrap_or_else(PoisonError::into_inner);
        clients.retain(|c| {
            use std::io::Write;
            let mut w = c;
            w.write_all(&buf).is_ok()
        });
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
    registry: &Mutex<Vec<UnixStream>>,
) {
    let Ok(mut framed) = Framed::new(stream) else {
        return;
    };
    loop {
        match framed.read() {
            Ok(Some(Message::Request(req))) => {
                let id = req.id.clone();
                let result = handler.handle(&req);
                if req.is_notification() {
                    continue;
                }
                let resp = match result {
                    Ok(v) => Response::ok(id, v),
                    Err(e) => Response::err(id, e),
                };
                if framed.write(&Message::Response(resp)).is_err() {
                    break;
                }
            }
            Ok(Some(Message::Response(_))) => {} // clients do not answer us (yet)
            Err(IpcError::Protocol(detail)) => {
                let _ = framed.write(&Message::Response(Response::err(
                    None,
                    RpcError::new(RpcError::PARSE_ERROR, detail),
                )));
            }
            // Clean EOF or a broken connection: either way we are done.
            Ok(None) | Err(_) => break,
        }
    }
    framed.close();
    registry
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|c| {
            // Drop registry entries whose peer is gone: a zero-byte write fails on a closed socket.
            use std::io::Write;
            let mut w = c;
            w.write_all(&[]).is_ok() && c.peer_addr().is_ok()
        });
}
