//! Backend-independent cell, attribute and geometry types.
//!
//! These are the types that eventually cross the C ABI in `vt-ffi`, so they
//! stay `#[repr(C)]`-compatible plain data: no `String`, no `Vec` per cell.

/// Grid dimensions in cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    /// Number of columns.
    pub cols: u16,
    /// Number of visible rows (excludes scrollback).
    pub rows: u16,
}

/// Cursor position and shape, zero-based from the top-left visible cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    /// Column.
    pub col: u16,
    /// Row within the visible area.
    pub row: u16,
    /// Whether the cursor is currently shown (DECTCEM).
    pub visible: bool,
}

/// Shell-integration mark on a row (OSC 133 / OSC 633), as the backend tracks it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PromptMark {
    /// No prompt cells on this row.
    #[default]
    None,
    /// A primary prompt line (`OSC 133;A` … `B`).
    Prompt,
    /// A prompt continuation line.
    Continuation,
}

/// Per-row facts that are not per-cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowMeta {
    /// Shell-integration mark.
    pub prompt: PromptMark,
    /// The row soft-wraps into the next one (logical line continues).
    pub wrapped: bool,
    /// The row is the continuation of a soft-wrapped logical line.
    pub wrap_continuation: bool,
}

/// A full-grid snapshot handed to a newly attached viewer.
///
/// Row-major; `cells.len() == cols * rows`. The wire/FFI representation is
/// defined in `vt-proto` and `vt-ffi`, not here.
#[derive(Clone, Debug)]
pub struct CellSnapshot {
    /// Grid geometry the snapshot was taken at.
    pub size: GridSize,
    /// Cursor state at snapshot time.
    pub cursor: Cursor,
    /// Cell contents, row-major.
    pub cells: Vec<Cell>,
    /// One entry per visible row.
    pub rows: Vec<RowMeta>,
}

/// One grid cell. Wide characters occupy a leading cell plus a spacer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// Base code point. Combining marks are tracked by the backend and
    /// resolved at snapshot time; this stays a single scalar.
    pub ch: char,
    /// Foreground colour.
    pub fg: Color,
    /// Background colour.
    pub bg: Color,
    /// Style attribute bits.
    pub attrs: Attrs,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: Color::Default,
            bg: Color::Default,
            attrs: Attrs::default(),
        }
    }
}

/// Colour as the terminal specified it — the renderer resolves the palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    /// Default foreground/background for the position.
    Default,
    /// Indexed palette entry (0–255).
    Indexed(u8),
    /// Direct 24-bit colour.
    Rgb(u8, u8, u8),
}

/// Style bits, kept as a plain integer so it crosses the C ABI unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attrs(pub u16);

impl Attrs {
    /// Bold.
    pub const BOLD: u16 = 1 << 0;
    /// Italic.
    pub const ITALIC: u16 = 1 << 1;
    /// Single underline. Styled underlines (curly, dotted…) extend from here.
    pub const UNDERLINE: u16 = 1 << 2;
    /// Strikethrough.
    pub const STRIKEOUT: u16 = 1 << 3;
    /// Inverse video.
    pub const INVERSE: u16 = 1 << 4;
    /// Dim / faint.
    pub const DIM: u16 = 1 << 5;
    /// Concealed (SGR 8).
    pub const HIDDEN: u16 = 1 << 6;
    /// Leading half of a wide character.
    pub const WIDE: u16 = 1 << 7;
    /// Trailing spacer of a wide character.
    pub const WIDE_SPACER: u16 = 1 << 8;
}
