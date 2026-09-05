//! Headless test substrate (docs/07 M1): feed a byte stream, dump the grid as
//! text. Every terminal bug fixed adds a fixture under `tests/fixtures/vt/`
//! (CLAUDE.md § Testing); `crates/vt-core/tests/snapshots.rs` replays them.

use crate::cell::CellSnapshot;
use crate::core::TerminalCore;

/// Render a snapshot as one line per row, trailing blanks trimmed, followed
/// by a `#cursor row,col` line — the same shape the M0 spikes dump, so
/// fixtures captured with them stay valid.
pub fn dump(snapshot: &CellSnapshot) -> String {
    let cols = usize::from(snapshot.size.cols);
    let mut out = String::new();
    for row in snapshot.cells.chunks(cols) {
        let line: String = row.iter().map(|c| c.ch).collect();
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.push_str(&format!(
        "#cursor {},{}\n",
        snapshot.cursor.row, snapshot.cursor.col
    ));
    out
}

/// Feed `bytes` through `core` and dump the result.
pub fn replay(core: &mut dyn TerminalCore, bytes: &[u8]) -> String {
    core.advance(bytes);
    dump(&core.snapshot())
}
