//! `vtermd` — the daemon that owns every PTY (ADR-0004).
//!
//! * `registry` — session table; one thread per session owning its PTY and
//!   terminal core (libghostty handles are `!Send`, so the core never leaves
//!   the thread that created it).
//! * `session`  — the per-session loop: PTY bytes → core → damage, flushed to
//!   attached viewers at display cadence (≥ 8 ms between deltas).
//! * `rpc`      — the JSON-RPC handler over `vt-ipc`.
//! * `wire`     — snapshot/delta encoding.
//!
//! Recovery: a PTY master file descriptor dies with the process that holds
//! it, so sessions from a previous daemon incarnation are re-listed as
//! **orphaned** (never silently dropped) until a per-session fd holder lands
//! (ADR-0004 amendment, proposed 2026-09-05).

mod registry;
mod rpc;
mod session;
mod wire;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use vt_store::Store;

fn state_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("VAMBIANT_TERM_STATE") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    home.join(".local/state/vambiant-term")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("vtermd {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let socket = args
        .iter()
        .position(|a| a == "--socket")
        .and_then(|i| args.get(i + 1))
        .map_or_else(vt_ipc::transport::socket_path, PathBuf::from);
    let db = state_dir().join("state.db");

    let store = match Store::open(&db) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("vtermd: {e}");
            std::process::exit(2);
        }
    };
    match store.sweep(vt_store::retention::Retention::default(), &registry::now()) {
        Ok(swept) => eprintln!("vtermd: retention sweep removed {swept:?}"),
        Err(e) => eprintln!("vtermd: retention sweep failed: {e}"),
    }
    // Sessions that were live when the previous daemon died cannot be
    // re-attached to their PTYs; say so rather than pretend.
    match store.live_sessions() {
        Ok(live) => {
            for rec in live {
                if let Err(e) = store.mark_orphaned(&rec.info.id) {
                    eprintln!("vtermd: cannot mark {} orphaned: {e}", rec.info.id.0);
                } else {
                    eprintln!(
                        "vtermd: session {} ({}) is orphaned: its PTY belonged to a previous daemon",
                        rec.info.id.0, rec.info.name
                    );
                }
            }
        }
        Err(e) => eprintln!("vtermd: cannot read previous sessions: {e}"),
    }

    let store = Arc::new(Mutex::new(store));
    let registry = Arc::new(registry::Registry::new(Arc::clone(&store)));
    let handler = Arc::new(rpc::Rpc::new(Arc::clone(&registry), Arc::clone(&store)));
    let server = match vt_ipc::Server::start(&socket, handler) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("vtermd: {e}");
            std::process::exit(2);
        }
    };
    registry.set_server(Arc::new(server));
    eprintln!(
        "vtermd {} listening on {} (state: {})",
        env!("CARGO_PKG_VERSION"),
        socket.display(),
        db.display()
    );

    // Park until killed. launchd (KeepAlive) restarts us; the socket file is
    // recreated on start, so a stale one is harmless.
    loop {
        std::thread::park();
    }
}
