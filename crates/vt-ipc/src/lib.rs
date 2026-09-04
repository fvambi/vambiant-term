//! JSON-RPC 2.0 plumbing between `vtermd`, the app and `vterm` (ADR-0004).
//!
//! Transport is a Unix socket (mode 0700, `LOCAL_PEERCRED` checked) plus a
//! token-authenticated loopback HTTP/WebSocket listener. Framing is
//! newline-delimited JSON. The client half is shared by the app and the CLI.

pub mod auth;
pub mod client;
pub mod error;
pub mod server;
pub mod transport;

pub use error::IpcError;
