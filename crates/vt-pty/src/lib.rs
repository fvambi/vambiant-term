//! PTY lifecycle: spawn under `login`, window size, signals, drain-on-exit.
//!
//! This crate is deliberately tiny and dependency-light (docs/02 §9): it is
//! on the hot path and it is the one place the daemon touches `unsafe`
//! syscalls. The macOS details worth copying from Alacritty — `login -q`
//! when `~/.hushlogin` exists, draining output after the child exits — live
//! in [`spawn`].

#![allow(unsafe_code)] // openpty/ioctl/fork live here and nowhere else.

pub mod error;
pub mod signals;
pub mod spawn;
pub mod winsize;

pub use error::PtyError;
