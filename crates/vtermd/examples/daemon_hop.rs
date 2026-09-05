//! Daemon hop cost (docs/08 §7, ADR-0004): byte in → echo out through a
//! PTY running `cat`, once directly and once through `vtermd`. The direct
//! number is the floor the kernel and `cat` impose; the difference is what
//! the daemon adds (IPC, parse, flush, notification).
//!
//!     cargo run --release -p vtermd --example daemon_hop -- <vtermd.sock> [iterations]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use base64::Engine as _;
use vt_ipc::Client;
use vt_proto::session::{NewSession, OutputDelta, SessionInfo, method, notification};
use vt_pty::{Pty, SpawnSpec, WinSize};

fn percentile(sorted: &[Duration], pct: usize) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let idx = (sorted.len() * pct).div_ceil(100).clamp(1, sorted.len()) - 1;
    sorted[idx]
}

fn report(label: &str, mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    let p50 = percentile(&samples, 50);
    let p99 = percentile(&samples, 99);
    println!(
        "{label:<8} n={:<4} p50={:>8.3?} p99={:>8.3?} max={:>8.3?}",
        samples.len(),
        p50,
        p99,
        samples.last().copied().unwrap_or_default()
    );
    p99
}

fn direct(iterations: usize) -> Vec<Duration> {
    let spec = SpawnSpec::program("hop-direct", vec!["/bin/cat".into()]);
    let mut pty = Pty::spawn(&spec, WinSize::cells(80, 24)).expect("spawn cat");
    let mut reader = pty.reader().expect("reader");
    let mut buf = [0u8; 64];
    let mut samples = Vec::with_capacity(iterations);
    for i in 0..iterations + 5 {
        let t0 = Instant::now();
        pty.write_all(b"a").expect("write");
        // cat echoes after the line discipline echoes; either way one byte
        // (or the tty's echo plus cat's) comes back.
        let n = reader.read(&mut buf).expect("read");
        assert!(n > 0);
        if i >= 5 {
            samples.push(t0.elapsed());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    samples
}

fn through_daemon(socket: &Path, iterations: usize) -> Vec<Duration> {
    let mut input = Client::connect(socket).expect("connect");
    let mut stream = Client::connect(socket).expect("connect");
    let req = NewSession {
        name: Some("hop".into()),
        argv: vec!["/bin/cat".into()],
        cwd: Some(std::env::temp_dir()),
        ..Default::default()
    };
    let info: SessionInfo = serde_json::from_value(
        input
            .call(method::SESSION_NEW, serde_json::to_value(req).ok())
            .expect("session.new"),
    )
    .expect("session info");
    let first: OutputDelta = serde_json::from_value(
        stream
            .call(
                method::SESSION_ATTACH,
                Some(serde_json::json!({ "id": info.id.0 })),
            )
            .expect("attach"),
    )
    .expect("snapshot");
    let mut seq = first.seq;
    let mut samples = Vec::with_capacity(iterations);
    let b64 = base64::engine::general_purpose::STANDARD.encode(b"a");
    for i in 0..iterations + 5 {
        let t0 = Instant::now();
        input
            .call(
                method::SESSION_INPUT,
                Some(serde_json::json!({ "id": info.id.0, "bytes": b64 })),
            )
            .expect("input");
        loop {
            let n = stream.next_notification().expect("stream").expect("open");
            if n.method != notification::SESSION_OUTPUT {
                continue;
            }
            let d: OutputDelta =
                serde_json::from_value(n.params.unwrap_or_default()).expect("delta");
            if d.seq > seq {
                seq = d.seq;
                break;
            }
        }
        if i >= 5 {
            samples.push(t0.elapsed());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = input.call(
        method::SESSION_KILL,
        Some(serde_json::json!({ "id": info.id.0 })),
    );
    samples
}

fn main() {
    let mut args = std::env::args().skip(1);
    let socket = PathBuf::from(
        args.next()
            .expect("usage: daemon_hop <vtermd.sock> [iterations]"),
    );
    let iterations: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(200);
    let direct_samples = direct(iterations);
    let daemon_samples = through_daemon(&socket, iterations);
    let d = report("direct", direct_samples);
    let v = report("vtermd", daemon_samples);
    let added = v.saturating_sub(d);
    println!(
        "hop      p99 added by the daemon: {added:.3?} (budget 1 ms; {})",
        if added <= Duration::from_millis(1) {
            "within budget"
        } else {
            "OVER BUDGET — ADR-0004 fallback applies"
        }
    );
}
