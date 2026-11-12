//! Port of `EngineClockTests.swift`, plus the Rust-only tests of Review Focus 5 (a long suspend) and of the chunked
//! sleep.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::time::Duration;

use calendar_core::failure::Failure;
use calendar_core::time::{add_seconds, seconds_between};
use jiff::Timestamp;

use super::support::{LockedBox, ManualClock, ts};
use crate::clock::{EngineClock, SystemClock};

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn manual_clock_wakes_sleepers_only_when_deadline_passes() {
    let clock = Arc::new(ManualClock::new(ts(1_000)));
    let woke = Arc::new(LockedBox::new(false));
    let task = tokio::spawn({
        let (clock, woke) = (Arc::clone(&clock), Arc::clone(&woke));
        async move {
            clock.sleep_until(ts(1_060)).await.unwrap();
            woke.set(true);
        }
    });
    clock.wait_for_sleepers(1).await;
    clock.advance(30.0).await;
    assert!(!woke.get());
    clock.advance(30.0).await;
    task.await.unwrap();
    assert!(woke.get());
    assert_eq!(clock.now(), ts(1_060));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn manual_clock_returns_immediately_for_past_deadline() {
    let clock = ManualClock::new(ts(1_000));
    clock.sleep_until(ts(900)).await.unwrap();
}

/// Swift cancels the sleeping task and the sleep throws `CancellationError`; here `cancel_sleepers` makes the sleep
/// return `Err(Failure::Cancelled)`.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn cancelled_sleep_throws() {
    let clock = Arc::new(ManualClock::new(ts(0)));
    let task = tokio::spawn({
        let clock = Arc::clone(&clock);
        async move { clock.sleep_until(ts(100)).await }
    });
    clock.wait_for_sleepers(1).await;
    clock.cancel_sleepers();
    assert_eq!(task.await.unwrap(), Err(Failure::Cancelled));
    assert_eq!(clock.pending_sleeps(), 0);
    assert_eq!(clock.resumed_sleeps(), 0);
}

/// Rust-only: a sleep whose task is aborted (the engine's `JoinHandle::abort`) leaves no waiter behind.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn an_aborted_sleep_removes_its_waiter() {
    let clock = Arc::new(ManualClock::new(ts(0)));
    let task = tokio::spawn({
        let clock = Arc::clone(&clock);
        async move { clock.sleep_until(ts(100)).await }
    });
    clock.wait_for_sleepers(1).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(clock.pending_sleeps(), 0);
    clock.advance(200.0).await;
    assert_eq!(clock.resumed_sleeps(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn system_clock_sleeps_until_deadline() {
    let clock = SystemClock;
    let start = Timestamp::now();
    clock.sleep_until(add_seconds(start, 0.05)).await.unwrap();
    assert!(seconds_between(start, Timestamp::now()) >= 0.04);
}

/// Swift sleeps until `.distantFuture`; here `Timestamp::MAX` (the remaining time is about 2.5e11 s, and the sleep
/// is in chunks of at most a day).
#[tokio::test(flavor = "current_thread")]
async fn system_clock_does_not_trap_for_far_future_deadline() {
    let task = tokio::spawn(async { SystemClock.sleep_until(Timestamp::MAX).await });
    tokio::time::sleep(Duration::from_millis(20)).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

/// A wall clock that follows the paused `tokio` clock plus a jump, standing in for the machine's clock.
struct FakeWall {
    base: Timestamp,
    started: tokio::time::Instant,
    jump_seconds: AtomicI64,
    reads: AtomicUsize,
}

impl FakeWall {
    fn new() -> Arc<FakeWall> {
        Arc::new(FakeWall {
            base: ts(1_800_000_000),
            started: tokio::time::Instant::now(),
            jump_seconds: AtomicI64::new(0),
            reads: AtomicUsize::new(0),
        })
    }

    fn now(&self) -> Timestamp {
        self.reads.fetch_add(1, Ordering::SeqCst);
        add_seconds(
            self.base,
            self.started.elapsed().as_secs_f64() + self.jump_seconds.load(Ordering::SeqCst) as f64,
        )
    }
}

/// Review Focus 5: the machine sleeps for hours. A paused `tokio` clock advances 26 hours in one step while a 26-hour
/// wait is in its first chunk. The wall clock (which moves with the tokio clock here, as it does on a running machine)
/// then shows the deadline reached: the sleeper wakes once, late by nothing more than the step, without a second sleep,
/// a trap or a loop.
///
/// `SystemClock` itself reads `Timestamp::now()`, which a paused `tokio` clock does not move, so the test drives the
/// same chunked loop (`sleep_in_chunks`) that `SystemClock::sleep_until` calls, over a wall clock that follows the
/// paused clock.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn system_clock_sleep_survives_a_26_hour_step_of_the_tokio_clock() {
    let wall = FakeWall::new();
    let deadline = add_seconds(wall.base, 26.0 * 3600.0);
    let woke = Arc::new(AtomicUsize::new(0));
    let task = tokio::spawn({
        let (wall, woke) = (Arc::clone(&wall), Arc::clone(&woke));
        async move {
            let result = crate::clock::sleep_in_chunks(move || wall.now(), deadline).await;
            woke.fetch_add(1, Ordering::SeqCst);
            result
        }
    });
    tokio::task::yield_now().await;
    assert_eq!(woke.load(Ordering::SeqCst), 0);
    tokio::time::advance(Duration::from_secs(26 * 3600)).await;
    assert_eq!(task.await.unwrap(), Ok(()));
    assert_eq!(woke.load(Ordering::SeqCst), 1);
    assert_eq!(wall.started.elapsed(), Duration::from_secs(26 * 3600));
    // One read before the first chunk, one after it: the second showed the deadline passed, so no second sleep.
    assert_eq!(wall.reads.load(Ordering::SeqCst), 2);
}

/// Review Focus 5: the wall clock jumps 60 hours (a long suspend) while a 72-hour wait is in its first 24-hour chunk.
/// The sleeper notices at the end of that chunk, not 48 hours later.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_wall_clock_jump_ends_the_sleep_at_the_next_chunk() {
    let wall = FakeWall::new();
    let deadline = add_seconds(wall.base, 72.0 * 3600.0);
    let task = tokio::spawn({
        let wall = Arc::clone(&wall);
        async move { crate::clock::sleep_in_chunks(move || wall.now(), deadline).await }
    });
    tokio::task::yield_now().await;
    wall.jump_seconds.store(60 * 3600, Ordering::SeqCst);
    tokio::time::advance(Duration::from_secs(24 * 3600)).await;
    assert_eq!(task.await.unwrap(), Ok(()));
    assert_eq!(wall.started.elapsed(), Duration::from_secs(24 * 3600));
    assert_eq!(wall.reads.load(Ordering::SeqCst), 2);
}

/// The wait is cut into chunks of at most a day, re-reading the clock each time: ten days take ten chunks.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_long_wait_is_slept_in_chunks_of_at_most_a_day() {
    let wall = FakeWall::new();
    let deadline = add_seconds(wall.base, 10.0 * 86_400.0);
    let sleeper = tokio::spawn({
        let wall = Arc::clone(&wall);
        async move { crate::clock::sleep_in_chunks(move || wall.now(), deadline).await }
    });
    assert_eq!(sleeper.await.unwrap(), Ok(()));
    assert_eq!(wall.started.elapsed(), Duration::from_secs(10 * 86_400));
    assert_eq!(wall.reads.load(Ordering::SeqCst), 11);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_past_deadline_returns_at_once() {
    let started = tokio::time::Instant::now();
    SystemClock
        .sleep_until(add_seconds(Timestamp::now(), -5.0))
        .await
        .unwrap();
    assert_eq!(started.elapsed(), Duration::ZERO);
}
