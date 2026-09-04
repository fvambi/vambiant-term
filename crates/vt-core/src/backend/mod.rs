//! Concrete [`TerminalCore`](crate::TerminalCore) backends.
//!
//! This module tree is the **only** place allowed to name backend types.
//! The `alacritty_terminal` implementation lands in M1 once the M0 spike
//! (`spikes/term-core-spike`) has confirmed the API shapes and the
//! go/no-go against `libghostty-vt`.

pub mod alacritty;
