//! Interactive attach: render the session's grid in the current terminal and
//! forward keystrokes. Two connections: one streams output deltas, one sends
//! input, so a slow render never delays a keystroke.
//!
//! Rendering is deliberately dumb — damaged rows are repainted whole with SGR
//! per cell — because this viewer is a bridge until M4, not the product.

#![allow(unsafe_code)] // termios raw mode and TIOCGWINSZ; nothing else.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::thread;

use base64::Engine as _;
use vt_ipc::Client;
use vt_proto::session::{OutputDelta, WireCell, method, notification};

const DETACH_PREFIX: u8 = 0x1c; // Ctrl-\

struct RawMode(libc::termios);

impl RawMode {
    fn enable() -> std::io::Result<Self> {
        let mut t = std::mem::MaybeUninit::<libc::termios>::uninit();
        // SAFETY: tcgetattr fills the struct on success.
        if unsafe { libc::tcgetattr(0, t.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: initialised by tcgetattr above.
        let orig = unsafe { t.assume_init() };
        let mut raw = orig;
        // SAFETY: cfmakeraw only writes the struct.
        unsafe { libc::cfmakeraw(&raw mut raw) };
        // SAFETY: valid termios.
        if unsafe { libc::tcsetattr(0, libc::TCSANOW, &raw const raw) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self(orig))
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // SAFETY: restoring the termios we saved.
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &raw const self.0) };
        let _ = std::io::stdout().write_all(b"\x1b[0m\x1b[?25h\r\n");
    }
}

fn local_size() -> (u16, u16) {
    let mut ws = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: TIOCGWINSZ writes a winsize.
    if unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &raw mut ws) } == 0
        && ws.ws_col > 0
        && ws.ws_row > 0
    {
        (ws.ws_col, ws.ws_row)
    } else {
        (80, 24)
    }
}

fn sgr(cell: &WireCell) -> String {
    let mut s = String::from("\x1b[0");
    let a = cell.attrs;
    if a & 1 != 0 {
        s.push_str(";1");
    }
    if a & (1 << 1) != 0 {
        s.push_str(";3");
    }
    if a & (1 << 2) != 0 {
        s.push_str(";4");
    }
    if a & (1 << 3) != 0 {
        s.push_str(";9");
    }
    if a & (1 << 4) != 0 {
        s.push_str(";7");
    }
    if a & (1 << 5) != 0 {
        s.push_str(";2");
    }
    for (color, base) in [(cell.fg, 38), (cell.bg, 48)] {
        match color[0] {
            1 => {
                let _ = write!(s, ";{base};5;{}", color[1]);
            }
            2 => {
                let _ = write!(s, ";{base};2;{};{};{}", color[1], color[2], color[3]);
            }
            _ => {}
        }
    }
    s.push('m');
    s
}

fn paint(out: &mut impl Write, delta: &OutputDelta) -> std::io::Result<()> {
    let mut buf = String::new();
    buf.push_str("\x1b[?25l");
    for row in &delta.lines {
        let _ = write!(buf, "\x1b[{};1H", row.row + 1);
        let mut last = String::new();
        for cell in &row.cells {
            if cell.attrs & (1 << 8) != 0 {
                continue; // wide spacer
            }
            let s = sgr(cell);
            if s != last {
                buf.push_str(&s);
                last = s;
            }
            buf.push(if cell.c == '\0' { ' ' } else { cell.c });
        }
        buf.push_str("\x1b[0m\x1b[K");
    }
    let (r, c, visible) = delta.cursor;
    let _ = write!(buf, "\x1b[{};{}H", r + 1, c + 1);
    if visible {
        buf.push_str("\x1b[?25h");
    }
    out.write_all(buf.as_bytes())?;
    out.flush()
}

/// Attach until the user presses Ctrl-\ d or the session ends.
#[allow(clippy::too_many_lines)]
pub fn run(socket: &Path, session: &str) -> Result<(), String> {
    let mut control = Client::connect(socket).map_err(|e| e.to_string())?;
    let mut stream = Client::connect(socket).map_err(|e| e.to_string())?;
    let info = control
        .call(
            method::SESSION_GET,
            Some(serde_json::json!({ "id": session })),
        )
        .map_err(|e| e.to_string())?;
    let id = info
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or(session)
        .to_string();
    let (cols, rows) = local_size();
    control
        .call(
            method::SESSION_RESIZE,
            Some(serde_json::json!({ "id": id, "cols": cols, "rows": rows })),
        )
        .map_err(|e| e.to_string())?;
    let snap = stream
        .call(
            method::SESSION_ATTACH,
            Some(serde_json::json!({ "id": id })),
        )
        .map_err(|e| e.to_string())?;
    let snap: OutputDelta = serde_json::from_value(snap).map_err(|e| e.to_string())?;

    let _raw =
        RawMode::enable().map_err(|e| format!("cannot put the terminal in raw mode: {e}"))?;
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(b"\x1b[2J");
    paint(&mut out, &snap).map_err(|e| e.to_string())?;
    drop(out);

    // Output stream on its own thread.
    let (tx, rx) = mpsc::channel::<Result<OutputDelta, String>>();
    let sid = id.clone();
    thread::spawn(move || {
        loop {
            match stream.next_notification() {
                Ok(Some(n)) if n.method == notification::SESSION_OUTPUT => {
                    if let Some(d) = n
                        .params
                        .and_then(|p| serde_json::from_value::<OutputDelta>(p).ok())
                        && d.id.0 == sid
                        && tx.send(Ok(d)).is_err()
                    {
                        break;
                    }
                }
                Ok(Some(n)) if n.method == notification::SESSION_EXITED => {
                    if n.params
                        .as_ref()
                        .and_then(|p| p.get("id"))
                        .and_then(|v| v.as_str())
                        == Some(&sid)
                    {
                        let _ = tx.send(Err("session exited".into()));
                        break;
                    }
                }
                Ok(Some(_)) => {}
                Ok(None) => {
                    let _ = tx.send(Err("daemon closed the connection".into()));
                    break;
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    break;
                }
            }
        }
    });
    // Keyboard on its own thread.
    let (ktx, krx) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut buf = [0u8; 1024];
        while let Ok(n) = stdin.read(&mut buf) {
            if n == 0 || ktx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut prefix = false;
    loop {
        // Prefer output; poll keys with a short timeout so neither starves.
        match rx.recv_timeout(std::time::Duration::from_millis(4)) {
            Ok(Ok(delta)) => {
                let mut out = std::io::stdout().lock();
                paint(&mut out, &delta).map_err(|e| e.to_string())?;
            }
            Ok(Err(why)) => {
                return Err(why);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("output stream ended".into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        while let Ok(keys) = krx.try_recv() {
            let mut forward = Vec::with_capacity(keys.len());
            for b in keys {
                if prefix {
                    prefix = false;
                    if b == b'd' {
                        return Ok(());
                    }
                    forward.push(DETACH_PREFIX);
                    forward.push(b);
                } else if b == DETACH_PREFIX {
                    prefix = true;
                } else {
                    forward.push(b);
                }
            }
            if !forward.is_empty() {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&forward);
                control
                    .call(
                        method::SESSION_INPUT,
                        Some(serde_json::json!({ "id": id, "bytes": b64 })),
                    )
                    .map_err(|e| e.to_string())?;
            }
        }
    }
}
