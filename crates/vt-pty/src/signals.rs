//! `SIGCHLD` and `SIGWINCH` handling.
//!
//! The daemon reaps children explicitly and forwards resizes; nothing here
//! installs a global handler behind the caller's back.
