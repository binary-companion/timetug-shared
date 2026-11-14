//! Each mirror: a fully populated engine value goes to the mirror and back and compares equal; a value the host never
//! sends is checked in the forward direction only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};

use calendar_core::connection::{
    AuthorizationMethod, Connection, CredentialField, CredentialHelp, CredentialSnapshot,
};
use calendar_core::diagnostic_log::{
    DiagnosticEvent, DiagnosticField, DiagnosticFieldValue, DiagnosticLevel,
};
use calendar_core::model::{
    AccessRole, Attendee, AttendeeRole, Availability, CalendarDescriptor, CalendarEvent,
    CalendarKind, CalendarPermissions, CalendarProvider, CalendarService, ConferenceInfo,
    ConferenceOrigin, ConferenceProvider, EventKind, EventStatus, Participation, Reminder,
    ReminderAnchor, ReminderProximity, ReminderTrigger, ReminderType, ResponseStatus, SeriesInfo,
    StructuredLocation, UidScope, Visibility,
};
use calendar_core::recurrence::rule::{
    Frequency, RecurrenceRule, RuleEnd, UnrecognizedPart, Weekday, WeekdayOccurrence,
};
use calendar_core::recurrence::set::{CalendarSeries, RecurrenceSet};
use calendar_core::source_types::{
    CalendarChange, ProvidedField, SourceCapabilities, SourceError, SourceNotice, SourceNoticeKind,
    SyncKind,
};
use calendar_core::time::Zone;
use calendar_core::url_string::UrlString;
use calendar_core::write::event_draft::{AttendeeDraft, EventDraft, EventTiming};
use calendar_core::write::event_patch::{AttendeeChanges, EventEdit, EventPatch, FieldUpdate};
use calendar_core::write::types::{
    ConferenceChange, ConferenceRequest, EventField, EventRef, NotifyPolicy, RecurrenceScope,
};
use jiff::Timestamp;
use timetug_core::agenda::day_agenda::{DayAgenda, Item, State};
use timetug_core::dedup::adjudication::EngineInfo;
use timetug_core::takeover::ledger::TakeoverLedger;
use timetug_core::takeover::request::TakeoverRequest;
use timetug_core::takeover::scheduler::ScheduledTakeover;
use timetug_core::widget::snapshot::{WidgetEvent, WidgetSnapshot};
use timetug_core::{
    CalendarInfo, InferenceStatus, MergeProvenance, MergedMember, SourceStatus, TakeoverSettings,
    TimeTugCalendarEvent,
};
use timetug_engine::accounts::account_manager::{Status, WaitingStyle};
use timetug_engine::command::{EngineCommand, SystemEvent};
use timetug_engine::diagnostic::{EngineDiagnostic, Level};
use timetug_engine::event::EngineEvent;
use timetug_engine::settings::EngineSettings;
use timetug_engine::state::{AccountKindInfo, Authorization, EngineState};

use crate::values::{ToEngine, ToMirror};

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).unwrap()
}

/// The engine value goes to the mirror and back and is equal.
fn round_trip<E>(value: E)
where
    E: ToMirror + Clone + PartialEq + std::fmt::Debug,
    E::Mirror: ToEngine<Engine = E>,
{
    let mirror = value.clone().to_mirror();
    assert_eq!(mirror.to_engine(), value);
}

fn location() -> StructuredLocation {
    StructuredLocation {
        title: Some("Office \u{1F3E2}".into()),
        latitude: Some(52.52),
        longitude: Some(13.405),
        radius: Some(150.0),
    }
}

fn reminders() -> Vec<Reminder> {
    let mut all = vec![
        Reminder::new(ReminderTrigger::Relative {
            offset: -900.0,
            to: ReminderAnchor::Start,
        }),
        Reminder::new(ReminderTrigger::Absolute(at(1_700_000_000))),
        Reminder::new(ReminderTrigger::Location(
            location(),
            ReminderProximity::Leave,
        )),
    ];
    for (reminder, kind) in all.iter_mut().zip([
        ReminderType::Display,
        ReminderType::Audio {
            sound_name: Some("chime".into()),
        },
        ReminderType::Procedure {
            url: UrlString::parse("https://example.com/run"),
        },
    ]) {
        reminder.kind = kind;
        reminder.repeat_count = 2;
        reminder.repeat_interval = Some(300.0);
        reminder.is_calendar_default = Some(true);
    }
    all.push(Reminder {
        kind: ReminderType::Email {
            address: Some("a@example.com".into()),
        },
        ..Reminder::new(ReminderTrigger::Relative {
            offset: 60.0,
            to: ReminderAnchor::End,
        })
    });
    all.push(Reminder {
        kind: ReminderType::Other("vendor".into()),
        ..Reminder::new(ReminderTrigger::Relative {
            offset: 0.0,
            to: ReminderAnchor::Start,
        })
    });
    all
}

fn attendee(email: &str, is_self: bool) -> Attendee {
    Attendee::new(
        Some("Ann \u{e9}".into()),
        Some(email),
        AttendeeRole::Optional,
        ResponseStatus::Tentative,
        is_self,
        !is_self,
    )
}

fn recurrence_rule() -> RecurrenceRule {
    RecurrenceRule {
        frequency: Frequency::Weekly,
        interval: 2,
        week_start: Weekday::Sunday,
        weekdays: vec![
            WeekdayOccurrence {
                weekday: Weekday::Monday,
                ordinal: Some(-1),
            },
            WeekdayOccurrence {
                weekday: Weekday::Friday,
                ordinal: None,
            },
        ],
        month_days: vec![1, -1],
        months: vec![3, 9],
        year_days: vec![100],
        week_numbers: vec![20],
        set_positions: vec![-1],
        hours: vec![9],
        minutes: vec![30],
        seconds: vec![0],
        end: RuleEnd::Until(at(1_800_000_000)),
        unrecognized_parts: vec![UnrecognizedPart {
            name: "X-VENDOR".into(),
            value: "1".into(),
        }],
    }
}

fn rich_calendar_event() -> CalendarEvent {
    let mut event = CalendarEvent::new(
        "e1",
        "cal1",
        "Title with \u{1F600} and e\u{301}",
        at(1_700_000_000),
        at(1_700_003_600),
    );
    event.uid = Some("uid-1".into());
    event.uid_scope = Some(UidScope::Provider);
    event.notes = Some("notes\nsecond line".into());
    event.location = Some("Room 1".into());
    event.time_zone = Zone::named("Europe/Berlin").unwrap();
    event.is_all_day = true;
    event.status = EventStatus::Tentative;
    event.availability = Some(Availability::Unavailable);
    event.visibility = Some(Visibility::Confidential);
    event.kind = Some(EventKind::FocusTime);
    event.series = Some(SeriesInfo::Occurrence {
        series_id: "s1".into(),
        original_start: Some(at(1_699_999_000)),
    });
    event.attendees = vec![
        attendee("ann@example.com", true),
        attendee("bob@example.com", false),
    ];
    event.organizer = Some(attendee("org@example.com", false));
    event.conferences = vec![ConferenceInfo::new(
        UrlString::parse("https://meet.example.com/abc").unwrap(),
        ConferenceProvider::GoToMeeting,
        ConferenceOrigin::EventUrl,
    )];
    event.reminders = Some(reminders());
    event.url = UrlString::parse("https://example.com/event");
    event.version = Some("etag-1".into());
    event.last_modified = Some(at(1_699_000_000));
    event.created = Some(at(1_698_000_000));
    event.participation = Some(Participation::Invited(ResponseStatus::Declined));
    event.source_id = Some("google-1".into());
    event
}

fn rich_event() -> TimeTugCalendarEvent {
    let calendar_event = rich_calendar_event();
    TimeTugCalendarEvent {
        source_id: "google-1".into(),
        conferences: calendar_event.conferences.clone(),
        event: calendar_event,
        other_attendee_count: 3,
        response_status: Some(ResponseStatus::NeedsAction),
        additional_calendar_keys: ["a/b", "c/d"].iter().map(|k| k.to_string()).collect(),
        merged_members: vec![MergedMember {
            title: "t".into(),
            calendar_key: "k".into(),
            content_key: "c".into(),
            details: "d".into(),
            start: at(1),
            end: at(2),
        }],
        merge_provenance: Some(MergeProvenance::Inference {
            engine_id: "e".into(),
            engine_name: "Engine".into(),
        }),
        calendar_service: Some("google".into()),
        calendar_provider: Some("google".into()),
        display_start: Some(at(1_700_000_100)),
    }
}

#[test]
fn model_values_round_trip() {
    round_trip(rich_calendar_event());
    round_trip(rich_event());
    for event in [
        SeriesInfo::NotRecurring,
        SeriesInfo::Occurrence {
            series_id: "s".into(),
            original_start: None,
        },
    ] {
        round_trip(event);
    }
    for participation in [
        Participation::NotInvited,
        Participation::Invited(ResponseStatus::Accepted),
    ] {
        round_trip(participation);
    }
    for reminder in reminders() {
        round_trip(reminder);
    }
    round_trip(location());
    round_trip(CalendarPermissions {
        can_view_details: false,
        can_edit: true,
        can_share: Some(false),
        can_view_private: Some(true),
    });
    for provenance in [
        MergeProvenance::Rule,
        MergeProvenance::UserConfirmed,
        MergeProvenance::Inference {
            engine_id: "i".into(),
            engine_name: "n".into(),
        },
    ] {
        round_trip(provenance);
    }
}

#[test]
fn every_plain_enum_case_round_trips() {
    macro_rules! all_cases {
        ($($ty:ty: [$($case:expr),+ $(,)?]),+ $(,)?) => {$( for case in [$($case),+] { round_trip::<$ty>(case); } )+};
    }
    all_cases!(
        CalendarKind: [CalendarKind::Standard, CalendarKind::Subscribed, CalendarKind::Birthdays],
        AccessRole: [AccessRole::Owner, AccessRole::Writer, AccessRole::Reader, AccessRole::FreeBusyReader],
        EventStatus: [EventStatus::Confirmed, EventStatus::Tentative, EventStatus::Cancelled],
        Availability: [Availability::Busy, Availability::Free, Availability::Tentative, Availability::Unavailable],
        Visibility: [Visibility::Default, Visibility::PublicEvent, Visibility::PrivateEvent, Visibility::Confidential],
        EventKind: [EventKind::Standard, EventKind::FocusTime, EventKind::OutOfOffice, EventKind::WorkingLocation, EventKind::Birthday, EventKind::Other],
        UidScope: [UidScope::Global, UidScope::Provider],
        ResponseStatus: [ResponseStatus::Accepted, ResponseStatus::Tentative, ResponseStatus::Declined, ResponseStatus::NeedsAction],
        AttendeeRole: [AttendeeRole::Required, AttendeeRole::Optional, AttendeeRole::Resource],
        ConferenceProvider: [ConferenceProvider::Meet, ConferenceProvider::Teams, ConferenceProvider::Zoom, ConferenceProvider::Webex, ConferenceProvider::GoToMeeting, ConferenceProvider::Whereby, ConferenceProvider::Jitsi, ConferenceProvider::Slack, ConferenceProvider::Other],
        ConferenceOrigin: [ConferenceOrigin::Structured, ConferenceOrigin::Location, ConferenceOrigin::Url, ConferenceOrigin::Notes, ConferenceOrigin::EventUrl],
        SyncKind: [SyncKind::None, SyncKind::Token, SyncKind::Notification],
        SourceNoticeKind: [SourceNoticeKind::UnreadableRecurrence],
        Frequency: [Frequency::Secondly, Frequency::Minutely, Frequency::Hourly, Frequency::Daily, Frequency::Weekly, Frequency::Monthly, Frequency::Yearly],
        Weekday: [Weekday::Monday, Weekday::Tuesday, Weekday::Wednesday, Weekday::Thursday, Weekday::Friday, Weekday::Saturday, Weekday::Sunday],
        EventField: [EventField::Title, EventField::Notes, EventField::Location, EventField::Timing, EventField::Availability, EventField::Visibility, EventField::Reminders, EventField::Attendees, EventField::Recurrence, EventField::Conference],
        NotifyPolicy: [NotifyPolicy::All, NotifyPolicy::ExternalOnly, NotifyPolicy::None],
        RecurrenceScope: [RecurrenceScope::ThisInstance, RecurrenceScope::ThisAndFollowing, RecurrenceScope::AllInSeries],
        ConferenceRequest: [ConferenceRequest::None, ConferenceRequest::Generate],
        ConferenceChange: [ConferenceChange::Generate, ConferenceChange::Remove],
        SystemEvent: [SystemEvent::Wake, SystemEvent::ClockChanged, SystemEvent::TimeZoneChanged, SystemEvent::DayChanged, SystemEvent::NetworkReturned, SystemEvent::DisplayChanged],
    );
    for field in ProvidedField::ALL {
        round_trip(*field);
    }
    for proximity in ReminderProximity::ALL {
        round_trip(*proximity);
    }
}

#[test]
fn calendars_and_sources_round_trip() {
    let mut descriptor = CalendarDescriptor::new("c1", "Calendar", CalendarService::google())
        .with_color_hex(Some("#12ab34"));
    descriptor.permissions = CalendarPermissions {
        can_view_details: true,
        can_edit: true,
        can_share: Some(true),
        can_view_private: None,
    };
    descriptor.is_default = Some(true);
    descriptor.time_zone = Zone::named("Asia/Tokyo");
    descriptor.account_name = Some("me@example.com".into());
    descriptor.kind = CalendarKind::Subscribed;
    descriptor.default_reminders = Some(reminders());
    descriptor.provider = Some(CalendarProvider::google());
    descriptor.supported_availabilities = Some([Availability::Busy, Availability::Free].into());
    assert!(descriptor.color_hex().is_some());
    round_trip(descriptor);
    let info = CalendarInfo::new("google-1", "cal1", "Work").with_color_hex(Some("#FF8800"));
    let mut info = info;
    info.account_name = Some("me@example.com".into());
    info.kind = CalendarKind::Birthdays;
    info.service = Some(CalendarService::event_kit());
    info.provider = Some(CalendarProvider::new("custom"));
    round_trip(info);
    round_trip(CalendarService::new("made-up"));
    round_trip(SourceCapabilities {
        can_write: true,
        can_edit_attendees: true,
        can_respond_to_invite: false,
        provided_fields: [ProvidedField::Kind, ProvidedField::Series].into(),
        sync_kind: SyncKind::Token,
        supports_push: true,
        writable_fields: [EventField::Title, EventField::Timing].into(),
        controls_notifications: true,
        recurrence_scopes: [RecurrenceScope::ThisInstance, RecurrenceScope::AllInSeries].into(),
    });
    round_trip(SourceNotice {
        kind: SourceNoticeKind::UnreadableRecurrence,
        count: 4,
    });
    for change in [
        CalendarChange::CalendarsChanged,
        CalendarChange::EventsChanged { calendar_ids: None },
        CalendarChange::EventsChanged {
            calendar_ids: Some(["a".to_string(), "b".to_string()].into()),
        },
        CalendarChange::SourceFailed(SourceError::RateLimited {
            retry_after: Some(2.0),
        }),
    ] {
        round_trip(change);
    }
    for status in [
        SourceStatus::Ok,
        SourceStatus::NeedsPermission,
        SourceStatus::AuthExpired,
        SourceStatus::Failing("down".into()),
    ] {
        round_trip(status);
    }
}

#[test]
fn recurrence_values_round_trip() {
    for end in [RuleEnd::Never, RuleEnd::Count(10), RuleEnd::Until(at(5))] {
        round_trip(end);
    }
    round_trip(recurrence_rule());
    let set = RecurrenceSet {
        rules: vec![recurrence_rule()],
        extra_dates: Some(vec![at(10), at(20)]),
        excluded_dates: Some(vec![at(30)]),
        unparsed: vec!["RDATE;VALUE=PERIOD:x".into()],
    };
    round_trip(set.clone());
    round_trip(CalendarSeries {
        series_id: "s".into(),
        calendar_id: "c".into(),
        start: at(100),
        time_zone: Zone::fixed(3600).unwrap(),
        is_all_day: false,
        recurrence: set,
    });
    round_trip(calendar_core::all_day::CalendarDate::new(2026, 11, 14));
}

#[test]
fn an_attendee_address_is_normalised_on_the_way_back_in() {
    let mut mirror = attendee("x@example.com", false).to_mirror();
    mirror.email = Some("  Mixed@Example.COM ".into());
    assert_eq!(
        mirror.clone().to_engine().email(),
        Some("mixed@example.com")
    );
    mirror.email = Some("   ".into());
    assert_eq!(mirror.to_engine().email(), None);
}

#[test]
fn write_values_round_trip() {
    let timing = EventTiming::new(
        at(1_700_000_000),
        at(1_700_003_600),
        Zone::named("America/New_York"),
        false,
    );
    let mut draft_attendee =
        AttendeeDraft::new("Guest@Example.com", Some("Guest"), AttendeeRole::Resource);
    assert_eq!(draft_attendee.email(), "guest@example.com");
    round_trip(draft_attendee.clone());
    draft_attendee = AttendeeDraft::new("g2@example.com", None, AttendeeRole::Required);
    round_trip(draft_attendee.clone());
    round_trip(timing.clone());
    round_trip(EventDraft {
        title: "Draft".into(),
        notes: Some("n".into()),
        location: Some("l".into()),
        timing: timing.clone(),
        availability: Availability::Free,
        visibility: Visibility::PrivateEvent,
        reminders: Some(reminders()),
        attendees: vec![draft_attendee.clone()],
        conference: ConferenceRequest::Generate,
        recurrence: Some(recurrence_rule()),
        uid: Some("uid-draft".into()),
    });
    let mut patch = EventPatch::default();
    patch.title = Some("New".into());
    patch.notes = FieldUpdate::Set("n".into());
    patch.location = FieldUpdate::Clear;
    patch.timing = Some(timing);
    patch.availability = Some(Availability::Busy);
    patch.visibility = Some(Visibility::Default);
    patch.reminders = FieldUpdate::Set(reminders());
    patch.attendees = Some(AttendeeChanges::new(
        vec![draft_attendee],
        vec!["Gone@Example.com".into()],
    ));
    patch.recurrence = FieldUpdate::Set(recurrence_rule());
    patch.conference = Some(ConferenceChange::Remove);
    round_trip(patch);
    round_trip(EventPatch::default());
    let mut edit = EventEdit::new(rich_calendar_event());
    edit.event.title = "Edited".into();
    round_trip(edit);
    let mut reference = EventRef::new("cal", "ev").with_series_id(Some("series"));
    reference.version = Some("v".into());
    reference.original_start = Some(at(7));
    round_trip(reference);
    round_trip(EventRef::new("cal", "ev"));
    round_trip(FieldUpdate::<String>::Keep);
}

#[test]
fn state_values_convert_forward() {
    let event = rich_event();
    let mirror_event = event.clone().to_mirror();
    assert_eq!(
        mirror_event.event.title,
        "Title with \u{1F600} and e\u{301}"
    );
    assert_eq!(
        mirror_event.event.attendees[0].email.as_deref(),
        Some("ann@example.com")
    );
    assert_eq!(mirror_event.additional_calendar_keys.len(), 2);

    let agenda = DayAgenda {
        items: vec![Item {
            event: event.clone(),
            state: State::Current,
        }],
        next: Some(event.clone()),
    };
    let mirrored = agenda.to_mirror();
    assert_eq!(mirrored.items.len(), 1);
    assert_eq!(
        mirrored.items[0].state,
        crate::values::state::DayAgendaItemState::Current
    );
    assert_eq!(mirrored.next, Some(mirror_event.clone()));

    let request = TakeoverRequest {
        event: event.clone(),
        has_started: true,
        join_url: UrlString::parse("https://meet.example.com/abc"),
        snooze_options: vec![60.0, 300.0],
    }
    .to_mirror();
    assert!(request.has_started);
    assert_eq!(request.snooze_options, vec![60.0, 300.0]);
    let scheduled = ScheduledTakeover {
        event: event.clone(),
        fire_at: at(99),
    }
    .to_mirror();
    assert_eq!(scheduled.fire_at, at(99));

    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&event, at(5));
    assert_eq!(
        ledger.clone().to_mirror().key_count,
        ledger.key_count() as u64
    );
    assert!(ledger.key_count() > 0);

    assert_eq!(
        InferenceStatus::Disabled.to_mirror(),
        crate::values::state::InferenceStatus::Disabled
    );
    let active = InferenceStatus::Active(EngineInfo {
        id: "e".into(),
        display_name: "Engine".into(),
        is_on_device: true,
    })
    .to_mirror();
    assert_eq!(
        active,
        crate::values::state::InferenceStatus::Active(crate::values::state::EngineInfo {
            id: "e".into(),
            display_name: "Engine".into(),
            is_on_device: true
        })
    );
    assert_eq!(
        InferenceStatus::Unavailable { reason: "r".into() }.to_mirror(),
        crate::values::state::InferenceStatus::Unavailable { reason: "r".into() }
    );

    let connection = Connection {
        kind_id: "google".into(),
        connection_id: "ID-1".into(),
        display_name: "Me".into(),
        config: BTreeMap::from([("k".to_string(), "v".to_string())]),
    };
    let status = Status {
        accounts: vec![connection.clone()],
        is_working: true,
        build_failures: BTreeMap::from([("ID-1".to_string(), "bad".to_string())]),
        error_message: Some("oops".into()),
        waiting_style: WaitingStyle::InApp,
    };
    let state = EngineState {
        revision: 7,
        agenda: DayAgenda::empty(),
        events: vec![event.clone()],
        calendars: vec![CalendarInfo::new("s", "c", "t")],
        statuses: BTreeMap::from([("s".to_string(), SourceStatus::Failing("x".into()))]),
        notices: BTreeMap::from([(
            "s".to_string(),
            vec![SourceNotice {
                kind: SourceNoticeKind::UnreadableRecurrence,
                count: 1,
            }],
        )]),
        source_names: BTreeMap::from([("s".to_string(), "Source".to_string())]),
        candidates: BTreeMap::from([("e".to_string(), vec![event.clone()])]),
        accounts: vec![connection],
        account_status: status,
        settings: EngineSettings::default(),
        inference_status: InferenceStatus::NotOnDevice,
        ledger,
        next_takeover: Some(ScheduledTakeover {
            event,
            fire_at: at(11),
        }),
        takeover_visible: true,
        next_pending_start: Some(at(12)),
        available_account_kinds: vec![AccountKindInfo::new(
            "google",
            "Google",
            Authorization::Oauth,
            false,
        )],
    };
    let mirrored = state.to_mirror();
    assert_eq!(mirrored.revision, 7);
    assert_eq!(
        mirrored.statuses["s"],
        crate::values::model::SourceStatus::Failing("x".into())
    );
    assert_eq!(
        mirrored.account_status.waiting_style,
        crate::values::accounts::WaitingStyle::InApp
    );
    assert_eq!(mirrored.account_status.build_failures["ID-1"], "bad");
    assert_eq!(
        mirrored.available_account_kinds[0].authorization,
        crate::values::state::Authorization::Oauth
    );
    assert!(!mirrored.available_account_kinds[0].is_configured);
    assert!(mirrored.takeover_visible);
    assert_eq!(mirrored.next_pending_start, Some(at(12)));
    assert!(mirrored.settings.system_calendar_enabled);
    assert_eq!(
        mirrored.settings.takeover.lead_time,
        TakeoverSettings::default().lead_time
    );
}

#[test]
fn settings_round_trip_and_the_patch_changes_only_what_it_names() {
    let mut settings = EngineSettings::default();
    settings.takeover.lead_time = 90.0;
    settings.takeover.takeover_calendar_keys = ["a/b".to_string()].into();
    settings.takeover.hidden_calendar_keys = ["c/d".to_string()].into();
    settings.takeover.require_conference_link = true;
    settings.takeover.skip_all_day_events = false;
    settings.inference_enabled = true;
    settings.system_calendar_enabled = false;
    round_trip(settings.clone());
    round_trip(TakeoverSettings::default());

    use crate::values::settings::SettingsPatch;
    let mut changed = settings.clone();
    SettingsPatch::default().apply(&mut changed);
    assert_eq!(changed, settings, "an empty patch changes nothing");
    SettingsPatch {
        lead_time: Some(30.0),
        takeover_calendar_keys: Some(["z".to_string()].into()),
        enabled: Some(false),
        inference_enabled: Some(false),
        ..SettingsPatch::default()
    }
    .apply(&mut changed);
    assert_eq!(changed.takeover.lead_time, 30.0);
    assert_eq!(
        changed.takeover.takeover_calendar_keys,
        BTreeSet::from(["z".to_string()])
    );
    assert_eq!(
        changed.takeover.hidden_calendar_keys,
        settings.takeover.hidden_calendar_keys
    );
    assert!(!changed.takeover.enabled);
    assert!(!changed.inference_enabled);
    assert_eq!(
        changed.system_calendar_enabled,
        settings.system_calendar_enabled
    );
}

#[test]
fn commands_convert_to_engine_commands() {
    use crate::values::commands::EngineCommand as Command;
    let event = rich_event();
    let mirror_event = event.clone().to_mirror();
    let command = Command::AddAccount {
        kind_id: "google".into(),
    }
    .to_engine();
    assert!(matches!(command, EngineCommand::AddAccount { ref kind_id } if kind_id == "google"));
    assert!(
        matches!(Command::Snooze { event_id: "e".into(), seconds: 60.0 }.to_engine(), EngineCommand::Snooze { seconds, .. } if seconds == 60.0)
    );
    assert!(
        matches!(Command::Merge(mirror_event.clone(), mirror_event.clone()).to_engine(), EngineCommand::Merge(a, b) if a == event && b == event)
    );
    assert!(
        matches!(Command::Separate { event: mirror_event.clone(), members: event.merged_members.clone().to_mirror() }.to_engine(), EngineCommand::Separate { members, .. } if members == event.merged_members)
    );
    assert!(matches!(
        Command::System(crate::values::commands::SystemEvent::Wake).to_engine(),
        EngineCommand::System(SystemEvent::Wake)
    ));
    for (mirror, name) in [
        (Command::Start, "Start"),
        (Command::Refresh, "Refresh"),
        (Command::CancelAuthorization, "CancelAuthorization"),
        (
            Command::SettingsChangedExternally,
            "SettingsChangedExternally",
        ),
        (Command::ForgetCorrections, "ForgetCorrections"),
        (Command::Stop, "Stop"),
    ] {
        assert_eq!(format!("{:?}", mirror.to_engine()), name);
    }
    let applying = Command::ApplySettings(crate::values::settings::SettingsPatch {
        lead_time: Some(45.0),
        ..Default::default()
    })
    .to_engine();
    let EngineCommand::ApplySettings(change) = applying else {
        panic!("not ApplySettings")
    };
    let mut settings = EngineSettings::default();
    change(&mut settings);
    assert_eq!(settings.takeover.lead_time, 45.0);
}

#[test]
fn events_widget_and_diagnostics_convert_forward() {
    let event = rich_event();
    let request = TakeoverRequest {
        event,
        has_started: false,
        join_url: None,
        snooze_options: vec![],
    };
    let due = EngineEvent::TakeoverDue {
        request,
        calendar_title: Some("Work".into()),
    }
    .to_mirror();
    assert!(
        matches!(due, crate::values::events::EngineEvent::TakeoverDue { calendar_title: Some(ref t), .. } if t == "Work")
    );
    assert_eq!(
        EngineEvent::TakeoverWithdrawn {
            event_id: "e".into()
        }
        .to_mirror(),
        crate::values::events::EngineEvent::TakeoverWithdrawn {
            event_id: "e".into()
        }
    );
    let url = UrlString::parse("https://meet.example.com/x").unwrap();
    assert_eq!(
        EngineEvent::JoinRequested(url.clone()).to_mirror(),
        crate::values::events::EngineEvent::JoinRequested(url)
    );
    let widget_event = WidgetEvent {
        id: "w".into(),
        title: "T".into(),
        start: at(1),
        end: at(2),
        is_all_day: true,
        color_hex: Some("#112233".into()),
        join_url: UrlString::parse("https://x.example/"),
    };
    let snapshot = WidgetSnapshot {
        generated_at: at(3),
        events: vec![widget_event],
    };
    let mirrored = EngineEvent::WidgetSnapshot(snapshot).to_mirror();
    let crate::values::events::EngineEvent::WidgetSnapshot(mirrored) = mirrored else {
        panic!("not a snapshot")
    };
    assert_eq!(mirrored.events[0].color_hex.as_deref(), Some("#112233"));
    assert_eq!(mirrored.generated_at, at(3));
    let diagnostic = EngineDiagnostic::new(Level::Warning, "tokens", "message");
    assert_eq!(
        EngineEvent::Diagnostic(diagnostic).to_mirror(),
        crate::values::events::EngineEvent::Diagnostic(
            crate::values::diagnostics::EngineDiagnostic {
                level: crate::values::diagnostics::EngineDiagnosticLevel::Warning,
                category: "tokens".into(),
                message: "message".into()
            }
        )
    );
    for (level, text) in [(DiagnosticLevel::Debug, "d"), (DiagnosticLevel::Error, "e")] {
        let event = DiagnosticEvent {
            level,
            category: "c".into(),
            name: text.into(),
            fields: vec![
                DiagnosticField {
                    name: "i".into(),
                    value: DiagnosticFieldValue::Int(1),
                    is_private: false,
                },
                DiagnosticField {
                    name: "d".into(),
                    value: DiagnosticFieldValue::Double(1.5),
                    is_private: true,
                },
                DiagnosticField {
                    name: "b".into(),
                    value: DiagnosticFieldValue::Bool(true),
                    is_private: false,
                },
                DiagnosticField {
                    name: "s".into(),
                    value: DiagnosticFieldValue::String("v".into()),
                    is_private: true,
                },
            ],
            date: at(8),
        };
        round_trip(event);
    }
    for level in [
        DiagnosticLevel::Notice,
        DiagnosticLevel::Info,
        DiagnosticLevel::Warning,
    ] {
        round_trip(level);
    }
}

#[test]
fn account_values_round_trip() {
    round_trip(Connection {
        kind_id: "icloud".into(),
        connection_id: "ID".into(),
        display_name: "iCloud".into(),
        config: BTreeMap::from([("a".to_string(), "b".to_string())]),
    });
    round_trip(CredentialField {
        key: "user".into(),
        label: "User".into(),
        is_secret: false,
    });
    for method in [
        AuthorizationMethod::Oauth,
        AuthorizationMethod::System,
        AuthorizationMethod::Password {
            fields: vec![CredentialField {
                key: "pw".into(),
                label: "Password".into(),
                is_secret: true,
            }],
        },
    ] {
        round_trip(method);
    }
    round_trip(CredentialHelp {
        text: "Use an app password".into(),
        link_title: Some("Learn more".into()),
        url: UrlString::parse("https://support.example.com/"),
    });
    round_trip(CredentialSnapshot {
        secrets: BTreeMap::from([("token".to_string(), "x".to_string())]),
        revision: uuid::Uuid::from_u128(7),
    });
}

#[test]
fn a_platform_names_the_engine_flag() {
    use crate::values::accounts::Platform;
    use calendar_core::connection::Platform as Flag;
    assert_eq!(Platform::Macos.to_flag(), Flag::MACOS);
    assert_eq!(Platform::Ios.to_flag(), Flag::IOS);
    assert_eq!(Platform::Linux.to_flag(), Flag::LINUX);
    assert_eq!(Platform::Windows.to_flag(), Flag::WINDOWS);
}
