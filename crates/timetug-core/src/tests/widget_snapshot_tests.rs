//! Port of `TimeTugCoreTests/WidgetSnapshotTests.swift`, plus the JSON contract with the shipped widgets.

use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use serde_json::{Value, json};

use crate::model::calendar_info::CalendarInfo;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::takeover_settings::TakeoverSettings;
use crate::widget::snapshot::{WidgetEvent, WidgetSnapshot};

use super::support::{EventSpec, date, day, make_all_day, make_event, make_event_with, zone};
use calendar_core::all_day::AllDay;

fn now() -> Timestamp {
    date("2026-09-18T12:00:00Z")
}

fn make(
    events: &[TimeTugCalendarEvent],
    settings: &TakeoverSettings,
    calendars: &[CalendarInfo],
) -> WidgetSnapshot {
    WidgetSnapshot::make(events, calendars, settings, now(), &zone("UTC"))
}

fn ev(id: &str, title: &str, start: &str) -> TimeTugCalendarEvent {
    make_event_with(EventSpec {
        id: id.to_string(),
        title: title.to_string(),
        start: start.to_string(),
        ..EventSpec::default()
    })
}

fn ids(snapshot: &WidgetSnapshot) -> Vec<String> {
    snapshot.events.iter().map(|e| e.id.clone()).collect()
}

fn defaults() -> TakeoverSettings {
    TakeoverSettings::default()
}

#[test]
fn snapshot_skips_all_day_events_when_setting_is_on() {
    let all_day = make_event_with(EventSpec {
        id: "a".to_string(),
        start: "2026-09-18T00:00:00Z".to_string(),
        minutes: 24 * 60,
        is_all_day: true,
        ..EventSpec::default()
    });
    assert!(
        make(std::slice::from_ref(&all_day), &defaults(), &[])
            .events
            .is_empty()
    );
    let mut settings = defaults();
    settings.skip_all_day_events = false;
    assert_eq!(
        ids(&make(std::slice::from_ref(&all_day), &settings, &[])),
        vec![all_day.id()]
    );
}

#[test]
fn snapshot_hides_events_on_hidden_calendars() {
    let mut settings = defaults();
    settings.set_shown_in_list(false, "fake/cal");
    assert!(make(&[make_event()], &settings, &[]).events.is_empty());
}

#[test]
fn snapshot_covers_today_through_the_horizon_only() {
    let earlier_today = ev("early", "Standup", "2026-09-18T08:00:00Z");
    let in_horizon = ev("in", "Standup", "2026-09-20T10:00:00Z");
    let beyond = ev("out", "Standup", "2026-09-21T10:00:00Z");
    let yesterday = ev("old", "Standup", "2026-09-17T10:00:00Z");
    let result = make(
        &[beyond, in_horizon.clone(), yesterday, earlier_today.clone()],
        &defaults(),
        &[],
    );
    assert_eq!(ids(&result), vec![earlier_today.id(), in_horizon.id()]);
}

#[test]
fn snapshot_sorts_by_start_then_title() {
    let b = ev("b", "B", "2026-09-18T15:00:00Z");
    let a = ev("a", "A", "2026-09-18T15:00:00Z");
    let early = ev("e", "Z", "2026-09-18T13:00:00Z");
    let titles: Vec<String> = make(&[b, a, early], &defaults(), &[])
        .events
        .iter()
        .map(|e| e.title.clone())
        .collect();
    assert_eq!(titles, vec!["Z", "A", "B"]);
}

#[test]
fn snapshot_carries_colour_join_link_and_shown_times() {
    let link = UrlString::parse("https://meet.google.com/aaa-bbbb-ccc").unwrap();
    let mut event = make_event_with(EventSpec {
        id: "m".to_string(),
        start: "2026-09-18T14:00:00Z".to_string(),
        conference_url: Some(link.clone()),
        ..EventSpec::default()
    });
    event.display_start = Some(date("2026-09-18T13:30:00Z"));
    let calendars = [CalendarInfo::new("fake", "cal", "Work").with_color_hex(Some("#ff0000"))];
    let widget_event = make(std::slice::from_ref(&event), &defaults(), &calendars)
        .events
        .remove(0);
    assert_eq!(widget_event.color_hex.as_deref(), Some("#FF0000"));
    assert_eq!(widget_event.join_url, Some(link));
    assert_eq!(widget_event.start, date("2026-09-18T13:30:00Z"));
    assert_eq!(widget_event.end, event.end());
}

#[test]
fn snapshot_round_trips_through_json() {
    let snapshot = make(&[make_event()], &defaults(), &[]);
    let text = serde_json::to_string(&snapshot).unwrap();
    let back: WidgetSnapshot = serde_json::from_str(&text).unwrap();
    assert_eq!(back, snapshot);
}

#[test]
fn snapshot_is_stale_after_max_age() {
    let snapshot = WidgetSnapshot::new(now(), vec![]);
    let at = |seconds: i64| now() + jiff::SignedDuration::from_secs(seconds);
    assert!(!snapshot.is_stale(at(WidgetSnapshot::MAX_AGE as i64)));
    assert!(snapshot.is_stale(at(WidgetSnapshot::MAX_AGE as i64 + 1)));
}

#[test]
fn snapshot_breaks_start_and_title_ties_by_id() {
    let one = ev("1", "Same", "2026-09-18T15:00:00Z");
    let two = ev("2", "Same", "2026-09-18T15:00:00Z");
    let mut expected = vec![one.id(), two.id()];
    expected.sort();
    assert_eq!(
        ids(&make(&[one.clone(), two.clone()], &defaults(), &[])),
        expected
    );
    assert_eq!(ids(&make(&[two, one], &defaults(), &[])), expected);
}

#[test]
fn snapshot_excludes_event_ending_exactly_at_start_of_today() {
    let ends = ev("x", "Standup", "2026-09-17T23:30:00Z");
    assert!(make(&[ends], &defaults(), &[]).events.is_empty());
}

#[test]
fn snapshot_excludes_event_starting_exactly_at_horizon_end() {
    let starts = ev("x", "Standup", "2026-09-21T00:00:00Z");
    assert!(make(&[starts], &defaults(), &[]).events.is_empty());
}

#[test]
fn snapshot_includes_event_started_yesterday_and_still_running() {
    let running = make_event_with(EventSpec {
        id: "x".to_string(),
        start: "2026-09-17T23:00:00Z".to_string(),
        minutes: 120,
        ..EventSpec::default()
    });
    assert_eq!(
        ids(&make(std::slice::from_ref(&running), &defaults(), &[])),
        vec![running.id()]
    );
}

#[test]
fn snapshot_hides_merged_event_only_when_all_its_calendars_are_hidden() {
    let mut merged = ev("m", "Standup", "2026-09-18T10:00:00Z");
    merged.additional_calendar_keys = ["fake/other".to_string()].into();
    let mut settings = defaults();
    settings.set_shown_in_list(false, "fake/cal");
    assert_eq!(
        ids(&make(std::slice::from_ref(&merged), &settings, &[])),
        vec![merged.id()]
    );
    settings.set_shown_in_list(false, "fake/other");
    assert!(make(&[merged], &settings, &[]).events.is_empty());
}

#[test]
fn snapshot_shows_an_all_day_event_on_its_own_dates_in_the_viewers_midnights() {
    let la = zone("America/Los_Angeles");
    let tokyo_sep21 = make_all_day("Asia/Tokyo", day(2026, 9, 21), day(2026, 9, 22));
    let mut settings = defaults();
    settings.skip_all_day_events = false;
    let snapshot = |time: &str| {
        WidgetSnapshot::make(
            std::slice::from_ref(&tokyo_sep21),
            &[],
            &settings,
            date(time),
            &la,
        )
    };
    let events = snapshot("2026-09-21T18:00:00Z").events;
    assert_eq!(events.len(), 1);
    assert!(events[0].is_all_day);
    assert_eq!(
        Some(events[0].start),
        AllDay::start_of_day(&day(2026, 9, 21), &la)
    );
    assert_eq!(
        Some(events[0].end),
        AllDay::start_of_day(&day(2026, 9, 22), &la)
    );
    assert_eq!(snapshot("2026-09-19T18:00:00Z").events.len(), 1); // Sep 19 + 3-day horizon reaches Sep 21
    assert!(snapshot("2026-09-18T18:00:00Z").events.is_empty()); // horizon ends before Sep 21
}

// Additions (not Swift tests): the JSON contract with shipped widgets.
//
// `WidgetSnapshotStore.write` encodes with `dateEncodingStrategy = .iso8601`, so dates are ISO 8601 strings with whole
// seconds (the fraction is dropped by flooring), not the reference-date numbers of the persisted files. The text
// below was produced by this Swift snippet (macOS, Foundation; `WidgetEvent` and `WidgetSnapshot` are the structs of
// `WidgetSnapshot.swift` with their memberwise initialisers):
//
//     func d(_ s: String) -> Date { ISO8601DateFormatter().date(from: s)! }
//     let snapshot = WidgetSnapshot(generatedAt: d("2026-09-18T12:00:00Z"), events: [
//         WidgetEvent(id: "fake/m/1789740000", title: "Standup \u{00E9}\"q\"/x",
//                     start: d("2026-09-18T13:30:00Z"), end: d("2026-09-18T14:00:00Z"), isAllDay: false,
//                     colorHex: "#FF0000", joinURL: URL(string: "https://meet.google.com/aaa-bbbb-ccc")!),
//         WidgetEvent(id: "fake/d/1", title: "Holiday", start: d("2026-09-19T07:00:00Z"),
//                     end: d("2026-09-20T07:00:00Z"), isAllDay: true, colorHex: nil, joinURL: nil),
//     ])
//     let encoder = JSONEncoder(); encoder.dateEncodingStrategy = .iso8601
//     encoder.outputFormatting = [.sortedKeys]
//     print(String(data: try! encoder.encode(snapshot), encoding: .utf8)!)
const SWIFT_ENCODED: &str = r##"{"events":[{"colorHex":"#FF0000","end":"2026-09-18T14:00:00Z","id":"fake\/m\/1789740000","isAllDay":false,"joinURL":"https:\/\/meet.google.com\/aaa-bbbb-ccc","start":"2026-09-18T13:30:00Z","title":"Standup é\"q\"\/x"},{"end":"2026-09-20T07:00:00Z","id":"fake\/d\/1","isAllDay":true,"start":"2026-09-19T07:00:00Z","title":"Holiday"}],"generatedAt":"2026-09-18T12:00:00Z"}"##;

fn fixed_snapshot() -> WidgetSnapshot {
    WidgetSnapshot::new(
        date("2026-09-18T12:00:00Z"),
        vec![
            WidgetEvent::new(
                "fake/m/1789740000",
                "Standup \u{00E9}\"q\"/x",
                date("2026-09-18T13:30:00Z"),
                date("2026-09-18T14:00:00Z"),
                false,
                Some("#FF0000".to_string()),
                UrlString::parse("https://meet.google.com/aaa-bbbb-ccc"),
            ),
            WidgetEvent::new(
                "fake/d/1",
                "Holiday",
                date("2026-09-19T07:00:00Z"),
                date("2026-09-20T07:00:00Z"),
                true,
                None,
                None,
            ),
        ],
    )
}

#[test]
fn snapshot_json_reads_what_swift_wrote() {
    let decoded: WidgetSnapshot = serde_json::from_str(SWIFT_ENCODED).unwrap();
    assert_eq!(decoded, fixed_snapshot());
}

#[test]
fn snapshot_json_writes_what_swift_wrote() {
    let written: Value = serde_json::to_value(fixed_snapshot()).unwrap();
    let expected: Value = serde_json::from_str(SWIFT_ENCODED).unwrap();
    assert_eq!(written, expected);
    // Absent optionals are omitted, not null.
    assert!(written["events"][1].get("colorHex").is_none());
    assert!(written["events"][1].get("joinURL").is_none());
}

#[test]
fn snapshot_json_drops_fractions_by_flooring_like_swift() {
    // Swift: Date(timeIntervalSince1970: 1789743600.75) -> "2026-09-18T15:00:00Z"; -0.5 -> "1969-12-31T23:59:59Z";
    // -1.5 -> "1969-12-31T23:59:58Z" (same snippet with those dates).
    let event = WidgetEvent::new(
        "o",
        "o",
        Timestamp::from_millisecond(-500).unwrap(),
        Timestamp::from_millisecond(-1500).unwrap(),
        false,
        None,
        None,
    );
    let snapshot = WidgetSnapshot::new(
        Timestamp::from_millisecond(1_789_743_600_750).unwrap(),
        vec![event],
    );
    let written: Value = serde_json::to_value(snapshot).unwrap();
    assert_eq!(written["generatedAt"], json!("2026-09-18T15:00:00Z"));
    assert_eq!(written["events"][0]["start"], json!("1969-12-31T23:59:59Z"));
    assert_eq!(written["events"][0]["end"], json!("1969-12-31T23:59:58Z"));
}

#[test]
fn snapshot_json_reads_offsets_and_fractions_like_swift_iso8601() {
    // Swift's `.iso8601` decoder reads "2026-09-18T14:00:00+02:00" as 12:00Z and keeps a fraction
    // ("2026-09-18T12:00:00.500Z" is 1789732800.5).
    let offset: WidgetSnapshot =
        serde_json::from_str(r#"{"generatedAt":"2026-09-18T14:00:00+02:00","events":[]}"#).unwrap();
    assert_eq!(offset.generated_at, date("2026-09-18T12:00:00Z"));
    let fraction: WidgetSnapshot =
        serde_json::from_str(r#"{"generatedAt":"2026-09-18T12:00:00.500Z","events":[]}"#).unwrap();
    assert_eq!(fraction.generated_at.as_millisecond(), 1_789_732_800_500);
}

#[test]
fn snapshot_json_rejects_what_swift_rejects() {
    // A date that is not ISO 8601, a number (family A style) for a date, a missing isAllDay, a missing events list.
    for text in [
        r#"{"generatedAt":"2026-09-18 12:00:00","events":[]}"#,
        r#"{"generatedAt":"2026-09-18 12:00:00Z","events":[]}"#,
        r#"{"generatedAt":811000000,"events":[]}"#,
        r#"{"generatedAt":"2026-09-18T12:00:00Z"}"#,
        r#"{"generatedAt":"2026-09-18T12:00:00Z","events":[{"id":"a","title":"a","start":"2026-09-18T12:00:00Z","end":"2026-09-18T13:00:00Z"}]}"#,
    ] {
        assert!(
            serde_json::from_str::<WidgetSnapshot>(text).is_err(),
            "{text}"
        );
    }
}

#[test]
fn snapshot_json_ignores_unknown_fields() {
    let decoded: WidgetSnapshot =
        serde_json::from_str(r#"{"generatedAt":"2026-09-18T12:00:00Z","events":[],"future":1}"#)
            .unwrap();
    assert!(decoded.events.is_empty());
}
