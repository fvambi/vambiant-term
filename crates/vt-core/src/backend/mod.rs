//! Concrete [`TerminalCore`](crate::TerminalCore) backends.
//!
//! This module tree is the **only** place allowed to name backend types.

pub mod alacritty;
#[cfg(feature = "ghostty")]
pub mod ghostty;

#[cfg(feature = "ghostty")]
pub use ghostty::GhosttyCore;
