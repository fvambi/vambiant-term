//! Window size (`TIOCSWINSZ`) in cells and pixels.

/// Terminal window size as the kernel sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinSize {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
    /// Width in pixels (0 when unknown).
    pub x_px: u16,
    /// Height in pixels (0 when unknown).
    pub y_px: u16,
}
