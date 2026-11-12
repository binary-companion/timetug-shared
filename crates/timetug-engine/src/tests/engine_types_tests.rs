//! Port of `EngineTypesTests.swift`.

use calendar_core::model::CalendarEvent;
use calendar_core::url_string::UrlString;
use timetug_core::takeover::ledger::TakeoverLedger;
use timetug_core::takeover::request::TakeoverRequest;
use timetug_core::widget::snapshot::WidgetSnapshot;
use timetug_core::{InferenceStatus, TimeTugCalendarEvent};

use super::support::ts;
use crate::accounts::account_manager::Status;
use crate::command::{EngineCommand, SystemEvent};
use crate::diagnostic::{EngineDiagnostic, Level};
use crate::event::EngineEvent;
use crate::settings::EngineSettings;
use crate::state::EngineState;

fn event(id: &str) -> TimeTugCalendarEvent {
    let start = ts(1_800_000_000);
    TimeTugCalendarEvent::new(
        CalendarEvent::new(id, "c", "Standup", start, ts(1_800_001_800)),
        "s",
    )
}

#[test]
fn empty_state_is_the_zero_state() {
    let s = EngineState::empty();
    assert_eq!(s.revision, 0);
    assert_eq!(
        s.agenda,
        timetug_core::agenda::day_agenda::DayAgenda::empty()
    );
    assert!(s.events.is_empty() && s.calendars.is_empty() && s.statuses.is_empty());
    assert!(
        s.notices.is_empty()
            && s.source_names.is_empty()
            && s.candidates.is_empty()
            && s.accounts.is_empty()
    );
    assert_eq!(s.account_status, Status::empty());
    assert_eq!(s.settings, EngineSettings::default());
    assert_eq!(s.inference_status, InferenceStatus::Disabled);
    assert_eq!(s.ledger, TakeoverLedger::new());
    assert!(s.next_takeover.is_none());
    assert!(!s.takeover_visible);
    assert!(s.available_account_kinds.is_empty());
}

#[test]
fn every_command_constructs() {
    let e = event("e1");
    let mut commands: Vec<EngineCommand> = vec![
        EngineCommand::Start,
        EngineCommand::Refresh,
        EngineCommand::AddAccount {
            kind_id: "google".into(),
        },
        EngineCommand::RemoveAccount {
            connection_id: "c".into(),
        },
        EngineCommand::ReauthorizeAccount {
            connection_id: "c".into(),
        },
        EngineCommand::CancelAuthorization,
        EngineCommand::ApplySettings(Box::new(|s| s.inference_enabled = true)),
        EngineCommand::SettingsChangedExternally,
        EngineCommand::SetCountdownVisible(true),
        EngineCommand::TakeoverPresented {
            event_id: "e1".into(),
            on_screen: false,
        },
        EngineCommand::TakeoverClosed {
            event_id: "e1".into(),
        },
        EngineCommand::Join {
            event_id: "e1".into(),
        },
        EngineCommand::Snooze {
            event_id: "e1".into(),
            seconds: 60.0,
        },
        EngineCommand::Dismiss {
            event_id: "e1".into(),
        },
        EngineCommand::TestTug {
            title: "Sample".into(),
            calendar_title: "Calendar".into(),
        },
        EngineCommand::Merge(e.clone(), e.clone()),
        EngineCommand::Unmerge(e.clone()),
        EngineCommand::Separate {
            event: e.clone(),
            members: vec![],
        },
        EngineCommand::ForgetCorrections,
        EngineCommand::System(SystemEvent::Wake),
        EngineCommand::System(SystemEvent::NetworkReturned),
        EngineCommand::Stop,
    ];
    assert_eq!(commands.len(), 22);
    match commands.remove(6) {
        EngineCommand::ApplySettings(change) => {
            let mut settings = EngineSettings::default();
            change(&mut settings);
            assert!(settings.inference_enabled);
        }
        _ => panic!("expected applySettings"),
    }
}

#[test]
fn every_event_constructs() {
    let e = event("e1");
    let events = [
        EngineEvent::TakeoverDue {
            request: TakeoverRequest::make(&e, e.start()),
            calendar_title: None,
        },
        EngineEvent::TakeoverWithdrawn {
            event_id: "e1".into(),
        },
        EngineEvent::WidgetSnapshot(WidgetSnapshot::new(e.start(), vec![])),
        EngineEvent::JoinRequested(UrlString::parse("https://example.com/j").unwrap()),
        EngineEvent::Diagnostic(EngineDiagnostic {
            level: Level::Info,
            category: "t".into(),
            message: "m".into(),
        }),
    ];
    assert_eq!(events.len(), 5);
}

#[test]
fn state_is_mutable_value() {
    let mut s = EngineState::empty();
    s.revision = 3;
    assert_eq!(EngineState::empty().revision, 0);
    assert_eq!(s.revision, 3);
}
