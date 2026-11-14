//! Port of `SchedulerTests.swift`, plus the Review Focus 3 tests of the phase 2 plan (a lead time that crosses a DST
//! change or midnight; the scheduler works in elapsed seconds and never reads a zone).

use super::support::{EventSpec, date, make_event, make_event_with, opted_in, recorded_at, zone};
use super::takeover_test_support::{ev_at, ev_minutes, ev_on, ev_starting, ev_with_link};
use crate::takeover::ledger::TakeoverLedger;
use crate::takeover::request::TakeoverRequest;
use crate::takeover::scheduler::Scheduler;
use calendar_core::time::add_seconds;
use jiff::Timestamp;

fn ten_am() -> Timestamp {
    date("2026-09-18T10:00:00Z")
}

#[test]
fn fires_at_start_minus_lead_time() {
    let event = ev_starting("2026-09-18T10:00:00Z");
    let next = Scheduler::next(
        std::slice::from_ref(&event),
        &opted_in(|s| s.lead_time = 120.0),
        &TakeoverLedger::new(),
        date("2026-09-18T09:00:00Z"),
    )
    .unwrap();
    assert_eq!(next.fire_at, date("2026-09-18T09:58:00Z"));
    assert_eq!(next.event, event);
}

#[test]
fn zero_lead_time_fires_at_start() {
    let next = Scheduler::next(
        &[make_event()],
        &opted_in(|s| s.lead_time = 0.0),
        &TakeoverLedger::new(),
        date("2026-09-18T09:00:00Z"),
    )
    .unwrap();
    assert_eq!(next.fire_at, ten_am());
}

#[test]
fn late_fire_is_immediate_while_meeting_in_progress() {
    let now = date("2026-09-18T10:05:00Z");
    let next = Scheduler::next(
        &[make_event()],
        &opted_in(|_| {}),
        &TakeoverLedger::new(),
        now,
    )
    .unwrap();
    assert_eq!(next.fire_at, now);
}

#[test]
fn ended_meetings_are_ignored() {
    let next = Scheduler::next(
        &[make_event()],
        &opted_in(|_| {}),
        &TakeoverLedger::new(),
        date("2026-09-18T10:30:00Z"),
    );
    assert!(next.is_none());
}

#[test]
fn fired_events_do_not_repeat() {
    let event = make_event();
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&event, recorded_at());
    assert!(
        Scheduler::next(
            &[event],
            &opted_in(|_| {}),
            &ledger,
            date("2026-09-18T09:59:30Z")
        )
        .is_none()
    );
}

#[test]
fn rescheduled_event_counts_as_new() {
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&ev_starting("2026-09-18T10:00:00Z"), recorded_at());
    let moved = ev_starting("2026-09-18T11:00:00Z");
    assert!(
        Scheduler::next(
            &[moved],
            &opted_in(|_| {}),
            &ledger,
            date("2026-09-18T09:00:00Z")
        )
        .is_some()
    );
}

#[test]
fn snooze_refires_at_snooze_end() {
    let event = ev_minutes(60);
    let mut ledger = TakeoverLedger::new();
    let now = date("2026-09-18T09:59:00Z");
    ledger.mark_fired(&event, recorded_at());
    ledger.snooze(&event, 300.0, now);
    let next = Scheduler::next(&[event], &opted_in(|_| {}), &ledger, now).unwrap();
    assert_eq!(next.fire_at, date("2026-09-18T10:04:00Z"));
}

#[test]
fn snooze_is_capped_at_meeting_end() {
    let event = ev_minutes(5);
    let mut ledger = TakeoverLedger::new();
    let now = date("2026-09-18T10:03:00Z");
    ledger.snooze(&event, 600.0, now);
    // Capped at 10:05 == end, so nothing left to fire.
    assert!(Scheduler::next(&[event], &opted_in(|_| {}), &ledger, now).is_none());
}

#[test]
fn picks_earliest_qualifying_event() {
    let later = ev_at("late", "2026-09-18T11:00:00Z");
    let sooner = ev_at("soon", "2026-09-18T10:00:00Z");
    let next = Scheduler::next(
        &[later, sooner.clone()],
        &opted_in(|_| {}),
        &TakeoverLedger::new(),
        date("2026-09-18T09:00:00Z"),
    )
    .unwrap();
    assert_eq!(next.event, sooner);
}

#[test]
fn non_qualifying_events_are_skipped() {
    let next = Scheduler::next(
        &[ev_on("family")],
        &opted_in(|_| {}),
        &TakeoverLedger::new(),
        date("2026-09-18T09:00:00Z"),
    );
    assert!(next.is_none());
}

#[test]
fn after_midnight_meeting_fires_before_midnight() {
    let event = ev_starting("2026-09-19T00:05:00Z");
    let next = Scheduler::next(
        &[event],
        &opted_in(|s| s.lead_time = 600.0),
        &TakeoverLedger::new(),
        date("2026-09-18T23:00:00Z"),
    )
    .unwrap();
    assert_eq!(next.fire_at, date("2026-09-18T23:55:00Z"));
}

#[test]
fn ledger_prunes_by_event_end() {
    let done = ev_at("done", "2026-09-18T08:00:00Z");
    let running = ev_at("running", "2026-09-18T10:00:00Z");
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&done, recorded_at());
    ledger.mark_fired(&running, recorded_at());
    ledger.prune(date("2026-09-18T10:10:00Z"));
    assert!(ledger.entry_for(&done).is_none());
    assert!(ledger.entry_for(&running).is_some());
}

#[test]
fn request_marks_started_and_filters_snooze_options() {
    let event = ev_with_link("https://meet.google.com/a-b-c");
    let before = TakeoverRequest::make(&event, date("2026-09-18T09:59:00Z"));
    assert!(!before.has_started);
    assert!(before.join_url.is_some());
    assert_eq!(before.snooze_options, vec![60.0, 300.0, 600.0]);

    let late = TakeoverRequest::make(&event, date("2026-09-18T10:28:00Z"));
    assert!(late.has_started);
    assert_eq!(late.snooze_options, vec![60.0]);
}

#[test]
fn nothing_is_scheduled_while_tug_is_off() {
    let next = Scheduler::next(
        &[make_event()],
        &opted_in(|s| s.enabled = false),
        &TakeoverLedger::new(),
        date("2026-09-18T09:00:00Z"),
    );
    assert!(next.is_none());
}

// Review Focus 3: the lead time is elapsed seconds, not wall-clock time. The vector file pins it for the anchor day;
// these two pin it across a DST change, where a wall-clock subtraction would be off by an hour.

#[test]
fn lead_time_across_us_fall_back_is_elapsed_seconds() {
    // 2026-11-01 in America/New_York: clocks go back at 06:00Z (02:00 EDT -> 01:00 EST). The meeting starts at
    // 07:30Z (02:30 EST); a three hour lead time reaches back across the change to 04:30Z (00:30 EDT): three elapsed
    // hours, though the wall clock moved only two.
    let mut spec = EventSpec::default();
    spec.start = "2026-11-01T07:30:00Z".to_string();
    let mut event = make_event_with(spec);
    event.set_time_zone(zone("America/New_York"));
    let next = Scheduler::next(
        &[event],
        &opted_in(|s| s.lead_time = 3.0 * 3600.0),
        &TakeoverLedger::new(),
        date("2026-11-01T03:00:00Z"),
    )
    .unwrap();
    assert_eq!(next.fire_at, date("2026-11-01T04:30:00Z"));
}

#[test]
fn lead_time_across_us_spring_forward_and_midnight_is_elapsed_seconds() {
    // 2026-03-08: clocks go forward at 07:00Z (02:00 EST -> 03:00 EDT) in America/New_York. A meeting at 07:30Z
    // (03:30 EDT) with a one hour lead time fires at 06:30Z (01:30 EST), 60 elapsed minutes earlier though the wall
    // clock moved two hours.
    let mut event = ev_starting("2026-03-08T07:30:00Z");
    event.set_time_zone(zone("America/New_York"));
    let now = date("2026-03-08T05:00:00Z");
    let next = Scheduler::next(
        std::slice::from_ref(&event),
        &opted_in(|s| s.lead_time = 3600.0),
        &TakeoverLedger::new(),
        now,
    )
    .unwrap();
    assert_eq!(next.fire_at, add_seconds(event.start(), -3600.0));
    assert_eq!(next.fire_at, date("2026-03-08T06:30:00Z"));
    // A midnight crossing in the event's own zone: 00:05 on 2026-03-19 in Pacific/Auckland is 11:05Z the day before.
    let mut late = ev_starting("2026-03-18T11:05:00Z");
    late.set_time_zone(zone("Pacific/Auckland"));
    let next = Scheduler::next(
        &[late],
        &opted_in(|s| s.lead_time = 600.0),
        &TakeoverLedger::new(),
        date("2026-03-18T10:00:00Z"),
    )
    .unwrap();
    assert_eq!(next.fire_at, date("2026-03-18T10:55:00Z"));
}
