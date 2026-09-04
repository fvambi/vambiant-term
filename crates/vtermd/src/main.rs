//! `vtermd` — launchd LaunchAgent that owns every PTY (ADR-0004).
//!
//! Planned module structure (M2):
//! * `registry`  — session table, PTY ownership, attach/detach, re-adoption
//! * `flush`     — damage coalescing at display cadence, snapshot + delta
//! * `hooks`     — HTTP hook receiver on the same socket
//! * `api`       — JSON-RPC over Unix socket + loopback HTTP/WS
//! * `launchd`   — plist, `KeepAlive`, crash recovery
//!
//! Nothing here is implemented in M0; this binary exists so the workspace,
//! CI and `cargo deny` see the real dependency graph from day one.

fn main() {
    println!("vtermd {} (M0 scaffold; daemon lands in M2)", env!("CARGO_PKG_VERSION"));
}
