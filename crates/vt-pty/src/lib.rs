//! PTY lifecycle: spawn under `login`, window size, child exit, drain-on-exit.
//!
//! This crate is deliberately tiny and dependency-light (docs/02 §9): it is on
//! the hot path and it is the one place the daemon touches `unsafe` syscalls.
//! The macOS details worth copying from Alacritty — `login -q` when
//! `~/.hushlogin` exists, draining output after the child exits — live in
//! [`spawn`].
//!
//! Threading model: the daemon clones the master fd once per direction
//! ([`Pty::reader`] / [`Pty::writer`]) so a stalled write can never block reads
//! (docs/02 §4). Nothing here installs a global signal handler; exit is
//! observed with `waitpid(WNOHANG)` from whichever thread owns the [`Pty`].

#![allow(unsafe_code)] // openpty/fork/exec/ioctl live here and nowhere else.

pub mod error;
pub mod fdpass;
pub mod signals;
pub mod spawn;
pub mod winsize;

pub use error::PtyError;
pub use spawn::{Pty, SpawnSpec};
pub use winsize::WinSize;
