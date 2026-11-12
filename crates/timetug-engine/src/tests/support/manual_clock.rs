//! Port of `Support/ManualClock.swift`: a clock tests drive by hand. Sleepers wake only when `advance` passes their
//! deadline.
//!
//! Swift cancels a sleeper through `Task.cancel()`, which makes `sleep(until:)` throw `CancellationError`. A Rust
//! future that is dropped (a task that is aborted) just stops, so here dropping the sleeping future removes its waiter
//! (the drop-guard rule of conventions section 10, item 11), and `cancel_sleepers` makes every sleeper return
//! `Err(Failure::Cancelled)` for tests that need the error itself.

use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use calendar_core::failure::{Failure, Outcome};
use calendar_core::time::add_seconds;
use jiff::Timestamp;
use tokio::sync::oneshot;

use crate::clock::EngineClock;

use super::poll;

struct Waiter {
    id: u64,
    deadline: Timestamp,
    wake: oneshot::Sender<Outcome<()>>,
}

struct State {
    current: Timestamp,
    waiters: Vec<Waiter>,
    next_id: u64,
    resumed: usize,
}

pub(crate) struct ManualClock {
    state: Mutex<State>,
}

/// Removes the waiter of a sleep that is dropped before it was woken.
struct WaiterGuard<'a> {
    clock: &'a ManualClock,
    id: u64,
}

impl Drop for WaiterGuard<'_> {
    fn drop(&mut self) {
        self.clock.lock().waiters.retain(|w| w.id != self.id);
    }
}

impl ManualClock {
    pub(crate) fn new(start: Timestamp) -> ManualClock {
        ManualClock {
            state: Mutex::new(State {
                current: start,
                waiters: Vec::new(),
                next_id: 0,
                resumed: 0,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn pending_sleeps(&self) -> usize {
        self.lock().waiters.len()
    }

    /// Sleeps that returned normally (deadline reached), as opposed to cancelled ones.
    pub(crate) fn resumed_sleeps(&self) -> usize {
        self.lock().resumed
    }

    fn move_forward(&self, interval: f64) {
        let due: Vec<Waiter> = {
            let mut state = self.lock();
            state.current = add_seconds(state.current, interval);
            let current = state.current;
            let (due, waiting) = std::mem::take(&mut state.waiters)
                .into_iter()
                .partition(|w| w.deadline <= current);
            state.waiters = waiting;
            state.resumed += due.len();
            due
        };
        for waiter in due {
            let _ = waiter.wake.send(Ok(()));
        }
    }

    /// Synchronous variant for code that must move time from inside a non-async callback.
    pub(crate) fn advance_now(&self, interval: f64) {
        self.move_forward(interval);
    }

    pub(crate) async fn advance(&self, interval: f64) {
        self.move_forward(interval);
        for _ in 0..3 {
            tokio::task::yield_now().await;
        }
    }

    /// Makes every current sleeper return `Err(Failure::Cancelled)`.
    pub(crate) fn cancel_sleepers(&self) {
        let waiters = std::mem::take(&mut self.lock().waiters);
        for waiter in waiters {
            let _ = waiter.wake.send(Err(Failure::Cancelled));
        }
    }

    pub(crate) async fn wait_for_sleepers(&self, count: usize) {
        poll(
            || format!("{count} sleeper(s); {} pending", self.pending_sleeps()),
            30.0,
            || async { self.pending_sleeps() >= count },
        )
        .await;
    }
}

#[async_trait]
impl EngineClock for ManualClock {
    fn now(&self) -> Timestamp {
        self.lock().current
    }

    async fn sleep_until(&self, deadline: Timestamp) -> Outcome<()> {
        let (id, receiver) = {
            let mut state = self.lock();
            if deadline <= state.current {
                state.resumed += 1;
                return Ok(());
            }
            let id = state.next_id;
            state.next_id += 1;
            let (wake, receiver) = oneshot::channel();
            state.waiters.push(Waiter { id, deadline, wake });
            (id, receiver)
        };
        let _guard = WaiterGuard { clock: self, id };
        match receiver.await {
            Ok(outcome) => outcome,
            Err(_) => Err(Failure::Cancelled),
        }
    }
}
