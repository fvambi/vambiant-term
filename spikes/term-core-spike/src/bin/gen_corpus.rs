//! Generates the benchmark input corpus (docs/08 §7, Ghostty methodology:
//! inputs are pre-generated so generation cost is never measured, and the same
//! files are reused across revisions).
//!
//! Usage: `gen-corpus <out-dir>`. Deterministic; no randomness.

// Throwaway spike: pedantic lints are noise here. Product crates keep them.
#![allow(clippy::pedantic, clippy::type_complexity, unsafe_code)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

const TARGET_BYTES: usize = 32 * 1024 * 1024;

fn repeat_to(target: usize, mut f: impl FnMut(&mut String, usize)) -> Vec<u8> {
    let mut s = String::with_capacity(target + 4096);
    let mut i = 0;
    while s.len() < target {
        f(&mut s, i);
        i += 1;
    }
    s.into_bytes()
}

fn ascii_lines() -> Vec<u8> {
    repeat_to(TARGET_BYTES, |s, i| {
        let _ = writeln!(
            s,
            "{i:08} lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod"
        );
    })
}

fn japanese_lines() -> Vec<u8> {
    // Wide characters exercise width lookup and the wide-spacer path.
    repeat_to(TARGET_BYTES, |s, i| {
        let _ = writeln!(
            s,
            "{i:08} 吾輩は猫である。名前はまだ無い。どこで生れたかとんと見当がつかぬ。"
        );
    })
}

fn sgr_heavy() -> Vec<u8> {
    // Every word changes colour and attributes: CSI parsing dominated.
    repeat_to(TARGET_BYTES, |s, i| {
        for w in 0..10u8 {
            let _ = write!(
                s,
                "\x1b[38;5;{}m\x1b[1m{i:06}\x1b[22m\x1b[48;2;{};{};{}mword{w}\x1b[0m ",
                w.wrapping_mul(25),
                w,
                w.wrapping_mul(7),
                w.wrapping_mul(13)
            );
        }
        s.push('\n');
    })
}

fn cursor_motion() -> Vec<u8> {
    // Full-screen TUI style: absolute positioning, partial-line rewrites.
    repeat_to(TARGET_BYTES, |s, i| {
        let row = (i % 40) + 1;
        let col = (i % 70) + 1;
        let _ = write!(s, "\x1b[{row};{col}H\x1b[Kframe {i:07}\x1b[{row};1H");
        if i % 40 == 39 {
            s.push_str("\x1b[2J\x1b[H");
        }
    })
}

fn long_lines() -> Vec<u8> {
    // 4 KiB lines force wrapping on every row: reflow bookkeeping.
    repeat_to(TARGET_BYTES, |s, i| {
        let _ = write!(s, "{i:08}:");
        for _ in 0..512 {
            s.push_str("abcdefgh");
        }
        s.push('\n');
    })
}

fn scrolling_region() -> Vec<u8> {
    // DECSTBM plus output inside it: the scroll path without full-screen clears.
    let mut v = b"\x1b[5;20r\x1b[5;1H".to_vec();
    v.extend(repeat_to(TARGET_BYTES, |s, i| {
        let _ = writeln!(
            s,
            "region line {i:08} ------------------------------------------------"
        );
    }));
    v
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: gen-corpus <out-dir>");
    let out = Path::new(&out);
    fs::create_dir_all(out).expect("create out dir");
    let cases: [(&str, fn() -> Vec<u8>); 6] = [
        ("ascii-lines.vt", ascii_lines),
        ("japanese-lines.vt", japanese_lines),
        ("sgr-heavy.vt", sgr_heavy),
        ("cursor-motion.vt", cursor_motion),
        ("long-lines.vt", long_lines),
        ("scrolling-region.vt", scrolling_region),
    ];
    for (name, generate) in cases {
        let path = out.join(name);
        let data = generate();
        fs::write(&path, &data).expect("write corpus file");
        println!("{} {} bytes", path.display(), data.len());
    }
}
