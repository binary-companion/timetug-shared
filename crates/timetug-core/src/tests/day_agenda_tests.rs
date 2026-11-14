//! Port of `TimeTugCoreTests/DayAgendaTests.swift`.

use jiff::Timestamp;

use crate::agenda::day_agenda::{DayAgenda, State};
use crate::model::event::TimeTugCalendarEvent;
use crate::model::takeover_settings::TakeoverSettings;

use super::support::{EventSpec, date, day, make_all_day, make_event_with, opted_in, zone};

fn now() -> Timestamp {
    date("2026-09-18T10:15:00Z")
}

/// `makeEvent(id, start:, ...)` with the arguments the tests in this file use.
fn ev(id: &str, start: &str) -> TimeTugCalendarEvent {
    make_event_with(EventSpec {
        id: id.to_string(),
        start: start.to_string(),
        ..EventSpec::default()
    })
}

fn all_day_utc(id: &str, start: &str) -> TimeTugCalendarEvent {
    make_event_with(EventSpec {
        id: id.to_string(),
        start: start.to_string(),
        minutes: 1440,
        is_all_day: true,
        ..EventSpec::default()
    })
}

fn agenda_at(
    events: &[TimeTugCalendarEvent],
    settings: &TakeoverSettings,
    at: Timestamp,
) -> DayAgenda {
    DayAgenda::make(events, settings, at, &zone("UTC"))
}

fn agenda(events: &[TimeTugCalendarEvent], settings: &TakeoverSettings) -> DayAgenda {
    agenda_at(events, settings, now())
}

fn source_ids(agenda: &DayAgenda) -> Vec<String> {
    agenda
        .items
        .iter()
        .map(|i| i.event.source_event_id().to_string())
        .collect()
}

#[test]
fn marks_past_current_upcoming() {
    let past = ev("p", "2026-09-18T09:00:00Z"); // ends 09:30
    let current = ev("c", "2026-09-18T10:00:00Z"); // ends 10:30
    let upcoming = ev("u", "2026-09-18T11:00:00Z");
    let result = agenda(&[past, current, upcoming], &opted_in(|_| {}));
    let states: Vec<State> = result.items.iter().map(|i| i.state).collect();
    assert_eq!(states, vec![State::Past, State::Current, State::Upcoming]);
}

#[test]
fn next_is_first_timed_event_starting_after_now() {
    let current = ev("c", "2026-09-18T10:00:00Z");
    let all_day = all_day_utc("a", "2026-09-18T00:00:00Z");
    let upcoming = ev("u", "2026-09-18T11:00:00Z");
    let settings = opted_in(|s| s.skip_all_day_events = false);
    assert_eq!(
        agenda(&[all_day, current, upcoming.clone()], &settings).next,
        Some(upcoming)
    );
}

#[test]
fn next_is_nil_when_nothing_remains() {
    let event = make_event_with(EventSpec {
        start: "2026-09-18T09:00:00Z".to_string(),
        ..EventSpec::default()
    });
    assert_eq!(agenda(&[event], &opted_in(|_| {})).next, None);
}

#[test]
fn hidden_calendars_are_excluded() {
    let settings = opted_in(|s| {
        s.hidden_calendar_keys = ["fake/family".to_string()].into();
    });
    let a = make_event_with(EventSpec {
        id: "a".to_string(),
        calendar_id: "family".to_string(),
        ..EventSpec::default()
    });
    let b = ev("b", "2026-09-18T10:00:00Z");
    assert_eq!(source_ids(&agenda(&[a, b], &settings)), vec!["b"]);
}

#[test]
fn merged_meeting_is_hidden_only_when_all_its_calendars_are_hidden() {
    let mut merged = make_event_with(EventSpec {
        id: "m".to_string(),
        calendar_id: "family".to_string(),
        ..EventSpec::default()
    });
    merged.additional_calendar_keys = ["fake/work".to_string()].into();
    let hide_family_only = opted_in(|s| {
        s.hidden_calendar_keys = ["fake/family".to_string()].into();
    });
    let hide_both = opted_in(|s| {
        s.hidden_calendar_keys = ["fake/family".to_string(), "fake/work".to_string()].into();
    });
    assert_eq!(
        agenda(std::slice::from_ref(&merged), &hide_family_only)
            .items
            .len(),
        1
    );
    assert!(agenda(&[merged], &hide_both).items.is_empty());
}

#[test]
fn includes_events_that_overlap_today() {
    let overnight = make_event_with(EventSpec {
        id: "n".to_string(),
        start: "2026-09-17T23:30:00Z".to_string(),
        minutes: 120,
        ..EventSpec::default()
    });
    assert_eq!(agenda(&[overnight], &opted_in(|_| {})).items.len(), 1);
}

#[test]
fn excludes_yesterday_and_far_tomorrow() {
    let yesterday = ev("y", "2026-09-17T10:00:00Z");
    let tomorrow = ev("t", "2026-09-19T10:00:00Z");
    assert!(
        agenda(&[yesterday, tomorrow], &opted_in(|_| {}))
            .items
            .is_empty()
    );
}

#[test]
fn after_midnight_event_appears_only_inside_lead_time() {
    let event = ev("m", "2026-09-19T00:05:00Z");
    let settings = opted_in(|s| s.lead_time = 600.0);
    assert!(
        agenda_at(
            std::slice::from_ref(&event),
            &settings,
            date("2026-09-18T23:40:00Z")
        )
        .items
        .is_empty()
    );
    assert_eq!(
        agenda_at(&[event], &settings, date("2026-09-18T23:56:00Z"))
            .items
            .len(),
        1
    );
}

#[test]
fn tomorrows_all_day_event_never_spills_in() {
    let all_day = all_day_utc("a", "2026-09-19T00:00:00Z");
    let settings = opted_in(|s| {
        s.lead_time = 600.0;
        s.skip_all_day_events = false;
    });
    assert!(
        agenda_at(&[all_day], &settings, date("2026-09-18T23:59:00Z"))
            .items
            .is_empty()
    );
}

#[test]
fn all_day_event_is_hidden_by_default() {
    let all_day = all_day_utc("a", "2026-09-18T00:00:00Z");
    assert!(agenda(&[all_day], &opted_in(|_| {})).items.is_empty());
}

#[test]
fn all_day_event_is_shown_when_not_skipped() {
    let all_day = all_day_utc("a", "2026-09-18T00:00:00Z");
    let settings = opted_in(|s| s.skip_all_day_events = false);
    assert_eq!(source_ids(&agenda(&[all_day], &settings)), vec!["a"]);
}

#[test]
fn timed_event_is_unaffected_by_skip_all_day_setting() {
    let timed = ev("t", "2026-09-18T11:00:00Z");
    let skip = opted_in(|s| s.skip_all_day_events = true);
    let keep = opted_in(|s| s.skip_all_day_events = false);
    assert_eq!(agenda(std::slice::from_ref(&timed), &skip).items.len(), 1);
    assert_eq!(agenda(&[timed], &keep).items.len(), 1);
}

fn la_agenda(events: &[TimeTugCalendarEvent], at: &str) -> DayAgenda {
    DayAgenda::make(
        events,
        &opted_in(|s| s.skip_all_day_events = false),
        date(at),
        &zone("America/Los_Angeles"),
    )
}

#[test]
fn all_day_event_stays_on_its_own_date_for_a_viewer_in_another_zone() {
    let tokyo_sep21 = make_all_day("Asia/Tokyo", day(2026, 9, 21), day(2026, 9, 22));
    // 10:00Z on Sep 21 is 03:00 in Los Angeles: still Sep 21 there.
    let on_the_day = la_agenda(std::slice::from_ref(&tokyo_sep21), "2026-09-21T10:00:00Z");
    let states: Vec<State> = on_the_day.items.iter().map(|i| i.state).collect();
    assert_eq!(states, vec![State::Current]);
    // 10:00Z on Sep 20 is Sep 20 in Los Angeles: not shown (the Tokyo event's instants begin at 15:00Z on Sep 20,
    // which the old instant rule would have shown as today's).
    assert!(
        la_agenda(&[tokyo_sep21], "2026-09-20T10:00:00Z")
            .items
            .is_empty()
    );
}

#[test]
fn all_day_event_ending_at_local_midnight_is_not_shown_on_the_exclusive_end_date() {
    let two_days = make_all_day("America/Los_Angeles", day(2026, 9, 19), day(2026, 9, 21));
    assert_eq!(
        la_agenda(std::slice::from_ref(&two_days), "2026-09-20T18:00:00Z")
            .items
            .len(),
        1
    ); // Sep 20 in LA
    assert!(
        la_agenda(&[two_days], "2026-09-21T18:00:00Z")
            .items
            .is_empty()
    ); // Sep 21 in LA
}

#[test]
fn all_day_items_are_hidden_when_skip_all_day_is_on() {
    let holiday = make_all_day("America/Los_Angeles", day(2026, 9, 21), day(2026, 9, 22));
    let result = DayAgenda::make(
        &[holiday],
        &opted_in(|_| {}),
        date("2026-09-21T18:00:00Z"),
        &zone("America/Los_Angeles"),
    );
    assert!(result.items.is_empty());
}

// Additions (not Swift tests): the empty agenda and the sort order of titles outside ASCII.

#[test]
fn empty_agenda_has_no_items_and_no_next() {
    let empty = DayAgenda::empty();
    assert!(empty.items.is_empty());
    assert_eq!(empty.next, None);
}

#[test]
fn items_with_the_same_start_sort_by_title_in_scalar_order() {
    // Swift compares `String`s by unicode scalar values (after normalization), as Rust does for already normalized
    // text: "Zebra" (U+005A) sorts before "Éclair" (U+00C9).
    let mut accent = ev("1", "2026-09-18T11:00:00Z");
    accent.set_title("\u{00C9}clair");
    let mut zebra = ev("2", "2026-09-18T11:00:00Z");
    zebra.set_title("Zebra");
    let titles: Vec<String> = agenda(&[accent, zebra], &opted_in(|_| {}))
        .items
        .iter()
        .map(|i| i.event.title().to_string())
        .collect();
    assert_eq!(titles, vec!["Zebra", "\u{00C9}clair"]);
}
