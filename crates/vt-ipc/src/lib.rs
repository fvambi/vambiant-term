//! JSON-RPC 2.0 plumbing between `vtermd`, the app and `vterm` (ADR-0004).
//!
//! Transport is a Unix socket in a directory only the user can enter, with
//! the peer's uid checked on every accepted connection ([`auth`]). Framing is
//! one JSON object per line. The client half is shared by the app and the CLI.
//!
//! Deliberately synchronous: a local daemon serves a handful of viewers, and
//! a thread per connection is simpler to reason about than an executor on the
//! hot path. The loopback HTTP/WebSocket listener (token-authenticated) is a
//! separate M2 item and does not share this transport.

#![allow(unsafe_code)] // getpeereid / geteuid in `auth`; nothing else.

pub mod auth;
pub mod client;
pub mod error;
pub mod server;
pub mod transport;

pub use client::Client;
pub use error::IpcError;
pub use server::{Handler, Server};
