//! Child exit observation and signalling.
//!
//! The daemon reaps children explicitly with `waitpid(WNOHANG)` and never
//! installs a global `SIGCHLD` handler: a library must not own process-wide
//! signal state. `SIGWINCH` is delivered to the child by the kernel as a side
//! effect of `TIOCSWINSZ` on the master, so nothing is needed here for it.

use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

/// Non-blocking reap. `Ok(None)` while the child is still running.
pub(crate) fn try_wait(pid: libc::pid_t) -> io::Result<Option<ExitStatus>> {
    let mut status: libc::c_int = 0;
    // SAFETY: waitpid with WNOHANG on a pid we spawned; `status` is a valid out-pointer.
    let r = unsafe { libc::waitpid(pid, &raw mut status, libc::WNOHANG) };
    match r {
        0 => Ok(None),
        p if p == pid => Ok(Some(ExitStatus::from_raw(status))),
        _ => Err(io::Error::last_os_error()),
    }
}

/// Send a signal to the child.
pub(crate) fn kill(pid: libc::pid_t, signal: libc::c_int) -> io::Result<()> {
    // SAFETY: plain syscall on a pid we own.
    if unsafe { libc::kill(pid, signal) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
