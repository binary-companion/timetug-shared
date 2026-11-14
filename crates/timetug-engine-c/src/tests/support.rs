//! Port of the helpers of `Support.swift` that the table and service tests use (`LockedBox`, `poll`). The tests of the
//! session and the C functions have their own support in `engine_support.rs`.

use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// A value shared between a test and the tasks it observes (Swift's `LockedBox`).
pub(crate) struct LockedBox<T> {
    stored: Mutex<T>,
}

impl<T> LockedBox<T> {
    pub(crate) fn new(value: T) -> Arc<LockedBox<T>> {
        Arc::new(LockedBox {
            stored: Mutex::new(value),
        })
    }

    fn lock(&self) -> MutexGuard<'_, T> {
        self.stored.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn mutate<R>(&self, change: impl FnOnce(&mut T) -> R) -> R {
        change(&mut self.lock())
    }
}

impl<T: Clone> LockedBox<T> {
    pub(crate) fn value(&self) -> T {
        self.lock().clone()
    }
}

/// Waits (in real time) until `condition` holds; panics after `timeout` seconds. Swift's `poll` records an issue and
/// returns false; every caller then stops the test, which a panic does.
pub(crate) async fn poll_for(what: &str, timeout: f64, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs_f64(timeout);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timed out after {timeout} s waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// `poll` with the default timeout of 10 seconds.
pub(crate) async fn poll(what: &str, condition: impl FnMut() -> bool) {
    poll_for(what, 10.0, condition).await;
}

/// Spawns `future` and returns its handle (the tests' `Task { ... }`).
pub(crate) fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(future)
}
