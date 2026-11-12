//! Ports of `TimeTugEngineTests/Support/ManualClock.swift` and `Support/Fakes.swift`, plus small helpers the Rust tests
//! share (`ts`, `standup`). `account_fakes` is `Support/AccountFakes.swift` (phase 3 task 2). `engine_harness`, `scripted_source` and `timeline_recorder` are `Support/EngineHarness.swift` and `Support/TimelineRecorder.swift` (phase 3 task 4). Items that only later tasks use stay, as in the Swift files.
#![allow(dead_code)]

pub(crate) mod account_fakes;
pub(crate) mod engine_harness;
mod fakes;
pub(crate) mod manual_clock;
pub(crate) mod scripted_source;
pub(crate) mod timeline_recorder;

pub(crate) use fakes::{Fixtures, LockedBox, TempDir, poll};
pub(crate) use manual_clock::ManualClock;

use calendar_core::model::CalendarEvent;
use jiff::Timestamp;
use timetug_core::TimeTugCalendarEvent;

/// `Date(timeIntervalSince1970: seconds)`.
pub(crate) fn ts(unix_seconds: i64) -> Timestamp {
    Timestamp::from_second(unix_seconds).unwrap()
}

/// The event the Swift persistence and scheduling tests build: id `id`, calendar `c`, "Standup", 30 minutes from
/// Unix 1_800_000_000, source `s`.
pub(crate) fn standup(id: &str) -> TimeTugCalendarEvent {
    let start = ts(1_800_000_000);
    TimeTugCalendarEvent::new(
        CalendarEvent::new(id, "c", "Standup", start, ts(1_800_001_800)),
        "s",
    )
}
