//! Shorthand for the Swift `makeEvent(...)` calls of the takeover tests (Swift has default arguments and labels; Rust
//! spells the changed arguments out with `EventSpec { .. }`).

use calendar_core::model::ResponseStatus;
use calendar_core::url_string::UrlString;

use super::support::{EventSpec, make_event_with};
use crate::model::event::TimeTugCalendarEvent;

/// `makeEvent(id)`.
pub(super) fn ev(id: &str) -> TimeTugCalendarEvent {
    make_event_with(EventSpec {
        id: id.to_string(),
        ..EventSpec::default()
    })
}

/// `makeEvent(id, title: title)`.
pub(super) fn ev_titled(id: &str, title: &str) -> TimeTugCalendarEvent {
    make_event_with(EventSpec {
        id: id.to_string(),
        title: title.to_string(),
        ..EventSpec::default()
    })
}

/// `makeEvent(id, start: start)`.
pub(super) fn ev_at(id: &str, start: &str) -> TimeTugCalendarEvent {
    make_event_with(EventSpec {
        id: id.to_string(),
        start: start.to_string(),
        ..EventSpec::default()
    })
}

/// `makeEvent(start: start)`.
pub(super) fn ev_starting(start: &str) -> TimeTugCalendarEvent {
    ev_at("e1", start)
}

/// `makeEvent(minutes: minutes)`.
pub(super) fn ev_minutes(minutes: i64) -> TimeTugCalendarEvent {
    make_event_with(EventSpec {
        minutes,
        ..EventSpec::default()
    })
}

/// `makeEvent(calendarID: id)`.
pub(super) fn ev_on(calendar_id: &str) -> TimeTugCalendarEvent {
    make_event_with(EventSpec {
        calendar_id: calendar_id.to_string(),
        ..EventSpec::default()
    })
}

/// `makeEvent(status: status)`.
pub(super) fn ev_status(status: ResponseStatus) -> TimeTugCalendarEvent {
    make_event_with(EventSpec {
        status: Some(status),
        ..EventSpec::default()
    })
}

/// `makeEvent(others: n)`.
pub(super) fn ev_others(others: i64) -> TimeTugCalendarEvent {
    make_event_with(EventSpec {
        others,
        ..EventSpec::default()
    })
}

/// `makeEvent(conferenceURL: URL(string: url))`.
pub(super) fn ev_with_link(url: &str) -> TimeTugCalendarEvent {
    make_event_with(EventSpec {
        conference_url: Some(UrlString::parse(url).unwrap()),
        ..EventSpec::default()
    })
}

/// Swift's `MergedMember(event)` (an extension in `DuplicateRules.swift`, which is ported by Task 3): the member's
/// details are `DuplicateRules.detailSummary(event)`, which is "bare" for the events of the takeover tests (no
/// location, notes, link or attendees). The takeover rules read only `content_key`, so the details are inert here.
pub(super) fn merged_member(
    event: &TimeTugCalendarEvent,
) -> crate::model::merge_types::MergedMember {
    crate::model::merge_types::MergedMember::new(
        event.title(),
        event.calendar_key(),
        event.content_key(),
        "bare",
        event.start(),
        event.end(),
    )
}
