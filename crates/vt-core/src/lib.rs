//! Terminal state model: parser, grid, scrollback, modes and damage.
//!
//! Everything outside this crate speaks only [`TerminalCore`], [`cell`] and
//! [`damage`] types. The concrete backend (`alacritty_terminal`, ADR-0001) is
//! confined to [`backend`]; **no other module may name an `alacritty_terminal`
//! type**, so a swap to `libghostty-vt` is a crate-internal change.
//!
//! Rust owns cells and attributes; Swift owns fonts, shaping and pixels
//! (ADR-0003). Nothing font-related lives here.

pub mod backend;
pub mod cell;
pub mod core;
pub mod damage;
pub mod error;

pub use core::TerminalCore;
pub use error::CoreError;
