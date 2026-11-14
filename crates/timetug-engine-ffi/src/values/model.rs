//! Mirrors of `calendar_core::model`, the source types, the recurrence types and the `timetug_core` model.
#![allow(missing_docs)]

use calendar_core::all_day::CalendarDate as EngineCalendarDate;
use calendar_core::model as eng;
use calendar_core::recurrence::rule;
use calendar_core::recurrence::set;
use calendar_core::source_types as src;
use jiff::Timestamp;
use timetug_core::model::merge_types as merge;
use timetug_core::sources::SourceStatus as EngineSourceStatus;

use super::write::{EventField, RecurrenceScope};
use super::{ToEngine, ToMirror, mirror_record, mirror_unit_enum};
use calendar_core::time::Zone;
use calendar_core::url_string::UrlString;

mirror_unit_enum!(CalendarKind <=> eng::CalendarKind { Standard, Subscribed, Birthdays });
mirror_unit_enum!(AccessRole <=> eng::AccessRole { Owner, Writer, Reader, FreeBusyReader });
mirror_unit_enum!(EventStatus <=> eng::EventStatus { Confirmed, Tentative, Cancelled });
mirror_unit_enum!(Availability <=> eng::Availability { Busy, Free, Tentative, Unavailable });
mirror_unit_enum!(Visibility <=> eng::Visibility { Default, PublicEvent, PrivateEvent, Confidential });
mirror_unit_enum!(EventKind <=> eng::EventKind { Standard, FocusTime, OutOfOffice, WorkingLocation, Birthday, Other });
mirror_unit_enum!(UidScope <=> eng::UidScope { Global, Provider });
mirror_unit_enum!(ResponseStatus <=> eng::ResponseStatus { Accepted, Tentative, Declined, NeedsAction });
mirror_unit_enum!(AttendeeRole <=> eng::AttendeeRole { Required, Optional, Resource });
mirror_unit_enum!(ConferenceProvider <=> eng::ConferenceProvider { Meet, Teams, Zoom, Webex, GoToMeeting, Whereby, Jitsi, Slack, Other });
mirror_unit_enum!(ConferenceOrigin <=> eng::ConferenceOrigin { Structured, Location, Url, Notes, EventUrl });
mirror_unit_enum!(ReminderAnchor <=> eng::ReminderAnchor { Start, End });
mirror_unit_enum!(ReminderProximity <=> eng::ReminderProximity { Enter, Leave });
mirror_unit_enum!(SyncKind <=> src::SyncKind { None, Token, Notification });
mirror_unit_enum!(SourceNoticeKind <=> src::SourceNoticeKind { UnreadableRecurrence });
mirror_unit_enum!(ProvidedField <=> src::ProvidedField {
    Kind, Visibility, Availability, Reminders, Series, Participation, StructuredConference, Version, LastModified,
    Created, UidScope, RecurrenceRules, IsDefault, CalendarTimeZone, DefaultReminders, Provider, SupportedAvailabilities,
    PermissionDetails
});
mirror_unit_enum!(Frequency <=> rule::Frequency { Secondly, Minutely, Hourly, Daily, Weekly, Monthly, Yearly });
mirror_unit_enum!(Weekday <=> rule::Weekday { Monday, Tuesday, Wednesday, Thursday, Friday, Saturday, Sunday });

/// Whether an event repeats.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SeriesInfo {
    NotRecurring,
    Occurrence {
        series_id: String,
        original_start: Option<Timestamp>,
    },
}
impl ToMirror for eng::SeriesInfo {
    type Mirror = SeriesInfo;
    fn to_mirror(self) -> SeriesInfo {
        match self {
            eng::SeriesInfo::NotRecurring => SeriesInfo::NotRecurring,
            eng::SeriesInfo::Occurrence {
                series_id,
                original_start,
            } => SeriesInfo::Occurrence {
                series_id,
                original_start,
            },
        }
    }
}
impl ToEngine for SeriesInfo {
    type Engine = eng::SeriesInfo;
    fn to_engine(self) -> eng::SeriesInfo {
        match self {
            SeriesInfo::NotRecurring => eng::SeriesInfo::NotRecurring,
            SeriesInfo::Occurrence {
                series_id,
                original_start,
            } => eng::SeriesInfo::Occurrence {
                series_id,
                original_start,
            },
        }
    }
}

/// The account owner's relation to an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Participation {
    NotInvited,
    Invited(ResponseStatus),
}
impl ToMirror for eng::Participation {
    type Mirror = Participation;
    fn to_mirror(self) -> Participation {
        match self {
            eng::Participation::NotInvited => Participation::NotInvited,
            eng::Participation::Invited(status) => Participation::Invited(status.to_mirror()),
        }
    }
}
impl ToEngine for Participation {
    type Engine = eng::Participation;
    fn to_engine(self) -> eng::Participation {
        match self {
            Participation::NotInvited => eng::Participation::NotInvited,
            Participation::Invited(status) => eng::Participation::Invited(status.to_engine()),
        }
    }
}

/// An attendee. The engine normalises the address (trimmed, lower case, empty is none) on the way back in.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Attendee {
    pub name: Option<String>,
    pub email: Option<String>,
    pub role: AttendeeRole,
    pub response: ResponseStatus,
    pub is_self: bool,
    pub is_organizer: bool,
}
impl ToMirror for eng::Attendee {
    type Mirror = Attendee;
    fn to_mirror(self) -> Attendee {
        Attendee {
            email: self.email().map(str::to_string),
            name: self.name,
            role: self.role.to_mirror(),
            response: self.response.to_mirror(),
            is_self: self.is_self,
            is_organizer: self.is_organizer,
        }
    }
}
impl ToEngine for Attendee {
    type Engine = eng::Attendee;
    fn to_engine(self) -> eng::Attendee {
        eng::Attendee::new(
            self.name,
            self.email.as_deref(),
            self.role.to_engine(),
            self.response.to_engine(),
            self.is_self,
            self.is_organizer,
        )
    }
}

mirror_record!(ConferenceInfo <=> eng::ConferenceInfo {
    url: UrlString,
    provider: ConferenceProvider,
    origin: ConferenceOrigin,
});

mirror_record!(StructuredLocation <=> eng::StructuredLocation {
    title: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    radius: Option<f64>,
});

/// When a reminder fires.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReminderTrigger {
    Relative { offset: f64, to: ReminderAnchor },
    Absolute(Timestamp),
    Location(StructuredLocation, ReminderProximity),
}
impl ToMirror for eng::ReminderTrigger {
    type Mirror = ReminderTrigger;
    fn to_mirror(self) -> ReminderTrigger {
        match self {
            eng::ReminderTrigger::Relative { offset, to } => ReminderTrigger::Relative {
                offset,
                to: to.to_mirror(),
            },
            eng::ReminderTrigger::Absolute(date) => ReminderTrigger::Absolute(date),
            eng::ReminderTrigger::Location(place, proximity) => {
                ReminderTrigger::Location(place.to_mirror(), proximity.to_mirror())
            }
        }
    }
}
impl ToEngine for ReminderTrigger {
    type Engine = eng::ReminderTrigger;
    fn to_engine(self) -> eng::ReminderTrigger {
        match self {
            ReminderTrigger::Relative { offset, to } => eng::ReminderTrigger::Relative {
                offset,
                to: to.to_engine(),
            },
            ReminderTrigger::Absolute(date) => eng::ReminderTrigger::Absolute(date),
            ReminderTrigger::Location(place, proximity) => {
                eng::ReminderTrigger::Location(place.to_engine(), proximity.to_engine())
            }
        }
    }
}

/// How a reminder alerts.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReminderType {
    Display,
    Audio { sound_name: Option<String> },
    Email { address: Option<String> },
    Procedure { url: Option<UrlString> },
    Other(String),
}
impl ToMirror for eng::ReminderType {
    type Mirror = ReminderType;
    fn to_mirror(self) -> ReminderType {
        match self {
            eng::ReminderType::Display => ReminderType::Display,
            eng::ReminderType::Audio { sound_name } => ReminderType::Audio { sound_name },
            eng::ReminderType::Email { address } => ReminderType::Email { address },
            eng::ReminderType::Procedure { url } => ReminderType::Procedure { url },
            eng::ReminderType::Other(text) => ReminderType::Other(text),
        }
    }
}
impl ToEngine for ReminderType {
    type Engine = eng::ReminderType;
    fn to_engine(self) -> eng::ReminderType {
        match self {
            ReminderType::Display => eng::ReminderType::Display,
            ReminderType::Audio { sound_name } => eng::ReminderType::Audio { sound_name },
            ReminderType::Email { address } => eng::ReminderType::Email { address },
            ReminderType::Procedure { url } => eng::ReminderType::Procedure { url },
            ReminderType::Other(text) => eng::ReminderType::Other(text),
        }
    }
}

mirror_record!(Reminder <=> eng::Reminder {
    trigger: ReminderTrigger,
    kind: ReminderType,
    repeat_count: i64,
    repeat_interval: Option<f64>,
    is_calendar_default: Option<bool>,
});

mirror_record!(CalendarPermissions <=> eng::CalendarPermissions {
    can_view_details: bool,
    can_edit: bool,
    can_share: Option<bool>,
    can_view_private: Option<bool>,
});

/// Swift's string-backed `CalendarService` (`eventkit`, `google`, `microsoft`, `caldav`, `icalsub`, or a host's own).
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Record)]
pub struct CalendarService {
    pub raw_value: String,
}
impl ToMirror for eng::CalendarService {
    type Mirror = CalendarService;
    fn to_mirror(self) -> CalendarService {
        CalendarService { raw_value: self.0 }
    }
}
impl ToEngine for CalendarService {
    type Engine = eng::CalendarService;
    fn to_engine(self) -> eng::CalendarService {
        eng::CalendarService::new(self.raw_value)
    }
}

/// Swift's string-backed `CalendarProvider`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Record)]
pub struct CalendarProvider {
    pub raw_value: String,
}
impl ToMirror for eng::CalendarProvider {
    type Mirror = CalendarProvider;
    fn to_mirror(self) -> CalendarProvider {
        CalendarProvider { raw_value: self.0 }
    }
}
impl ToEngine for CalendarProvider {
    type Engine = eng::CalendarProvider;
    fn to_engine(self) -> eng::CalendarProvider {
        eng::CalendarProvider::new(self.raw_value)
    }
}

/// A calendar as a source describes it. The colour is normalised by the engine on the way back in.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CalendarDescriptor {
    pub id: String,
    pub title: String,
    pub color_hex: Option<String>,
    pub permissions: CalendarPermissions,
    pub is_default: Option<bool>,
    pub time_zone: Option<Zone>,
    pub account_name: Option<String>,
    pub kind: CalendarKind,
    pub default_reminders: Option<Vec<Reminder>>,
    pub service: CalendarService,
    pub provider: Option<CalendarProvider>,
    pub supported_availabilities: Option<std::collections::HashSet<Availability>>,
}
impl ToMirror for eng::CalendarDescriptor {
    type Mirror = CalendarDescriptor;
    fn to_mirror(self) -> CalendarDescriptor {
        CalendarDescriptor {
            color_hex: self.color_hex().map(str::to_string),
            id: self.id,
            title: self.title,
            permissions: self.permissions.to_mirror(),
            is_default: self.is_default,
            time_zone: self.time_zone,
            account_name: self.account_name,
            kind: self.kind.to_mirror(),
            default_reminders: self.default_reminders.to_mirror(),
            service: self.service.to_mirror(),
            provider: self.provider.to_mirror(),
            supported_availabilities: self.supported_availabilities.to_mirror(),
        }
    }
}
impl ToEngine for CalendarDescriptor {
    type Engine = eng::CalendarDescriptor;
    fn to_engine(self) -> eng::CalendarDescriptor {
        let mut descriptor =
            eng::CalendarDescriptor::new(self.id, self.title, self.service.to_engine())
                .with_color_hex(self.color_hex.as_deref());
        descriptor.permissions = self.permissions.to_engine();
        descriptor.is_default = self.is_default;
        descriptor.time_zone = self.time_zone;
        descriptor.account_name = self.account_name;
        descriptor.kind = self.kind.to_engine();
        descriptor.default_reminders = self.default_reminders.to_engine();
        descriptor.provider = self.provider.to_engine();
        descriptor.supported_availabilities = self.supported_availabilities.to_engine();
        descriptor
    }
}

mirror_record!(CalendarEvent <=> eng::CalendarEvent {
    event_id: String,
    uid: Option<String>,
    uid_scope: Option<UidScope>,
    calendar_id: String,
    title: String,
    notes: Option<String>,
    location: Option<String>,
    start: Timestamp,
    end: Timestamp,
    time_zone: Zone,
    is_all_day: bool,
    status: EventStatus,
    availability: Option<Availability>,
    visibility: Option<Visibility>,
    kind: Option<EventKind>,
    series: Option<SeriesInfo>,
    attendees: Vec<Attendee>,
    organizer: Option<Attendee>,
    conferences: Vec<ConferenceInfo>,
    reminders: Option<Vec<Reminder>>,
    url: Option<UrlString>,
    version: Option<String>,
    last_modified: Option<Timestamp>,
    created: Option<Timestamp>,
    participation: Option<Participation>,
    source_id: Option<String>,
});

mirror_record!(CalendarDate <=> EngineCalendarDate { year: i64, month: i64, day: i64 });

mirror_record!(WeekdayOccurrence <=> rule::WeekdayOccurrence { weekday: Weekday, ordinal: Option<i64> });
mirror_record!(UnrecognizedPart <=> rule::UnrecognizedPart { name: String, value: String });

/// How a recurrence rule ends.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum RuleEnd {
    Never,
    Count(i64),
    Until(Timestamp),
}
impl ToMirror for rule::RuleEnd {
    type Mirror = RuleEnd;
    fn to_mirror(self) -> RuleEnd {
        match self {
            rule::RuleEnd::Never => RuleEnd::Never,
            rule::RuleEnd::Count(count) => RuleEnd::Count(count),
            rule::RuleEnd::Until(date) => RuleEnd::Until(date),
        }
    }
}
impl ToEngine for RuleEnd {
    type Engine = rule::RuleEnd;
    fn to_engine(self) -> rule::RuleEnd {
        match self {
            RuleEnd::Never => rule::RuleEnd::Never,
            RuleEnd::Count(count) => rule::RuleEnd::Count(count),
            RuleEnd::Until(date) => rule::RuleEnd::Until(date),
        }
    }
}

mirror_record!(RecurrenceRule <=> rule::RecurrenceRule {
    frequency: Frequency,
    interval: i64,
    week_start: Weekday,
    weekdays: Vec<WeekdayOccurrence>,
    month_days: Vec<i64>,
    months: Vec<i64>,
    year_days: Vec<i64>,
    week_numbers: Vec<i64>,
    set_positions: Vec<i64>,
    hours: Vec<i64>,
    minutes: Vec<i64>,
    seconds: Vec<i64>,
    end: RuleEnd,
    unrecognized_parts: Vec<UnrecognizedPart>,
});

mirror_record!(RecurrenceSet <=> set::RecurrenceSet {
    rules: Vec<RecurrenceRule>,
    extra_dates: Option<Vec<Timestamp>>,
    excluded_dates: Option<Vec<Timestamp>>,
    unparsed: Vec<String>,
});

mirror_record!(CalendarSeries <=> set::CalendarSeries {
    series_id: String,
    calendar_id: String,
    start: Timestamp,
    time_zone: Zone,
    is_all_day: bool,
    recurrence: RecurrenceSet,
});

mirror_record!(SourceCapabilities <=> src::SourceCapabilities {
    can_write: bool,
    can_edit_attendees: bool,
    can_respond_to_invite: bool,
    provided_fields: std::collections::HashSet<ProvidedField>,
    sync_kind: SyncKind,
    supports_push: bool,
    writable_fields: std::collections::HashSet<EventField>,
    controls_notifications: bool,
    recurrence_scopes: std::collections::HashSet<RecurrenceScope>,
});

mirror_record!(SourceNotice <=> src::SourceNotice { kind: SourceNoticeKind, count: i64 });

/// What a source reports through its change stream.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum CalendarChange {
    CalendarsChanged,
    EventsChanged {
        calendar_ids: Option<std::collections::HashSet<String>>,
    },
    SourceFailed(crate::errors::SourceError),
}
impl ToMirror for src::CalendarChange {
    type Mirror = CalendarChange;
    fn to_mirror(self) -> CalendarChange {
        match self {
            src::CalendarChange::CalendarsChanged => CalendarChange::CalendarsChanged,
            src::CalendarChange::EventsChanged { calendar_ids } => CalendarChange::EventsChanged {
                calendar_ids: calendar_ids.to_mirror(),
            },
            src::CalendarChange::SourceFailed(error) => CalendarChange::SourceFailed(error.into()),
        }
    }
}
impl ToEngine for CalendarChange {
    type Engine = src::CalendarChange;
    fn to_engine(self) -> src::CalendarChange {
        match self {
            CalendarChange::CalendarsChanged => src::CalendarChange::CalendarsChanged,
            CalendarChange::EventsChanged { calendar_ids } => src::CalendarChange::EventsChanged {
                calendar_ids: calendar_ids.to_engine(),
            },
            CalendarChange::SourceFailed(error) => src::CalendarChange::SourceFailed(error.into()),
        }
    }
}

/// Health of a source.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SourceStatus {
    Ok,
    NeedsPermission,
    AuthExpired,
    Failing(String),
}
impl ToMirror for EngineSourceStatus {
    type Mirror = SourceStatus;
    fn to_mirror(self) -> SourceStatus {
        match self {
            EngineSourceStatus::Ok => SourceStatus::Ok,
            EngineSourceStatus::NeedsPermission => SourceStatus::NeedsPermission,
            EngineSourceStatus::AuthExpired => SourceStatus::AuthExpired,
            EngineSourceStatus::Failing(text) => SourceStatus::Failing(text),
        }
    }
}
impl ToEngine for SourceStatus {
    type Engine = EngineSourceStatus;
    fn to_engine(self) -> EngineSourceStatus {
        match self {
            SourceStatus::Ok => EngineSourceStatus::Ok,
            SourceStatus::NeedsPermission => EngineSourceStatus::NeedsPermission,
            SourceStatus::AuthExpired => EngineSourceStatus::AuthExpired,
            SourceStatus::Failing(text) => EngineSourceStatus::Failing(text),
        }
    }
}

mirror_record!(MergedMember <=> merge::MergedMember {
    title: String,
    calendar_key: String,
    content_key: String,
    details: String,
    start: Timestamp,
    end: Timestamp,
});

/// Why two events were merged.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum MergeProvenance {
    Rule,
    Inference {
        engine_id: String,
        engine_name: String,
    },
    UserConfirmed,
}
impl ToMirror for merge::MergeProvenance {
    type Mirror = MergeProvenance;
    fn to_mirror(self) -> MergeProvenance {
        match self {
            merge::MergeProvenance::Rule => MergeProvenance::Rule,
            merge::MergeProvenance::Inference {
                engine_id,
                engine_name,
            } => MergeProvenance::Inference {
                engine_id,
                engine_name,
            },
            merge::MergeProvenance::UserConfirmed => MergeProvenance::UserConfirmed,
        }
    }
}
impl ToEngine for MergeProvenance {
    type Engine = merge::MergeProvenance;
    fn to_engine(self) -> merge::MergeProvenance {
        match self {
            MergeProvenance::Rule => merge::MergeProvenance::Rule,
            MergeProvenance::Inference {
                engine_id,
                engine_name,
            } => merge::MergeProvenance::Inference {
                engine_id,
                engine_name,
            },
            MergeProvenance::UserConfirmed => merge::MergeProvenance::UserConfirmed,
        }
    }
}

mirror_record!(TimeTugCalendarEvent <=> timetug_core::TimeTugCalendarEvent {
    event: CalendarEvent,
    source_id: String,
    conferences: Vec<ConferenceInfo>,
    other_attendee_count: i64,
    response_status: Option<ResponseStatus>,
    additional_calendar_keys: std::collections::HashSet<String>,
    merged_members: Vec<MergedMember>,
    merge_provenance: Option<MergeProvenance>,
    calendar_service: Option<String>,
    calendar_provider: Option<String>,
    display_start: Option<Timestamp>,
});

/// A calendar as the engine lists it (a source's calendar). The colour is normalised on the way back in.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CalendarInfo {
    pub source_id: String,
    pub calendar_id: String,
    pub title: String,
    pub account_name: Option<String>,
    pub color_hex: Option<String>,
    pub kind: CalendarKind,
    pub service: Option<CalendarService>,
    pub provider: Option<CalendarProvider>,
}
impl ToMirror for timetug_core::CalendarInfo {
    type Mirror = CalendarInfo;
    fn to_mirror(self) -> CalendarInfo {
        CalendarInfo {
            color_hex: self.color_hex().map(str::to_string),
            source_id: self.source_id,
            calendar_id: self.calendar_id,
            title: self.title,
            account_name: self.account_name,
            kind: self.kind.to_mirror(),
            service: self.service.to_mirror(),
            provider: self.provider.to_mirror(),
        }
    }
}
impl ToEngine for CalendarInfo {
    type Engine = timetug_core::CalendarInfo;
    fn to_engine(self) -> timetug_core::CalendarInfo {
        let mut info =
            timetug_core::CalendarInfo::new(self.source_id, self.calendar_id, self.title)
                .with_color_hex(self.color_hex.as_deref());
        info.account_name = self.account_name;
        info.kind = self.kind.to_engine();
        info.service = self.service.to_engine();
        info.provider = self.provider.to_engine();
        info
    }
}
