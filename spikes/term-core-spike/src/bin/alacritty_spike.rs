//! M0 spike for `alacritty_terminal` 0.26.0.
//!
//! Two modes:
//! * `alacritty-spike pty` — spawn a real PTY through `tty::new`, feed its
//!   output through `Processor` + `Term`, print the damage report after each
//!   read, resize twice (reflow) and dump the grid. This is the API-shape
//!   verification docs/10 §1 asked for.
//! * `alacritty-spike bench <file> [cols] [rows]` — parse a pre-generated
//!   corpus file as fast as possible and print a grid checksum. Timed by
//!   hyperfine from `scripts/bench/run.sh`; nothing is timed in-process.

// Throwaway spike: pedantic lints are noise here. Product crates keep them.
#![allow(clippy::pedantic, clippy::type_complexity, unsafe_code)]

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermDamage};
use alacritty_terminal::tty::{self, EventedReadWrite, Options, Shell};
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

/// Collects the events `Term` emits so the spike can print them. `Term` takes
/// the listener by value, so it is shared through an `Arc`.
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<String>>>);

impl EventListener for Recorder {
    fn send_event(&self, event: Event) {
        let s = match event {
            Event::Title(t) => format!("Title({t:?})"),
            Event::ResetTitle => "ResetTitle".into(),
            Event::PtyWrite(w) => format!("PtyWrite({w:?})"),
            Event::Wakeup => "Wakeup".into(),
            Event::Bell => "Bell".into(),
            Event::Exit => "Exit".into(),
            Event::ChildExit(st) => format!("ChildExit({st:?})"),
            Event::MouseCursorDirty => "MouseCursorDirty".into(),
            Event::CursorBlinkingChange => "CursorBlinkingChange".into(),
            Event::ClipboardStore(..) => "ClipboardStore".into(),
            Event::ClipboardLoad(..) => "ClipboardLoad".into(),
            Event::ColorRequest(..) => "ColorRequest".into(),
            Event::TextAreaSizeRequest(_) => "TextAreaSizeRequest".into(),
        };
        self.0.lock().expect("recorder poisoned").push(s);
    }
}

fn config() -> Config {
    Config {
        scrolling_history: 10_000,
        ..Config::default()
    }
}

fn dump_grid<T>(term: &Term<T>) -> String {
    let grid = term.grid();
    let mut out = String::new();
    for line in 0..grid.screen_lines() {
        let row = &grid[Line(line as i32)];
        let text: String = (0..grid.columns()).map(|c| row[Column(c)].c).collect();
        out.push_str(text.trim_end());
        out.push('\n');
    }
    out
}

fn print_damage<T>(term: &mut Term<T>, label: &str) {
    match term.damage() {
        TermDamage::Full => println!("[{label}] damage: Full"),
        TermDamage::Partial(iter) => {
            let lines: Vec<String> = iter
                .map(|d| format!("{}:{}-{}", d.line, d.left, d.right))
                .collect();
            println!(
                "[{label}] damage: Partial {} line(s) {}",
                lines.len(),
                lines.join(" ")
            );
        }
    }
    term.reset_damage();
}

/// Reads the PTY until the child exits and the fd drains. The master fd is
/// non-blocking (alacritty sets it up for `polling`), so we spin gently.
fn drain_pty(pty: &mut tty::Pty, mut on_chunk: impl FnMut(&[u8])) {
    let mut buf = vec![0u8; 64 * 1024];
    let started = Instant::now();
    loop {
        match pty.reader().read(&mut buf) {
            Ok(0) => break,
            Ok(n) => on_chunk(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if child_exited(pty) {
                    // Child gone; one last read pass then stop (drain_on_exit).
                    if let Ok(n) = pty.reader().read(&mut buf)
                        && n > 0
                    {
                        on_chunk(&buf[..n]);
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            // EIO on macOS once the slave side closes.
            Err(_) => break,
        }
        if started.elapsed() > Duration::from_secs(10) {
            eprintln!("timeout draining pty");
            break;
        }
    }
}

/// `Pty::child()` only hands out `&Child`, and `try_wait` needs `&mut`, so ask
/// the kernel directly. This is exactly the gap `vt-pty` exists to close.
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
    let options = Options {
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

    let recorder = Recorder::default();
    let mut term = Term::new(config(), &TermSize::new(80, 10), recorder.clone());
    let mut processor: Processor<StdSyncHandler> = Processor::new();

    let mut chunk_no = 0;
    drain_pty(&mut pty, |bytes| {
        chunk_no += 1;
        processor.advance(&mut term, bytes);
        print_damage(&mut term, &format!("chunk {chunk_no} ({} B)", bytes.len()));
    });
    println!("--- grid at 80x10 ---\n{}", dump_grid(&term));

    // Resize narrower: long lines must reflow (wrap) and damage must be Full.
    term.resize(TermSize::new(40, 10));
    pty.on_resize(WindowSize {
        num_lines: 10,
        num_cols: 40,
        cell_width: 8,
        cell_height: 16,
    });
    print_damage(&mut term, "after resize 40x10");
    println!("--- grid at 40x10 (reflowed) ---\n{}", dump_grid(&term));

    term.resize(TermSize::new(80, 10));
    print_damage(&mut term, "after resize 80x10");
    println!(
        "--- grid at 80x10 (reflowed back) ---\n{}",
        dump_grid(&term)
    );

    println!(
        "history_size={} total_lines={}",
        term.grid().history_size(),
        term.grid().total_lines()
    );
    println!(
        "events: {:?}",
        recorder.0.lock().expect("recorder poisoned")
    );
}

fn spike_bench(path: &str, cols: usize, rows: usize) {
    let data = fs::read(path).expect("read corpus file");
    let mut term = Term::new(config(), &TermSize::new(cols, rows), Recorder::default());
    let mut processor: Processor<StdSyncHandler> = Processor::new();
    for chunk in data.chunks(64 * 1024) {
        processor.advance(&mut term, chunk);
    }
    // Touch the grid so the work cannot be optimised away, and emit a checksum
    // that must match across backends for the same input.
    let mut checksum: u64 = 0;
    let grid = term.grid();
    for line in 0..grid.screen_lines() {
        let row = &grid[Line(line as i32)];
        for c in 0..grid.columns() {
            checksum = checksum
                .wrapping_mul(31)
                .wrapping_add(row[Column(c)].c as u64);
        }
    }
    let history = grid.history_size();
    let damaged = match term.damage() {
        TermDamage::Full => "full".to_string(),
        TermDamage::Partial(it) => it.count().to_string(),
    };
    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "alacritty bytes={} checksum={checksum:016x} history={history} damage={damaged}",
        data.len(),
    )
    .ok();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("pty") => spike_pty(),
        Some("dump") => {
            let path = args.get(2).expect("dump <file> [cols] [rows]");
            let cols = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(80);
            let rows = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(24);
            let data = fs::read(path).expect("read file");
            let mut term = Term::new(config(), &TermSize::new(cols, rows), Recorder::default());
            let mut processor: Processor<StdSyncHandler> = Processor::new();
            processor.advance(&mut term, &data);
            let c = term.grid().cursor.point;
            print!("{}", dump_grid(&term));
            println!(
                "#cursor {},{} history={}",
                c.line.0,
                c.column.0,
                term.grid().history_size()
            );
        }
        Some("bench") => {
            let path = args.get(2).expect("bench <file> [cols] [rows]");
            let cols = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);
            let rows = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(50);
            spike_bench(path, cols, rows);
        }
        _ => {
            eprintln!("usage: alacritty-spike pty | bench <file> [cols] [rows]");
            std::process::exit(2);
        }
    }
}
