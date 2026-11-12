//! Port of `EngineClock.swift`: the only place the engine reads time.

use std::time::Duration;

use async_trait::async_trait;
use calendar_core::failure::Outcome;
use calendar_core::time::seconds_between;
use jiff::Timestamp;

/// The only place the engine reads time. Hosts normally do not supply one; tests supply a manual clock.
#[async_trait]
pub trait EngineClock: Send + Sync {
    fn now(&self) -> Timestamp;
    /// Returns when `now()` reaches `deadline`; returns at once for a past deadline; `Err(Failure::Cancelled)` if
    /// cancelled.
    ///
    /// A Rust future that is dropped (a task that is aborted) just stops; an implementation that keeps state for a
    /// sleeper removes it in a `Drop` guard (conventions section 10, item 11).
    async fn sleep_until(&self, deadline: Timestamp) -> Outcome<()>;
}

/// The longest single sleep: a chunk is at most a day, so a clock change or a long suspend is noticed at the latest one
/// chunk later.
const MAX_CHUNK_SECONDS: f64 = 86_400.0;

/// The wall clock. Sleeps in chunks of at most 86,400 s, re-reading `now()` each chunk. It is the only code of the
/// workspace that calls `tokio::time` (`scripts/ci/check-no-clock.py`).
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

#[async_trait]
impl EngineClock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    async fn sleep_until(&self, deadline: Timestamp) -> Outcome<()> {
        sleep_in_chunks(Timestamp::now, deadline).await
    }
}

/// `SystemClock.sleep(until:)` over any wall clock `now`. Swift's comment: sleep in bounded chunks, because converting a
/// huge interval to nanoseconds would trap; here `Duration::from_secs_f64` would panic for the same reason.
pub(crate) async fn sleep_in_chunks(
    now: impl Fn() -> Timestamp + Send,
    deadline: Timestamp,
) -> Outcome<()> {
    loop {
        let seconds = seconds_between(now(), deadline);
        if seconds <= 0.0 {
            return Ok(());
        }
        let chunk = seconds.min(MAX_CHUNK_SECONDS);
        tokio::time::sleep(Duration::from_secs_f64(chunk)).await;
    }
}
