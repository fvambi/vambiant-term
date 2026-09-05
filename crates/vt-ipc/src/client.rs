//! Client half: connect, call, receive notifications.

use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use vt_proto::jsonrpc::{Id, Message, Request};

use crate::error::IpcError;
use crate::transport::Framed;

/// A connection to `vtermd`. Calls are sequential per client; notifications
/// that arrive while waiting for a response are queued and returned by
/// [`Client::next_notification`].
#[derive(Debug)]
pub struct Client {
    framed: Framed,
    next_id: AtomicU64,
    pending: std::collections::VecDeque<Request>,
}

impl Client {
    /// Connect to the socket at `path`.
    pub fn connect(path: &Path) -> Result<Self, IpcError> {
        let stream = UnixStream::connect(path).map_err(|source| IpcError::Connect {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(Self {
            framed: Framed::new(stream)?,
            next_id: AtomicU64::new(1),
            pending: std::collections::VecDeque::new(),
        })
    }

    /// Call `method` and wait for its response.
    pub fn call(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, IpcError> {
        let id = Id::Number(self.next_id.fetch_add(1, Ordering::Relaxed));
        self.framed
            .write(&Message::Request(Request::new(id.clone(), method, params)))?;
        loop {
            match self.framed.read()? {
                Some(Message::Response(resp)) if resp.id == Some(id.clone()) => {
                    return match (resp.result, resp.error) {
                        (Some(v), None) => Ok(v),
                        (_, Some(e)) => Err(IpcError::Remote {
                            method: method.into(),
                            code: e.code,
                            message: e.message,
                        }),
                        (None, None) => Ok(serde_json::Value::Null),
                    };
                }
                Some(Message::Response(resp)) => {
                    // An unsolicited error (e.g. auth refusal) carries no id.
                    if let Some(e) = resp.error {
                        return Err(IpcError::Remote {
                            method: method.into(),
                            code: e.code,
                            message: e.message,
                        });
                    }
                }
                Some(Message::Request(n)) if n.is_notification() => self.pending.push_back(n),
                Some(Message::Request(_)) => {}
                None => {
                    return Err(IpcError::Protocol(
                        "connection closed before the response arrived".into(),
                    ));
                }
            }
        }
    }

    /// Fire-and-forget notification to the daemon.
    pub fn notify(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), IpcError> {
        self.framed
            .write(&Message::Request(Request::notification(method, params)))
    }

    /// Next queued notification, reading from the socket when the queue is
    /// empty. Blocks. `Ok(None)` when the daemon closed the connection.
    pub fn next_notification(&mut self) -> Result<Option<Request>, IpcError> {
        if let Some(n) = self.pending.pop_front() {
            return Ok(Some(n));
        }
        loop {
            match self.framed.read()? {
                Some(Message::Request(n)) if n.is_notification() => return Ok(Some(n)),
                Some(_) => {}
                None => return Ok(None),
            }
        }
    }
}
