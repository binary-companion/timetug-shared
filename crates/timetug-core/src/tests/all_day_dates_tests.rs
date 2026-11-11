//! Port of `AllDayDatesTests.swift`.

use super::support::{EventSpec, day, make_all_day, make_event, make_event_with};

#[test]
fn all_day_dates_are_read_in_the_events_own_zone() {
    let e = make_all_day("Asia/Tokyo", day(2026, 9, 21), day(2026, 9, 22));
    assert_eq!(e.all_day_dates().map(|d| d.0), Some(day(2026, 9, 21)));
    assert!(e.covers(&day(2026, 9, 21)));
    assert!(!e.covers(&day(2026, 9, 22)));
    assert!(!e.covers(&day(2026, 9, 20)));
}

#[test]
fn multi_day_all_day_covers_every_day_but_not_the_exclusive_end() {
    let e = make_all_day("America/Los_Angeles", day(2026, 9, 21), day(2026, 9, 24));
    assert!(e.covers(&day(2026, 9, 23)));
    assert!(!e.covers(&day(2026, 9, 24)));
}

#[test]
fn all_day_dates_survive_a_missing_midnight() {
    // Sao Paulo skipped local midnight on 2018-11-04 (DST began at 00:00); the noon-based start of day still works.
    let e = make_all_day("America/Sao_Paulo", day(2018, 11, 4), day(2018, 11, 5));
    assert_eq!(e.all_day_dates().map(|d| d.0), Some(day(2018, 11, 4)));
    assert_eq!(e.all_day_dates().map(|d| d.1), Some(day(2018, 11, 5)));
    assert!(e.covers(&day(2018, 11, 4)));
}

#[test]
fn timed_events_have_no_all_day_dates() {
    let e = make_event();
    assert_eq!(e.all_day_dates(), None);
    assert!(!e.covers(&day(2026, 9, 18)));
}

#[test]
fn all_day_range_within_one_date_is_treated_as_that_day() {
    // Non-canonical input (start and end on the same date in the event's zone): must not vanish.
    let e = make_event_with(EventSpec {
        id: "x".into(),
        minutes: 30,
        is_all_day: true,
        ..Default::default()
    });
    assert_eq!(e.all_day_dates().map(|d| d.0), Some(day(2026, 9, 18)));
    assert_eq!(e.all_day_dates().map(|d| d.1), Some(day(2026, 9, 19)));
    assert!(e.covers(&day(2026, 9, 18)));
    assert!(!e.covers(&day(2026, 9, 19)));
}
