//! Port of `Delivery.swift`: `HostCallback`, `Delivery` and `Outbox`. With `c_interface.rs`, one of the two files that
//! contain `unsafe`: the call of the host's callback.
#![allow(unsafe_code)]

use std::ffi::{CString, c_char, c_void};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use timetug_engine::host_delivery::{self, DeliverySink};

/// The host's `tt_message_fn`: `void (*)(void *context, const char *message_json)`.
pub type HostFunction = unsafe extern "C" fn(*mut c_void, *const c_char);

/// What a message is delivered to. The host's callback is the production sink ([`HostCallback`]); tests record.
pub trait MessageSink: Send + Sync {
    /// Delivers one complete JSON message. Called on the delivery thread, one message at a time, in order.
    fn deliver(&self, message: &str);
}

/// The `event.diagnostic` that replaces a message holding a NUL byte, which a C string cannot carry. The engine's own
/// JSON escapes every control character, so this never happens for a message the engine encodes; it is a last guard.
const NUL_DIAGNOSTIC: &str = r#"{"v":1,"type":"event.diagnostic","payload":{"level":"error","category":"delivery","message":"a message held a NUL byte and was not delivered"}}"#;

/// The host's `tt_message_fn` and its context pointer. The engine never reads the context; it hands it back.
pub struct HostCallback {
    function: HostFunction,
    context: *mut c_void,
}

// SAFETY: the pointer is an opaque token owned by the host. The engine never dereferences it, only passes it back to the
// host's own function, on the delivery thread, as the C header specifies; whether the host's function and context may be
// used from that thread is the host's contract (the header: "called on a thread of the engine").
unsafe impl Send for HostCallback {}
// SAFETY: as for `Send`: the engine shares the pair between the session and its delivery thread but never reads through
// the pointer, and `deliver` only reads the two fields.
unsafe impl Sync for HostCallback {}

impl HostCallback {
    /// # Safety
    ///
    /// `function` must be callable with `context` and a NUL-terminated string that is valid for the duration of the
    /// call, from any thread, until the engine is destroyed (the contract of `tt_engine_create`).
    pub unsafe fn new(function: HostFunction, context: *mut c_void) -> HostCallback {
        HostCallback { function, context }
    }
}

impl MessageSink for HostCallback {
    fn deliver(&self, message: &str) {
        let text = CString::new(message)
            .or_else(|_| CString::new(NUL_DIAGNOSTIC))
            .unwrap_or_default();
        // SAFETY: `HostCallback::new` obliges the host's function to accept `context` and a NUL-terminated string valid
        // for the call. `text` is a live `CString` for the whole call, so `as_ptr()` is non-null, NUL-terminated and
        // valid until `text` drops after the callback returns.
        unsafe { (self.function)(self.context, text.as_ptr()) };
    }
}

/// Adapts a text sink to the shared queue's sink.
struct TextSink(Arc<dyn MessageSink>);

impl DeliverySink<String> for TextSink {
    fn deliver(&self, message: String) {
        self.0.deliver(&message);
    }
}

/// Delivers messages to the host on one dedicated thread, one at a time, in order: the shared queue
/// (`timetug_engine::host_delivery::Delivery`, whose documentation has the rules) carrying JSON text.
pub struct Delivery {
    inner: Arc<host_delivery::Delivery<String>>,
}

impl Delivery {
    /// Creates a delivery that passes messages to `sink`. No thread runs until [`Delivery::start`].
    pub fn new(sink: Arc<dyn MessageSink>) -> Arc<Delivery> {
        Arc::new(Delivery {
            inner: host_delivery::Delivery::new(Arc::new(TextSink(sink))),
        })
    }

    /// Starts the delivery thread. Called once, after the session is fully built. Fails when the system cannot start a
    /// thread.
    pub fn start(self: &Arc<Delivery>) -> std::io::Result<()> {
        self.inner.start()
    }

    /// Queues an event or service request. It is never dropped or reordered; ignored once closed.
    pub fn enqueue(&self, message: String) {
        self.inner.enqueue(message);
    }

    /// Queues a state message, replacing a state still waiting (coalescing); ignored once closed.
    pub fn enqueue_state(&self, message: String) {
        self.inner.enqueue_state(message);
    }

    /// True on the delivery thread, that is inside one of this engine's callbacks.
    pub fn is_delivery_thread(&self) -> bool {
        self.inner.is_delivery_thread()
    }

    /// Stops delivery; see the shared queue's documentation. Idempotent.
    pub fn close(&self, wait_for_callback: bool) {
        self.inner.close(wait_for_callback);
    }
}

struct OutboxState {
    open: bool,
    held: Vec<String>,
}

/// Holds every outgoing message until `ready` has been delivered (the spec makes `ready` the first callback), then
/// passes them to `Delivery` in order. Diagnostics from building the engine (an unreadable settings file) therefore
/// arrive right after `ready`.
pub struct Outbox {
    delivery: Arc<Delivery>,
    state: Mutex<OutboxState>,
}

impl Outbox {
    /// Creates a closed outbox in front of `delivery`; messages are held until [`Outbox::open_with`].
    pub fn new(delivery: Arc<Delivery>) -> Outbox {
        Outbox {
            delivery,
            state: Mutex::new(OutboxState {
                open: false,
                held: Vec::new(),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, OutboxState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sends `message` on in order once the outbox is open; before that it is held.
    pub fn post(&self, message: String) {
        let mut state = self.lock();
        if state.open {
            self.delivery.enqueue(message);
        } else {
            state.held.push(message);
        }
    }

    /// States are only produced after `ready` (the state pump starts with it), so they never wait here.
    pub fn post_state(&self, message: String) {
        let state = self.lock();
        if state.open {
            self.delivery.enqueue_state(message);
        }
    }

    /// Delivers `ready`, then everything held. Returns false (and does nothing) if it already ran.
    pub fn open_with(&self, ready: String) -> bool {
        let mut state = self.lock();
        if state.open {
            return false;
        }
        state.open = true;
        self.delivery.enqueue(ready);
        for message in std::mem::take(&mut state.held) {
            self.delivery.enqueue(message);
        }
        true
    }
}
