//! Port of `EngineReviewFixTests.swift`: whole-engine review fixes: invalid account commands, the add-account choices,
//! teardown without `.stop`, and the system calendar switch under a concurrent external change.

use std::sync::{Arc, Weak};

use calendar_core::connection::AuthorizationMethod;
use timetug_core::SourceStatus;

use super::support::account_fakes::{FakeConnectorKind, FakeOutcome, FakeSource, Gate};
use super::support::engine_harness::{EngineHarness, HarnessOptions, sample};
use super::support::poll;
use crate::command::EngineCommand;
use crate::diagnostic::{EngineDiagnostic, Level};
use crate::engine::Engine;
use crate::event::EngineEvent;
use crate::settings::SettingsStorage;
use crate::state::{AccountKindInfo, Authorization};

async fn started(h: &EngineHarness) {
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| s.statuses.get("fake-c1") == Some(&SourceStatus::Ok))
        .await;
    h.settle().await;
}

fn account_diagnostics(h: &EngineHarness) -> Vec<EngineDiagnostic> {
    h.emitted.mutate(|list| {
        list.iter()
            .filter_map(|event| match event {
                EngineEvent::Diagnostic(d) if d.category == "accounts" => Some(d.clone()),
                _ => None,
            })
            .collect()
    })
}

// 1
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn invalid_account_commands_set_an_error_and_emit_a_diagnostic() {
    let h = EngineHarness::new(vec![sample("e1", 3600.0)]).await;
    started(&h).await;

    h.engine
        .send(EngineCommand::AddAccount {
            kind_id: "nope".to_string(),
        })
        .await;
    let mut state = h
        .wait_for_state(|s| s.account_status.error_message.is_some())
        .await;
    assert_eq!(
        state.account_status.error_message.as_deref(),
        Some("Unknown account type.")
    );
    h.wait_until("the add diagnostic", || account_diagnostics(&h).len() == 1)
        .await;

    h.engine
        .send(EngineCommand::ReauthorizeAccount {
            connection_id: "missing".to_string(),
        })
        .await;
    state = h.engine.current_state();
    assert_eq!(
        state.account_status.error_message.as_deref(),
        Some("That account no longer exists.")
    );
    h.wait_until("the reauthorize diagnostic", || {
        account_diagnostics(&h).len() == 2
    })
    .await;

    h.engine
        .send(EngineCommand::RemoveAccount {
            connection_id: "gone".to_string(),
        })
        .await;
    state = h.engine.current_state();
    assert_eq!(
        state.account_status.error_message.as_deref(),
        Some("That account no longer exists.")
    );
    h.wait_until("the remove diagnostic", || {
        account_diagnostics(&h).len() == 3
    })
    .await;
    let accounts: Vec<&str> = state
        .accounts
        .iter()
        .map(|a| a.connection_id.as_str())
        .collect();
    assert_eq!(accounts, vec!["c1"]);
    assert!(
        account_diagnostics(&h)
            .iter()
            .all(|d| d.level == Level::Warning)
    );
}

// 2
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn available_account_kinds_list_the_choices_with_caldav_last_and_unconfigured_marked() {
    let password = || AuthorizationMethod::Password { fields: Vec::new() };
    let caldav =
        FakeConnectorKind::with("caldav", "Other CalDAV", password(), FakeOutcome::Succeed);
    let icloud = FakeConnectorKind::with("icloud", "iCloud", password(), FakeOutcome::Succeed);
    let system = FakeConnectorKind::with(
        "eventkit",
        "Apple Calendar",
        AuthorizationMethod::System,
        FakeOutcome::Succeed,
    );
    let microsoft = FakeConnectorKind::with(
        "microsoft",
        "Microsoft",
        AuthorizationMethod::Oauth,
        FakeOutcome::Succeed,
    );
    let h = EngineHarness::with(HarnessOptions {
        events: vec![sample("e1", 3600.0)],
        extra_kinds: vec![caldav, icloud, system, microsoft],
        unconfigured_kind_ids: vec!["google".to_string(), "microsoft".to_string()],
        ..HarnessOptions::default()
    })
    .await;
    started(&h).await;
    let kinds = h.engine.current_state().available_account_kinds;
    assert_eq!(
        kinds,
        vec![
            AccountKindInfo::new("fake", "Fake", Authorization::Oauth, true),
            AccountKindInfo::new("google", "Google", Authorization::Oauth, false),
            AccountKindInfo::new("icloud", "iCloud", Authorization::Password, true),
            AccountKindInfo::new("microsoft", "Microsoft", Authorization::Oauth, false),
            AccountKindInfo::new("caldav", "Other CalDAV", Authorization::Password, true),
        ]
    );
}

// 3
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn dropping_the_engine_without_stop_ends_every_task() {
    let weak_engine: Weak<Engine>;
    let clock;
    let source;
    {
        let h = EngineHarness::new(vec![sample("e1", 600.0)]).await;
        started(&h).await;
        h.engine
            .send(EngineCommand::SetCountdownVisible(true))
            .await;
        clock = h.clock.clone();
        source = h.c1().clone();
        assert!(clock.pending_sleeps() > 0);
        assert!(!source.listener_ended());
        weak_engine = Arc::downgrade(&h.engine);
    }
    poll(
        || "the engine and its tasks to end".to_string(),
        30.0,
        || async {
            weak_engine.upgrade().is_none()
                && clock.pending_sleeps() == 0
                && source.listener_ended()
        },
    )
    .await;
    assert!(weak_engine.upgrade().is_none());
    assert_eq!(clock.pending_sleeps(), 0);
    assert!(source.listener_ended());
}

// 4
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_last_external_system_calendar_value_wins_over_an_in_flight_change() {
    let gate = Arc::new(Gate::new());
    let h = EngineHarness::with(HarnessOptions {
        events: vec![sample("e1", 3600.0)],
        system_source: Some(Arc::new(FakeSource::new("eventkit"))),
        system_access_gate: Some(gate.clone()),
        ..HarnessOptions::default()
    })
    .await;
    started(&h).await;
    let saved_before = h.settings.saved().len();

    // Turning it on waits in the host's access request...
    let engine = h.engine.clone();
    let turn_on = tokio::spawn(async move {
        engine
            .send(EngineCommand::ApplySettings(Box::new(|s| {
                s.system_calendar_enabled = true;
            })))
            .await;
    });
    h.wait_until("the access request", || gate.is_waiting_now())
        .await;
    // ...while another process turns it off again.
    let mut external = h.settings.load();
    external.system_calendar_enabled = false;
    h.settings.save(&external);
    h.engine
        .send(EngineCommand::SettingsChangedExternally)
        .await;
    gate.open().await;
    turn_on.await.unwrap();
    h.settle().await;

    // Only the engine wrote (the change itself); the account manager never wrote the stale `true` back.
    assert_eq!(h.settings.saved().len(), saved_before + 2);
    assert!(!h.settings.load().system_calendar_enabled);
    let state = h.engine.current_state();
    assert!(!state.settings.system_calendar_enabled);
    assert!(!state.source_names.contains_key("eventkit"));
}
