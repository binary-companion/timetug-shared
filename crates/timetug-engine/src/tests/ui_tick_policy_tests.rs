//! Port of `Scheduling/UITickPolicyTests.swift`.

use calendar_core::model::CalendarEvent;
use calendar_core::time::{Zone, add_seconds, seconds_between};
use jiff::Timestamp;
use timetug_core::TimeTugCalendarEvent;

use super::support::ts;
use crate::scheduling::ui_tick_policy::UITickPolicy;

fn now() -> Timestamp {
    ts(1_800_000_000)
}

fn zone() -> Zone {
    Zone::fixed(0).unwrap()
}

fn event(start_after: f64, length: f64) -> TimeTugCalendarEvent {
    let start = add_seconds(now(), start_after);
    TimeTugCalendarEvent::new(
        CalendarEvent::new("e", "c", "Meeting", start, add_seconds(start, length)),
        "s",
    )
}

#[test]
fn countdown_uses_minute_transitions_then_seconds() {
    let far = event(61.0, 1800.0);
    let wake = UITickPolicy::next(now(), true, Some(&far), &[], 60.0, &zone()).unwrap();
    assert!((seconds_between(now(), wake.date) - 1.02).abs() < 0.01);
    assert!(!wake.rebuild_agenda);
    let boundary = event(60.0, 1800.0);
    let boundary_wake =
        UITickPolicy::next(now(), true, Some(&boundary), &[], 60.0, &zone()).unwrap();
    assert!((seconds_between(now(), boundary_wake.date) - 0.02).abs() < 0.01);
    let near = event(59.0, 1800.0);
    let near_wake = UITickPolicy::next(now(), true, Some(&near), &[], 60.0, &zone()).unwrap();
    assert!((seconds_between(now(), near_wake.date) - 0.02).abs() < 0.01);
}

#[test]
fn icon_only_idle_waits_for_day_boundary_and_event_start_rebuilds() {
    let idle = UITickPolicy::next(now(), false, None, &[], 60.0, &zone()).unwrap();
    assert!(seconds_between(now(), idle.date) > 3600.0);
    assert!(idle.rebuild_agenda);
    let meeting = event(30.0, 1800.0);
    let wake = UITickPolicy::next(
        now(),
        false,
        None,
        std::slice::from_ref(&meeting),
        60.0,
        &zone(),
    )
    .unwrap();
    assert_eq!(wake.date, meeting.start());
    assert!(wake.rebuild_agenda);
}

#[test]
fn an_idle_hour_has_no_tick_triggered_structural_rebuilds() {
    let mut cursor = now();
    let mut rebuilds = 0;
    let hour_end = add_seconds(now(), 3600.0);
    while let Some(wake) = UITickPolicy::next(cursor, false, None, &[], 60.0, &zone()) {
        if wake.date >= hour_end {
            break;
        }
        if wake.rebuild_agenda {
            rebuilds += 1;
        }
        cursor = wake.date;
    }
    assert_eq!(rebuilds, 0);
}
