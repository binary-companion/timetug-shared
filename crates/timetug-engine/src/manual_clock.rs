//! Port of `ManualEngineClock.swift` (moved from `timetug-engine-c` so the bindings share it).

use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::clock::EngineClock;
use async_trait::async_trait;
use calendar_core::failure::{Failure, Outcome};
use calendar_core::time::add_seconds;
use jiff::Timestamp;
use tokio::sync::oneshot;

struct Waiter {
    id: u64,
    deadline: Timestamp,
    wake: oneshot::Sender<()>,
}

struct State {
    current: Timestamp,
    waiters: Vec<Waiter>,
    last_id: u64,
}

/// The clock of a `testing: true` engine: time moves only when the host sends `test.advanceClock`. Sleepers whose
/// deadline the clock reaches wake in deadline order. Shipped in the library (not a test target) because the tested
/// library is the one that ships; without `testing` the engine uses `SystemClock` and the command is rejected.
///
/// Swift tracked sleeps that were issued but not yet registered, to cancel them early. A Rust sleep registers before
/// its first suspension point, and a sleep whose future is dropped (a cancelled task) removes its waiter in a drop
/// guard (conventions section 10, item 11), so there is nothing to track.
pub struct ManualEngineClock {
    state: Mutex<State>,
}

/// Removes the waiter of a sleep that is dropped before it was woken.
struct SleepGuard<'a> {
    clock: &'a ManualEngineClock,
    id: u64,
}

impl Drop for SleepGuard<'_> {
    fn drop(&mut self) {
        self.clock.cancel(self.id);
    }
}

impl ManualEngineClock {
    /// Creates a clock that reads `start` until it is advanced.
    pub fn new(start: Timestamp) -> ManualEngineClock {
        ManualEngineClock {
            state: Mutex::new(State {
                current: start,
                waiters: Vec::new(),
                last_id: 0,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sleeps that have not woken yet.
    pub fn pending_sleeps(&self) -> usize {
        self.lock().waiters.len()
    }

    /// The cancellation handler's work: forgets sleep `id` if it is still waiting. One that already woke needs nothing.
    pub fn cancel(&self, id: u64) {
        self.lock().waiters.retain(|waiter| waiter.id != id);
    }

    /// Moves time forward by `seconds` (never backward) and wakes every sleeper whose deadline has passed.
    pub fn advance(&self, seconds: f64) {
        let due: Vec<Waiter> = {
            let mut state = self.lock();
            // `max(0, seconds)`: a negative or NaN step moves nothing.
            let step = if seconds > 0.0 { seconds } else { 0.0 };
            state.current = add_seconds(state.current, step);
            let current = state.current;
            let (mut due, kept): (Vec<Waiter>, Vec<Waiter>) = std::mem::take(&mut state.waiters)
                .into_iter()
                .partition(|waiter| waiter.deadline <= current);
            state.waiters = kept;
            due.sort_by_key(|waiter| waiter.deadline);
            due
        };
        for waiter in due {
            // A closed receiver is a sleeper that was dropped in the meantime.
            let _ = waiter.wake.send(());
        }
    }
}

#[async_trait]
impl EngineClock for ManualEngineClock {
    fn now(&self) -> Timestamp {
        self.lock().current
    }

    async fn sleep_until(&self, deadline: Timestamp) -> Outcome<()> {
        let (id, woken) = {
            let mut state = self.lock();
            if deadline <= state.current {
                return Ok(());
            }
            state.last_id = state.last_id.wrapping_add(1);
            let id = state.last_id;
            let (wake, woken) = oneshot::channel();
            state.waiters.push(Waiter { id, deadline, wake });
            (id, woken)
        };
        let _guard = SleepGuard { clock: self, id };
        // The sender is dropped only with its waiter, which only this sleep's guard or `advance` removes.
        woken.await.map_err(|_| Failure::Cancelled)
    }
}
