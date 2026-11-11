//! Port of `TimeTugCoreTests/WidgetTimelineTests.swift`.

use jiff::Timestamp;

use crate::agenda::day_agenda::State;
use crate::widget::snapshot::{WidgetEvent, WidgetSnapshot};
use crate::widget::timeline::{NextUp, TodayRow, WidgetTimeline};
use calendar_core::time::add_seconds;

use super::support::{date, zone};

fn now() -> Timestamp {
    date("2026-09-18T12:00:00Z")
}

fn event(id: &str, start: &str, minutes: i64, all_day: bool) -> WidgetEvent {
    let s = date(start);
    WidgetEvent::new(
        id,
        id,
        s,
        add_seconds(s, (minutes * 60) as f64),
        all_day,
        None,
        None,
    )
}

fn timed(id: &str, start: &str) -> WidgetEvent {
    event(id, start, 30, false)
}

fn snapshot(events: Vec<WidgetEvent>) -> WidgetSnapshot {
    WidgetSnapshot::new(now(), events)
}

#[test]
fn change_dates_include_future_starts_ends_and_midnights() {
    let s = snapshot(vec![
        timed("a", "2026-09-18T13:00:00Z"),
        timed("b", "2026-09-18T11:00:00Z"),
    ]);
    let dates = WidgetTimeline::change_dates(&s, now(), &zone("UTC"), 20);
    assert_eq!(dates.first(), Some(&date("2026-09-18T13:00:00Z")));
    assert!(dates.contains(&date("2026-09-18T13:30:00Z")));
    assert!(dates.contains(&date("2026-09-19T00:00:00Z")));
    assert!(!dates.contains(&date("2026-09-18T11:00:00Z")));
    assert!(!dates.contains(&date("2026-09-18T11:30:00Z")));
    let mut sorted = dates.clone();
    sorted.sort();
    assert_eq!(dates, sorted);
    sorted.dedup();
    assert_eq!(sorted.len(), dates.len());
}

#[test]
fn change_dates_respect_limit() {
    let s = snapshot(vec![
        timed("a", "2026-09-18T13:00:00Z"),
        timed("b", "2026-09-18T14:00:00Z"),
    ]);
    assert_eq!(
        WidgetTimeline::change_dates(&s, now(), &zone("UTC"), 2).len(),
        2
    );
}

#[test]
fn change_dates_for_empty_snapshot_are_just_midnights() {
    let dates = WidgetTimeline::change_dates(&snapshot(vec![]), now(), &zone("UTC"), 10);
    assert_eq!(
        dates,
        vec![date("2026-09-19T00:00:00Z"), date("2026-09-20T00:00:00Z")]
    );
}

#[test]
fn change_dates_ignore_all_day_event_boundaries_but_keep_midnights() {
    let s = snapshot(vec![event("d", "2026-09-18T00:00:00Z", 24 * 60, true)]);
    let dates = WidgetTimeline::change_dates(&s, now(), &zone("UTC"), 10);
    assert_eq!(
        dates,
        vec![date("2026-09-19T00:00:00Z"), date("2026-09-20T00:00:00Z")]
    );
}

#[test]
fn next_up_finds_current_and_upcoming() {
    let s = snapshot(vec![
        timed("now", "2026-09-18T11:45:00Z"),
        timed("n1", "2026-09-18T13:00:00Z"),
        timed("n2", "2026-09-18T14:00:00Z"),
        timed("n3", "2026-09-19T09:00:00Z"),
        timed("past", "2026-09-18T09:00:00Z"),
    ]);
    let result = WidgetTimeline::next_up(&s, now(), 2);
    assert_eq!(result.current.map(|e| e.id), Some("now".to_string()));
    let ids: Vec<&str> = result.upcoming.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["n1", "n2"]);
}

#[test]
fn next_up_ignores_all_day_events() {
    let s = snapshot(vec![event("d", "2026-09-18T00:00:00Z", 24 * 60, true)]);
    assert_eq!(
        WidgetTimeline::next_up(&s, now(), 3),
        NextUp::new(None, vec![])
    );
}

#[test]
fn next_up_picks_earliest_started_when_meetings_overlap() {
    let s = snapshot(vec![
        timed("late", "2026-09-18T11:50:00Z"),
        timed("early", "2026-09-18T11:40:00Z"),
    ]);
    assert_eq!(
        WidgetTimeline::next_up(&s, now(), 3).current.map(|e| e.id),
        Some("early".to_string())
    );
}

#[test]
fn today_covers_only_today_and_assigns_states() {
    let s = snapshot(vec![
        timed("past", "2026-09-18T09:00:00Z"),
        timed("cur", "2026-09-18T11:45:00Z"),
        timed("up", "2026-09-18T15:00:00Z"),
        timed("tomorrow", "2026-09-19T09:00:00Z"),
    ]);
    let rows = WidgetTimeline::today(&s, now(), &zone("UTC"));
    let ids: Vec<&str> = rows.iter().map(|r| r.event.id.as_str()).collect();
    assert_eq!(ids, vec!["past", "cur", "up"]);
    let states: Vec<State> = rows.iter().map(|r| r.state).collect();
    assert_eq!(states, vec![State::Past, State::Current, State::Upcoming]);
}

#[test]
fn today_keeps_all_day_events_as_current() {
    let s = snapshot(vec![event("d", "2026-09-18T00:00:00Z", 24 * 60, true)]);
    let states: Vec<State> = WidgetTimeline::today(&s, now(), &zone("UTC"))
        .iter()
        .map(|r| r.state)
        .collect();
    assert_eq!(states, vec![State::Current]);
}

// MARK: visibleToday

fn rows(specs: &[(&str, State)]) -> Vec<TodayRow> {
    specs
        .iter()
        .map(|(id, state)| TodayRow::new(timed(id, "2026-09-18T09:00:00Z"), *state))
        .collect()
}

fn row_ids(rows: &[TodayRow]) -> Vec<String> {
    rows.iter().map(TodayRow::id).collect()
}

#[test]
fn visible_today_keeps_all_rows_within_capacity() {
    let r = rows(&[("a", State::Past), ("b", State::Upcoming)]);
    assert_eq!(WidgetTimeline::visible_today(&r, 2), r);
}

#[test]
fn visible_today_drops_past_rows_when_over_capacity() {
    let r = rows(&[
        ("a", State::Past),
        ("b", State::Current),
        ("c", State::Upcoming),
    ]);
    assert_eq!(
        row_ids(&WidgetTimeline::visible_today(&r, 2)),
        vec!["b", "c"]
    );
}

#[test]
fn visible_today_keeps_last_finished_rows_when_everything_is_past() {
    let r = rows(&[("a", State::Past), ("b", State::Past), ("c", State::Past)]);
    assert_eq!(
        row_ids(&WidgetTimeline::visible_today(&r, 2)),
        vec!["b", "c"]
    );
}

#[test]
fn visible_today_stays_empty_when_there_are_no_rows() {
    assert!(WidgetTimeline::visible_today(&[], 2).is_empty());
}

#[test]
fn visible_today_never_drops_the_current_meeting() {
    let r = rows(&[
        ("a", State::Past),
        ("b", State::Past),
        ("c", State::Current),
    ]);
    assert_eq!(row_ids(&WidgetTimeline::visible_today(&r, 2)), vec!["c"]);
}
