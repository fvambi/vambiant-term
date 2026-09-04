//! M0 spike for `libghostty-vt` 0.2.1 (feature `ghostty`; needs zig 0.15.2).
//!
//! Same two modes as `alacritty-spike` so the two can be compared on identical
//! input. The PTY itself comes from `alacritty_terminal::tty` — libghostty-vt
//! deliberately has no PTY layer, and the PTY is not what is under test.

// Throwaway spike: pedantic lints are noise here. Product crates keep them.
#![allow(clippy::pedantic, clippy::type_complexity, unsafe_code)]

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{self, EventedReadWrite, Options as PtyOptions, Shell};
use libghostty_vt::render::{CellIterator, Dirty, RenderState, RowIterator};
use libghostty_vt::terminal::{Options, Terminal};

fn new_term(cols: u16, rows: u16) -> Terminal<'static, 'static> {
    Terminal::new(Options {
        cols,
        rows,
        max_scrollback: 10_000,
    })
    .expect("ghostty terminal")
}

/// Walks the render state once, returning (dirty, dirty_rows, grid text, checksum).
fn snapshot(term: &Terminal<'static, 'static>) -> (Dirty, usize, String, u64) {
    let mut state = RenderState::new().expect("render state");
    let mut rows = RowIterator::new().expect("row iterator");
    let mut cells = CellIterator::new().expect("cell iterator");
    let snap = state.update(term).expect("update");
    let dirty = snap.dirty().expect("dirty");
    let mut text = String::new();
    let mut checksum: u64 = 0;
    let mut dirty_rows = 0;
    let mut it = rows.update(&snap).expect("rows");
    while let Some(row) = it.next() {
        if row.dirty().unwrap_or(false) {
            dirty_rows += 1;
        }
        let mut line = String::new();
        let mut cit = cells.update(row).expect("cells");
        while let Some(cell) = cit.next() {
            let cp = cell.raw_cell().and_then(|c| c.codepoint()).unwrap_or(0);
            let ch = char::from_u32(cp).filter(|c| *c != '\0').unwrap_or(' ');
            line.push(ch);
            checksum = checksum
                .wrapping_mul(31)
                .wrapping_add(u64::from(cp.max(0x20)));
        }
        text.push_str(line.trim_end());
        text.push('\n');
    }
    (dirty, dirty_rows, text, checksum)
}

fn drain_pty(pty: &mut tty::Pty, mut on_chunk: impl FnMut(&[u8])) {
    let mut buf = vec![0u8; 64 * 1024];
    let started = Instant::now();
    loop {
        match pty.reader().read(&mut buf) {
            Ok(0) => break,
            Ok(n) => on_chunk(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if child_exited(pty) {
                    if let Ok(n) = pty.reader().read(&mut buf)
                        && n > 0
                    {
                        on_chunk(&buf[..n]);
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(_) => break,
        }
        if started.elapsed() > Duration::from_secs(10) {
            eprintln!("timeout draining pty");
            break;
        }
    }
}

fn child_exited(pty: &tty::Pty) -> bool {
    let pid = pty.child().id();
    let mut status = 0i32;
    // SAFETY: waitpid on a pid we own with WNOHANG; no memory is shared.
    let r = unsafe {
        waitpid(pid.cast_signed(), &raw mut status, 1 /* WNOHANG */)
    };
    r == pid.cast_signed()
}

unsafe extern "C" {
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
}

fn spike_pty() {
    let script = "printf 'hello from pty\\n'; printf '\\033]0;spike-title\\007'; \
                  for i in 1 2 3 4 5; do printf 'line %d: the quick brown fox jumps over the lazy dog again and again\\n' $i; done; \
                  printf '\\033[3;5Hinserted\\033[0m'; printf '\\a'; sleep 0.2; echo done";
    let options = PtyOptions {
        shell: Some(Shell::new(
            "/bin/sh".into(),
            vec!["-c".into(), script.into()],
        )),
        working_directory: None,
        drain_on_exit: true,
        env: HashMap::new(),
    };
    let window = WindowSize {
        num_lines: 10,
        num_cols: 80,
        cell_width: 8,
        cell_height: 16,
    };
    let mut pty = tty::new(&options, window, 0).expect("spawn pty");
    println!("spawned child pid {}", pty.child().id());

    let mut term = new_term(80, 10);
    term.resize(80, 10, 8, 16)
        .expect("initial resize sets cell px");
    let mut chunk_no = 0;
    drain_pty(&mut pty, |bytes| {
        chunk_no += 1;
        term.vt_write(bytes);
        let (dirty, dirty_rows, _, _) = snapshot(&term);
        println!(
            "[chunk {chunk_no} ({} B)] dirty: {dirty:?} rows={dirty_rows}",
            bytes.len()
        );
    });
    let (_, _, text, _) = snapshot(&term);
    println!("--- grid at 80x10 ---\n{text}");
    println!("title={:?}", term.title().unwrap_or(""));

    term.resize(40, 10, 8, 16).expect("resize");
    pty.on_resize(WindowSize {
        num_lines: 10,
        num_cols: 40,
        cell_width: 8,
        cell_height: 16,
    });
    let (dirty, rows, text, _) = snapshot(&term);
    println!(
        "[after resize 40x10] dirty: {dirty:?} rows={rows}\n--- grid at 40x10 (reflowed) ---\n{text}"
    );

    term.resize(80, 10, 8, 16).expect("resize");
    let (dirty, rows, text, _) = snapshot(&term);
    println!(
        "[after resize 80x10] dirty: {dirty:?} rows={rows}\n--- grid at 80x10 (reflowed back) ---\n{text}"
    );
    println!(
        "scrollback_rows={} total_rows={}",
        term.scrollback_rows().unwrap_or(0),
        term.total_rows().unwrap_or(0)
    );
}

fn spike_bench(path: &str, cols: u16, rows: u16) {
    let data = fs::read(path).expect("read corpus file");
    let mut term = new_term(cols, rows);
    for chunk in data.chunks(64 * 1024) {
        term.vt_write(chunk);
    }
    let (dirty, _, _, checksum) = snapshot(&term);
    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "ghostty bytes={} checksum={checksum:016x} history={} damage={dirty:?}",
        data.len(),
        term.scrollback_rows().unwrap_or(0)
    )
    .ok();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("pty") => spike_pty(),
        Some("bench") => {
            let path = args.get(2).expect("bench <file> [cols] [rows]");
            let cols = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);
            let rows = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(50);
            spike_bench(path, cols, rows);
        }
        _ => {
            eprintln!("usage: ghostty-spike pty | bench <file> [cols] [rows]");
            std::process::exit(2);
        }
    }
}
