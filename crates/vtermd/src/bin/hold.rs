//! `vtermd-hold` — the per-session fd holder (ADR-0004 amendment).
//!
//! Spawns the session's child under a PTY and then does as little as
//! possible: it is the only reader of the PTY master, keeps the last
//! [`RING_BYTES`] of output so a restarted daemon can rebuild the grid,
//! relays output to the attached daemon, hands the master fd over
//! `SCM_RIGHTS` (the daemon writes input, resizes and signals through it
//! directly), and reports the child's exit because it is the child's parent.
//!
//! Control socket protocol (one daemon at a time):
//! * on connect the holder sends the master fd with a header line
//!   `{"pid":N,"exited":null|code}`, then frames;
//! * frame = `type:u8, len:u32 BE, payload`; type 1 = replay (buffered
//!   output from before this connection), type 2 = live output, type 3 =
//!   exit (`payload` = JSON `{"exited":code}`);
//! * when the daemon disconnects the holder keeps buffering.
//!
//! When the child exits and no daemon is attached, `<socket>.exit` is written
//! with the exit code so the next daemon can close the session honestly.
//!
//! Usage: `vtermd-hold --session ID --socket PATH --cols C --rows R
//!         [--cwd DIR] [--env K=V]... -- ARGV...`

#![allow(unsafe_code)] // poll(2); nothing else.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use vt_pty::{Pty, SpawnSpec, WinSize};

/// Rolling output buffer size.
const RING_BYTES: usize = 1024 * 1024;

const FRAME_REPLAY: u8 = 1;
const FRAME_OUTPUT: u8 = 2;
const FRAME_EXIT: u8 = 3;

struct Args {
    session: String,
    socket: PathBuf,
    cols: u16,
    rows: u16,
    cwd: Option<PathBuf>,
    env: Vec<(String, String)>,
    argv: Vec<String>,
}

fn parse_args() -> Args {
    let mut a = Args {
        session: String::new(),
        socket: PathBuf::new(),
        cols: 80,
        rows: 24,
        cwd: None,
        env: Vec::new(),
        argv: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--session" => a.session = it.next().unwrap_or_default(),
            "--socket" => a.socket = PathBuf::from(it.next().unwrap_or_default()),
            "--cols" => a.cols = it.next().and_then(|v| v.parse().ok()).unwrap_or(80),
            "--rows" => a.rows = it.next().and_then(|v| v.parse().ok()).unwrap_or(24),
            "--cwd" => a.cwd = it.next().map(PathBuf::from),
            "--env" => {
                if let Some((k, v)) = it.next().and_then(|kv| {
                    kv.split_once('=')
                        .map(|(k, v)| (k.to_owned(), v.to_owned()))
                }) {
                    a.env.push((k, v));
                }
            }
            "--" => {
                a.argv = it.collect();
                break;
            }
            other => {
                eprintln!("vtermd-hold: unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    if a.session.is_empty() || a.socket.as_os_str().is_empty() {
        eprintln!("vtermd-hold: --session and --socket are required");
        std::process::exit(2);
    }
    a
}

struct Ring {
    buf: VecDeque<u8>,
}

impl Ring {
    fn push(&mut self, bytes: &[u8]) {
        if bytes.len() >= RING_BYTES {
            self.buf.clear();
            self.buf.extend(&bytes[bytes.len() - RING_BYTES..]);
            return;
        }
        let overflow = (self.buf.len() + bytes.len()).saturating_sub(RING_BYTES);
        self.buf.drain(..overflow);
        self.buf.extend(bytes);
    }
}

fn write_frame(stream: &mut UnixStream, kind: u8, payload: &[u8]) -> std::io::Result<()> {
    let len = u32::try_from(payload.len()).map_err(|_| std::io::Error::other("frame too large"))?;
    let mut head = [0u8; 5];
    head[0] = kind;
    head[1..].copy_from_slice(&len.to_be_bytes());
    stream.write_all(&head)?;
    stream.write_all(payload)
}

fn poll2(a: i32, b: Option<i32>, timeout_ms: i32) -> (bool, bool) {
    let mut fds = [
        libc::pollfd {
            fd: a,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: b.unwrap_or(-1),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    // SAFETY: fds is a valid array of 2 pollfd.
    let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout_ms) };
    if n <= 0 {
        return (false, false);
    }
    (fds[0].revents != 0, fds[1].revents != 0)
}

fn poll3(a: i32, b: Option<i32>, c: Option<i32>, timeout_ms: i32) -> (bool, bool, bool) {
    let mut fds = [
        libc::pollfd {
            fd: a,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: b.unwrap_or(-1),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: c.unwrap_or(-1),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    // SAFETY: fds is a valid array of 3 pollfd.
    let n = unsafe { libc::poll(fds.as_mut_ptr(), 3, timeout_ms) };
    if n <= 0 {
        return (false, false, false);
    }
    (
        fds[0].revents != 0,
        fds[1].revents != 0,
        fds[2].revents != 0,
    )
}

#[allow(clippy::too_many_lines)]
fn main() {
    let args = parse_args();
    let _ = std::fs::remove_file(&args.socket);
    let listener = match UnixListener::bind(&args.socket) {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "vtermd-hold: cannot listen on {}: {e}",
                args.socket.display()
            );
            std::process::exit(2);
        }
    };
    let spec = SpawnSpec {
        argv: args.argv.clone(),
        cwd: args.cwd.clone(),
        env: args.env.clone(),
        session: args.session.clone(),
    };
    let mut pty = match Pty::spawn(&spec, WinSize::cells(args.cols, args.rows)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("vtermd-hold: {e}");
            let _ = std::fs::remove_file(&args.socket);
            std::process::exit(2);
        }
    };
    let mut master = pty.reader().expect("master clone");
    let mut ring = Ring {
        buf: VecDeque::with_capacity(RING_BYTES),
    };
    let mut client: Option<UnixStream> = None;
    let mut exit_code: Option<i32> = None;
    let mut exited = false;
    let mut master_eof = false;
    let mut buf = vec![0u8; 64 * 1024];
    let exit_file = args.socket.with_extension("exit");

    loop {
        // Reap the child (we are its parent).
        if !exited && let Ok(Some(status)) = pty.try_wait() {
            exited = true;
            exit_code = status.code();
        }
        if exited && master_eof {
            let payload = serde_json::json!({ "exited": exit_code }).to_string();
            match client.as_mut() {
                Some(c) => {
                    let _ = write_frame(c, FRAME_EXIT, payload.as_bytes());
                }
                None => {
                    let _ = std::fs::write(
                        &exit_file,
                        exit_code.map_or("null".to_string(), |c| c.to_string()),
                    );
                }
            }
            break;
        }

        let master_fd = if master_eof {
            None
        } else {
            Some(master.as_raw_fd())
        };
        let client_fd = client.as_ref().map(AsRawFd::as_raw_fd);
        let (listener_ready, master_ready, client_ready) = if master_fd.is_some() {
            poll3(listener.as_raw_fd(), master_fd, client_fd, 200)
        } else {
            let (l, c) = poll2(listener.as_raw_fd(), client_fd, 200);
            (l, false, c)
        };

        if listener_ready && let Ok((stream, _)) = listener.accept() {
            // One daemon at a time; a newer one replaces the old connection.
            let header = serde_json::json!({ "pid": pty.pid(), "exited": if exited { exit_code } else { None } });
            let mut line = header.to_string();
            line.push('\n');
            if vt_pty::fdpass::send_fd(&stream, &master.as_fd(), line.as_bytes()).is_ok() {
                let mut s = stream;
                let (a, b) = ring.buf.as_slices();
                let replay: Vec<u8> = a.iter().chain(b.iter()).copied().collect();
                if write_frame(&mut s, FRAME_REPLAY, &replay).is_ok() {
                    client = Some(s);
                }
            }
            continue;
        }
        if master_ready {
            match master.read(&mut buf) {
                Ok(n) if n > 0 => {
                    ring.push(&buf[..n]);
                    if let Some(c) = client.as_mut()
                        && write_frame(c, FRAME_OUTPUT, &buf[..n]).is_err()
                    {
                        client = None;
                    }
                }
                _ => master_eof = true,
            }
        }
        if client_ready && let Some(c) = client.as_mut() {
            // Anything readable on the control socket means the daemon went away.
            let mut probe = [0u8; 256];
            match c.read(&mut probe) {
                Ok(0) | Err(_) => client = None,
                Ok(_) => {} // ignore chatter
            }
        }
    }
    let _ = std::fs::remove_file(&args.socket);
}
