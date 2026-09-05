//! Terminal state model: parser, grid, scrollback, modes and damage.
//!
//! Everything outside this crate speaks only [`TerminalCore`], [`cell`] and
//! [`damage`] types. Concrete backends are confined to [`backend`]; **no other
//! module may name a backend type**. The primary backend is `libghostty-vt`
//! (ADR-0001 amendment, gates met 2026-09-05); `alacritty_terminal` remains
//! the documented fallback behind the same trait.
//!
//! Rust owns cells and attributes; Swift owns fonts, shaping and pixels
//! (ADR-0003). Nothing font-related lives here.

pub mod backend;
pub mod cell;
pub mod core;
pub mod damage;
pub mod error;
pub mod event;
pub mod harness;

pub use core::TerminalCore;
pub use error::CoreError;
pub use event::TermEvent;
