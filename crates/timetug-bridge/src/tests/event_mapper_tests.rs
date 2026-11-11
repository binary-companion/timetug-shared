//! Port of `Tests/CalendarBridgeTests/EventMapperTests.swift`.

use calendar_core::all_day::{AllDay, CalendarDate};
use calendar_core::model::{
    Attendee, AttendeeRole, CalendarDescriptor, CalendarEvent, CalendarKind, CalendarProvider,
    CalendarService, ConferenceInfo, ConferenceOrigin, ConferenceProvider, EventStatus,
    Participation, ResponseStatus,
};
use calendar_core::time::Zone;
use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use timetug_core::CalendarInfo;
use timetug_core::dedup::rules::{DuplicateRules, MergeReason, PairDecision};

use crate::event_mapper::EventMapper;

fn zone(id: &str) -> Zone {
    Zone::named(id).unwrap()
}

/// `ISO8601DateFormatter().date(from:)`.
fn iso(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn mapper() -> EventMapper {
    EventMapper::new()
}

fn url(text: &str) -> UrlString {
    UrlString::parse(text).unwrap()
}

fn timed(configure: impl FnOnce(&mut CalendarEvent)) -> CalendarEvent {
    let mut e = CalendarEvent::new(
        "e1",
        "cal",
        "Standup",
        iso("2026-09-18T14:00:00Z"),
        iso("2026-09-18T14:30:00Z"),
    );
    e.uid = Some("uid-1".to_string());
    e.notes = Some("n".to_string());
    e.location = Some("Room 1".to_string());
    e.url = Some(url("https://x.test/e"));
    configure(&mut e);
    e
}

fn all_day(first: (i64, i64, i64), end_exclusive: (i64, i64, i64), zone_id: &str) -> CalendarEvent {
    let z = zone(zone_id);
    let (start, end) = AllDay::canonical(
        &CalendarDate {
            year: first.0,
            month: first.1,
            day: first.2,
        },
        &CalendarDate {
            year: end_exclusive.0,
            month: end_exclusive.1,
            day: end_exclusive.2,
        },
        &z,
    )
    .unwrap();
    let mut e = CalendarEvent::new("d", "cal", "Holiday", start, end);
    e.time_zone = z;
    e.is_all_day = true;
    e
}

#[test]
fn wraps_timed_event_unchanged() {
    let source = timed(|e| {
        e.conferences = vec![ConferenceInfo::new(
            url("https://meet.example/x"),
            ConferenceProvider::Meet,
            ConferenceOrigin::Structured,
        )];
    });
    let mapped = mapper().event(source.clone(), "google-1").unwrap();
    assert_eq!(mapped.event, source);
    assert_eq!(mapped.source_id, "google-1");
}

#[test]
fn all_day_events_pass_through_unchanged() {
    let source = all_day((2026, 9, 21), (2026, 9, 22), "Asia/Tokyo");
    let mapped = mapper().event(source.clone(), "google-1").unwrap();
    assert_eq!(mapped.event, source);
    assert!(mapped.event.start == source.start && mapped.event.end == source.end);
    assert_eq!(mapped.source_id, "google-1");
}

#[test]
fn cancelled_events_are_dropped() {
    assert!(
        mapper()
            .event(timed(|e| e.status = EventStatus::Cancelled), "s")
            .is_none()
    );
}

#[test]
fn titles_pass_through_unchanged_including_empty() {
    let mapped = mapper()
        .event(timed(|e| e.title = String::new()), "s")
        .unwrap();
    assert_eq!(mapped.title(), "");
}

#[test]
fn maps_calendar_descriptors() {
    let mut d = CalendarDescriptor::new("c", "Work", CalendarService::google())
        .with_color_hex(Some("#abc"));
    d.account_name = Some("me@x.test".to_string());
    d.provider = Some(CalendarProvider::google());
    let info = mapper().calendar_info(&d, "google-1");
    let mut expected = CalendarInfo::new("google-1", "c", "Work").with_color_hex(Some("#AABBCC"));
    expected.account_name = Some("me@x.test".to_string());
    expected.service = Some(CalendarService::google());
    expected.provider = Some(CalendarProvider::google());
    assert_eq!(info, expected);
}

#[test]
fn event_kit_and_google_shaped_events_agree_on_shared_keys_and_dedup_as_exact() {
    let me = Attendee::new(
        Some("Me".to_string()),
        Some("me@x.test"),
        AttendeeRole::Required,
        ResponseStatus::Accepted,
        true,
        false,
    );
    let other = Attendee::new(
        Some("Bo".to_string()),
        Some("bo@x.test"),
        AttendeeRole::Required,
        ResponseStatus::NeedsAction,
        false,
        false,
    );
    fn library(configure: impl FnOnce(&mut CalendarEvent)) -> CalendarEvent {
        let mut e = CalendarEvent::new(
            "id",
            "cal",
            "Design Review",
            iso("2026-09-18T14:00:00Z"),
            iso("2026-09-18T14:30:00Z"),
        );
        e.uid = Some("UID-1".to_string());
        configure(&mut e);
        e
    }
    let event_kit = library(|e| e.attendees = vec![me.clone(), other.clone()]);
    let google = library(|e| {
        e.attendees = vec![other.clone()];
        e.participation = Some(Participation::Invited(ResponseStatus::Accepted));
    });
    let a = mapper().event(event_kit, "eventkit").unwrap();
    let b = mapper().event(google, "google-1").unwrap();
    assert!(a.external_uid() == Some("UID-1") && a.external_uid() == b.external_uid());
    assert!(a.title() == b.title() && a.start() == b.start() && a.end() == b.end());
    assert_eq!(a.content_key(), b.content_key());
    assert_ne!(a.source_id, b.source_id);
    assert_eq!(
        DuplicateRules::decide(&a, &b),
        PairDecision::Merge(MergeReason::ExactMatch)
    );
}

#[test]
fn calendar_info_carries_the_calendar_kind() {
    let mut d = CalendarDescriptor::new("c", "Birthdays", CalendarService::google());
    d.kind = CalendarKind::Birthdays;
    assert_eq!(
        mapper().calendar_info(&d, "eventkit").kind,
        CalendarKind::Birthdays
    );
    assert_eq!(
        mapper()
            .calendar_info(
                &CalendarDescriptor::new("c", "Work", CalendarService::google()),
                "eventkit"
            )
            .kind,
        CalendarKind::Standard
    );
}
