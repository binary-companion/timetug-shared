//! Mirrors of `calendar_core::write`: drafts, patches, references and the write error.
#![allow(missing_docs)]

use calendar_core::time::Zone;
use calendar_core::write::event_draft as draft;
use calendar_core::write::event_patch as patch;
use calendar_core::write::types;
use jiff::Timestamp;

use super::model::{
    AttendeeRole, Availability, CalendarEvent, RecurrenceRule, Reminder, Visibility,
};
use super::{ToEngine, ToMirror, mirror_record, mirror_unit_enum};

mirror_unit_enum!(EventField <=> types::EventField {
    Title, Notes, Location, Timing, Availability, Visibility, Reminders, Attendees, Recurrence, Conference
});
mirror_unit_enum!(NotifyPolicy <=> types::NotifyPolicy { All, ExternalOnly, None });
mirror_unit_enum!(RecurrenceScope <=> types::RecurrenceScope { ThisInstance, ThisAndFollowing, AllInSeries });
mirror_unit_enum!(ConferenceRequest <=> types::ConferenceRequest { None, Generate });
mirror_unit_enum!(ConferenceChange <=> types::ConferenceChange { Generate, Remove });

/// What a write can fail with.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum WriteError {
    Unsupported {
        fields: std::collections::HashSet<EventField>,
    },
    Conflict {
        fields: std::collections::HashSet<EventField>,
    },
    NotFound,
    Forbidden(Option<String>),
    Invalid(String),
    AlreadyExists(Box<CalendarEvent>),
    Partial(String),
}
impl ToMirror for types::WriteError {
    type Mirror = WriteError;
    fn to_mirror(self) -> WriteError {
        match self {
            types::WriteError::Unsupported { fields } => WriteError::Unsupported {
                fields: fields.to_mirror(),
            },
            types::WriteError::Conflict { fields } => WriteError::Conflict {
                fields: fields.to_mirror(),
            },
            types::WriteError::NotFound => WriteError::NotFound,
            types::WriteError::Forbidden(reason) => WriteError::Forbidden(reason),
            types::WriteError::Invalid(reason) => WriteError::Invalid(reason),
            types::WriteError::AlreadyExists(event) => WriteError::AlreadyExists(event.to_mirror()),
            types::WriteError::Partial(reason) => WriteError::Partial(reason),
        }
    }
}
impl ToEngine for WriteError {
    type Engine = types::WriteError;
    fn to_engine(self) -> types::WriteError {
        match self {
            WriteError::Unsupported { fields } => types::WriteError::Unsupported {
                fields: fields.to_engine(),
            },
            WriteError::Conflict { fields } => types::WriteError::Conflict {
                fields: fields.to_engine(),
            },
            WriteError::NotFound => types::WriteError::NotFound,
            WriteError::Forbidden(reason) => types::WriteError::Forbidden(reason),
            WriteError::Invalid(reason) => types::WriteError::Invalid(reason),
            WriteError::AlreadyExists(event) => types::WriteError::AlreadyExists(event.to_engine()),
            WriteError::Partial(reason) => types::WriteError::Partial(reason),
        }
    }
}

/// What a write needs to find an event.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Record)]
pub struct EventRef {
    pub calendar_id: String,
    pub event_id: String,
    pub version: Option<String>,
    pub series_id: Option<String>,
    pub original_start: Option<Timestamp>,
}
impl ToMirror for types::EventRef {
    type Mirror = EventRef;
    fn to_mirror(self) -> EventRef {
        EventRef {
            series_id: self.series_id().map(str::to_string),
            calendar_id: self.calendar_id,
            event_id: self.event_id,
            version: self.version,
            original_start: self.original_start,
        }
    }
}
impl ToEngine for EventRef {
    type Engine = types::EventRef;
    fn to_engine(self) -> types::EventRef {
        let mut reference = types::EventRef::new(self.calendar_id, self.event_id);
        reference.version = self.version;
        reference.original_start = self.original_start;
        reference.with_series_id(self.series_id.as_deref())
    }
}

mirror_record!(EventTiming <=> draft::EventTiming {
    start: Timestamp,
    end: Timestamp,
    time_zone: Option<Zone>,
    is_all_day: bool,
});

/// An attendee to invite. The engine normalises the address on the way back in.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AttendeeDraft {
    pub name: Option<String>,
    pub email: String,
    pub role: AttendeeRole,
}
impl ToMirror for draft::AttendeeDraft {
    type Mirror = AttendeeDraft;
    fn to_mirror(self) -> AttendeeDraft {
        AttendeeDraft {
            email: self.email().to_string(),
            name: self.name,
            role: self.role.to_mirror(),
        }
    }
}
impl ToEngine for AttendeeDraft {
    type Engine = draft::AttendeeDraft;
    fn to_engine(self) -> draft::AttendeeDraft {
        draft::AttendeeDraft::new(&self.email, self.name.as_deref(), self.role.to_engine())
    }
}

mirror_record!(EventDraft <=> draft::EventDraft {
    title: String,
    notes: Option<String>,
    location: Option<String>,
    timing: EventTiming,
    availability: Availability,
    visibility: Visibility,
    reminders: Option<Vec<Reminder>>,
    attendees: Vec<AttendeeDraft>,
    conference: ConferenceRequest,
    recurrence: Option<RecurrenceRule>,
    uid: Option<String>,
});

/// Swift's generic `FieldUpdate<Value>` has one concrete mirror per instantiation the model uses; the Swift package
/// restores the generic spelling (`docs/bindings.md`).
macro_rules! field_update {
    ($name:ident, $engine_value:ty, $value:ty) => {
        #[derive(Clone, Debug, PartialEq, uniffi::Enum)]
        pub enum $name {
            Keep,
            Set($value),
            Clear,
        }
        impl ToMirror for patch::FieldUpdate<$engine_value> {
            type Mirror = $name;
            fn to_mirror(self) -> $name {
                match self {
                    patch::FieldUpdate::Keep => $name::Keep,
                    patch::FieldUpdate::Set(value) => $name::Set(value.to_mirror()),
                    patch::FieldUpdate::Clear => $name::Clear,
                }
            }
        }
        impl ToEngine for $name {
            type Engine = patch::FieldUpdate<$engine_value>;
            fn to_engine(self) -> patch::FieldUpdate<$engine_value> {
                match self {
                    $name::Keep => patch::FieldUpdate::Keep,
                    $name::Set(value) => patch::FieldUpdate::Set(value.to_engine()),
                    $name::Clear => patch::FieldUpdate::Clear,
                }
            }
        }
    };
}
field_update!(FieldUpdateString, String, String);
field_update!(
    FieldUpdateReminders,
    Vec<calendar_core::model::Reminder>,
    Vec<Reminder>
);
field_update!(
    FieldUpdateRecurrenceRule,
    calendar_core::recurrence::rule::RecurrenceRule,
    RecurrenceRule
);

/// Attendees to add and addresses to remove. The engine normalises the addresses on the way back in.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AttendeeChanges {
    pub add: Vec<AttendeeDraft>,
    pub remove: Vec<String>,
}
impl ToMirror for patch::AttendeeChanges {
    type Mirror = AttendeeChanges;
    fn to_mirror(self) -> AttendeeChanges {
        AttendeeChanges {
            add: self.add.to_mirror(),
            remove: self.remove,
        }
    }
}
impl ToEngine for AttendeeChanges {
    type Engine = patch::AttendeeChanges;
    fn to_engine(self) -> patch::AttendeeChanges {
        patch::AttendeeChanges::new(self.add.to_engine(), self.remove)
    }
}

/// A change to an event. `base` is the event the patch was made against (set when the patch comes from a comparison of
/// two copies, `eventPatchDiff`); the engine's optimistic merge uses it. Coming back from the host, a patch with a base is
/// rebuilt as the comparison of the base with the base plus the patch, so a no-op field (set to what the base already
/// holds) becomes `keep`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct EventPatch {
    pub title: Option<String>,
    pub notes: FieldUpdateString,
    pub location: FieldUpdateString,
    pub timing: Option<EventTiming>,
    pub availability: Option<Availability>,
    pub visibility: Option<Visibility>,
    pub reminders: FieldUpdateReminders,
    pub attendees: Option<AttendeeChanges>,
    pub recurrence: FieldUpdateRecurrenceRule,
    pub conference: Option<ConferenceChange>,
    pub base: Option<CalendarEvent>,
}
impl ToMirror for patch::EventPatch {
    type Mirror = EventPatch;
    fn to_mirror(self) -> EventPatch {
        let base = self.base().cloned().to_mirror();
        EventPatch {
            title: self.title,
            notes: self.notes.to_mirror(),
            location: self.location.to_mirror(),
            timing: self.timing.to_mirror(),
            availability: self.availability.to_mirror(),
            visibility: self.visibility.to_mirror(),
            reminders: self.reminders.to_mirror(),
            attendees: self.attendees.to_mirror(),
            recurrence: self.recurrence.to_mirror(),
            base,
            conference: self.conference.to_mirror(),
        }
    }
}
impl ToEngine for EventPatch {
    type Engine = patch::EventPatch;
    fn to_engine(self) -> patch::EventPatch {
        let mut result = patch::EventPatch::default();
        result.title = self.title;
        result.notes = self.notes.to_engine();
        result.location = self.location.to_engine();
        result.timing = self.timing.to_engine();
        result.availability = self.availability.to_engine();
        result.visibility = self.visibility.to_engine();
        result.reminders = self.reminders.to_engine();
        result.attendees = self.attendees.to_engine();
        result.recurrence = self.recurrence.to_engine();
        result.conference = self.conference.to_engine();
        match self.base {
            Some(base) => {
                let base = base.to_engine();
                let edited = result.applied_to(&base);
                patch::EventPatch::diff(&base, &edited)
            }
            None => result,
        }
    }
}

/// An event being edited: the copy as it was and the copy as it is.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct EventEdit {
    pub original: CalendarEvent,
    pub event: CalendarEvent,
}
impl ToMirror for patch::EventEdit {
    type Mirror = EventEdit;
    fn to_mirror(self) -> EventEdit {
        EventEdit {
            original: self.original().clone().to_mirror(),
            event: self.event.to_mirror(),
        }
    }
}
impl ToEngine for EventEdit {
    type Engine = patch::EventEdit;
    fn to_engine(self) -> patch::EventEdit {
        let mut edit = patch::EventEdit::new(self.original.to_engine());
        edit.event = self.event.to_engine();
        edit
    }
}
