//! Shell-integration marks carry the absolute row they landed on, so a
//! block's line range survives scrolling into scrollback.

use vt_core::backend::GhosttyCore;
use vt_core::cell::GridSize;
use vt_core::{ShellMark, TermEvent, TerminalCore};

fn marks(core: &mut GhosttyCore) -> Vec<(ShellMark, u64)> {
    core.take_events()
        .into_iter()
        .filter_map(|e| match e {
            TermEvent::ShellMark { mark, row } => Some((mark, row)),
            _ => None,
        })
        .collect()
}

#[test]
fn marks_report_absolute_rows_across_scrollback() {
    let mut core = GhosttyCore::new(GridSize { cols: 20, rows: 3 }).unwrap();
    core.advance(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07");
    let m = marks(&mut core);
    assert_eq!(m.len(), 3);
    assert_eq!(m[0], (ShellMark::PromptStart { params: vec![] }, 0));
    assert_eq!(m[1], (ShellMark::CommandStart, 0));
    assert_eq!(
        m[2],
        (ShellMark::CommandExecuted, 1),
        "C lands after the newline"
    );

    // Ten lines of output push everything into scrollback.
    for i in 0..10 {
        core.advance(format!("line {i}\r\n").as_bytes());
    }
    core.advance(b"\x1b]133;D;0\x07\x1b]133;A\x07$ ");
    let m = marks(&mut core);
    assert_eq!(m.len(), 2);
    assert_eq!(m[0].0, ShellMark::CommandFinished { exit: Some(0) });
    assert_eq!(
        m[0].1, 11,
        "row 1 + 10 lines, counted from the top of scrollback"
    );
    assert_eq!(m[1].1, 11);
    assert!(
        core.scrollback_rows() >= 8,
        "the grid is 3 rows; the rest scrolled"
    );
}

#[test]
fn marks_split_across_chunks_still_position_correctly() {
    let mut core = GhosttyCore::new(GridSize { cols: 20, rows: 4 }).unwrap();
    let stream = b"a\r\nb\r\n\x1b]133;D;3\x07";
    for chunk in stream.chunks(2) {
        core.advance(chunk);
    }
    let m = marks(&mut core);
    assert_eq!(m, vec![(ShellMark::CommandFinished { exit: Some(3) }, 2)]);
}
