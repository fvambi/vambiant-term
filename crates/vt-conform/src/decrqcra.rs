//! DECRQCRA (`CSI Pid ; Pp ; Pt ; Pl ; Pb ; Pr * y`) request detection and
//! reply (`DCS Pid ! ~ XXXX ST`) computed from a grid snapshot.
//!
//! xterm ≥ #334 semantics: every cell contributes its character code, empty
//! cells count as a space, the sum is taken modulo 2^16. Attributes are
//! ignored, which matches what esctest asserts on.

use vt_core::CellSnapshot;

/// A parsed DECRQCRA request with 1-based inclusive bounds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// Request id echoed in the reply.
    pub pid: u32,
    /// Top row.
    pub top: u16,
    /// Left column.
    pub left: u16,
    /// Bottom row, `None` = last row.
    pub bottom: Option<u16>,
    /// Right column, `None` = last column.
    pub right: Option<u16>,
}

/// Byte-stream scanner that spots `CSI … * y` sequences across chunk boundaries.
#[derive(Default)]
pub struct Scanner {
    state: State,
    params: Vec<u8>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    Esc,
    Csi,
    Star,
}

impl Scanner {
    /// Feed bytes; returns every complete DECRQCRA request found.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Request> {
        let mut out = Vec::new();
        for &b in bytes {
            self.state = match (self.state, b) {
                (State::Esc, b'[') => {
                    self.params.clear();
                    State::Csi
                }
                (State::Csi, b'0'..=b'9' | b';') => {
                    self.params.push(b);
                    State::Csi
                }
                (State::Csi, b'*') => State::Star,
                (State::Star, b'y') => {
                    if let Some(req) = parse(&self.params) {
                        out.push(req);
                    }
                    State::Ground
                }
                (State::Csi | State::Star, 0x20..=0x3f) => State::Csi,
                (_, 0x1b) => State::Esc,
                _ => State::Ground,
            };
        }
        out
    }
}

fn parse(params: &[u8]) -> Option<Request> {
    let text = std::str::from_utf8(params).ok()?;
    let mut it = text.split(';').map(|p| p.parse::<u32>().ok());
    let pid = it.next().flatten().unwrap_or(0);
    let _page = it.next();
    let top = it.next().flatten().unwrap_or(1).max(1);
    let left = it.next().flatten().unwrap_or(1).max(1);
    let bottom = it.next().flatten();
    let right = it.next().flatten();
    Some(Request {
        pid,
        top: u16::try_from(top).ok()?,
        left: u16::try_from(left).ok()?,
        bottom: bottom.and_then(|v| u16::try_from(v).ok()),
        right: right.and_then(|v| u16::try_from(v).ok()),
    })
}

/// Compute the reply bytes for `req` over `snap`.
pub fn reply(req: &Request, snap: &CellSnapshot) -> Vec<u8> {
    let cols = snap.size.cols;
    let rows = snap.size.rows;
    let bottom = req.bottom.unwrap_or(rows).min(rows);
    let right = req.right.unwrap_or(cols).min(cols);
    let mut sum: u32 = 0;
    for r in req.top..=bottom {
        for c in req.left..=right {
            let idx = usize::from(r - 1) * usize::from(cols) + usize::from(c - 1);
            let ch = snap.cells.get(idx).map_or(' ', |cell| cell.ch);
            let code = if ch == '\0' { 32 } else { u32::from(ch) };
            sum = (sum + code) & 0xffff;
        }
    }
    format!("\x1bP{}!~{:04X}\x1b\\", req.pid, sum).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vt_core::cell::{Cell, Cursor, GridSize};

    #[test]
    fn scans_across_chunks_and_replies() {
        let mut s = Scanner::default();
        assert!(s.feed(b"abc\x1b[7;0;2;").is_empty());
        let reqs = s.feed(b"3;2;4*yXYZ");
        assert_eq!(
            reqs,
            vec![Request {
                pid: 7,
                top: 2,
                left: 3,
                bottom: Some(2),
                right: Some(4)
            }]
        );
        let mut cells = vec![Cell::default(); 80 * 24];
        cells[80 + 2].ch = 'A';
        cells[80 + 3].ch = 'B';
        let snap = CellSnapshot {
            size: GridSize { cols: 80, rows: 24 },
            cursor: Cursor {
                col: 0,
                row: 0,
                visible: true,
            },
            cells,
            rows: Vec::new(),
        };
        assert_eq!(reply(&reqs[0], &snap), b"\x1bP7!~0083\x1b\\");
    }

    #[test]
    fn ignores_other_csi() {
        let mut s = Scanner::default();
        assert!(
            s.feed(b"\x1b[2J\x1b[1;1H\x1b[?25l\x1b[38;5;1m*y")
                .is_empty()
        );
    }
}
