//! Helpers for the duplicate-detection tests that Swift writes inline: a chainable form of `makeEvent(...)` (the Swift
//! call with labelled arguments), `makeAllDay(...)` with its id, title and calendar, `Attendee(email:)` and `URL(string:)`.

use calendar_core::all_day::{AllDay, CalendarDate};
use calendar_core::model::{
    Attendee, AttendeeRole, CalendarEvent, ConferenceInfo, ResponseStatus, UidScope,
};
use calendar_core::url_string::UrlString;
use jiff::Timestamp;

use super::support::{EventSpec, make_event_with, zone};
use crate::model::event::TimeTugCalendarEvent;

/// `URL(string:)!`.
pub(super) fn url(text: &str) -> UrlString {
    UrlString::parse(text).unwrap()
}

/// `CalendarCore.Attendee(email:)`.
pub(super) fn attendee(email: &str) -> Attendee {
    Attendee::new(
        None,
        Some(email),
        AttendeeRole::Required,
        ResponseStatus::NeedsAction,
        false,
        false,
    )
}

/// `CalendarCore.Attendee(name:email:)`.
pub(super) fn named_attendee(name: Option<&str>, email: Option<&str>) -> Attendee {
    Attendee::new(
        name.map(str::to_string),
        email,
        AttendeeRole::Required,
        ResponseStatus::NeedsAction,
        false,
        false,
    )
}

/// The labelled arguments of `makeEvent`, set one by one: `ev("1").title("Sync").cal("work").build()`.
pub(super) struct Ev(EventSpec);

pub(super) fn ev(id: &str) -> Ev {
    Ev(EventSpec {
        id: id.to_string(),
        ..EventSpec::default()
    })
}

impl Ev {
    pub fn title(mut self, title: &str) -> Ev {
        self.0.title = title.to_string();
        self
    }
    pub fn start(mut self, start: &str) -> Ev {
        self.0.start = start.to_string();
        self
    }
    pub fn minutes(mut self, minutes: i64) -> Ev {
        self.0.minutes = minutes;
        self
    }
    pub fn cal(mut self, calendar_id: &str) -> Ev {
        self.0.calendar_id = calendar_id.to_string();
        self
    }
    pub fn all_day(mut self) -> Ev {
        self.0.is_all_day = true;
        self
    }
    pub fn others(mut self, others: i64) -> Ev {
        self.0.others = others;
        self
    }
    pub fn status(mut self, status: Option<ResponseStatus>) -> Ev {
        self.0.status = status;
        self
    }
    pub fn location(mut self, location: &str) -> Ev {
        self.0.location = Some(location.to_string());
        self
    }
    pub fn notes(mut self, notes: &str) -> Ev {
        self.0.notes = Some(notes.to_string());
        self
    }
    pub fn url(mut self, link: &str) -> Ev {
        self.0.url = Some(url(link));
        self
    }
    pub fn conf(mut self, link: &str) -> Ev {
        self.0.conference_url = Some(url(link));
        self
    }
    pub fn attendees(mut self, attendees: Vec<Attendee>) -> Ev {
        self.0.attendees = attendees;
        self
    }
    pub fn uid(mut self, uid: &str) -> Ev {
        self.0.external_uid = Some(uid.to_string());
        self
    }
    pub fn scope(mut self, scope: UidScope) -> Ev {
        self.0.uid_scope = Some(scope);
        self
    }
    pub fn service(mut self, service: &str) -> Ev {
        self.0.service = Some(service.to_string());
        self
    }
    pub fn provider(mut self, provider: &str) -> Ev {
        self.0.provider = Some(provider.to_string());
        self
    }
    pub fn build(self) -> TimeTugCalendarEvent {
        make_event_with(self.0)
    }
}

/// `makeAllDay(id, zone:, first:, endExclusive:, title:, calendarID:)`.
pub(super) fn make_all_day_in(
    id: &str,
    zone_id: &str,
    first: CalendarDate,
    end_exclusive: CalendarDate,
    title: &str,
    calendar_id: &str,
) -> TimeTugCalendarEvent {
    let tz = zone(zone_id);
    let (start, end) = AllDay::canonical(&first, &end_exclusive, &tz).unwrap();
    let mut base = CalendarEvent::new(id, calendar_id, title, start, end);
    base.time_zone = tz;
    base.is_all_day = true;
    TimeTugCalendarEvent::new(base, "fake")
}

/// `Date(timeIntervalSince1970: seconds)`.
pub(super) fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).unwrap()
}

/// The identities of an event's conference links, in order.
pub(super) fn identities(conferences: &[ConferenceInfo]) -> Vec<String> {
    conferences.iter().map(ConferenceInfo::identity).collect()
}
