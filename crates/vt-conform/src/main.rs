//! `vt-conform` — runs Thomas Dickey's esctest2 against `vt-core` the way it
//! would run against a real terminal: esctest is spawned under a PTY, its
//! output is parsed by the core, and the core's query responses are written
//! back. This is the `conformance` CI job (docs/08 §9).
//!
//! esctest checks cell contents with DECRQCRA (a rectangle checksum), which
//! libghostty does not implement; this runner answers it from the grid
//! snapshot using xterm's ≥#334 convention (empty cells count as spaces).
//!
//! Usage: `vt-conform [--include REGEX] [-- extra esctest args]`.
//! Set `VT_ESCTEST=/path/to/esctest.py` to override the vendored copy.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use vt_core::backend::GhosttyCore;
use vt_core::cell::GridSize;
use vt_core::{CellSnapshot, TerminalCore};

mod decrqcra;

const COLS: u16 = 80;
const ROWS: u16 = 24;

fn esctest_path() -> PathBuf {
    if let Some(p) = std::env::var_os("VT_ESCTEST") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../third_party/esctest2/esctest/esctest.py")
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut esctest_args = vec![
        "--expected-terminal=xterm".to_string(),
        "--xterm-checksum=334".to_string(),
        "--force".to_string(),
        "--timeout=2".to_string(),
        "--no-print-logs".to_string(),
    ];
    if let Some(pos) = args.iter().position(|a| a == "--") {
        esctest_args.extend(args.split_off(pos + 1));
        args.pop();
    }
    esctest_args.extend(args);

    let script = esctest_path();
    assert!(
        script.is_file(),
        "esctest not found at {} (git submodule update --init)",
        script.display()
    );
    let logfile = std::env::temp_dir().join("vt-conform-esctest.log");
    let _ = std::fs::remove_file(&logfile);
    let mut argv = vec!["python3".to_string(), script.display().to_string()];
    argv.extend(esctest_args);
    argv.push(format!("--logfile={}", logfile.display()));

    let spec = vt_pty::SpawnSpec {
        argv,
        cwd: script.parent().map(PathBuf::from),
        env: vec![("TERM".into(), "xterm-256color".into())],
        session: "vt-conform".into(),
    };
    let mut pty =
        vt_pty::Pty::spawn(&spec, vt_pty::WinSize::cells(COLS, ROWS)).expect("spawn esctest");
    let mut core = GhosttyCore::new(GridSize {
        cols: COLS,
        rows: ROWS,
    })
    .expect("core");
    core.set_cell_pixel_size(8, 16);

    // Reader thread: blocking reads off the PTY, forwarded to the core owner.
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let mut reader = pty.reader().expect("reader");
    thread::spawn(move || {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut writer = pty.writer().expect("writer");
    let mut scanner = decrqcra::Scanner::default();
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(bytes) => {
                core.advance(&bytes);
                let mut responses = core.take_responses();
                for req in scanner.feed(&bytes) {
                    let snap: CellSnapshot = core.snapshot();
                    responses.extend(decrqcra::reply(&req, &snap));
                }
                if !responses.is_empty() {
                    let _ = writer.write_all(&responses);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if pty.try_wait().ok().flatten().is_some() {
                    while let Ok(bytes) = rx.try_recv() {
                        core.advance(&bytes);
                    }
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = pty.try_wait().ok().flatten();
    let log = std::fs::read_to_string(&logfile).unwrap_or_default();
    let summary = summarize(&log);
    println!("{summary}");
    println!("esctest exit: {status:?}; full log: {}", logfile.display());
    if !summary.starts_with("PASS") {
        std::process::exit(1);
    }
}

/// Reduce esctest's log to counts plus the failing test names.
fn summarize(log: &str) -> String {
    let mut passed = 0usize;
    let mut failed = Vec::new();
    let mut known = 0usize;
    for line in log.lines() {
        if line.contains("Passed.") {
            passed += 1;
        } else if let Some(rest) = line.strip_prefix("Failed: ") {
            failed.push(rest.trim().to_string());
        } else if line.contains("known bug") || line.contains("KnownBug") {
            known += 1;
        }
    }
    // Zero tests means the harness itself broke (esctest never ran); never green.
    let status = if failed.is_empty() && passed > 0 {
        "PASS"
    } else {
        "FAIL"
    };
    let mut out = format!(
        "{status}: {passed} passed, {} failed, {known} known-bug-skipped",
        failed.len()
    );
    for f in failed {
        out.push_str("\n  failed: ");
        out.push_str(&f);
    }
    out
}
