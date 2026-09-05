//! Cold-path notifications: one connection that forwards every daemon
//! broadcast (`config.changed`, `session.changed`, inbox updates…) to a C
//! callback on its own thread. The shell marshals to the main thread.

use std::ffi::{CString, c_char, c_void};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use vt_ipc::Client;

use crate::viewer::cstr;

/// `(ctx, method, params_json)`; strings are valid for the call only.
pub type EventCallback = Option<
    unsafe extern "C" fn(ctx: *mut c_void, method: *const c_char, params_json: *const c_char),
>;

/// A subscription. Opaque to C.
pub struct Events {
    alive: Arc<AtomicBool>,
}

struct Ctx(*mut c_void);
// SAFETY: the shell promised the context is safe to use from any thread.
unsafe impl Send for Ctx {}

/// Subscribes to all daemon notifications at `socket`. `on_event` runs on
/// the subscription's thread; a final call with method `"disconnected"`
/// and empty params marks the end. Returns null on connection failure
/// (reason in [`crate::vt_viewer_last_error`]).
#[unsafe(no_mangle)]
pub extern "C" fn vt_events_subscribe(
    socket: *const c_char,
    on_event: EventCallback,
    ctx: *mut c_void,
) -> *mut Events {
    let Some(socket) = cstr(socket) else {
        crate::viewer::set_error("socket is required".into());
        return std::ptr::null_mut();
    };
    let mut client = match Client::connect(&PathBuf::from(socket)) {
        Ok(c) => c,
        Err(e) => {
            crate::viewer::set_error(format!("cannot connect to vtermd at {socket}: {e}"));
            return std::ptr::null_mut();
        }
    };
    let alive = Arc::new(AtomicBool::new(true));
    let al = Arc::clone(&alive);
    let ctx = Ctx(ctx);
    let spawned = thread::Builder::new()
        .name("vt-events".into())
        .spawn(move || {
            let ctx = ctx;
            let deliver = |method: &str, params: &str| {
                if let (Some(cb), Ok(m), Ok(p)) =
                    (on_event, CString::new(method), CString::new(params))
                {
                    // SAFETY: the shell's callback with the shell's context.
                    unsafe { cb(ctx.0, m.as_ptr(), p.as_ptr()) };
                }
            };
            while al.load(Ordering::Relaxed) {
                match client.next_notification() {
                    Ok(Some(n)) => {
                        let params = n.params.map(|p| p.to_string()).unwrap_or_default();
                        deliver(&n.method, &params);
                    }
                    Ok(None) | Err(_) => break,
                }
            }
            deliver("disconnected", "");
        });
    if let Err(e) = spawned {
        crate::viewer::set_error(e.to_string());
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(Events { alive }))
}

/// Stops delivering and frees. The thread ends at the next message.
///
/// # Safety
/// `e` must come from [`vt_events_subscribe`] and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vt_events_free(e: *mut Events) {
    if e.is_null() {
        return;
    }
    // SAFETY: caller guarantees ownership returns here.
    let events = unsafe { Box::from_raw(e) };
    events.alive.store(false, Ordering::Relaxed);
}
