//! Port of `GoldenMessages.swift`: the engine-to-host messages the golden fixtures pin, built from fixed engine
//! values. A fixture file is the reviewed JSON of exactly one of these; the test fails if the mapping drifts from it.

use std::collections::BTreeMap;

use calendar_core::connection::Connection;
use calendar_core::model::ResponseStatus;
use calendar_core::source_types::{SourceNotice, SourceNoticeKind};
use calendar_core::time::{Zone, add_seconds};
use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use timetug_core::agenda::day_agenda::DayAgenda;
use timetug_core::dedup::adjudication::EngineInfo;
use timetug_core::takeover::ledger::TakeoverLedger;
use timetug_core::takeover::request::TakeoverRequest;
use timetug_core::takeover::scheduler::Scheduler;
use timetug_core::widget::snapshot::WidgetSnapshot;
use timetug_core::{
    CalendarInfo, InferenceStatus, MergeProvenance, MergedMember, SourceStatus, TakeoverSettings,
    TimeTugCalendarEvent,
};
use timetug_engine::accounts::account_manager::{Status, WaitingStyle};
use timetug_engine::diagnostic::{EngineDiagnostic, Level};
use timetug_engine::event::EngineEvent;
use timetug_engine::settings::EngineSettings;
use timetug_engine::state::{AccountKindInfo, Authorization, EngineState};

use super::fixture;
use crate::envelope::{Envelope, MessageId};
use crate::message_catalog::{WireReady, WireReadyCapabilities};
use crate::state_mapping::{wire_diagnostic, wire_message_for_event, wire_message_for_state};

/// 2025-10-09T08:53:20Z.
pub(crate) fn base() -> Timestamp {
    super::ts(1_760_000_000)
}

pub(crate) fn calendar() -> Zone {
    Zone::named("UTC").unwrap()
}

pub(crate) fn settings() -> EngineSettings {
    let mut s = EngineSettings::default();
    s.takeover.lead_time = 120.0;
    s.takeover.takeover_calendar_keys = ["google-1/primary".to_string()].into();
    s.takeover.hidden_calendar_keys = ["google-1/holidays".to_string()].into();
    s.inference_enabled = true;
    s
}

pub(crate) fn standup() -> TimeTugCalendarEvent {
    let mut e = fixture::timed_with(
        "evt-1",
        "Standup",
        add_seconds(base(), 1800.0),
        15,
        "primary",
        "google-1",
        Some("https://meet.google.com/abc-defg-hij"),
    );
    e.other_attendee_count = 4;
    e.response_status = Some(ResponseStatus::Accepted);
    e
}

pub(crate) fn merged() -> TimeTugCalendarEvent {
    let mut e = fixture::timed_with(
        "evt-2",
        "Design review",
        add_seconds(base(), 7200.0),
        60,
        "primary",
        "google-1",
        None,
    );
    e.additional_calendar_keys = ["caldav-1/work".to_string()].into();
    e.display_start = Some(add_seconds(e.start(), -300.0));
    e.merged_members = vec![
        MergedMember::new(
            "Design review",
            "google-1/primary",
            "design review|1760007200|1760010800",
            "location",
            e.start(),
            e.end(),
        ),
        MergedMember::new(
            "Design Review",
            "caldav-1/work",
            "design review|1760006900|1760010500",
            "bare",
            add_seconds(e.start(), -300.0),
            add_seconds(e.end(), -300.0),
        ),
    ];
    e.merge_provenance = Some(MergeProvenance::Inference {
        engine_id: "foundation-models".into(),
        engine_name: "Apple Intelligence".into(),
    });
    e
}

pub(crate) fn holiday() -> TimeTugCalendarEvent {
    fixture::all_day("evt-3", &fixture::berlin())
}

pub(crate) fn state() -> EngineState {
    let events = vec![standup(), merged(), holiday()];
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&events[0], base());
    let mut state = EngineState::empty();
    state.revision = 12;
    state.events = events.clone();
    state.agenda = DayAgenda::make(&events, &settings().takeover, base(), &calendar());
    let mut work = CalendarInfo::new("google-1", "primary", "Work").with_color_hex(Some("#3366cc"));
    work.account_name = Some("me@example.com".into());
    work.service = Some(calendar_core::model::CalendarService::google());
    work.provider = Some(calendar_core::model::CalendarProvider::google());
    let mut holidays = CalendarInfo::new("google-1", "holidays", "Holidays");
    holidays.kind = calendar_core::model::CalendarKind::Subscribed;
    holidays.service = Some(calendar_core::model::CalendarService::google());
    holidays.provider = Some(calendar_core::model::CalendarProvider::google());
    state.calendars = vec![work, holidays];
    state.statuses = BTreeMap::from([
        ("google-1".to_string(), SourceStatus::Ok),
        (
            "caldav-1".to_string(),
            SourceStatus::Failing("Server unreachable".into()),
        ),
        ("ms-1".to_string(), SourceStatus::AuthExpired),
        ("eventkit".to_string(), SourceStatus::NeedsPermission),
    ]);
    state.notices = BTreeMap::from([(
        "google-1".to_string(),
        vec![SourceNotice::new(SourceNoticeKind::UnreadableRecurrence, 2)],
    )]);
    state.source_names = BTreeMap::from([
        ("google-1".to_string(), "me@example.com".to_string()),
        ("caldav-1".to_string(), "Work CalDAV".to_string()),
    ]);
    state.candidates = BTreeMap::from([(events[0].id(), vec![events[1].clone()])]);
    state.accounts = vec![
        Connection::new("google", "1", "me@example.com"),
        Connection::new("caldav", "1", "Work CalDAV")
            .with_config(&[("server", "https://cal.example.com")]),
    ];
    let mut status = Status::empty();
    status.accounts = state.accounts.clone();
    status.is_working = true;
    status.build_failures = BTreeMap::from([("2".to_string(), "Could not connect".to_string())]);
    status.waiting_style = WaitingStyle::InApp;
    state.account_status = status;
    state.settings = settings();
    state.inference_status = InferenceStatus::Active(EngineInfo::new(
        "foundation-models",
        "Apple Intelligence",
        true,
    ));
    state.ledger = ledger;
    let mut takeover = settings().takeover;
    takeover.takeover_calendar_keys = ["google-1/primary".to_string()].into();
    takeover.hidden_calendar_keys = Default::default();
    state.next_takeover = Scheduler::next(&events, &takeover, &TakeoverLedger::new(), base());
    state.takeover_visible = true;
    state.available_account_kinds = vec![
        AccountKindInfo::new("google", "Google", Authorization::Oauth, true),
        AccountKindInfo::new("microsoft", "Microsoft", Authorization::Oauth, false),
        AccountKindInfo::new("caldav", "Other CalDAV", Authorization::Password, true),
    ];
    state
}

pub(crate) fn widget() -> WidgetSnapshot {
    let mut takeover = TakeoverSettings::default();
    takeover.skip_all_day_events = false;
    WidgetSnapshot::make(
        &[standup(), holiday()],
        &state().calendars,
        &takeover,
        base(),
        &calendar(),
    )
}

pub(crate) fn ready() -> Envelope {
    let ready = WireReady::new("1.4.0", "windows", WireReadyCapabilities::new(false, false));
    Envelope::new("ready").with_payload(serde_json::to_value(ready).unwrap())
}

/// File name (without `.json`) to message.
pub(crate) fn all() -> BTreeMap<String, Envelope> {
    let request = TakeoverRequest::make(&standup(), add_seconds(base(), 1750.0));
    let mut all_day_only = EngineState::empty();
    all_day_only.events = vec![holiday()];
    all_day_only.agenda = DayAgenda::make(
        &[holiday()],
        &TakeoverSettings::default(),
        base(),
        &calendar(),
    );
    all_day_only.revision = 1;
    let mut each_status = EngineState::empty();
    each_status.statuses = BTreeMap::from([
        ("a".to_string(), SourceStatus::Ok),
        ("b".to_string(), SourceStatus::NeedsPermission),
        ("c".to_string(), SourceStatus::AuthExpired),
        ("d".to_string(), SourceStatus::Failing("HTTP 503".into())),
    ]);
    each_status.inference_status = InferenceStatus::Unavailable {
        reason: "model not ready".into(),
    };
    let list: Vec<(&str, Envelope)> = vec![
        ("ready", ready()),
        ("state", wire_message_for_state(&state())),
        (
            "event.takeoverDue",
            wire_message_for_event(&EngineEvent::TakeoverDue {
                request,
                calendar_title: Some("Work".into()),
            }),
        ),
        (
            "event.takeoverWithdrawn",
            wire_message_for_event(&EngineEvent::TakeoverWithdrawn {
                event_id: standup().id(),
            }),
        ),
        (
            "event.widgetSnapshot",
            wire_message_for_event(&EngineEvent::WidgetSnapshot(widget())),
        ),
        (
            "event.joinRequested",
            wire_message_for_event(&EngineEvent::JoinRequested(
                UrlString::parse("https://meet.google.com/abc-defg-hij").unwrap(),
            )),
        ),
        (
            "event.diagnostic",
            wire_message_for_event(&EngineEvent::Diagnostic(EngineDiagnostic::new(
                Level::Warning,
                "settings",
                "settings.json unreadable; using defaults",
            ))),
        ),
        (
            "edge-state-empty",
            wire_message_for_state(&EngineState::empty()),
        ),
        (
            "edge-state-all-day-only",
            wire_message_for_state(&all_day_only),
        ),
        (
            "edge-event-diagnostic-echoes-id",
            wire_diagnostic(
                &EngineDiagnostic::new(Level::Warning, "command", "unknown event id"),
                Some(MessageId::String("cmd-9".into())),
            ),
        ),
        (
            "edge-state-sources-each-status",
            wire_message_for_state(&each_status),
        ),
    ];
    list.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}
