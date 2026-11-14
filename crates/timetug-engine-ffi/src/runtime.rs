//! What the engine object shares with the C interface's session: one engine thread with a `tokio` current-thread runtime,
//! one delivery thread (`timetug_engine::host_delivery`), and the signal that says the engine thread has ended.
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// A one-shot signal a thread can wait on with a timeout.
#[derive(Default)]
pub struct Signal {
    fired: Mutex<bool>,
    changed: Condvar,
}

impl Signal {
    /// Creates a signal that has not fired.
    pub fn new() -> Signal {
        Signal::default()
    }

    /// Fires the signal and wakes every waiter. It stays fired.
    pub fn fire(&self) {
        *self.fired.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.changed.notify_all();
    }

    /// Returns true when fired, false after `timeout`.
    pub fn wait(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut fired = self.fired.lock().unwrap_or_else(PoisonError::into_inner);
        while !*fired {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            fired = self
                .changed
                .wait_timeout(fired, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}

/// Fires its signal when dropped, however the engine thread ends.
pub struct FireOnDrop(pub std::sync::Arc<Signal>);

impl Drop for FireOnDrop {
    fn drop(&mut self) {
        self.0.fire();
    }
}

/// How long `shutdown` waits for the engine thread to end (the C interface waits the same).
pub const SHUTDOWN_WAIT: Duration = Duration::from_secs(10);
