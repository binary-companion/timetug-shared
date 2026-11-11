//! Port of `TimeTugCoreTests/Support.swift`: the helpers the Swift tests share. Helpers for the calendar store
//! (`FakeSource`, `utcCalendar`, `calendar(in:)`) arrive with the tasks that use them.

use calendar_core::all_day::{AllDay, CalendarDate};
use calendar_core::conference_detector::ConferenceDetector;
use calendar_core::model::{
    Attendee, CalendarEvent, ConferenceInfo, ConferenceOrigin, ConferenceProvider, ResponseStatus,
    UidScope,
};
use calendar_core::time::{Zone, add_seconds};
use calendar_core::url_string::UrlString;
use jiff::Timestamp;

use crate::model::event::TimeTugCalendarEvent;
use crate::model::takeover_settings::TakeoverSettings;

/// `ISO8601DateFormatter().date(from:)` for the form "2026-09-18T09:00:00Z".
pub(super) fn date(iso: &str) -> Timestamp {
    iso.parse().unwrap()
}

/// `TimeZone(identifier:)!`.
pub(super) fn zone(identifier: &str) -> Zone {
    Zone::named(identifier).unwrap()
}

/// The arguments of Swift's `makeEvent(...)`, each at its Swift default.
pub(super) struct EventSpec {
    pub id: String,
    pub title: String,
    pub start: String,
    pub minutes: i64,
    pub calendar_id: String,
    pub is_all_day: bool,
    pub others: i64,
    pub status: Option<ResponseStatus>,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub url: Option<UrlString>,
    pub conference_url: Option<UrlString>,
    pub attendees: Vec<Attendee>,
    pub external_uid: Option<String>,
    pub uid_scope: Option<UidScope>,
    pub service: Option<String>,
    pub provider: Option<String>,
}

impl Default for EventSpec {
    fn default() -> EventSpec {
        EventSpec {
            id: "e1".to_string(),
            title: "Standup".to_string(),
            start: "2026-09-18T10:00:00Z".to_string(),
            minutes: 30,
            calendar_id: "cal".to_string(),
            is_all_day: false,
            others: 1,
            status: Some(ResponseStatus::Accepted),
            location: None,
            notes: None,
            url: None,
            conference_url: None,
            attendees: Vec::new(),
            external_uid: None,
            uid_scope: None,
            service: None,
            provider: None,
        }
    }
}

/// `makeEvent()` with every default.
pub(super) fn make_event() -> TimeTugCalendarEvent {
    make_event_with(EventSpec::default())
}

/// `makeEvent("id", ...)`: an event from the `fake` source with the given arguments.
pub(super) fn make_event_with(spec: EventSpec) -> TimeTugCalendarEvent {
    let start = date(&spec.start);
    let mut base = CalendarEvent::new(
        spec.id,
        spec.calendar_id,
        spec.title,
        start,
        add_seconds(start, (spec.minutes * 60) as f64),
    );
    base.notes = spec.notes.clone();
    base.location = spec.location.clone();
    base.uid = spec.external_uid;
    base.uid_scope = spec.uid_scope;
    base.time_zone = zone("UTC");
    base.is_all_day = spec.is_all_day;
    base.attendees = spec.attendees;
    base.url = spec.url.clone();
    let mut event = TimeTugCalendarEvent::new(base, "fake");
    event.other_attendee_count = spec.others;
    event.calendar_service = spec.service;
    event.calendar_provider = spec.provider;
    event.response_status = spec.status;
    // What a connector does: the provider's link first, then links found in the text.
    let structured: Vec<ConferenceInfo> = spec
        .conference_url
        .map(|url| {
            let provider =
                ConferenceDetector::provider_of(&url).unwrap_or(ConferenceProvider::Other);
            vec![ConferenceInfo::new(
                url,
                provider,
                ConferenceOrigin::Structured,
            )]
        })
        .unwrap_or_default();
    event.conferences = ConferenceDetector::conferences(
        &structured,
        spec.location.as_deref(),
        spec.url.as_ref(),
        spec.notes.as_deref(),
    );
    event
}

/// Settings with the default test calendar ("fake/cal") opted in for takeovers.
#[allow(dead_code)] // used by the takeover tests of Task 2
pub(super) fn opted_in(configure: impl FnOnce(&mut TakeoverSettings)) -> TakeoverSettings {
    let mut settings = TakeoverSettings::default();
    settings
        .takeover_calendar_keys
        .insert("fake/cal".to_string());
    configure(&mut settings);
    settings
}

/// A "now" at which test ledger entries are recorded (before the default test events).
#[allow(dead_code)] // used by the takeover tests of Task 2
pub(super) fn recorded_at() -> Timestamp {
    date("2026-09-18T09:00:00Z")
}

/// A canonical all-day event: `first` to `end_exclusive` (calendar dates) in `zone_id`.
pub(super) fn make_all_day(
    zone_id: &str,
    first: CalendarDate,
    end_exclusive: CalendarDate,
) -> TimeTugCalendarEvent {
    let tz = zone(zone_id);
    let (start, end) = AllDay::canonical(&first, &end_exclusive, &tz).unwrap();
    let mut base = CalendarEvent::new("d", "cal", "Holiday", start, end);
    base.time_zone = tz;
    base.is_all_day = true;
    TimeTugCalendarEvent::new(base, "fake")
}

pub(super) fn day(y: i64, m: i64, d: i64) -> CalendarDate {
    CalendarDate::new(y, m, d)
}
