//! Drives the `vt-ffi` viewer exactly as the Swift shell will: create a
//! session through the cold-path passthrough, attach the hot-path viewer,
//! read the grid by pointer between acquire/release, type through
//! `vt_viewer_send_key`, resize, and watch the dirty callback fire on the
//! reader thread.

#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use vambiant_term::{
    GridView, KeyEvent, Viewer, vt_daemon_call, vt_string_free, vt_viewer_acquire,
    vt_viewer_attach, vt_viewer_free, vt_viewer_last_error, vt_viewer_release, vt_viewer_resize,
    vt_viewer_send_bytes, vt_viewer_send_key, vt_viewer_seq,
};

struct Daemon {
    child: Child,
    socket: CString,
    dir: PathBuf,
}

impl Daemon {
    fn start(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vtermd-ffi-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("vtermd.sock");
        let child = Command::new(env!("CARGO_BIN_EXE_vtermd"))
            .arg("--socket")
            .arg(&socket)
            .env("VAMBIANT_TERM_STATE", dir.join("state"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn vtermd");
        let start = Instant::now();
        while !socket.exists() {
            assert!(start.elapsed() < Duration::from_secs(10), "no socket");
            std::thread::sleep(Duration::from_millis(20));
        }
        Self {
            child,
            socket: CString::new(socket.display().to_string()).unwrap(),
            dir,
        }
    }

    fn call(&self, method: &str, params: &str) -> serde_json::Value {
        let m = CString::new(method).unwrap();
        let p = CString::new(params).unwrap();
        let raw = vt_daemon_call(self.socket.as_ptr(), m.as_ptr(), p.as_ptr());
        assert!(!raw.is_null());
        // SAFETY: `raw` was just returned by the library and is NUL-terminated.
        let text = unsafe { CStr::from_ptr(raw) }.to_str().unwrap().to_owned();
        // SAFETY: freeing what the library allocated, once.
        unsafe { vt_string_free(raw) };
        serde_json::from_str(&text).unwrap()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = Command::new("pkill")
            .arg("-f")
            .arg(self.dir.display().to_string())
            .status();
    }
}

unsafe extern "C" fn count_dirty(ctx: *mut c_void) {
    // SAFETY: ctx is the AtomicUsize the test passed to attach.
    unsafe { &*ctx.cast::<AtomicUsize>() }.fetch_add(1, Ordering::SeqCst);
}

fn text_of(view: &GridView) -> String {
    let n = usize::from(view.cols) * usize::from(view.rows);
    // SAFETY: the view is held between acquire and release.
    let cells = unsafe { std::slice::from_raw_parts(view.cells, n) };
    cells
        .chunks(usize::from(view.cols))
        .map(|row| {
            row.iter()
                .map(|c| char::from_u32(c.ch).unwrap_or(' '))
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn wait_grid(v: *mut Viewer, needle: &str) -> (String, GridView) {
    let start = Instant::now();
    loop {
        // SAFETY: v is a live, unacquired viewer.
        let view = unsafe { vt_viewer_acquire(v) };
        let text = text_of(&view);
        // SAFETY: paired with the acquire above.
        unsafe { vt_viewer_release(v) };
        if text.contains(needle) {
            return (text, view);
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "never saw {needle:?}; grid:\n{text}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn key(code: u16, text: &str) -> KeyEvent {
    let mut utf8 = [0u8; 8];
    utf8[..text.len()].copy_from_slice(text.as_bytes());
    KeyEvent {
        action: 0,
        key: code,
        mods: 0,
        utf8,
        utf8_len: u8::try_from(text.len()).unwrap(),
        unshifted: text.chars().next().map_or(0, u32::from),
    }
}

#[test]
fn viewer_sees_output_and_routes_keys() {
    // Declared before the daemon so it outlives the daemon's shutdown, which
    // is when a late dirty signal could still arrive.
    let dirty = Box::new(AtomicUsize::new(0));
    let daemon = Daemon::start("viewer");
    let created = daemon.call(
        "session.new",
        &serde_json::json!({
            "name": "ffi",
            "argv": ["/bin/sh", "-c", "echo READY; read line; echo GOT:$line; sleep 30"],
            "cwd": std::env::temp_dir(),
            "size": [40, 6],
        })
        .to_string(),
    );
    let id = created["result"]["id"]
        .as_str()
        .expect("session id")
        .to_owned();

    let ctx: *const AtomicUsize = &raw const *dirty;
    let session = CString::new(id.clone()).unwrap();
    let v = vt_viewer_attach(
        daemon.socket.as_ptr(),
        session.as_ptr(),
        Some(count_dirty),
        ctx.cast_mut().cast::<c_void>(),
    );
    assert!(
        !v.is_null(),
        "attach failed: {}",
        // SAFETY: the library keeps the error string alive on this thread.
        unsafe { CStr::from_ptr(vt_viewer_last_error()) }.to_string_lossy()
    );

    let (_, view) = wait_grid(v, "READY");
    assert_eq!((view.cols, view.rows), (40, 6));
    assert!(view.cursor_visible);
    assert!(!view.disconnected);
    // The first signal comes from the viewer's thread, which may not have
    // run yet when READY was already in the attach snapshot.
    let start = Instant::now();
    while dirty.load(Ordering::SeqCst) == 0 {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "dirty never fired after attach"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    // Type "hi" + Enter through the key path (Enter = 58, see KeyCode).
    for k in [key(0, "h"), key(0, "i"), key(58, "\r")] {
        // SAFETY: live viewer.
        assert!(unsafe { vt_viewer_send_key(v, k) });
    }
    let (text, view) = wait_grid(v, "GOT:hi");
    assert!(text.starts_with("READY"), "{text}");
    // SAFETY: live viewer.
    assert_eq!(unsafe { vt_viewer_seq(v) }, view.seq);
    let before = dirty.load(Ordering::SeqCst);
    assert!(
        before >= 2,
        "dirty fired for the output delta, count {before}"
    );

    // SAFETY: live viewer.
    assert!(unsafe { vt_viewer_resize(v, 30, 4) });
    let start = Instant::now();
    loop {
        // SAFETY: live, unacquired viewer.
        let view = unsafe { vt_viewer_acquire(v) };
        let size = (view.cols, view.rows);
        // SAFETY: paired.
        unsafe { vt_viewer_release(v) };
        if size == (30, 4) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "resize never landed"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    // Raw bytes reach the pty too (the shell is in `sleep`, so it is just
    // buffered; the call must still succeed).
    let raw = b"x";
    // SAFETY: live viewer, valid buffer.
    assert!(unsafe { vt_viewer_send_bytes(v, raw.as_ptr(), raw.len()) });

    // Freeing while acquired is refused; released, it detaches.
    // SAFETY: live, unacquired viewer.
    let _held = unsafe { vt_viewer_acquire(v) };
    // SAFETY: still live.
    assert!(!unsafe { vt_viewer_free(v) });
    // SAFETY: paired.
    unsafe { vt_viewer_release(v) };
    // SAFETY: not used afterwards.
    assert!(unsafe { vt_viewer_free(v) });

    let ls = daemon.call("session.list", "");
    assert_eq!(ls["result"].as_array().map(Vec::len), Some(1));
}

#[test]
fn viewer_reports_exit_as_disconnected() {
    let daemon = Daemon::start("exit");
    let created = daemon.call(
        "session.new",
        &serde_json::json!({
            "name": "short",
            "argv": ["/bin/sh", "-c", "echo BYE; sleep 0.3"],
            "cwd": std::env::temp_dir(),
        })
        .to_string(),
    );
    let id = created["result"]["id"]
        .as_str()
        .expect("session id")
        .to_owned();
    let session = CString::new(id).unwrap();
    let v = vt_viewer_attach(
        daemon.socket.as_ptr(),
        session.as_ptr(),
        None,
        std::ptr::null_mut(),
    );
    assert!(!v.is_null());
    let start = Instant::now();
    loop {
        // SAFETY: live, unacquired viewer.
        let view = unsafe { vt_viewer_acquire(v) };
        let (gone, text) = (view.disconnected, text_of(&view));
        // SAFETY: paired.
        unsafe { vt_viewer_release(v) };
        if gone {
            assert!(
                text.contains("BYE"),
                "last grid survives disconnect: {text}"
            );
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "never disconnected"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    // SAFETY: not used afterwards.
    assert!(unsafe { vt_viewer_free(v) });
}

#[test]
fn attach_errors_are_readable() {
    let daemon = Daemon::start("errors");
    let session = CString::new("no-such-session").unwrap();
    let v = vt_viewer_attach(
        daemon.socket.as_ptr(),
        session.as_ptr(),
        None,
        std::ptr::null_mut(),
    );
    assert!(v.is_null());
    // SAFETY: valid until the next attach on this thread.
    let err = unsafe { CStr::from_ptr(vt_viewer_last_error()) }.to_string_lossy();
    assert!(err.contains("no-such-session"), "{err}");

    let nowhere = CString::new("/nonexistent/vtermd.sock").unwrap();
    let v = vt_viewer_attach(
        nowhere.as_ptr(),
        session.as_ptr(),
        None,
        std::ptr::null_mut(),
    );
    assert!(v.is_null());
    let err = unsafe { CStr::from_ptr(vt_viewer_last_error()) }.to_string_lossy();
    assert!(err.contains("cannot connect"), "{err}");

    let bad = daemon.call("session.list", "{not json");
    assert!(
        bad["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not JSON")
    );
    let unknown = daemon.call("no.such.method", "");
    assert!(unknown.get("error").is_some(), "{unknown}");
    let null_method: *const c_char = std::ptr::null();
    let raw = vt_daemon_call(daemon.socket.as_ptr(), null_method, null_method);
    // SAFETY: library-owned string.
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: freed once.
    unsafe { vt_string_free(raw) };
    assert!(text.contains("required"), "{text}");
}
