//! Snapshot and delta encoding for viewers.

use vt_core::cell::{Cell, CellSnapshot, Color};
use vt_core::damage::DamageSet;
use vt_proto::session::{OutputDelta, SessionId, WireCell, WireRow};

fn color(c: Color) -> [u8; 4] {
    match c {
        Color::Default => [0, 0, 0, 0],
        Color::Indexed(i) => [1, i, 0, 0],
        Color::Rgb(r, g, b) => [2, r, g, b],
    }
}

fn cell(c: &Cell) -> WireCell {
    WireCell {
        c: c.ch,
        fg: color(c.fg),
        bg: color(c.bg),
        attrs: c.attrs.0,
    }
}

fn row(snap: &CellSnapshot, r: u16) -> WireRow {
    let cols = usize::from(snap.size.cols);
    let start = usize::from(r) * cols;
    WireRow {
        row: r,
        cells: snap.cells[start..start + cols].iter().map(cell).collect(),
    }
}

/// Every row (attach, resize, full damage).
pub fn full(id: &SessionId, snap: &CellSnapshot, seq: u64) -> OutputDelta {
    OutputDelta {
        id: id.clone(),
        cols: snap.size.cols,
        rows: snap.size.rows,
        full: true,
        lines: (0..snap.size.rows).map(|r| row(snap, r)).collect(),
        cursor: (snap.cursor.row, snap.cursor.col, snap.cursor.visible),
        seq,
    }
}

/// Only damaged rows.
pub fn delta(id: &SessionId, snap: &CellSnapshot, damage: &DamageSet, seq: u64) -> OutputDelta {
    match damage {
        DamageSet::Full => full(id, snap, seq),
        DamageSet::Lines(lines) => OutputDelta {
            id: id.clone(),
            cols: snap.size.cols,
            rows: snap.size.rows,
            full: false,
            lines: lines
                .iter()
                .filter(|l| l.row < snap.size.rows)
                .map(|l| row(snap, l.row))
                .collect(),
            cursor: (snap.cursor.row, snap.cursor.col, snap.cursor.visible),
            seq,
        },
    }
}

/// Visible grid as plain text, one line per row, trailing blanks trimmed.
pub fn text(snap: &CellSnapshot, last_lines: Option<usize>) -> String {
    let cols = usize::from(snap.size.cols);
    let rows: Vec<String> = snap
        .cells
        .chunks(cols)
        .map(|r| {
            r.iter()
                .filter(|c| c.attrs.0 & vt_core::cell::Attrs::WIDE_SPACER == 0)
                .map(|c| c.ch)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect();
    let keep = last_lines.unwrap_or(rows.len()).min(rows.len());
    rows[rows.len() - keep..].join("\n")
}
