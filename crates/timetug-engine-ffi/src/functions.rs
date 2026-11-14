//! Pure functions of the engine's libraries that the Mac app (its EventKit adapter and its join policy) calls directly.
//! Each is a thin export; the logic stays in `calendar-core`.

use std::collections::HashSet;

use calendar_core::all_day::{AllDay, CalendarDate as EngineCalendarDate};
use calendar_core::calendar_user_address::CalendarUserAddress;
use calendar_core::conference_detector::ConferenceDetector;
use calendar_core::join_url_policy::JoinUrlPolicy;
use calendar_core::model::ConferenceInfo as EngineConferenceInfo;
use calendar_core::time::Zone;
use calendar_core::url_string::UrlString;
use calendar_core::write::patch_merge::PatchMerge;
use calendar_core::write::types::WriteValidation;
use jiff::Timestamp;

use crate::errors::FfiError;
use crate::values::model::{CalendarDate, CalendarEvent, ConferenceInfo, SourceCapabilities};
use crate::values::write::{EventField, EventPatch, EventRef};
use crate::values::{ToEngine, ToMirror};

/// `JoinURLPolicy.isAllowed`: whether the app may open a meeting link from calendar data.
#[uniffi::export]
pub fn join_url_is_allowed(url: UrlString) -> bool {
    JoinUrlPolicy::is_allowed(&url)
}

/// `CalendarUserAddress.email(from:)`: an attendee's `mailto:` address or URN as an email; `None` when it is not one.
#[uniffi::export]
pub fn calendar_user_address_email(address: Option<String>) -> Option<String> {
    CalendarUserAddress::email_from(address.as_deref())
}

/// `ConferenceDetector.conferences(structured:location:url:notes:)`.
#[uniffi::export]
pub fn detect_conferences(
    structured: Vec<ConferenceInfo>,
    location: Option<String>,
    url: Option<UrlString>,
    notes: Option<String>,
) -> Vec<ConferenceInfo> {
    let structured: Vec<EngineConferenceInfo> = structured.to_engine();
    ConferenceDetector::conferences(
        &structured,
        location.as_deref(),
        url.as_ref(),
        notes.as_deref(),
    )
    .to_mirror()
}

/// `WriteValidation.requireWritable`: `Unsupported` for the fields the source cannot write.
#[uniffi::export]
pub fn require_writable(
    fields: HashSet<EventField>,
    capabilities: SourceCapabilities,
) -> Result<(), FfiError> {
    WriteValidation::require_writable(&fields.to_engine(), &capabilities.to_engine())
        .map_err(|error| FfiError::Write(error.to_mirror()))
}

/// The two copies of an event compared as a patch that carries the first as its base (`EventPatch(from:to:)`).
#[uniffi::export]
pub fn event_patch_diff(original: CalendarEvent, edited: CalendarEvent) -> EventPatch {
    calendar_core::write::event_patch::EventPatch::diff(&original.to_engine(), &edited.to_engine())
        .to_mirror()
}

/// The fields of `patch` that someone else changed in `current` since the patch's base (`PatchMerge.conflicts`).
#[uniffi::export]
pub fn patch_merge_conflicts(patch: EventPatch, current: CalendarEvent) -> HashSet<EventField> {
    PatchMerge::conflicts(&patch.to_engine(), &current.to_engine()).to_mirror()
}

/// The reference a write needs, built from the event as read (`EventRef(event)`).
#[uniffi::export]
pub fn event_ref_from_event(event: CalendarEvent) -> EventRef {
    calendar_core::write::types::EventRef::from_event(&event.to_engine()).to_mirror()
}

/// The first day of an all-day span and the day after its last day (`AllDay.dates`: `first` and `endExclusive`).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AllDayDates {
    /// First day.
    pub first: CalendarDate,
    /// The day after the last day covered: a one-day event on 14 November has `first` 14 and `end_exclusive` 15.
    pub end_exclusive: CalendarDate,
}

/// `AllDay.dates(start:end:in:)`.
#[uniffi::export]
pub fn all_day_dates(start: Timestamp, end: Timestamp, zone: Zone) -> AllDayDates {
    let (first, end_exclusive) = AllDay::dates(start, end, &zone);
    AllDayDates {
        first: first.to_mirror(),
        end_exclusive: end_exclusive.to_mirror(),
    }
}

/// `AllDay.startOfDay(_:in:)`: the first instant of the day in the zone. A day that does not exist (31 February) resolves
/// forward, as Foundation does (3 March); `None` is for a value the calendar arithmetic rejects (a year beyond about
/// ±9999).
#[uniffi::export]
pub fn all_day_start_of_day(date: CalendarDate, zone: Zone) -> Option<Timestamp> {
    AllDay::start_of_day(&date.to_engine(), &zone)
}

/// `AllDay.dateOf`.
#[uniffi::export]
pub fn all_day_date_of(instant: Timestamp, zone: Zone) -> CalendarDate {
    AllDay::date_of(instant, &zone).to_mirror()
}

/// `AllDay.endExclusiveAfterLast`.
#[uniffi::export]
pub fn all_day_end_exclusive_after_last(last: CalendarDate) -> CalendarDate {
    let last: EngineCalendarDate = last.to_engine();
    AllDay::end_exclusive_after_last(&last).to_mirror()
}

/// `CalendarDate.adding(days:)`: the day `days` after (or, when negative, before) `date`, computed from noon UTC as the
/// Swift code does. A result outside the supported range leaves the date unchanged.
#[uniffi::export]
pub fn calendar_date_adding_days(date: CalendarDate, days: i64) -> CalendarDate {
    let date: EngineCalendarDate = date.to_engine();
    date.adding_days(days).to_mirror()
}
