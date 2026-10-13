//! kicksdk behind a C ABI.
//!
//! Six functions, all of the core: [`kicksdk_call`] runs any function by name
//! (see `kicksdk::dispatch` for the names and their arguments), and the
//! `kicksdk_session_*` functions hold a realtime session. Arguments and
//! results are UTF-8 JSON, NUL-terminated.
//!
//! Every result is an envelope, so a caller never has to guess:
//!
//! ```json
//! {"ok": <the value>}
//! {"error": {"kind": "status", "status": 404, "message": "Kick answered 404"}}
//! ```
//!
//! Strings returned here belong to the library: hand each back to
//! [`kicksdk_free_string`]. A session from [`kicksdk_session_new`] goes back
//! to [`kicksdk_session_free`]. A panic never crosses the boundary; it comes
//! back as an error envelope of kind `panic`.
//!
//! `include/kicksdk.h` declares the same for C.

use kicksdk::dispatch::{self, SessionHandle};
use serde_json::{Value, json};
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Bumped when a function here changes shape; bindings check it on load.
pub const ABI_VERSION: u32 = 1;

fn read(ptr: *const c_char) -> Result<String, String> {
    if ptr.is_null() {
        return Ok(String::new());
    }
    // SAFETY: the caller passes a NUL-terminated string it owns for the call.
    unsafe { CStr::from_ptr(ptr) }.to_str().map(str::to_owned).map_err(|_| "argument is not UTF-8".to_owned())
}

fn args(ptr: *const c_char) -> Result<Value, String> {
    let text = read(ptr)?;
    if text.is_empty() { Ok(Value::Null) } else { serde_json::from_str(&text).map_err(|e| format!("arguments: {e}")) }
}

fn envelope(result: Result<Value, dispatch::DispatchError>) -> Value {
    match result {
        Ok(v) => json!({ "ok": v }),
        Err(e) => json!({ "error": e }),
    }
}

fn fail(kind: &str, message: String) -> Value {
    json!({ "error": { "kind": kind, "status": null, "message": message } })
}

fn give(v: Value) -> *mut c_char {
    // serde_json never writes a NUL byte unescaped, so this cannot fail.
    CString::new(v.to_string()).map(CString::into_raw).unwrap_or(std::ptr::null_mut())
}

fn guarded(f: impl FnOnce() -> Value) -> *mut c_char {
    give(catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| fail("panic", "kicksdk panicked".into())))
}

/// The ABI version, [`ABI_VERSION`].
#[unsafe(no_mangle)]
pub extern "C" fn kicksdk_abi_version() -> u32 {
    ABI_VERSION
}

/// Runs one function by name with JSON arguments; returns a JSON envelope.
///
/// # Safety
/// `name` and `args_json` are NUL-terminated UTF-8 strings (or `args_json`
/// null for no arguments).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kicksdk_call(name: *const c_char, args_json: *const c_char) -> *mut c_char {
    guarded(|| match (read(name), args(args_json)) {
        (Ok(name), Ok(args)) => envelope(dispatch::call(&name, &args)),
        (Err(e), _) | (_, Err(e)) => fail("call", e),
    })
}

/// A new realtime session for the feeds in `channels_json` (a JSON list of
/// strings). Null if the list cannot be read.
///
/// # Safety
/// `channels_json` is a NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kicksdk_session_new(channels_json: *const c_char) -> *mut SessionHandle {
    let made = catch_unwind(|| {
        let channels: Vec<String> = serde_json::from_str(&read(channels_json).ok()?).ok()?;
        Some(Box::into_raw(Box::new(SessionHandle::new(channels))))
    });
    made.ok().flatten().unwrap_or(std::ptr::null_mut())
}

/// Calls a session method by name (`open`, `receive`, `refresh`, `tick`,
/// `closed`, `deadline`, `is_subscribed`, `is_connected`); returns a JSON
/// envelope.
///
/// # Safety
/// `session` came from [`kicksdk_session_new`] and has not been freed, and is
/// not used from two threads at once. `method` and `args_json` as for
/// [`kicksdk_call`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kicksdk_session_call(
    session: *mut SessionHandle,
    method: *const c_char,
    args_json: *const c_char,
) -> *mut c_char {
    if session.is_null() {
        return give(fail("call", "no session".into()));
    }
    // SAFETY: per the contract above, `session` is live and not shared.
    let s = unsafe { &mut *session };
    guarded(|| match (read(method), args(args_json)) {
        (Ok(method), Ok(args)) => envelope(s.call(&method, &args)),
        (Err(e), _) | (_, Err(e)) => fail("call", e),
    })
}

/// Frees a session.
///
/// # Safety
/// `session` came from [`kicksdk_session_new`] and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kicksdk_session_free(session: *mut SessionHandle) {
    if !session.is_null() {
        // SAFETY: per the contract, this is the box `kicksdk_session_new` made.
        drop(unsafe { Box::from_raw(session) });
    }
}

/// Frees a string this library returned.
///
/// # Safety
/// `s` came from this library and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kicksdk_free_string(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: per the contract, this is a CString this library gave out.
        drop(unsafe { CString::from_raw(s) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: &str) -> Value {
        let name = CString::new(name).unwrap();
        let args = CString::new(args).unwrap();
        let out = unsafe { kicksdk_call(name.as_ptr(), args.as_ptr()) };
        let text = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_owned();
        unsafe { kicksdk_free_string(out) };
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn calls_come_back_in_an_envelope() {
        let v = call("content.emote_url", r#"{"id":"7"}"#);
        assert_eq!(v["ok"], "https://files.kick.com/emotes/7/fullsize");
        let v = call("website.parse_channel", r#"{"status":404,"body":"{}"}"#);
        assert_eq!(v["error"]["status"], 404);
        assert_eq!(call("x", "not json")["error"]["kind"], "call");
    }

    #[test]
    fn a_session_lives_and_dies() {
        let channels = CString::new(r#"["a"]"#).unwrap();
        let s = unsafe { kicksdk_session_new(channels.as_ptr()) };
        assert!(!s.is_null());
        let method = CString::new("open").unwrap();
        let args = CString::new(r#"{"token":"t","now":0}"#).unwrap();
        let out = unsafe { kicksdk_session_call(s, method.as_ptr(), args.as_ptr()) };
        let v: Value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
        assert_eq!(v["ok"][0]["action"], "send");
        unsafe {
            kicksdk_free_string(out);
            kicksdk_session_free(s);
        }
        let bad = CString::new("nope").unwrap();
        assert!(unsafe { kicksdk_session_new(bad.as_ptr()) }.is_null());
    }
}
