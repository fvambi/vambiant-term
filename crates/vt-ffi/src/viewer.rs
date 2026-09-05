//! The hot path: a viewer attached to one daemon session. It keeps the grid
//! as a flat `#[repr(C)]` cell array the renderer reads by pointer, applies
//! output deltas on its own thread, and forwards keys on a second connection
//! so a slow render never delays a keystroke (same split as `vterm attach`).
//!
//! Threading: `on_dirty` is invoked on the viewer's reader thread, **never**
//! on the main thread — the shell marshals it. Everything else is callable
//! from any thread; `acquire`/`release` bracket the renderer's read and
//! block the reader for that long only.

use std::ffi::{CStr, c_char, c_void};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use base64::Engine as _;
use vt_core::key::{KeyAction, KeyCode, KeyEvent, KeyMods};
use vt_ipc::Client;
use vt_proto::session::{OutputDelta, WireCell, method, notification};

/// One cell as the renderer sees it. `ch` is a Unicode scalar; `fg`/`bg`
/// are `[kind, a, b, c]` with kind 0 = default, 1 = indexed (`a`),
/// 2 = rgb (`a`,`b`,`c`); `attrs` are `vt_core::cell::Attrs` bits.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VtCell {
    /// Base code point.
    pub ch: u32,
    /// Foreground.
    pub fg: [u8; 4],
    /// Background.
    pub bg: [u8; 4],
    /// Attribute bits.
    pub attrs: u16,
    /// Reserved; zero.
    pub reserved: u16,
}

impl VtCell {
    const BLANK: Self = Self {
        ch: ' ' as u32,
        fg: [0; 4],
        bg: [0; 4],
        attrs: 0,
        reserved: 0,
    };

    fn from_wire(w: &WireCell) -> Self {
        Self {
            ch: w.c as u32,
            fg: w.fg,
            bg: w.bg,
            attrs: w.attrs,
            reserved: 0,
        }
    }
}

/// A read-only view of the grid, valid between `vt_viewer_acquire` and
/// `vt_viewer_release`. `cells` is row-major, `cols * rows` long.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VtGridView {
    /// Cells.
    pub cells: *const VtCell,
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
    /// Cursor row.
    pub cursor_row: u16,
    /// Cursor column.
    pub cursor_col: u16,
    /// Cursor visible.
    pub cursor_visible: bool,
    /// Sequence number of the last applied delta; unchanged means nothing
    /// new to draw.
    pub seq: u64,
    /// The daemon connection is gone; the grid is the last known state.
    pub disconnected: bool,
}

/// A key event as the renderer reports it. `key` is a `KeyCode` numeric
/// value, `mods` are `KeyMods` bits, `action` 0 press / 1 release / 2 repeat.
/// `utf8` holds the produced text (up to 8 bytes, `utf8_len` used).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VtKeyEvent {
    /// Press / release / repeat.
    pub action: u8,
    /// Physical key code.
    pub key: u16,
    /// Modifier bits.
    pub mods: u16,
    /// Produced text.
    pub utf8: [u8; 8],
    /// Bytes used in `utf8`.
    pub utf8_len: u8,
    /// Unshifted character, or 0.
    pub unshifted: u32,
}

/// Callback type for dirty notifications; runs on the viewer's own thread.
pub type VtDirtyCallback = Option<unsafe extern "C" fn(ctx: *mut c_void)>;

struct Grid {
    cells: Vec<VtCell>,
    cols: u16,
    rows: u16,
    cursor: (u16, u16, bool),
    seq: u64,
}

impl Grid {
    fn apply(&mut self, d: &OutputDelta) {
        if d.full || d.cols != self.cols || d.rows != self.rows {
            self.cols = d.cols;
            self.rows = d.rows;
            self.cells = vec![VtCell::BLANK; usize::from(d.cols) * usize::from(d.rows)];
        }
        let cols = usize::from(self.cols);
        for line in &d.lines {
            let r = usize::from(line.row);
            if r >= usize::from(self.rows) {
                continue;
            }
            let row = &mut self.cells[r * cols..(r + 1) * cols];
            for (i, cell) in row.iter_mut().enumerate() {
                *cell = line.cells.get(i).map_or(VtCell::BLANK, VtCell::from_wire);
            }
        }
        self.cursor = d.cursor;
        self.seq = d.seq;
    }
}

/// An attached viewer. Opaque to C.
pub struct VtViewer {
    session: String,
    grid: Arc<Mutex<Grid>>,
    held: Option<MutexGuard<'static, Grid>>,
    input: Mutex<Option<Client>>,
    disconnected: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
}

// SAFETY: the guard is only ever created and dropped by the thread that
// calls acquire/release, and the Arc keeps the mutex alive for it.
unsafe impl Send for VtViewer {}

struct DirtyCtx(*mut c_void);
// SAFETY: the context pointer is handed to the shell's callback unchanged;
// the shell promised it is safe to use from the reader thread.
unsafe impl Send for DirtyCtx {}

fn cstr<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    // SAFETY: caller passes a NUL-terminated string that outlives the call.
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}

/// Attach to `session` (id or unique name) on the daemon at `socket`.
/// Returns null when the daemon is unreachable or the session unknown; the
/// reason is available from [`vt_viewer_last_error`]. `on_dirty(ctx)` fires
/// after every applied delta and on disconnect, on the viewer's thread.
#[unsafe(no_mangle)]
pub extern "C" fn vt_viewer_attach(
    socket: *const c_char,
    session: *const c_char,
    on_dirty: VtDirtyCallback,
    ctx: *mut c_void,
) -> *mut VtViewer {
    let (Some(socket), Some(session)) = (cstr(socket), cstr(session)) else {
        set_error("socket and session are required".into());
        return std::ptr::null_mut();
    };
    match attach(socket, session, on_dirty, DirtyCtx(ctx)) {
        Ok(v) => Box::into_raw(Box::new(v)),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

fn attach(
    socket: &str,
    session: &str,
    on_dirty: VtDirtyCallback,
    ctx: DirtyCtx,
) -> Result<VtViewer, String> {
    let path = PathBuf::from(socket);
    let mut stream =
        Client::connect(&path).map_err(|e| format!("cannot connect to vtermd at {socket}: {e}"))?;
    let input =
        Client::connect(&path).map_err(|e| format!("cannot connect to vtermd at {socket}: {e}"))?;
    let first: OutputDelta = stream
        .call(
            method::SESSION_ATTACH,
            Some(serde_json::json!({ "id": session })),
        )
        .and_then(|v| serde_json::from_value(v).map_err(vt_ipc::IpcError::from))
        .map_err(|e| format!("cannot attach to session {session}: {e}"))?;
    let id = first.id.0.clone();
    let mut grid = Grid {
        cells: Vec::new(),
        cols: 0,
        rows: 0,
        cursor: (0, 0, true),
        seq: 0,
    };
    grid.apply(&first);
    let grid = Arc::new(Mutex::new(grid));
    let disconnected = Arc::new(AtomicBool::new(false));
    let alive = Arc::new(AtomicBool::new(true));
    let (g, dc, al, sid) = (
        Arc::clone(&grid),
        Arc::clone(&disconnected),
        Arc::clone(&alive),
        id.clone(),
    );
    thread::Builder::new()
        .name(format!("vt-viewer-{id}"))
        .spawn(move || {
            let ctx = ctx;
            let fire = || {
                if let Some(cb) = on_dirty {
                    // SAFETY: the shell's callback with the shell's context.
                    unsafe { cb(ctx.0) };
                }
            };
            fire();
            while al.load(Ordering::Relaxed) {
                match stream.next_notification() {
                    Ok(Some(n)) if n.method == notification::SESSION_OUTPUT => {
                        if let Some(d) = n
                            .params
                            .and_then(|p| serde_json::from_value::<OutputDelta>(p).ok())
                            && d.id.0 == sid
                        {
                            g.lock().unwrap_or_else(PoisonError::into_inner).apply(&d);
                            fire();
                        }
                    }
                    Ok(Some(n)) if n.method == notification::SESSION_EXITED => {
                        if n.params
                            .as_ref()
                            .and_then(|p| p.get("id"))
                            .and_then(|v| v.as_str())
                            == Some(&sid)
                        {
                            dc.store(true, Ordering::Relaxed);
                            fire();
                            break;
                        }
                    }
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => {
                        dc.store(true, Ordering::Relaxed);
                        fire();
                        break;
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(VtViewer {
        session: id,
        grid,
        held: None,
        input: Mutex::new(Some(input)),
        disconnected,
        alive,
    })
}

thread_local! {
    static LAST_ERROR: std::cell::RefCell<std::ffi::CString> = std::cell::RefCell::new(std::ffi::CString::default());
}

fn set_error(e: String) {
    LAST_ERROR.with(|c| *c.borrow_mut() = std::ffi::CString::new(e).unwrap_or_default());
}

/// The last attach error on this thread; valid until the next call.
#[unsafe(no_mangle)]
pub extern "C" fn vt_viewer_last_error() -> *const c_char {
    LAST_ERROR.with(|c| c.borrow().as_ptr())
}

/// Lock the grid for reading and describe it. Must be paired with
/// [`vt_viewer_release`]; the reader thread waits in between.
///
/// # Safety
/// `v` must be a live viewer and not already acquired.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vt_viewer_acquire(v: *mut VtViewer) -> VtGridView {
    // SAFETY: caller guarantees a live viewer.
    let v = unsafe { &mut *v };
    let guard = v.grid.lock().unwrap_or_else(PoisonError::into_inner);
    // SAFETY: the guard borrows `v.grid`, an Arc the viewer keeps alive until
    // `vt_viewer_free`, which refuses while a guard is held.
    let guard: MutexGuard<'static, Grid> = unsafe { std::mem::transmute(guard) };
    let view = VtGridView {
        cells: guard.cells.as_ptr(),
        cols: guard.cols,
        rows: guard.rows,
        cursor_row: guard.cursor.0,
        cursor_col: guard.cursor.1,
        cursor_visible: guard.cursor.2,
        seq: guard.seq,
        disconnected: v.disconnected.load(Ordering::Relaxed),
    };
    v.held = Some(guard);
    view
}

/// Unlock after [`vt_viewer_acquire`]; the view's pointer is invalid after this.
///
/// # Safety
/// `v` must be a live viewer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vt_viewer_release(v: *mut VtViewer) {
    // SAFETY: caller guarantees a live viewer.
    let v = unsafe { &mut *v };
    v.held = None;
}

/// Current sequence number without locking (cheap poll for "anything new?").
///
/// # Safety
/// `v` must be a live viewer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vt_viewer_seq(v: *const VtViewer) -> u64 {
    // SAFETY: caller guarantees a live viewer.
    let v = unsafe { &*v };
    v.grid.lock().unwrap_or_else(PoisonError::into_inner).seq
}

fn with_input<T>(v: &VtViewer, f: impl FnOnce(&mut Client) -> Result<T, vt_ipc::IpcError>) -> bool {
    let mut guard = v.input.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(client) = guard.as_mut() else {
        return false;
    };
    match f(client) {
        Ok(_) => true,
        Err(_) => {
            *guard = None;
            v.disconnected.store(true, Ordering::Relaxed);
            false
        }
    }
}

/// Send a key event; the daemon encodes it with the session's current
/// keyboard modes. Returns `false` when the daemon is gone.
///
/// # Safety
/// `v` must be a live viewer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vt_viewer_send_key(v: *const VtViewer, key: VtKeyEvent) -> bool {
    // SAFETY: caller guarantees a live viewer.
    let v = unsafe { &*v };
    let len = usize::from(key.utf8_len).min(key.utf8.len());
    let utf8 = std::str::from_utf8(&key.utf8[..len])
        .ok()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let event = KeyEvent {
        action: match key.action {
            1 => KeyAction::Release,
            2 => KeyAction::Repeat,
            _ => KeyAction::Press,
        },
        key: KeyCode::from_code(key.key),
        mods: KeyMods(key.mods),
        utf8,
        unshifted: char::from_u32(key.unshifted).filter(|c| *c != '\0'),
    };
    let params = serde_json::json!({ "id": v.session, "key": event });
    with_input(v, |c| c.call(method::SESSION_KEY, Some(params)))
}

/// Send raw bytes (paste, IME commit). Returns `false` when the daemon is gone.
///
/// # Safety
/// `v` must be a live viewer; `bytes` must point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vt_viewer_send_bytes(
    v: *const VtViewer,
    bytes: *const u8,
    len: usize,
) -> bool {
    // SAFETY: caller guarantees a live viewer and a valid buffer.
    let (v, data) = unsafe { (&*v, std::slice::from_raw_parts(bytes, len)) };
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    let params = serde_json::json!({ "id": v.session, "bytes": b64 });
    with_input(v, |c| c.call(method::SESSION_INPUT, Some(params)))
}

/// Resize the session's grid. Returns `false` when the daemon is gone.
///
/// # Safety
/// `v` must be a live viewer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vt_viewer_resize(v: *const VtViewer, cols: u16, rows: u16) -> bool {
    // SAFETY: caller guarantees a live viewer.
    let v = unsafe { &*v };
    if cols == 0 || rows == 0 {
        return false;
    }
    let params = serde_json::json!({ "id": v.session, "cols": cols, "rows": rows });
    with_input(v, |c| c.call(method::SESSION_RESIZE, Some(params)))
}

/// Detach and free. Returns `false` (and does nothing) while the grid is
/// still acquired.
///
/// # Safety
/// `v` must come from [`vt_viewer_attach`] and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vt_viewer_free(v: *mut VtViewer) -> bool {
    if v.is_null() {
        return true;
    }
    // SAFETY: caller guarantees a live viewer they will not touch again.
    if unsafe { &*v }.held.is_some() {
        return false;
    }
    // SAFETY: as above; ownership returns here.
    let viewer = unsafe { Box::from_raw(v) };
    viewer.alive.store(false, Ordering::Relaxed);
    if let Some(mut c) = viewer
        .input
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    {
        let _ = c.notify(
            method::SESSION_DETACH,
            Some(serde_json::json!({ "id": viewer.session })),
        );
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use vt_proto::session::{SessionId, WireRow};

    #[test]
    fn deltas_resize_and_patch_rows() {
        let mut g = Grid {
            cells: Vec::new(),
            cols: 0,
            rows: 0,
            cursor: (0, 0, true),
            seq: 0,
        };
        let cell = |c: char| WireCell {
            c,
            fg: [1, 3, 0, 0],
            bg: [0; 4],
            attrs: 1,
        };
        g.apply(&OutputDelta {
            id: SessionId("s".into()),
            cols: 3,
            rows: 2,
            full: true,
            lines: vec![WireRow {
                row: 0,
                cells: vec![cell('a'), cell('b')],
            }],
            cursor: (0, 2, true),
            seq: 1,
        });
        assert_eq!(g.cells.len(), 6);
        assert_eq!(g.cells[0].ch, u32::from('a'));
        assert_eq!(g.cells[0].fg, [1, 3, 0, 0]);
        assert_eq!(g.cells[2], VtCell::BLANK, "short rows are padded");
        g.apply(&OutputDelta {
            id: SessionId("s".into()),
            cols: 3,
            rows: 2,
            full: false,
            lines: vec![
                WireRow {
                    row: 1,
                    cells: vec![cell('z')],
                },
                WireRow {
                    row: 9,
                    cells: vec![],
                },
            ],
            cursor: (1, 1, false),
            seq: 2,
        });
        assert_eq!(g.cells[3].ch, u32::from('z'));
        assert_eq!(g.cells[0].ch, u32::from('a'), "untouched rows stay");
        assert_eq!(g.seq, 2);
        assert!(!g.cursor.2);
    }
}
