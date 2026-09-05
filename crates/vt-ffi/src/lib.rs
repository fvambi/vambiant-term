//! Hot-path C ABI for the Swift shell (ADR-0002).
//!
//! Rules: plain `extern "C"` (or `"C-unwind"` where Swift may unwind
//! through a Rust frame), `#[unsafe(no_mangle)]` (edition 2024), flat
//! `#[repr(C)]` structs, no allocation across the boundary on the 120 Hz
//! path. Main-thread-only callbacks are documented as such and assert in
//! debug builds. The cold path (config, lifecycle, async) uses
//! `swift-bridge` and lives elsewhere.
//!
//! Fonts, shaping and glyphs never cross this boundary.

#![allow(unsafe_code)]

mod daemon;
mod viewer;

pub use daemon::*;
pub use viewer::*;

/// ABI version. Bumped on every incompatible change to any exported type
/// or function; Swift asserts equality at startup.
pub const VT_FFI_ABI_VERSION: u32 = 1;

/// Returns [`VT_FFI_ABI_VERSION`] so the Swift side can refuse a mismatched
/// static library before touching any other symbol.
#[unsafe(no_mangle)]
pub extern "C" fn vt_ffi_abi_version() -> u32 {
    VT_FFI_ABI_VERSION
}

/// Grid size as it crosses the boundary. Mirrors `vt_core::cell::GridSize`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VtGridSize {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
}

impl From<vt_core::cell::GridSize> for VtGridSize {
    fn from(s: vt_core::cell::GridSize) -> Self {
        Self {
            cols: s.cols,
            rows: s.rows,
        }
    }
}
