//! `libghostty-vt` backend — the primary [`TerminalCore`] (ADR-0001 amendment).
//!
//! Damage comes from Ghostty's render state: a persistent [`RenderState`] is
//! updated after every write, its `dirty()` verdict maps to
//! [`DamageSet::Full`] or per-row entries, and row dirt is cleared once
//! consumed. Ghostty tracks dirt per row, not per column span, so every
//! partial entry covers the full row width.
//!
//! All libghostty types are `!Send + !Sync`; a [`GhosttyCore`] therefore lives
//! on the reader thread that created it (docs/02 §4).

use std::cell::RefCell;
use std::rc::Rc;

use libghostty_vt::key::{self, Action, Encoder, Key, Mods, OptionAsAlt};
use libghostty_vt::render::{CellIterator, Dirty, RenderState, RowIterator};
use libghostty_vt::screen::{CellWide, RowSemanticPrompt};
use libghostty_vt::style::{StyleColor, Underline};
use libghostty_vt::terminal::{ClipboardLocation, Options, SizeReportSize, Terminal};

use crate::cell::{Attrs, Cell, CellSnapshot, Color, Cursor, GridSize, PromptMark, RowMeta};
use crate::core::TerminalCore;
use crate::damage::{DamageSet, LineDamage};
use crate::error::CoreError;
use crate::event::{ClipboardTarget, TermEvent, pwd_from_osc7};
use crate::key::{KeyAction, KeyEvent, KeyMods};

/// Scrollback budget in bytes (libghostty counts bytes, not lines).
const DEFAULT_SCROLLBACK_BYTES: usize = 32 * 1024 * 1024;

/// A terminal backed by libghostty-vt.
pub struct GhosttyCore {
    term: Terminal<'static, 'static>,
    render: RenderState<'static>,
    rows_it: RowIterator<'static>,
    cells_it: CellIterator<'static>,
    responses: Rc<RefCell<Vec<u8>>>,
    events: Rc<RefCell<Vec<TermEvent>>>,
    encoder: Encoder<'static>,
    size: GridSize,
    /// Shared with the XTWINOPS size reporter; updated on resize.
    reported: Rc<RefCell<(GridSize, (u32, u32))>>,
    /// Set by resize; the next `take_damage` reports `Full` regardless of rows.
    force_full: bool,
}

impl std::fmt::Debug for GhosttyCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GhosttyCore")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl GhosttyCore {
    /// Create a terminal of `size` with the default scrollback budget.
    pub fn new(size: GridSize) -> Result<Self, CoreError> {
        Self::with_scrollback(size, DEFAULT_SCROLLBACK_BYTES)
    }

    /// Create a terminal with an explicit scrollback budget in bytes.
    pub fn with_scrollback(size: GridSize, scrollback_bytes: usize) -> Result<Self, CoreError> {
        if size.cols == 0 || size.rows == 0 {
            return Err(CoreError::InvalidSize {
                cols: size.cols,
                rows: size.rows,
            });
        }
        let mut term = Terminal::new(Options {
            cols: size.cols,
            rows: size.rows,
            max_scrollback: scrollback_bytes,
        })
        .map_err(|e| CoreError::Backend {
            what: "Terminal::new",
            detail: e.to_string(),
        })?;
        let responses = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&responses);
        term.on_pty_write(move |_t, data: &[u8]| {
            sink.borrow_mut().extend_from_slice(data);
        })
        .map_err(|e| CoreError::Backend {
            what: "on_pty_write",
            detail: e.to_string(),
        })?;
        let (events, reported) = register_callbacks(&mut term, size)?;
        let render = RenderState::new().map_err(|e| CoreError::Backend {
            what: "RenderState::new",
            detail: e.to_string(),
        })?;
        let rows_it = RowIterator::new().map_err(|e| CoreError::Backend {
            what: "RowIterator::new",
            detail: e.to_string(),
        })?;
        let encoder = Encoder::new().map_err(|e| CoreError::Backend {
            what: "key::Encoder::new",
            detail: e.to_string(),
        })?;
        let cells_it = CellIterator::new().map_err(|e| CoreError::Backend {
            what: "CellIterator::new",
            detail: e.to_string(),
        })?;
        Ok(Self {
            term,
            render,
            rows_it,
            cells_it,
            responses,
            events,
            encoder,
            size,
            reported,
            force_full: true,
        })
    }

    /// Tell the terminal the renderer's real cell size in pixels, used for
    /// XTWINOPS pixel reports and kitty graphics placement.
    pub fn set_cell_pixel_size(&mut self, width: u32, height: u32) {
        self.reported.borrow_mut().1 = (width, height);
    }

    /// Number of scrollback rows currently retained.
    pub fn scrollback_rows(&self) -> usize {
        self.term.scrollback_rows().unwrap_or(0)
    }
}

impl TerminalCore for GhosttyCore {
    fn advance(&mut self, bytes: &[u8]) {
        self.term.vt_write(bytes);
    }

    fn resize(&mut self, size: GridSize) -> Result<(), CoreError> {
        if size.cols == 0 || size.rows == 0 {
            return Err(CoreError::InvalidSize {
                cols: size.cols,
                rows: size.rows,
            });
        }
        // Cell pixel size only matters for kitty graphics placement; the
        // renderer owns real pixel metrics on the Swift side.
        self.term
            .resize(size.cols, size.rows, 0, 0)
            .map_err(|e| CoreError::Backend {
                what: "resize",
                detail: e.to_string(),
            })?;
        self.size = size;
        self.reported.borrow_mut().0 = size;
        self.force_full = true;
        Ok(())
    }

    fn size(&self) -> GridSize {
        self.size
    }

    fn take_damage(&mut self) -> DamageSet {
        let Ok(snap) = self.render.update(&self.term) else {
            return DamageSet::Full;
        };
        let dirty = snap.dirty().unwrap_or(Dirty::Full);
        let full = self.force_full || matches!(dirty, Dirty::Full);
        self.force_full = false;
        let mut lines = Vec::new();
        if let Ok(mut rows) = self.rows_it.update(&snap) {
            let mut row_idx: u16 = 0;
            while let Some(row) = rows.next() {
                if !full && row.dirty().unwrap_or(true) {
                    lines.push(LineDamage {
                        row: row_idx,
                        left: 0,
                        right: self.size.cols.saturating_sub(1),
                    });
                }
                let _ = row.set_dirty(false);
                row_idx = row_idx.saturating_add(1);
            }
        }
        let _ = snap.set_dirty(Dirty::Clean);
        if full {
            DamageSet::Full
        } else {
            DamageSet::Lines(lines)
        }
    }

    fn snapshot(&mut self) -> CellSnapshot {
        let cols = usize::from(self.size.cols);
        let rows = usize::from(self.size.rows);
        let mut cells = vec![Cell::default(); cols * rows];
        let mut metas = vec![RowMeta::default(); rows];
        let mut cursor = Cursor {
            col: 0,
            row: 0,
            visible: true,
        };
        if let Ok(snap) = self.render.update(&self.term) {
            if let Ok(Some(cv)) = snap.cursor_viewport() {
                cursor.col = cv.x;
                cursor.row = cv.y;
            }
            cursor.visible = snap.cursor_visible().unwrap_or(true);
            if let Ok(mut it) = self.rows_it.update(&snap) {
                let mut r = 0usize;
                while let Some(row) = it.next() {
                    if r >= rows {
                        break;
                    }
                    if let Ok(raw) = row.raw_row() {
                        metas[r] = RowMeta {
                            prompt: match raw.semantic_prompt() {
                                Ok(RowSemanticPrompt::Prompt) => PromptMark::Prompt,
                                Ok(RowSemanticPrompt::Continuation) => PromptMark::Continuation,
                                _ => PromptMark::None,
                            },
                            wrapped: raw.is_wrapped().unwrap_or(false),
                            wrap_continuation: raw.is_wrap_continuation().unwrap_or(false),
                        };
                    }
                    if let Ok(mut cit) = self.cells_it.update(row) {
                        let mut c = 0usize;
                        while let Some(cell) = cit.next() {
                            if c >= cols {
                                break;
                            }
                            cells[r * cols + c] = convert_cell(cell);
                            c += 1;
                        }
                    }
                    r += 1;
                }
            }
        }
        CellSnapshot {
            size: self.size,
            cursor,
            cells,
            rows: metas,
        }
    }

    fn title(&self) -> Option<String> {
        self.term
            .title()
            .ok()
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    }

    fn take_responses(&mut self) -> Vec<u8> {
        std::mem::take(&mut *self.responses.borrow_mut())
    }

    fn take_events(&mut self) -> Vec<TermEvent> {
        std::mem::take(&mut *self.events.borrow_mut())
    }

    fn encode_key(&mut self, event: &KeyEvent) -> Vec<u8> {
        let mut out = Vec::new();
        let Ok(mut ev) = key::Event::new() else {
            return out;
        };
        ev.set_action(match event.action {
            KeyAction::Press => Action::Press,
            KeyAction::Release => Action::Release,
            KeyAction::Repeat => Action::Repeat,
        });
        // KeyCode values are libghostty's Key discriminants by construction.
        ev.set_key(Key::try_from(u32::from(event.key as u16)).unwrap_or(Key::Unidentified));
        ev.set_mods(convert_mods(event.mods));
        ev.set_utf8(event.utf8.as_deref());
        if let Some(c) = event.unshifted {
            ev.set_unshifted_codepoint(c);
        }
        // Modes (kitty flags, DECCKM, modifyOtherKeys) are read from the
        // terminal on every call so pushes/pops made by the program are seen.
        self.encoder
            .set_options_from_terminal(&self.term)
            .set_macos_option_as_alt(OptionAsAlt::False);
        let _ = self.encoder.encode_to_vec(&ev, &mut out);
        out
    }
}

fn convert_mods(m: KeyMods) -> Mods {
    let mut out = Mods::empty();
    if m.0 & KeyMods::SHIFT != 0 {
        out |= Mods::SHIFT;
    }
    if m.0 & KeyMods::CTRL != 0 {
        out |= Mods::CTRL;
    }
    if m.0 & KeyMods::ALT != 0 {
        out |= Mods::ALT;
    }
    if m.0 & KeyMods::SUPER != 0 {
        out |= Mods::SUPER;
    }
    if m.0 & KeyMods::CAPS_LOCK != 0 {
        out |= Mods::CAPS_LOCK;
    }
    if m.0 & KeyMods::NUM_LOCK != 0 {
        out |= Mods::NUM_LOCK;
    }
    out
}

/// Wire libghostty's host callbacks to the event queue and size reporter.
#[allow(clippy::type_complexity)]
fn register_callbacks(
    term: &mut Terminal<'static, 'static>,
    size: GridSize,
) -> Result<
    (
        Rc<RefCell<Vec<TermEvent>>>,
        Rc<RefCell<(GridSize, (u32, u32))>>,
    ),
    CoreError,
> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let backend = |what: &'static str| {
        move |e: libghostty_vt::Error| CoreError::Backend {
            what,
            detail: e.to_string(),
        }
    };
    let ev = Rc::clone(&events);
    term.on_bell(move |_t| ev.borrow_mut().push(TermEvent::Bell))
        .map_err(backend("on_bell"))?;
    let ev = Rc::clone(&events);
    term.on_title_changed(move |t| {
        if let Ok(title) = t.title() {
            ev.borrow_mut().push(TermEvent::Title(title.to_owned()));
        }
    })
    .map_err(backend("on_title_changed"))?;
    let ev = Rc::clone(&events);
    term.on_pwd_changed(move |t| {
        if let Ok(pwd) = t.pwd() {
            ev.borrow_mut().push(TermEvent::Pwd(pwd_from_osc7(pwd)));
        }
    })
    .map_err(backend("on_pwd_changed"))?;
    let ev = Rc::clone(&events);
    term.on_clipboard_write(move |_t, write| {
        let target = match write.location() {
            ClipboardLocation::Standard => ClipboardTarget::Clipboard,
            _ => ClipboardTarget::Selection,
        };
        // OSC 52 payloads arrive already base64-decoded, one entry per
        // mime type; text/plain is the only one a terminal clipboard takes.
        let contents = write
            .contents()
            .find(|c| c.mime.starts_with("text/plain") || c.mime.is_empty())
            .map(|c| c.data.as_bytes().to_vec())
            .unwrap_or_default();
        ev.borrow_mut()
            .push(TermEvent::ClipboardWrite { target, contents });
        Ok(())
    })
    .map_err(backend("on_clipboard_write"))?;
    // XTWINOPS size reports (CSI 14/16/18 t). Pixel metrics are nominal
    // until the renderer sets real ones; programs mostly want cells.
    let reported = Rc::new(RefCell::new((size, (8u32, 16u32))));
    let rep = Rc::clone(&reported);
    term.on_size(move |_t| {
        let (g, (w, h)) = *rep.borrow();
        Some(SizeReportSize {
            rows: g.rows,
            columns: g.cols,
            cell_width: w,
            cell_height: h,
        })
    })
    .map_err(backend("on_size"))?;
    Ok((events, reported))
}

fn convert_cell(cell: &libghostty_vt::render::CellIteration<'_, '_>) -> Cell {
    let raw = cell.raw_cell().ok();
    let cp = raw.and_then(|c| c.codepoint().ok()).unwrap_or(0);
    let ch = char::from_u32(cp).filter(|c| *c != '\0').unwrap_or(' ');
    let mut attrs = Attrs::default();
    match raw.and_then(|c| c.wide().ok()) {
        Some(CellWide::Wide) => attrs.0 |= Attrs::WIDE,
        Some(CellWide::SpacerTail | CellWide::SpacerHead) => attrs.0 |= Attrs::WIDE_SPACER,
        _ => {}
    }
    let (fg, bg) = match cell.style() {
        Ok(style) => {
            if style.bold {
                attrs.0 |= Attrs::BOLD;
            }
            if style.italic {
                attrs.0 |= Attrs::ITALIC;
            }
            if style.faint {
                attrs.0 |= Attrs::DIM;
            }
            if style.inverse {
                attrs.0 |= Attrs::INVERSE;
            }
            if style.invisible {
                attrs.0 |= Attrs::HIDDEN;
            }
            if style.strikethrough {
                attrs.0 |= Attrs::STRIKEOUT;
            }
            if !matches!(style.underline, Underline::None) {
                attrs.0 |= Attrs::UNDERLINE;
            }
            (convert_color(style.fg_color), convert_color(style.bg_color))
        }
        Err(_) => (Color::Default, Color::Default),
    };
    Cell { ch, fg, bg, attrs }
}

fn convert_color(c: StyleColor) -> Color {
    match c {
        StyleColor::None => Color::Default,
        StyleColor::Palette(idx) => Color::Indexed(idx.0),
        StyleColor::Rgb(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(core: &mut GhosttyCore) -> Vec<String> {
        let snap = core.snapshot();
        let cols = usize::from(snap.size.cols);
        snap.cells
            .chunks(cols)
            .map(|row| {
                row.iter()
                    .map(|c| c.ch)
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn writes_land_in_the_grid() {
        let mut core = GhosttyCore::new(GridSize { cols: 20, rows: 3 }).unwrap();
        core.advance(b"hello\r\nworld");
        let rows = text(&mut core);
        assert_eq!(rows, vec!["hello", "world", ""]);
        let snap = core.snapshot();
        assert_eq!((snap.cursor.row, snap.cursor.col), (1, 5));
    }

    #[test]
    fn damage_is_full_first_then_per_row() {
        let mut core = GhosttyCore::new(GridSize { cols: 20, rows: 3 }).unwrap();
        core.advance(b"a");
        assert_eq!(core.take_damage(), DamageSet::Full);
        assert!(core.take_damage().is_clean());
        core.advance(b"\x1b[3;1Hz");
        match core.take_damage() {
            DamageSet::Lines(lines) => {
                assert!(lines.iter().any(|l| l.row == 2), "row 2 damaged: {lines:?}");
            }
            DamageSet::Full => panic!("expected partial damage"),
        }
        assert!(core.take_damage().is_clean());
    }

    #[test]
    fn resize_reflows_and_reports_full() {
        let mut core = GhosttyCore::new(GridSize { cols: 10, rows: 2 }).unwrap();
        core.advance(b"abcdefghij0123");
        let _ = core.take_damage();
        core.resize(GridSize { cols: 20, rows: 2 }).unwrap();
        assert_eq!(core.take_damage(), DamageSet::Full);
        assert_eq!(text(&mut core)[0], "abcdefghij0123");
        assert!(matches!(
            core.resize(GridSize { cols: 0, rows: 2 }),
            Err(CoreError::InvalidSize { cols: 0, rows: 2 })
        ));
    }

    #[test]
    fn attributes_colours_and_wide_chars() {
        let mut core = GhosttyCore::new(GridSize { cols: 10, rows: 1 }).unwrap();
        core.advance(b"\x1b[1;4;38;5;9;48;2;1;2;3mX\x1b[0m\xe6\x97\xa5");
        let snap = core.snapshot();
        let x = snap.cells[0];
        assert_eq!(x.ch, 'X');
        assert_ne!(x.attrs.0 & Attrs::BOLD, 0);
        assert_ne!(x.attrs.0 & Attrs::UNDERLINE, 0);
        assert_eq!(x.fg, Color::Indexed(9));
        assert_eq!(x.bg, Color::Rgb(1, 2, 3));
        assert_eq!(snap.cells[1].ch, '日');
        assert_ne!(snap.cells[1].attrs.0 & Attrs::WIDE, 0);
        assert_ne!(snap.cells[2].attrs.0 & Attrs::WIDE_SPACER, 0);
    }

    #[test]
    fn events_bell_title_pwd_clipboard() {
        let mut core = GhosttyCore::new(GridSize { cols: 10, rows: 2 }).unwrap();
        core.advance(b"\x07\x1b]2;t1\x07\x1b]7;file://localhost/tmp/x\x07\x1b]52;c;aGVsbG8=\x07");
        let events = core.take_events();
        assert_eq!(
            events,
            vec![
                TermEvent::Bell,
                TermEvent::Title("t1".into()),
                TermEvent::Pwd("/tmp/x".into()),
                TermEvent::ClipboardWrite {
                    target: ClipboardTarget::Clipboard,
                    contents: b"hello".to_vec()
                },
            ]
        );
        assert!(core.take_events().is_empty());
    }

    #[test]
    fn key_encoding_follows_terminal_modes() {
        use crate::key::{KeyCode, KeyEvent, KeyMods};
        let mut core = GhosttyCore::new(GridSize { cols: 10, rows: 2 }).unwrap();
        assert_eq!(
            core.encode_key(&KeyEvent::press(KeyCode::A, 0, Some("a"))),
            b"a"
        );
        assert_eq!(
            core.encode_key(&KeyEvent::press(KeyCode::C, KeyMods::CTRL, Some("c"))),
            b"\x03"
        );
        assert_eq!(
            core.encode_key(&KeyEvent::press(KeyCode::Escape, 0, None)),
            b"\x1b"
        );
        assert_eq!(
            core.encode_key(&KeyEvent::press(KeyCode::ArrowUp, 0, None)),
            b"\x1b[A"
        );
        // Application cursor keys (DECCKM) change the arrow encoding.
        core.advance(b"\x1b[?1h");
        assert_eq!(
            core.encode_key(&KeyEvent::press(KeyCode::ArrowUp, 0, None)),
            b"\x1bOA"
        );
        // Kitty keyboard protocol: push "disambiguate escape codes".
        core.advance(b"\x1b[>1u");
        assert_eq!(
            core.encode_key(&KeyEvent::press(KeyCode::Escape, 0, None)),
            b"\x1b[27u"
        );
        // Pop restores legacy encoding.
        core.advance(b"\x1b[<u");
        assert_eq!(
            core.encode_key(&KeyEvent::press(KeyCode::Escape, 0, None)),
            b"\x1b"
        );
        // A lone modifier produces nothing.
        assert!(
            core.encode_key(&KeyEvent::press(
                KeyCode::Unidentified,
                KeyMods::SHIFT,
                None
            ))
            .is_empty()
        );
    }

    #[test]
    fn xtwinops_size_reports() {
        let mut core = GhosttyCore::new(GridSize {
            cols: 100,
            rows: 30,
        })
        .unwrap();
        core.set_cell_pixel_size(9, 18);
        core.advance(b"\x1b[18t\x1b[14t\x1b[16t");
        assert_eq!(
            core.take_responses(),
            b"\x1b[8;30;100t\x1b[4;540;900t\x1b[6;18;9t".to_vec()
        );
        core.resize(GridSize { cols: 80, rows: 24 }).unwrap();
        core.advance(b"\x1b[18t");
        assert_eq!(core.take_responses(), b"\x1b[8;24;80t".to_vec());
    }

    #[test]
    fn row_meta_prompt_marks_and_wrap() {
        let mut core = GhosttyCore::new(GridSize { cols: 8, rows: 4 }).unwrap();
        core.advance(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07abcdefghijkl\r\n");
        let snap = core.snapshot();
        assert_eq!(snap.rows.len(), 4);
        assert_eq!(snap.rows[0].prompt, PromptMark::Prompt);
        assert_eq!(snap.rows[1].prompt, PromptMark::None);
        assert!(snap.rows[1].wrapped, "row 1 soft-wraps: {:?}", snap.rows);
        assert!(snap.rows[2].wrap_continuation);
    }

    #[test]
    fn title_and_query_responses() {
        let mut core = GhosttyCore::new(GridSize { cols: 10, rows: 2 }).unwrap();
        core.advance(b"\x1b]0;hello\x07\x1b[6n");
        assert_eq!(core.title().as_deref(), Some("hello"));
        assert_eq!(core.take_responses(), b"\x1b[1;1R".to_vec());
        assert!(core.take_responses().is_empty());
    }
}
