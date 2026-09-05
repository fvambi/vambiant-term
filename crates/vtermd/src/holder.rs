//! Daemon side of the per-session fd holder (`vtermd-hold`, ADR-0004).
//!
//! The holder is spawned detached (own session id, stdio null) so it outlives
//! this daemon. We connect to its control socket, receive the PTY master over
//! `SCM_RIGHTS` for input/resize/signals, and read output frames from the
//! same socket: first a replay of what the holder buffered, then live bytes,
//! finally the child's exit.

use std::io::Read;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use vt_proto::session::NewSession;
use vt_pty::Pty;

/// The child's exit as reported by the holder (`code` is `None` for a signal).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChildExit {
    /// Exit code.
    pub code: Option<i32>,
}

/// A PTY obtained from a holder.
pub struct Held {
    /// The adopted master (write side only for us).
    pub pty: Pty,
    /// Control connection carrying output frames and the exit report.
    pub control: UnixStream,
    /// Set when the child had already exited when we connected.
    pub already_exited: Option<ChildExit>,
    /// Holder control socket path (persisted for re-adoption).
    pub socket: PathBuf,
}

/// One frame from the holder.
#[derive(Debug, PartialEq, Eq)]
pub enum Frame {
    /// Output buffered before this connection existed.
    Replay(Vec<u8>),
    /// Live output.
    Output(Vec<u8>),
    /// The child exited.
    Exit(ChildExit),
}

/// Why a session could not be re-adopted.
#[derive(Debug)]
pub enum ReadoptFailure {
    /// The holder had already recorded the child's exit.
    Exited(ChildExit),
    /// No holder answers on the recorded socket: the session is orphaned.
    Unreachable(String),
}

fn holder_binary() -> PathBuf {
    std::env::var_os("VTERMD_HOLD")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("vtermd-hold")))
        })
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("vtermd-hold"))
}

/// Control socket path for a session.
pub fn socket_for(runtime_dir: &Path, session_id: &str) -> PathBuf {
    runtime_dir.join(format!("hold-{session_id}.sock"))
}

/// Spawn a holder for a new session and attach to it.
pub fn spawn(
    runtime_dir: &Path,
    session_id: &str,
    name: &str,
    req: &NewSession,
    size: (u16, u16),
    cwd: &Path,
) -> Result<Held, String> {
    let socket = socket_for(runtime_dir, session_id);
    let _ = std::fs::remove_file(&socket);
    let _ = std::fs::remove_file(socket.with_extension("exit"));
    let mut cmd = Command::new(holder_binary());
    cmd.arg("--session")
        .arg(session_id)
        .arg("--socket")
        .arg(&socket)
        .arg("--cols")
        .arg(size.0.to_string())
        .arg("--rows")
        .arg(size.1.to_string())
        .arg("--cwd")
        .arg(cwd);
    for (k, v) in &req.env {
        cmd.arg("--env").arg(format!("{k}={v}"));
    }
    cmd.arg("--env")
        .arg(format!("VAMBIANT_TERM_SESSION={session_id}"));
    cmd.arg("--");
    cmd.args(&req.argv);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe and touches no Rust state.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("cannot start vtermd-hold for session {name}: {e}"))?;
    // Detached on purpose: the daemon's reaper loop collects it when it exits.
    std::mem::forget(child);
    attach(&socket, Duration::from_secs(5), name).map_err(|e| match e {
        ReadoptFailure::Exited(exit) => {
            format!("session {name} exited immediately with {:?}", exit.code)
        }
        ReadoptFailure::Unreachable(why) => {
            format!("vtermd-hold for session {name} did not come up: {why}")
        }
    })
}

/// Re-attach to an existing holder after a daemon restart.
pub fn readopt(socket: &Path, name: &str) -> Result<Held, ReadoptFailure> {
    attach(socket, Duration::from_millis(500), name)
}

fn attach(socket: &Path, wait: Duration, name: &str) -> Result<Held, ReadoptFailure> {
    let exit_file = socket.with_extension("exit");
    let start = Instant::now();
    let stream = loop {
        match UnixStream::connect(socket) {
            Ok(s) => break s,
            Err(e) => {
                if let Ok(text) = std::fs::read_to_string(&exit_file) {
                    return Err(ReadoptFailure::Exited(ChildExit {
                        code: text.trim().parse().ok(),
                    }));
                }
                if start.elapsed() >= wait {
                    return Err(ReadoptFailure::Unreachable(format!(
                        "{}: {e}",
                        socket.display()
                    )));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };
    // The fd rides on the first header byte. Stream sockets coalesce, so take
    // exactly one byte with the fd and then the rest of the line byte-wise:
    // reading any further would swallow the first frame.
    let (fd, first) = vt_pty::fdpass::recv_fd(&stream, 1)
        .map_err(|e| ReadoptFailure::Unreachable(format!("holder handshake failed: {e}")))?;
    let mut header = first;
    let mut r = &stream;
    while header.last() != Some(&b'\n') {
        let mut b = [0u8; 1];
        match r.read(&mut b) {
            Ok(1) => header.push(b[0]),
            _ => {
                return Err(ReadoptFailure::Unreachable(
                    "holder closed during handshake".into(),
                ));
            }
        }
        if header.len() > 4096 {
            return Err(ReadoptFailure::Unreachable("holder header too long".into()));
        }
    }
    let nl = header.len() - 1;
    let head: serde_json::Value = serde_json::from_slice(&header[..nl])
        .map_err(|e| ReadoptFailure::Unreachable(format!("holder header is not JSON: {e}")))?;
    let pid = head
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let already_exited = match head.get("exited") {
        Some(serde_json::Value::Null) | None => None,
        Some(v) => Some(ChildExit {
            code: v.as_i64().and_then(|c| i32::try_from(c).ok()),
        }),
    };
    Ok(Held {
        pty: Pty::adopt(fd, u32::try_from(pid).unwrap_or(0), name),
        control: stream,
        already_exited,
        socket: socket.to_path_buf(),
    })
}

/// Read the next frame; `Ok(None)` when the holder closed the connection.
pub fn read_frame(stream: &mut UnixStream) -> std::io::Result<Option<Frame>> {
    let mut head = [0u8; 5];
    let mut got = 0;
    while got < head.len() {
        let n = stream.read(&mut head[got..])?;
        if n == 0 {
            return Ok(None);
        }
        got += n;
    }
    let len =
        usize::try_from(u32::from_be_bytes([head[1], head[2], head[3], head[4]])).unwrap_or(0);
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload)?;
    Ok(Some(match head[0] {
        1 => Frame::Replay(payload),
        2 => Frame::Output(payload),
        3 => {
            let v: serde_json::Value =
                serde_json::from_slice(&payload).unwrap_or(serde_json::Value::Null);
            Frame::Exit(ChildExit {
                code: v
                    .get("exited")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|c| i32::try_from(c).ok()),
            })
        }
        other => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown holder frame type {other}"),
            ));
        }
    }))
}
