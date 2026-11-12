//! Port of `CInterface.swift`: the four exported functions of `include/timetug_engine.h`. With `delivery.rs`, one of the
//! two files that contain `unsafe`. Every function tolerates NULL arguments and unknown handles.
#![allow(unsafe_code)]

use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::Arc;

use crate::delivery::{HostCallback, HostFunction};
use crate::engine_session::EngineSession;
use crate::handle_registry::HandleRegistry;

/// `tt_engine`: the opaque type the header declares. A value of this type never exists; a handle is a token cast to a
/// pointer to it (see `HandleRegistry`).
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct tt_engine {
    _private: [u8; 0],
}

/// The only process-wide state: the live sessions by handle.
static REGISTRY: HandleRegistry<EngineSession> = HandleRegistry::new();

/// A NUL-terminated C string as owned text; invalid UTF-8 becomes U+FFFD. Copied, so the host may free it at once.
///
/// # Safety
///
/// `text` is non-null and points to a NUL-terminated string that stays valid for the call.
unsafe fn copy_string(text: *const c_char) -> String {
    // SAFETY: the caller guarantees a valid NUL-terminated string.
    unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned()
}

/// Like `copy_string`, but NULL is the empty string (Swift: `commandJSON.map { String(cString: $0) } ?? ""`).
///
/// # Safety
///
/// `text` is null or as for `copy_string`.
unsafe fn copy_optional_string(text: *const c_char) -> String {
    if text.is_null() {
        String::new()
    } else {
        // SAFETY: non-null, and the caller guarantees the rest.
        unsafe { copy_string(text) }
    }
}

/// Builds an engine (not started) from a UTF-8 JSON config. Returns NULL, without calling `on_message`, for a config
/// that is not valid JSON or fails validation, or when `on_message` is NULL.
///
/// # Safety
///
/// `config_json` is NULL or a NUL-terminated string valid for the call. `on_message`, when not NULL, must be callable
/// with `context` and a NUL-terminated string from any thread until `tt_engine_destroy` returns (outside a callback)
/// or the callback that called it returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_engine_create(
    config_json: *const c_char,
    on_message: Option<HostFunction>,
    context: *mut c_void,
) -> *mut tt_engine {
    if config_json.is_null() {
        return ptr::null_mut();
    }
    let Some(function) = on_message else {
        return ptr::null_mut();
    };
    // SAFETY: non-null, NUL-terminated and valid for the call per this function's contract.
    let config = unsafe { copy_string(config_json) };
    // SAFETY: the contract of this function is exactly the one `HostCallback::new` requires.
    let callback = unsafe { HostCallback::new(function, context) };
    let Some(session) = EngineSession::make(&config, Arc::new(callback)) else {
        return ptr::null_mut();
    };
    ptr::without_provenance_mut(REGISTRY.insert(session))
}

/// Enqueues a command (copied before returning). Never blocks; safe from any thread and from inside `on_message`.
///
/// # Safety
///
/// `engine` is any value (NULL, stale and unknown handles are ignored). `command_json` is NULL or a NUL-terminated
/// string valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_engine_send(engine: *mut tt_engine, command_json: *const c_char) {
    let Some(session) = REGISTRY.session(engine.addr()) else {
        return;
    };
    // SAFETY: null or a valid NUL-terminated string per this function's contract.
    let text = unsafe { copy_optional_string(command_json) };
    session.send(text);
}

/// Answers the service request `request_id` (copied before returning). Unknown or already answered ids are ignored.
///
/// # Safety
///
/// As for `tt_engine_send`, with `reply_json` in place of `command_json`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_engine_reply(
    engine: *mut tt_engine,
    request_id: u64,
    reply_json: *const c_char,
) {
    let Some(session) = REGISTRY.session(engine.addr()) else {
        return;
    };
    // SAFETY: null or a valid NUL-terminated string per this function's contract.
    let text = unsafe { copy_optional_string(reply_json) };
    session.reply(request_id, &text);
}

/// Stops and releases the engine. Outside a callback: no callback runs after it returns. Inside one: returns at once and
/// no further callback starts. A NULL, unknown or already destroyed handle is ignored.
///
/// # Safety
///
/// `engine` is any value (NULL, stale and unknown handles are ignored).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_engine_destroy(engine: *mut tt_engine) {
    if let Some(session) = REGISTRY.remove(engine.addr()) {
        session.shutdown();
    }
}

/// The four functions for a Rust host: the tests of this crate, and `tests/scenarios.rs`, which cannot use `unsafe`.
/// Each call goes through the exported function itself, with C strings and a C callback; the callback context is a
/// leaked `Arc` of the sink, so it stays valid for any callback that outlives `destroy` (one that called it).
#[doc(hidden)]
pub mod rust_host {
    use std::ffi::{CString, c_char, c_void};
    use std::sync::Arc;

    use super::{tt_engine, tt_engine_create, tt_engine_destroy, tt_engine_reply, tt_engine_send};

    pub type Sink = dyn Fn(&str) + Send + Sync;

    /// A handle as a plain value (a token, possibly NULL, stale or made up).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Handle(usize);

    impl Handle {
        pub const NULL: Handle = Handle(0);

        pub fn from_raw(value: usize) -> Handle {
            Handle(value)
        }

        fn pointer(self) -> *mut tt_engine {
            std::ptr::without_provenance_mut(self.0)
        }
    }

    unsafe extern "C" fn trampoline(context: *mut c_void, message: *const c_char) {
        if context.is_null() || message.is_null() {
            return;
        }
        // SAFETY: `context` is the leaked `Box<Arc<Sink>>` made by `create`, never freed; `message` is a NUL-terminated
        // string valid for this call, as the engine guarantees.
        let (sink, text) = unsafe {
            (
                &*context.cast::<Arc<Sink>>(),
                std::ffi::CStr::from_ptr(message).to_string_lossy(),
            )
        };
        sink(&text);
    }

    /// A C string from bytes; a C string cannot hold a NUL, so those bytes are dropped.
    fn c_string(bytes: &[u8]) -> CString {
        let without_nul: Vec<u8> = bytes.iter().copied().filter(|b| *b != 0).collect();
        CString::new(without_nul).unwrap_or_default()
    }

    fn create_with(config: Option<&CString>, sink: Option<Arc<Sink>>) -> Option<Handle> {
        let context: *mut c_void = match sink.clone() {
            Some(sink) => Box::into_raw(Box::new(sink)).cast(),
            None => std::ptr::null_mut(),
        };
        let function = sink.map(|_| trampoline as crate::delivery::HostFunction);
        // SAFETY: `config` is null or a live NUL-terminated string; `trampoline` accepts `context` (the leaked sink)
        // and a NUL-terminated string from any thread (the sink is `Send + Sync`).
        let handle = unsafe {
            tt_engine_create(
                config.map_or(std::ptr::null(), |c| c.as_ptr()),
                function,
                context,
            )
        };
        (!handle.is_null()).then(|| Handle(handle.addr()))
    }

    /// `tt_engine_create` with a config text (`None` is NULL) and a callback (`None` is NULL).
    pub fn create(config: Option<&str>, sink: Option<Arc<Sink>>) -> Option<Handle> {
        create_with(config.map(|c| c_string(c.as_bytes())).as_ref(), sink)
    }

    /// `tt_engine_create` with a config given as bytes (possibly not UTF-8).
    pub fn create_bytes(config: &[u8], sink: Arc<Sink>) -> Option<Handle> {
        create_with(Some(&c_string(config)), Some(sink))
    }

    /// `tt_engine_send` with a command text (`None` is NULL).
    pub fn send(handle: Handle, command: Option<&str>) {
        let text = command.map(|c| c_string(c.as_bytes()));
        // SAFETY: `handle` is only a value; the string is null or live and NUL-terminated for the call.
        unsafe {
            tt_engine_send(
                handle.pointer(),
                text.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            );
        }
    }

    /// `tt_engine_send` with arbitrary bytes.
    pub fn send_bytes(handle: Handle, command: &[u8]) {
        let text = c_string(command);
        // SAFETY: `text` is a live NUL-terminated string for the call.
        unsafe { tt_engine_send(handle.pointer(), text.as_ptr()) };
    }

    /// `tt_engine_reply` with a reply text (`None` is NULL).
    pub fn reply(handle: Handle, request_id: u64, reply: Option<&str>) {
        let text = reply.map(|c| c_string(c.as_bytes()));
        // SAFETY: as for `send`.
        unsafe {
            tt_engine_reply(
                handle.pointer(),
                request_id,
                text.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            );
        }
    }

    /// `tt_engine_reply` with arbitrary bytes.
    pub fn reply_bytes(handle: Handle, request_id: u64, reply: &[u8]) {
        let text = c_string(reply);
        // SAFETY: `text` is a live NUL-terminated string for the call.
        unsafe { tt_engine_reply(handle.pointer(), request_id, text.as_ptr()) };
    }

    pub fn destroy(handle: Handle) {
        // SAFETY: `handle` is only a value.
        unsafe { tt_engine_destroy(handle.pointer()) };
    }
}
