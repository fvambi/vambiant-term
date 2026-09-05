//! Throughput of `vt-core`'s primary backend through the `TerminalCore`
//! trait — the same bench/dump modes as the raw spikes, so the trait's
//! overhead over bare libghostty is measurable (M1 exit: within 2× of
//! Ghostty on the same corpus).

// Throwaway spike: pedantic lints are noise here. Product crates keep them.
#![allow(clippy::pedantic)]

use std::fs;
use std::io::Write;

use vt_core::TerminalCore;
use vt_core::backend::GhosttyCore;
use vt_core::cell::GridSize;
use vt_core::harness::dump;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("");
    let path = args
        .get(2)
        .expect("usage: vtcore-spike bench|dump <file> [cols] [rows]");
    let cols = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);
    let rows = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(50);
    let data = fs::read(path).expect("read file");
    let mut core =
        GhosttyCore::with_scrollback(GridSize { cols, rows }, 16 * 1024 * 1024).expect("core");
    for chunk in data.chunks(64 * 1024) {
        core.advance(chunk);
        // Damage is consumed per chunk as the daemon's flush would.
        let _ = core.take_damage();
    }
    match mode {
        "dump" => print!("{}", dump(&core.snapshot())),
        _ => {
            let snap = core.snapshot();
            let checksum = snap.cells.iter().fold(0u64, |acc, c| {
                acc.wrapping_mul(31).wrapping_add(u64::from(c.ch as u32))
            });
            let mut out = std::io::stdout().lock();
            writeln!(
                out,
                "vt-core bytes={} checksum={checksum:016x} history={}",
                data.len(),
                core.scrollback_rows()
            )
            .ok();
        }
    }
}
