//! IPC errors.

use std::path::PathBuf;

/// Transport and protocol errors, each naming what to look at.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// Could not reach the daemon.
    #[error(
        "cannot connect to vtermd at {path}: {source} (is vtermd running? `vterm daemon status`)"
    )]
    Connect {
        /// Socket path tried.
        path: PathBuf,
        /// OS error.
        #[source]
        source: std::io::Error,
    },
    /// Could not create or bind the socket.
    #[error("cannot listen on {path}: {source}")]
    Listen {
        /// Socket path.
        path: PathBuf,
        /// OS error.
        #[source]
        source: std::io::Error,
    },
    /// The peer is not the owner of the socket.
    #[error("rejected connection from uid {peer_uid}: socket is owned by uid {owner_uid}")]
    Unauthorized {
        /// Peer's uid.
        peer_uid: u32,
        /// Our uid.
        owner_uid: u32,
    },
    /// I/O on an established connection.
    #[error("vtermd connection failed: {0}")]
    Io(#[from] std::io::Error),
    /// The peer sent something that is not JSON-RPC.
    #[error("malformed message from peer: {0}")]
    Protocol(String),
    /// The daemon returned an error for a call.
    #[error("vtermd refused `{method}`: {message} (code {code})")]
    Remote {
        /// Method called.
        method: String,
        /// Error code.
        code: i64,
        /// Message.
        message: String,
    },
}
