//! `vtermd` — the daemon that owns every PTY (ADR-0004).
//!
//! * `registry` — session table; one thread per session owning its PTY and
//!   terminal core (libghostty handles are `!Send`, so the core never leaves
//!   the thread that created it).
//! * `holder`   — client side of `vtermd-hold`, the per-session process that
//!   keeps the PTY master alive across daemon restarts (ADR-0004 amendment).
//! * `session`  — the per-session loop: PTY bytes → core → damage, flushed to
//!   attached viewers at display cadence (≥ 8 ms between deltas).
//! * `rpc`      — the JSON-RPC handler over `vt-ipc`.
//! * `wire`     — snapshot/delta encoding.
//!
//! On start the daemon re-adopts every session its predecessor left running;
//! whatever cannot be re-adopted is marked orphaned, never dropped.

#![allow(unsafe_code)] // reaping detached holders with waitpid; nothing else.

mod holder;
mod registry;
mod rpc;
mod session;
mod wire;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    let runtime_dir = socket
        .parent()
        .map_or_else(vt_ipc::transport::runtime_dir, PathBuf::from);
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

    let store = Arc::new(Mutex::new(store));
    let registry = Arc::new(registry::Registry::new(Arc::clone(&store), runtime_dir));
    // Re-adopt before accepting clients so the first `session.list` is true.
    registry.readopt_all();
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

    // Holders are spawned detached but remain our children while we live;
    // reap the ones that exit so they do not linger as zombies.
    loop {
        std::thread::sleep(Duration::from_secs(1));
        loop {
            let mut status = 0;
            // SAFETY: waitpid with WNOHANG on any child; no memory is shared.
            let pid = unsafe { libc::waitpid(-1, &raw mut status, libc::WNOHANG) };
            if pid <= 0 {
                break;
            }
        }
    }
}
