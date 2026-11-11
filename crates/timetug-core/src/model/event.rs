use std::collections::BTreeSet;

use calendar_core::model::{Attendee, CalendarEvent, ConferenceInfo, ResponseStatus, UidScope};
use calendar_core::time::Zone;
use calendar_core::url_string::UrlString;
use jiff::Timestamp;

use crate::model::calendar_info::CalendarInfo;
use crate::model::merge_types::{MergeProvenance, MergedMember};
use crate::text::lowercased;

/// A library event as TimeTug sees it: the provider's event plus the source it came from and the state Core adds
/// (merged duplicates, the join link Core detects, the best attendance across copies). Times, all-day form and
/// attendees are the library's own; nothing is converted per refresh.
///
/// The library event's fields are forwarded through accessor methods named like the Swift properties in snake case
/// (`title()` and `set_title(..)`); the type does not implement `Deref`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TimeTugCalendarEvent {
    pub event: CalendarEvent,
    pub source_id: String,
    /// The join links, most likely first: the library's own list (structured links, then links found in the text),
    /// merged across copies by the duplicate resolver.
    pub conferences: Vec<ConferenceInfo>,
    /// Non-self attendees; the merge step raises it to the largest count across a group.
    pub other_attendee_count: i64,
    /// The account owner's response; `None` when the provider does not say. The merge step keeps the best across a group.
    pub response_status: Option<ResponseStatus>,
    /// Calendar keys of the other calendars where duplicate copies of this meeting appear.
    pub additional_calendar_keys: BTreeSet<String>,
    /// Every original copy folded into this event (including itself); empty when never merged.
    pub merged_members: Vec<MergedMember>,
    pub merge_provenance: Option<MergeProvenance>,
    /// The connector and host of this event's calendar (the raw `CalendarService` and `CalendarProvider` values), filled
    /// by the store from `CalendarInfo`; used only to decide where a `uid` is comparable.
    pub calendar_service: Option<String>,
    pub calendar_provider: Option<String>,
    /// Where the range shown to the user starts when it differs from `start` (a merged meeting shows the
    /// longer copy's range while `start` is the tug time); `None` means the same as `start`.
    pub display_start: Option<Timestamp>,
}

impl TimeTugCalendarEvent {
    pub fn new(event: CalendarEvent, source_id: impl Into<String>) -> TimeTugCalendarEvent {
        let conferences = event.conferences.clone();
        let other_attendee_count = event.attendees.iter().filter(|a| !a.is_self).count() as i64;
        let response_status = event.my_response().or_else(|| {
            event
                .attendees
                .iter()
                .find(|a| a.is_self)
                .map(|a| a.response)
        });
        TimeTugCalendarEvent {
            event,
            source_id: source_id.into(),
            conferences,
            other_attendee_count,
            response_status,
            additional_calendar_keys: BTreeSet::new(),
            merged_members: Vec::new(),
            merge_provenance: None,
            display_start: None,
            calendar_service: None,
            calendar_provider: None,
        }
    }

    /// The first join link.
    pub fn conference_url(&self) -> Option<&UrlString> {
        self.conferences.first().map(|c| &c.url)
    }

    pub fn title(&self) -> &str {
        &self.event.title
    }
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.event.title = title.into();
    }
    pub fn start(&self) -> Timestamp {
        self.event.start
    }
    pub fn set_start(&mut self, start: Timestamp) {
        self.event.start = start;
    }
    pub fn end(&self) -> Timestamp {
        self.event.end
    }
    pub fn set_end(&mut self, end: Timestamp) {
        self.event.end = end;
    }
    /// All-day events use the library's canonical form: midnight of the first day in `time_zone`, `end` exclusive.
    pub fn is_all_day(&self) -> bool {
        self.event.is_all_day
    }
    pub fn set_is_all_day(&mut self, is_all_day: bool) {
        self.event.is_all_day = is_all_day;
    }
    pub fn time_zone(&self) -> &Zone {
        &self.event.time_zone
    }
    pub fn set_time_zone(&mut self, zone: Zone) {
        self.event.time_zone = zone;
    }
    pub fn location(&self) -> Option<&str> {
        self.event.location.as_deref()
    }
    pub fn set_location(&mut self, location: Option<String>) {
        self.event.location = location;
    }
    pub fn notes(&self) -> Option<&str> {
        self.event.notes.as_deref()
    }
    pub fn set_notes(&mut self, notes: Option<String>) {
        self.event.notes = notes;
    }
    pub fn url(&self) -> Option<&UrlString> {
        self.event.url.as_ref()
    }
    pub fn set_url(&mut self, url: Option<UrlString>) {
        self.event.url = url;
    }
    pub fn calendar_id(&self) -> &str {
        &self.event.calendar_id
    }
    pub fn set_calendar_id(&mut self, calendar_id: impl Into<String>) {
        self.event.calendar_id = calendar_id.into();
    }

    pub fn source_event_id(&self) -> &str {
        &self.event.event_id
    }
    pub fn external_uid(&self) -> Option<&str> {
        self.event.uid.as_deref()
    }
    /// What two events' `uid`s are compared by. A `Global` uid (an iCalendar UID) matches across any source; a
    /// provider-specific one (Exchange) or an unknown scope only matches events from the same service and provider.
    /// `None` when there is no uid.
    pub fn uid_match_key(&self) -> Option<String> {
        let uid = self.event.uid.as_deref().filter(|uid| !uid.is_empty())?;
        if self.event.uid_scope == Some(UidScope::Global) {
            return Some(uid.to_string());
        }
        Some(format!(
            "{}|{}|{}",
            self.calendar_service.as_deref().unwrap_or("?"),
            self.calendar_provider.as_deref().unwrap_or("?"),
            uid
        ))
    }
    /// Attendees other than the calendar owner.
    pub fn attendees(&self) -> Vec<&Attendee> {
        self.event.attendees.iter().filter(|a| !a.is_self).collect()
    }
    pub fn organizer_email(&self) -> Option<&str> {
        self.event
            .organizer
            .as_ref()
            .and_then(|o| if o.is_self { None } else { o.email() })
    }

    /// The start of the range to display: `display_start` when set, else `start`.
    pub fn shown_start(&self) -> Timestamp {
        self.display_start.unwrap_or_else(|| self.start())
    }

    /// Unique per occurrence: recurring events share a source id but differ in start.
    pub fn id(&self) -> String {
        format!(
            "{}/{}/{}",
            self.source_id,
            self.source_event_id(),
            unix_seconds(self.start())
        )
    }
    /// Identity by content (title, start, end): survives a changed `source_event_id`, matching the
    /// store's duplicate merge.
    pub fn content_key(&self) -> String {
        format!(
            "{}|{}|{}",
            lowercased(self.title()),
            unix_seconds(self.start()),
            unix_seconds(self.end())
        )
    }
    pub fn calendar_key(&self) -> String {
        CalendarInfo::key_for(&self.source_id, self.calendar_id())
    }
    /// This event's own calendar plus every calendar its duplicates appear on.
    pub fn all_calendar_keys(&self) -> BTreeSet<String> {
        let mut keys = self.additional_calendar_keys.clone();
        keys.insert(self.calendar_key());
        keys
    }

    /// Content keys of this event and every copy merged into it.
    pub fn all_content_keys(&self) -> BTreeSet<String> {
        let mut keys: BTreeSet<String> = self
            .merged_members
            .iter()
            .map(|m| m.content_key.clone())
            .collect();
        keys.insert(self.content_key());
        keys
    }

    /// True for the same occurrence or when any merged copy's content matches (an armed timer's
    /// event may since have been merged into another, or split back out).
    pub fn is_same_meeting(&self, other: &TimeTugCalendarEvent) -> bool {
        self.id() == other.id()
            || !self
                .all_content_keys()
                .is_disjoint(&other.all_content_keys())
    }
}

/// Swift's `Int(date.timeIntervalSince1970)`: whole seconds, truncated toward zero (`Timestamp::as_second` truncates
/// toward zero as well, because jiff gives the second and the sub-second part the same sign).
fn unix_seconds(t: Timestamp) -> i64 {
    t.as_second()
}
