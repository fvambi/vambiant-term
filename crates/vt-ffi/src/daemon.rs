//! Cold-path passthrough to `vtermd`: one JSON-RPC call per invocation.
//! ADR-0002 reserves `swift-bridge` for this tier; until an async surface
//! is needed, a JSON string in and a JSON string out is simpler, has no
//! build-time code generation, and cannot rot with a 0.1.x crate.

use std::ffi::{CStr, CString, c_char};
use std::path::PathBuf;

fn cstr<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    // SAFETY: caller passes a NUL-terminated string that outlives the call.
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}

fn into_c(s: String) -> *mut c_char {
    CString::new(s).map_or(std::ptr::null_mut(), CString::into_raw)
}

/// The default daemon socket for this user (`VAMBIANT_TERM_RUNTIME` aware).
/// Free with [`vt_string_free`].
#[unsafe(no_mangle)]
pub extern "C" fn vt_default_socket() -> *mut c_char {
    into_c(vt_ipc::transport::socket_path().display().to_string())
}

/// Call `method` on the daemon at `socket` with `params_json` (may be
/// null). Returns a JSON document `{"result": …}` or `{"error": {…}}`,
/// never null. Blocking; not for the render thread. Free with
/// [`vt_string_free`].
#[unsafe(no_mangle)]
pub extern "C" fn vt_daemon_call(
    socket: *const c_char,
    method: *const c_char,
    params_json: *const c_char,
) -> *mut c_char {
    let reply = call(cstr(socket), cstr(method), cstr(params_json));
    into_c(reply.to_string())
}

fn call(socket: Option<&str>, method: Option<&str>, params: Option<&str>) -> serde_json::Value {
    let err = |m: String| serde_json::json!({ "error": { "message": m } });
    let (Some(socket), Some(method)) = (socket, method) else {
        return err("socket and method are required".into());
    };
    let params = match params {
        Some(p) if !p.trim().is_empty() => match serde_json::from_str(p) {
            Ok(v) => Some(v),
            Err(e) => return err(format!("params are not JSON: {e}")),
        },
        _ => None,
    };
    let mut client = match vt_ipc::Client::connect(&PathBuf::from(socket)) {
        Ok(c) => c,
        Err(e) => return err(format!("cannot connect to vtermd at {socket}: {e}")),
    };
    match client.call(method, params) {
        Ok(v) => serde_json::json!({ "result": v }),
        Err(e) => err(e.to_string()),
    }
}

/// Free a string returned by this library.
///
/// # Safety
/// `s` must come from this library and not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vt_string_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: allocated by CString::into_raw in this crate.
        drop(unsafe { CString::from_raw(s) });
    }
}
