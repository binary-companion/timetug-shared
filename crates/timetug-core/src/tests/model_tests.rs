//! Port of `ModelTests.swift`.

use calendar_core::model::{Attendee, AttendeeRole, CalendarKind, ResponseStatus};
use jiff::Timestamp;

use super::support::{EventSpec, date, make_event, make_event_with};
use crate::model::calendar_info::CalendarInfo;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::merge_types::MergedMember;
use crate::model::takeover_settings::TakeoverSettings;

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).unwrap()
}

#[test]
fn event_id_includes_start_so_recurring_instances_differ() {
    let a = make_event_with(EventSpec {
        id: "series".into(),
        start: "2026-09-18T10:00:00Z".into(),
        ..Default::default()
    });
    let b = make_event_with(EventSpec {
        id: "series".into(),
        start: "2026-09-19T10:00:00Z".into(),
        ..Default::default()
    });
    assert_ne!(a.id(), b.id());
}

#[test]
fn calendar_key_combines_source_and_calendar() {
    let work = make_event_with(EventSpec {
        calendar_id: "work".into(),
        ..Default::default()
    });
    assert_eq!(work.calendar_key(), "fake/work");
    assert_eq!(CalendarInfo::key_for("fake", "work"), "fake/work");
}

#[test]
fn settings_round_trip_through_json() {
    let mut settings = TakeoverSettings::default();
    settings.lead_time = 300.0;
    settings.takeover_calendar_keys = ["a/b".to_string()].into();
    let data = serde_json::to_vec(&settings).unwrap();
    assert_eq!(
        serde_json::from_slice::<TakeoverSettings>(&data).unwrap(),
        settings
    );
}

#[test]
fn settings_defaults() {
    let settings = TakeoverSettings::default();
    assert_eq!(settings.lead_time, 60.0);
    assert!(settings.enabled);
    assert!(!settings.require_conference_link);
    assert!(!settings.require_other_attendees);
    assert!(settings.takeover_calendar_keys.is_empty());
    assert!(settings.skip_all_day_events);
}

#[test]
fn settings_decode_old_format_json_without_new_keys() {
    let json = br#"{"leadTime":300,"takeoverCalendarKeys":["a/b"]}"#;
    let settings: TakeoverSettings = serde_json::from_slice(json).unwrap();
    assert_eq!(settings.lead_time, 300.0);
    assert_eq!(settings.takeover_calendar_keys, ["a/b".to_string()].into());
    assert!(settings.hidden_calendar_keys.is_empty());
    assert!(!settings.require_conference_link);
    assert!(settings.enabled);
    assert!(!settings.require_other_attendees);
    assert!(settings.skip_all_day_events);
}

#[test]
fn content_keys_include_merged_members() {
    let mut event = make_event_with(EventSpec {
        id: "1".into(),
        title: "Doctor".into(),
        ..Default::default()
    });
    assert_eq!(event.all_content_keys(), [event.content_key()].into());
    event.merged_members = vec![MergedMember::new(
        "Intermountain Health",
        "fake/other",
        "k2",
        "bare",
        at(0),
        at(3600),
    )];
    assert_eq!(
        event.all_content_keys(),
        [event.content_key(), "k2".to_string()].into()
    );
}

#[test]
fn is_same_meeting_matches_any_member_key() {
    let armed = make_event_with(EventSpec {
        id: "1".into(),
        title: "Scott: Doctor".into(),
        ..Default::default()
    });
    let mut merged = make_event_with(EventSpec {
        id: "2".into(),
        title: "Intermountain Health".into(),
        ..Default::default()
    });
    assert!(!merged.is_same_meeting(&armed));
    merged.merged_members = vec![MergedMember::new(
        armed.title(),
        armed.calendar_key(),
        armed.content_key(),
        "bare",
        at(0),
        at(3600),
    )];
    assert!(merged.is_same_meeting(&armed));
    assert!(armed.is_same_meeting(&merged));
}

#[test]
fn library_attendee_normalizes_emails() {
    let attendee = |email: &str| {
        Attendee::new(
            None,
            Some(email),
            AttendeeRole::Required,
            ResponseStatus::NeedsAction,
            false,
            false,
        )
    };
    assert_eq!(
        attendee("  Kristin@Example.COM ").email(),
        Some("kristin@example.com")
    );
    assert_eq!(attendee("   ").email(), None);
}

#[test]
fn calendar_info_defaults_to_standard_kind() {
    assert_eq!(
        CalendarInfo::new("s", "c", "T").kind,
        CalendarKind::Standard
    );
    let mut subscribed = CalendarInfo::new("s", "c", "T");
    subscribed.kind = CalendarKind::Subscribed;
    assert_eq!(subscribed.kind, CalendarKind::Subscribed);
    // Kind is descriptive, like color: it is not part of the stored key.
    let mut birthdays = CalendarInfo::new("s", "c", "T");
    birthdays.kind = CalendarKind::Birthdays;
    assert_eq!(birthdays.key(), "s/c");
}

#[test]
fn make_event_helper_matches_the_swift_defaults() {
    // Guards the port of `Support.swift`: the other tests rely on these defaults.
    let event: TimeTugCalendarEvent = make_event();
    assert_eq!(event.source_id, "fake");
    assert_eq!(event.start(), date("2026-09-18T10:00:00Z"));
    assert_eq!(event.other_attendee_count, 1);
    assert_eq!(event.response_status, Some(ResponseStatus::Accepted));
}
