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

impl WinSize {
    /// Size in cells with unknown pixel dimensions.
    pub const fn cells(cols: u16, rows: u16) -> Self {
        Self {
            cols,
            rows,
            x_px: 0,
            y_px: 0,
        }
    }

    pub(crate) fn to_raw(self) -> libc::winsize {
        libc::winsize {
            ws_row: self.rows,
            ws_col: self.cols,
            ws_xpixel: self.x_px,
            ws_ypixel: self.y_px,
        }
    }
}
