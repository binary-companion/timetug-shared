//! `Fixture` of `Support.swift`: builders shared by the protocol tests. Times are fixed so expected strings are
//! literals.

use calendar_core::all_day::{AllDay, CalendarDate};
use calendar_core::conference_detector::ConferenceDetector;
use calendar_core::model::{CalendarEvent, ConferenceInfo, ConferenceOrigin, ConferenceProvider};
use calendar_core::time::{Zone, add_seconds};
use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use timetug_core::TimeTugCalendarEvent;

use super::ts;

pub(crate) fn utc() -> Zone {
    Zone::named("UTC").unwrap()
}

/// The default event zone: a named IANA zone, because Foundation spells the UTC zone "GMT" on some platforms.
pub(crate) fn berlin() -> Zone {
    Zone::named("Europe/Berlin").unwrap()
}

/// Swift's `Date(timeIntervalSince1970: 1_760_000_000)`.
pub(crate) fn default_start() -> Timestamp {
    ts(1_760_000_000)
}

/// `Fixture.timed` with every argument at its default.
pub(crate) fn timed(id: &str) -> TimeTugCalendarEvent {
    timed_with(
        id,
        "Design review",
        default_start(),
        30,
        "cal",
        "fake",
        None,
    )
}

pub(crate) fn timed_with(
    id: &str,
    title: &str,
    start: Timestamp,
    minutes: i64,
    calendar_id: &str,
    source: &str,
    conference: Option<&str>,
) -> TimeTugCalendarEvent {
    let mut calendar_event = CalendarEvent::new(
        id,
        calendar_id,
        title,
        start,
        add_seconds(start, (minutes * 60) as f64),
    );
    calendar_event.time_zone = berlin();
    let mut event = TimeTugCalendarEvent::new(calendar_event, source);
    if let Some(url) = conference.and_then(UrlString::parse) {
        let provider = ConferenceDetector::provider_of(&url).unwrap_or(ConferenceProvider::Other);
        event.conferences = vec![ConferenceInfo::new(
            url,
            provider,
            ConferenceOrigin::Structured,
        )];
    }
    event
}

/// 2026-10-16 as an all-day event in `zone` (canonical form: local midnight, end exclusive).
pub(crate) fn all_day(id: &str, zone: &Zone) -> TimeTugCalendarEvent {
    let first = CalendarDate::new(2026, 10, 16);
    let (start, end) = AllDay::canonical(&first, &first.adding_days(1), zone).unwrap();
    let mut calendar_event = CalendarEvent::new(id, "cal", "Holiday", start, end);
    calendar_event.time_zone = zone.clone();
    calendar_event.is_all_day = true;
    TimeTugCalendarEvent::new(calendar_event, "fake")
}
