//! Spawning the child under a fresh PTY.
//!
//! On macOS a session with no explicit program is started through
//! `/usr/bin/login -flp <user>` so the shell appears as a real tty session
//! (motd, `utmp`, login-shell dotfiles); `-q` is added when `~/.hushlogin`
//! exists, mirroring what Terminal.app, Ghostty and Alacritty do. An explicit
//! program is exec'd directly.
//!
//! Between `fork` and `exec` only async-signal-safe calls are made; every
//! string is converted to a `CString` beforehand.

use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::ptr;

use crate::error::PtyError;
use crate::signals;
use crate::winsize::WinSize;

/// What to run and where.
#[derive(Clone, Debug)]
pub struct SpawnSpec {
    /// Program and arguments; empty means the user's login shell via `login`.
    pub argv: Vec<String>,
    /// Working directory; `None` inherits the daemon's.
    pub cwd: Option<PathBuf>,
    /// Extra environment entries layered over the inherited one.
    pub env: Vec<(String, String)>,
    /// Human-readable session name, used in error messages.
    pub session: String,
}

impl SpawnSpec {
    /// The user's login shell in `cwd`.
    pub fn login_shell(session: impl Into<String>, cwd: Option<PathBuf>) -> Self {
        Self {
            argv: Vec::new(),
            cwd,
            env: Vec::new(),
            session: session.into(),
        }
    }

    /// An explicit program.
    pub fn program(session: impl Into<String>, argv: Vec<String>) -> Self {
        Self {
            argv,
            cwd: None,
            env: Vec::new(),
            session: session.into(),
        }
    }
}

/// A spawned child attached to the master side of a PTY.
#[derive(Debug)]
pub struct Pty {
    master: File,
    pid: libc::pid_t,
    session: String,
    exit: Option<ExitStatus>,
}

impl Pty {
    /// Spawn `spec` under a new PTY with the given initial window size.
    pub fn spawn(spec: &SpawnSpec, size: WinSize) -> Result<Self, PtyError> {
        let session = spec.session.clone();
        let spawn_err = |stage: &'static str, source: io::Error| PtyError::Spawn {
            session: session.clone(),
            stage,
            source,
        };
        let nul = |what: &str| PtyError::NulByte {
            session: session.clone(),
            what: what.chars().take(64).collect(),
        };

        let (program, args) = resolve_command(spec);
        let program = resolve_in_path(&program).ok_or_else(|| PtyError::Spawn {
            session: session.clone(),
            stage: "resolve program",
            source: io::Error::new(
                io::ErrorKind::NotFound,
                format!("{program}: not found in PATH"),
            ),
        })?;
        let c_program = CString::new(program.as_str()).map_err(|_| nul(&program))?;
        let mut c_args = Vec::with_capacity(args.len());
        for a in &args {
            c_args.push(CString::new(a.as_str()).map_err(|_| nul(a))?);
        }
        let argv_ptrs: Vec<*const libc::c_char> = c_args
            .iter()
            .map(|a| a.as_ptr())
            .chain(std::iter::once(ptr::null()))
            .collect();

        let envp_owned = build_env(spec, &nul)?;
        let envp: Vec<*const libc::c_char> = envp_owned
            .iter()
            .map(|e| e.as_ptr())
            .chain(std::iter::once(ptr::null()))
            .collect();
        let c_cwd = match &spec.cwd {
            Some(p) => Some(
                CString::new(p.as_os_str().as_encoded_bytes())
                    .map_err(|_| nul(&p.display().to_string()))?,
            ),
            None => None,
        };

        let mut master: libc::c_int = -1;
        let mut slave: libc::c_int = -1;
        let ws = size.to_raw();
        // SAFETY: out-pointers are valid; name/termios may be NULL per openpty(3).
        let r = unsafe {
            libc::openpty(
                &raw mut master,
                &raw mut slave,
                ptr::null_mut(),
                ptr::null_mut(),
                (&raw const ws).cast_mut(),
            )
        };
        if r != 0 {
            return Err(spawn_err("openpty", io::Error::last_os_error()));
        }

        // SAFETY: fork(2). The child touches only async-signal-safe calls and
        // pre-built C strings before exec; the parent continues normally.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            let e = io::Error::last_os_error();
            // SAFETY: both fds are ours and unused elsewhere.
            unsafe {
                libc::close(master);
                libc::close(slave);
            }
            return Err(spawn_err("fork", e));
        }
        if pid == 0 {
            // SAFETY: child process; see module docs for the async-signal-safety argument.
            unsafe {
                child_exec(
                    slave,
                    master,
                    c_cwd.as_deref(),
                    &c_program,
                    &argv_ptrs,
                    &envp,
                )
            }
        }

        // SAFETY: parent; the slave fd belongs to the child now.
        unsafe { libc::close(slave) };
        // SAFETY: master is an open fd that we own exclusively from here on.
        let master = unsafe { File::from_raw_fd(master) };
        set_cloexec(&master).map_err(|e| spawn_err("fcntl(FD_CLOEXEC)", e))?;
        Ok(Self {
            master,
            pid,
            session,
            exit: None,
        })
    }

    /// Child pid.
    pub fn pid(&self) -> u32 {
        self.pid.unsigned_abs()
    }

    /// Session name this PTY was spawned for.
    pub fn session(&self) -> &str {
        &self.session
    }

    /// A reader on the master side. Blocking reads return `Ok(0)` or `EIO`
    /// once the child has exited and the buffer is drained — that is the
    /// drain-on-exit contract: keep reading until then.
    pub fn reader(&self) -> io::Result<File> {
        self.master.try_clone()
    }

    /// A writer on the master side (clone, so writes never contend with reads).
    pub fn writer(&self) -> io::Result<File> {
        self.master.try_clone()
    }

    /// Write bytes to the child's input.
    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.master.write_all(bytes)
    }

    /// Read from the master directly.
    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.master.read(buf)
    }

    /// Set the window size; the kernel raises `SIGWINCH` in the child.
    pub fn resize(&self, size: WinSize) -> Result<(), PtyError> {
        let ws = size.to_raw();
        // SAFETY: TIOCSWINSZ with a valid winsize pointer on our master fd.
        let r = unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &raw const ws) };
        if r == 0 {
            Ok(())
        } else {
            Err(PtyError::Resize {
                session: self.session.clone(),
                cols: size.cols,
                rows: size.rows,
                source: io::Error::last_os_error(),
            })
        }
    }

    /// Non-blocking exit check; caches the status once reaped.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, PtyError> {
        if let Some(s) = self.exit {
            return Ok(Some(s));
        }
        match signals::try_wait(self.pid) {
            Ok(Some(s)) => {
                self.exit = Some(s);
                Ok(Some(s))
            }
            Ok(None) => Ok(None),
            Err(source) => Err(PtyError::Child {
                session: self.session.clone(),
                pid: self.pid,
                op: "reap",
                source,
            }),
        }
    }

    /// Send a signal (`SIGHUP`, `SIGTERM`, `SIGKILL`, …) to the child.
    pub fn signal(&self, signal: i32) -> Result<(), PtyError> {
        signals::kill(self.pid, signal).map_err(|source| PtyError::Child {
            session: self.session.clone(),
            pid: self.pid,
            op: "signal",
            source,
        })
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // A dropped Pty must not leave a zombie or an orphan owning the tty:
        // hang up and reap. Sessions the daemon wants to keep alive are never
        // dropped; they are re-adopted (ADR-0004).
        if self.exit.is_none() {
            let _ = signals::kill(self.pid, libc::SIGHUP);
            let mut status: libc::c_int = 0;
            // SAFETY: blocking waitpid on our own child after SIGHUP.
            unsafe { libc::waitpid(self.pid, &raw mut status, 0) };
        }
    }
}

/// Decide what to exec: an explicit program, or `login` for the login shell.
fn resolve_command(spec: &SpawnSpec) -> (String, Vec<String>) {
    if let Some((program, rest)) = spec.argv.split_first() {
        let mut args = vec![program.clone()];
        args.extend(rest.iter().cloned());
        return (program.clone(), args);
    }
    let user = std::env::var("USER").unwrap_or_else(|_| "root".into());
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    // `login` only looks for .hushlogin in the *current* directory; since we
    // keep the cwd, check the home directory ourselves and pass -q.
    let quiet = Path::new(&home).join(".hushlogin").exists();
    let flags = if quiet { "-qflp" } else { "-flp" };
    (
        "/usr/bin/login".into(),
        vec!["login".into(), flags.into(), user],
    )
}

/// `execve` does no PATH search, so do it here, before `fork`, where
/// allocation is still allowed. Names containing `/` are used as given.
fn resolve_in_path(program: &str) -> Option<String> {
    if program.contains('/') {
        return Some(program.to_owned());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| {
            candidate.metadata().is_ok_and(|m| {
                m.is_file() && std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0
            })
        })
                .unwrap_or(false)
        })
        .map(|p| p.display().to_string())
}

fn build_env(spec: &SpawnSpec, nul: &dyn Fn(&str) -> PtyError) -> Result<Vec<CString>, PtyError> {
    let mut vars: Vec<(String, String)> = std::env::vars().collect();
    for (k, v) in &spec.env {
        vars.retain(|(ek, _)| ek != k);
        vars.push((k.clone(), v.clone()));
    }
    if !vars.iter().any(|(k, _)| k == "TERM") {
        vars.push(("TERM".into(), "xterm-256color".into()));
    }
    vars.iter()
        .map(|(k, v)| CString::new(format!("{k}={v}")).map_err(|_| nul(k)))
        .collect()
}

fn set_cloexec(f: &File) -> io::Result<()> {
    // SAFETY: fcntl on an fd we own.
    unsafe {
        let flags = libc::fcntl(f.as_raw_fd(), libc::F_GETFD);
        if flags < 0 || libc::fcntl(f.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Child side of `fork`. Never returns.
///
/// # Safety
/// Must only be called in the child immediately after `fork`; uses only
/// async-signal-safe functions.
unsafe fn child_exec(
    slave: libc::c_int,
    master: libc::c_int,
    cwd: Option<&CStr>,
    program: &CStr,
    argv: &[*const libc::c_char],
    envp: &[*const libc::c_char],
) -> ! {
    unsafe {
        libc::close(master);
        if libc::setsid() < 0 {
            libc::_exit(101);
        }
        if libc::ioctl(slave, u64::from(libc::TIOCSCTTY), 0) < 0 {
            libc::_exit(102);
        }
        if libc::dup2(slave, 0) < 0 || libc::dup2(slave, 1) < 0 || libc::dup2(slave, 2) < 0 {
            libc::_exit(103);
        }
        if slave > 2 {
            libc::close(slave);
        }
        if let Some(dir) = cwd
            && libc::chdir(dir.as_ptr()) < 0
        {
            libc::_exit(104);
        }
        // Restore default signal dispositions the daemon may have changed.
        for sig in [
            libc::SIGINT,
            libc::SIGQUIT,
            libc::SIGTERM,
            libc::SIGPIPE,
            libc::SIGCHLD,
        ] {
            libc::signal(sig, libc::SIG_DFL);
        }
        libc::execve(program.as_ptr(), argv.as_ptr(), envp.as_ptr());
        libc::_exit(127);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn drain(pty: &mut Pty) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match pty.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(e) if e.raw_os_error() == Some(libc::EIO) => break, // macOS: slave closed
                Err(e) => panic!("read: {e}"),
            }
        }
        out
    }

    #[test]
    fn spawns_reads_and_reaps() {
        let spec = SpawnSpec::program(
            "test-echo",
            vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf 'hi there'; exit 3".into(),
            ],
        );
        let mut pty = Pty::spawn(&spec, WinSize::cells(80, 24)).expect("spawn");
        let out = drain(&mut pty);
        assert_eq!(String::from_utf8_lossy(&out), "hi there");
        let mut status = None;
        for _ in 0..500 {
            status = pty.try_wait().expect("try_wait");
            if status.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(status.expect("exited").code(), Some(3));
    }

    #[test]
    fn window_size_reaches_the_child() {
        let spec = SpawnSpec::program(
            "test-size",
            vec!["/bin/sh".into(), "-c".into(), "stty size".into()],
        );
        let mut pty = Pty::spawn(&spec, WinSize::cells(132, 43)).expect("spawn");
        let out = drain(&mut pty);
        assert_eq!(String::from_utf8_lossy(&out).trim(), "43 132");
    }

    #[test]
    fn resize_then_query() {
        let spec = SpawnSpec::program(
            "test-resize",
            vec!["/bin/sh".into(), "-c".into(), "sleep 0.2; stty size".into()],
        );
        let mut pty = Pty::spawn(&spec, WinSize::cells(80, 24)).expect("spawn");
        pty.resize(WinSize::cells(100, 30)).expect("resize");
        let out = drain(&mut pty);
        assert_eq!(String::from_utf8_lossy(&out).trim(), "30 100");
    }

    #[test]
    fn env_and_cwd_apply() {
        let mut spec = SpawnSpec::program(
            "test-env",
            vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf '%s %s' \"$VT_TEST\" \"$(pwd -P)\"".into(),
            ],
        );
        spec.env.push(("VT_TEST".into(), "yes".into()));
        spec.cwd = Some(std::env::temp_dir());
        let mut pty = Pty::spawn(&spec, WinSize::cells(80, 24)).expect("spawn");
        let out = String::from_utf8_lossy(&drain(&mut pty)).to_string();
        let expected_dir = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(out, format!("yes {}", expected_dir.display()));
        // Read trait is used through `drain`; keep the import meaningful.
        let _ = <File as Read>::read;
    }

    #[test]
    fn programs_are_found_on_path() {
        let spec = SpawnSpec::program(
            "test-path",
            vec!["sh".into(), "-c".into(), "printf found".into()],
        );
        let mut pty = Pty::spawn(&spec, WinSize::cells(80, 24)).expect("spawn");
        assert_eq!(String::from_utf8_lossy(&drain(&mut pty)), "found");
        let missing = SpawnSpec::program("test-missing", vec!["vt-no-such-program-xyz".into()]);
        match Pty::spawn(&missing, WinSize::cells(80, 24)) {
            Err(PtyError::Spawn {
                stage: "resolve program",
                ..
            }) => {}
            other => panic!("expected resolve error, got {other:?}"),
        }
    }

    #[test]
    fn nul_in_argument_is_a_typed_error() {
        let spec = SpawnSpec::program("test-nul", vec!["/bin/sh\0".into()]);
        match Pty::spawn(&spec, WinSize::cells(80, 24)) {
            Err(PtyError::NulByte { session, .. }) => assert_eq!(session, "test-nul"),
            other => panic!("expected NulByte, got {other:?}"),
        }
    }
}
