//! Port of `TimeTugCalendarEventTests.swift`.

use calendar_core::model::{
    Attendee, AttendeeRole, CalendarEvent, ConferenceInfo, ConferenceOrigin, ConferenceProvider,
    Participation, ResponseStatus,
};
use calendar_core::time::add_seconds;
use calendar_core::url_string::UrlString;

use super::support::date;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::merge_types::MergedMember;

struct LibrarySpec {
    id: &'static str,
    start: &'static str,
    minutes: i64,
    attendees: Vec<Attendee>,
    organizer: Option<Attendee>,
    my_response: Option<ResponseStatus>,
    conference: Option<UrlString>,
}

impl Default for LibrarySpec {
    fn default() -> LibrarySpec {
        LibrarySpec {
            id: "e1",
            start: "2026-09-18T10:00:00Z",
            minutes: 30,
            attendees: Vec::new(),
            organizer: None,
            my_response: None,
            conference: None,
        }
    }
}

fn library(spec: LibrarySpec) -> CalendarEvent {
    let s = date(spec.start);
    let mut event = CalendarEvent::new(
        spec.id,
        "cal",
        "Standup",
        s,
        add_seconds(s, (spec.minutes * 60) as f64),
    );
    event.uid = Some("uid-1".to_string());
    event.attendees = spec.attendees;
    event.organizer = spec.organizer;
    event.conferences = spec
        .conference
        .map(|url| {
            vec![ConferenceInfo::new(
                url,
                ConferenceProvider::Other,
                ConferenceOrigin::Structured,
            )]
        })
        .unwrap_or_default();
    event.participation = spec.my_response.map(Participation::Invited);
    event
}

fn attendee(email: &str, response: ResponseStatus, is_self: bool, is_organizer: bool) -> Attendee {
    Attendee::new(
        None,
        Some(email),
        AttendeeRole::Required,
        response,
        is_self,
        is_organizer,
    )
}

#[test]
fn forwards_and_sets_the_events_fields() {
    let mut e = TimeTugCalendarEvent::new(library(LibrarySpec::default()), "src");
    assert!(
        e.title() == "Standup"
            && e.calendar_id() == "cal"
            && e.source_event_id() == "e1"
            && e.external_uid() == Some("uid-1")
    );
    e.set_start(date("2026-09-18T11:00:00Z"));
    e.set_location(Some("Room 4".to_string()));
    assert!(
        e.event.start == date("2026-09-18T11:00:00Z")
            && e.event.location.as_deref() == Some("Room 4")
    );
}

#[test]
fn derives_attendee_count_response_and_organizer_from_the_event() {
    let me = attendee("me@x.com", ResponseStatus::Tentative, true, false);
    let other = attendee("A@X.com", ResponseStatus::Accepted, false, false);
    let boss = attendee("boss@x.com", ResponseStatus::NeedsAction, false, true);
    let e = TimeTugCalendarEvent::new(
        library(LibrarySpec {
            attendees: vec![me, other],
            organizer: Some(boss),
            ..Default::default()
        }),
        "s",
    );
    assert_eq!(e.other_attendee_count, 1);
    assert_eq!(
        e.attendees().iter().map(|a| a.email()).collect::<Vec<_>>(),
        vec![Some("a@x.com")]
    );
    assert_eq!(e.response_status, Some(ResponseStatus::Tentative));
    assert_eq!(e.organizer_email(), Some("boss@x.com"));
}

#[test]
fn my_response_wins_and_self_organizer_is_hidden() {
    let me = attendee("me@x.com", ResponseStatus::NeedsAction, true, true);
    let e = TimeTugCalendarEvent::new(
        library(LibrarySpec {
            attendees: vec![me.clone()],
            organizer: Some(me),
            my_response: Some(ResponseStatus::Accepted),
            ..Default::default()
        }),
        "s",
    );
    assert_eq!(e.response_status, Some(ResponseStatus::Accepted));
    assert_eq!(e.organizer_email(), None);
    assert_eq!(
        TimeTugCalendarEvent::new(library(LibrarySpec::default()), "s").response_status,
        None
    );
}

#[test]
fn conferences_start_from_the_event_and_the_first_is_the_join_url() {
    let url = UrlString::parse("https://meet.google.com/aaa-bbbb-ccc").unwrap();
    let mut e = TimeTugCalendarEvent::new(
        library(LibrarySpec {
            conference: Some(url.clone()),
            ..Default::default()
        }),
        "s",
    );
    assert_eq!(e.conference_url(), Some(&url));
    e.conferences = Vec::new();
    assert_eq!(e.conference_url(), None);
    assert_eq!(e.event.conferences.first().map(|c| &c.url), Some(&url));
}

#[test]
fn is_same_meeting_matches_by_id_or_by_any_merged_content_key() {
    let a = TimeTugCalendarEvent::new(
        library(LibrarySpec {
            id: "e1",
            ..Default::default()
        }),
        "src",
    );
    let same_occurrence = TimeTugCalendarEvent::new(
        library(LibrarySpec {
            id: "e1",
            ..Default::default()
        }),
        "src",
    );
    let same_content_other_id = TimeTugCalendarEvent::new(
        library(LibrarySpec {
            id: "e9",
            ..Default::default()
        }),
        "other",
    );
    let mut merged = TimeTugCalendarEvent::new(
        library(LibrarySpec {
            id: "m",
            start: "2026-09-18T12:00:00Z",
            ..Default::default()
        }),
        "src",
    );
    merged.merged_members = vec![MergedMember::new(
        "Standup",
        "src/cal",
        a.content_key(),
        "bare",
        a.start(),
        a.end(),
    )];
    assert!(a.is_same_meeting(&same_occurrence));
    assert!(a.is_same_meeting(&same_content_other_id)); // same title, start and end
    assert!(merged.is_same_meeting(&a)); // a merged copy's content matches
    let other_time = TimeTugCalendarEvent::new(
        library(LibrarySpec {
            id: "e2",
            start: "2026-09-18T15:00:00Z",
            ..Default::default()
        }),
        "src",
    );
    assert!(!a.is_same_meeting(&other_time));
}

#[test]
fn id_and_content_key_keep_their_formulas() {
    let e = TimeTugCalendarEvent::new(library(LibrarySpec::default()), "src");
    let start = date("2026-09-18T10:00:00Z").as_second();
    let end = date("2026-09-18T10:30:00Z").as_second();
    assert_eq!(e.id(), format!("src/e1/{start}"));
    assert_eq!(e.content_key(), format!("standup|{start}|{end}"));
    assert_eq!(e.calendar_key(), "src/cal");
}
