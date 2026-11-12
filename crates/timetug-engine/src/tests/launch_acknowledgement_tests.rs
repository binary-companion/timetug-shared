//! Port of `Scheduling/LaunchAcknowledgementTests.swift`.

use std::collections::{BTreeMap, BTreeSet};

use calendar_core::model::CalendarEvent;
use calendar_core::time::add_seconds;
use jiff::Timestamp;
use timetug_core::{CalendarSnapshot, SourceStatus, TimeTugCalendarEvent};

use super::support::ts;
use crate::scheduling::launch_acknowledgement::LaunchAcknowledgement;

fn launch() -> Timestamp {
    ts(1_800_000_000)
}

fn event(id: &str, source: &str, start: Timestamp, end: Timestamp) -> TimeTugCalendarEvent {
    TimeTugCalendarEvent::new(CalendarEvent::new(id, "main", id, start, end), source)
}

fn snapshot(
    events: &[TimeTugCalendarEvent],
    statuses: &[(&str, SourceStatus)],
) -> CalendarSnapshot {
    CalendarSnapshot::new(
        events.to_vec(),
        vec![],
        statuses
            .iter()
            .map(|(id, status)| (id.to_string(), status.clone()))
            .collect::<BTreeMap<_, _>>(),
        BTreeMap::new(),
        launch(),
    )
}

fn ids(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

fn ids_of(events: &[TimeTugCalendarEvent]) -> Vec<String> {
    events.iter().map(TimeTugCalendarEvent::id).collect()
}

#[test]
fn recovery_acknowledges_only_prelaunch_meetings_on_healthy_sources() {
    let mut state = LaunchAcknowledgement::new(launch());
    state.register_sources(&ids(&["a", "b"]), add_seconds(launch(), 5.0));
    let recovered = add_seconds(launch(), 15.0 * 60.0);
    let old_a = event(
        "old-a",
        "a",
        add_seconds(launch(), -600.0),
        add_seconds(recovered, 600.0),
    );
    let old_b = event(
        "old-b",
        "b",
        add_seconds(launch(), -600.0),
        add_seconds(recovered, 600.0),
    );
    let new_a = event(
        "new-a",
        "a",
        add_seconds(launch(), 300.0),
        add_seconds(recovered, 600.0),
    );
    let events = [old_a.clone(), old_b.clone(), new_a];
    assert_eq!(
        ids_of(&state.events_to_acknowledge(
            &snapshot(
                &events,
                &[("a", SourceStatus::Ok), ("b", SourceStatus::AuthExpired)]
            ),
            recovered,
            120.0
        )),
        vec![old_a.id()]
    );
    assert_eq!(
        ids_of(&state.events_to_acknowledge(
            &snapshot(&events, &[("a", SourceStatus::Ok), ("b", SourceStatus::Ok)]),
            recovered,
            120.0
        )),
        vec![old_b.id()]
    );
}

#[test]
fn added_source_uses_addition_time_and_removed_source_is_dropped() {
    let mut state = LaunchAcknowledgement::new(launch());
    state.register_sources(&ids(&["a"]), launch());
    let _ =
        state.events_to_acknowledge(&snapshot(&[], &[("a", SourceStatus::Ok)]), launch(), 120.0);
    let added_at = add_seconds(launch(), 3600.0);
    state.register_sources(&ids(&["a", "b"]), added_at);
    let old = event(
        "old",
        "b",
        add_seconds(added_at, -300.0),
        add_seconds(added_at, 600.0),
    );
    assert_eq!(
        ids_of(&state.events_to_acknowledge(
            &snapshot(std::slice::from_ref(&old), &[("b", SourceStatus::Ok)]),
            added_at,
            120.0
        )),
        vec![old.id()]
    );
    state.register_sources(&ids(&["a"]), added_at);
    assert!(
        state
            .events_to_acknowledge(
                &snapshot(std::slice::from_ref(&old), &[("b", SourceStatus::Ok)]),
                added_at,
                120.0
            )
            .is_empty()
    );
}
