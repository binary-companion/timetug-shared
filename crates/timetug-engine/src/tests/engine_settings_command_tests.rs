//! Port of `EngineSettingsCommandTests.swift`.

use std::sync::Arc;

use calendar_core::time::add_seconds;
use timetug_core::widget::snapshot::WidgetSnapshot;
use timetug_core::{InferenceStatus, SourceStatus};

use super::support::account_fakes::{FakeSource, Gate};
use super::support::engine_harness::{EngineHarness, FakeEngineGenerator, HarnessOptions, sample};
use crate::command::EngineCommand;
use crate::inference::text_generator::TextGenerator;
use crate::settings::{EngineSettings, SettingsStorage};
use crate::state::EngineState;

fn apply(change: impl FnOnce(&mut EngineSettings) + Send + 'static) -> EngineCommand {
    EngineCommand::ApplySettings(Box::new(change))
}

async fn started(h: &EngineHarness, sources: usize) {
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| {
        s.statuses.len() == sources
            && s.statuses
                .values()
                .all(|status| *status == SourceStatus::Ok)
    })
    .await;
    h.settle().await;
}

fn expected_widget(state: &EngineState, h: &EngineHarness) -> WidgetSnapshot {
    use crate::clock::EngineClock;
    WidgetSnapshot::make(
        &state.events,
        &state.calendars,
        &state.settings.takeover,
        h.clock.now(),
        &EngineHarness::utc(),
    )
}

// Scenario 6
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn apply_settings_persists_through_the_host_and_emits_a_widget_snapshot() {
    let h = EngineHarness::new(vec![sample("e1", 3600.0)]).await;
    started(&h, 1).await;
    let saved = h.settings.saved().len();
    let widgets = h.widget_snapshots().len();

    h.engine.send(apply(|s| s.takeover.lead_time = 300.0)).await;
    let state = h.engine.current_state();
    assert_eq!(h.settings.saved().len(), saved + 1);
    assert_eq!(
        h.settings.saved().last().map(|s| s.takeover.lead_time),
        Some(300.0)
    );
    assert_eq!(state.settings.takeover.lead_time, 300.0);
    assert_eq!(
        state.next_takeover.as_ref().map(|n| n.fire_at),
        Some(add_seconds(EngineHarness::epoch(), 3300.0))
    );
    // Scenario 8: a settings change emits a widget snapshot built with the new settings.
    // At least one more widget snapshot; the last one is built with the new settings.
    h.wait_until("the widget snapshot", || {
        let snapshots = h.widget_snapshots();
        snapshots.len() > widgets && snapshots.last() == Some(&expected_widget(&state, &h))
    })
    .await;

    // No change, no write.
    h.engine.send(apply(|s| s.takeover.lead_time = 300.0)).await;
    assert_eq!(h.settings.saved().len(), saved + 1);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn settings_changed_externally_rereads_and_republishes_without_writing_back() {
    let h = EngineHarness::new(vec![sample("e1", 3600.0)]).await;
    started(&h, 1).await;
    let mut external = h.settings.load();
    external.takeover.lead_time = 300.0;
    external.takeover.skip_all_day_events = !external.takeover.skip_all_day_events;
    h.settings.save(&external);
    let saved = h.settings.saved().len();
    let before = h.engine.current_state();
    let widgets = h.widget_snapshots().len();

    h.engine
        .send(EngineCommand::SettingsChangedExternally)
        .await;
    let state = h.engine.current_state();
    assert_eq!(state.settings, external);
    assert!(state.revision > before.revision);
    assert_eq!(
        state.next_takeover.as_ref().map(|n| n.fire_at),
        Some(add_seconds(EngineHarness::epoch(), 3300.0))
    );
    assert_eq!(h.settings.saved().len(), saved);
    h.wait_until("the widget snapshot", || {
        let snapshots = h.widget_snapshots();
        snapshots.len() > widgets && snapshots.last() == Some(&expected_widget(&state, &h))
    })
    .await;

    // Nothing changed since: no publish.
    h.engine
        .send(EngineCommand::SettingsChangedExternally)
        .await;
    assert_eq!(h.engine.current_state().revision, state.revision);
    assert_eq!(h.settings.saved().len(), saved);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn inference_turned_on_externally_resolves_pending_pairs() {
    let generator = FakeEngineGenerator::new(Some("same"));
    let h = EngineHarness::with(HarnessOptions {
        accounts: [
            ("c1".to_string(), vec![sample("a", 3600.0)]),
            ("c2".to_string(), vec![sample("b", 3600.0)]),
        ]
        .into_iter()
        .collect(),
        text_generator: Some(generator.clone() as Arc<dyn TextGenerator>),
        ..HarnessOptions::default()
    })
    .await;
    started(&h, 2).await;
    assert_eq!(h.engine.current_state().events.len(), 2);
    let mut external = h.settings.load();
    external.inference_enabled = true;
    h.settings.save(&external);
    let saved = h.settings.saved().len();

    h.engine
        .send(EngineCommand::SettingsChangedExternally)
        .await;
    let state = h.engine.current_state();
    assert_eq!(
        state.inference_status,
        InferenceStatus::Active(FakeEngineGenerator::on_device())
    );
    assert_eq!(state.events.len(), 1);
    assert_eq!(generator.calls.get(), 1);
    assert_eq!(h.settings.saved().len(), saved);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_system_calendar_switch_goes_through_the_account_manager() {
    let h = EngineHarness::with(HarnessOptions {
        events: vec![sample("e1", 3600.0)],
        system_source: Some(Arc::new(FakeSource::new("eventkit"))),
        ..HarnessOptions::default()
    })
    .await;
    started(&h, 1).await;
    assert!(
        !h.engine
            .current_state()
            .source_names
            .contains_key("eventkit")
    );
    assert_eq!(h.system_access_requests.get(), 0);

    h.engine
        .send(apply(|s| s.system_calendar_enabled = true))
        .await;
    let mut state = h.engine.current_state();
    assert!(state.source_names.contains_key("eventkit"));
    assert!(state.settings.system_calendar_enabled);
    assert_eq!(
        h.settings.saved().last().map(|s| s.system_calendar_enabled),
        Some(true)
    );
    assert_eq!(h.system_access_requests.get(), 1);

    h.engine
        .send(apply(|s| s.system_calendar_enabled = false))
        .await;
    state = h.engine.current_state();
    assert!(!state.source_names.contains_key("eventkit"));
    assert_eq!(h.system_access_requests.get(), 1);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn without_a_host_system_source_the_switch_is_stored_but_builds_nothing() {
    let h = EngineHarness::new(vec![sample("e1", 3600.0)]).await;
    started(&h, 1).await;
    h.engine
        .send(apply(|s| s.system_calendar_enabled = true))
        .await;
    let state = h.engine.current_state();
    assert!(state.settings.system_calendar_enabled);
    let names: Vec<&String> = state.source_names.keys().collect();
    assert_eq!(names, vec!["fake-c1"]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn sign_in_progress_is_published_while_it_runs() {
    let h = EngineHarness::new(vec![sample("e1", 3600.0)]).await;
    started(&h, 1).await;
    let gate = Arc::new(Gate::new());
    h.kind.set_authorize_gate(gate.clone());
    let engine = h.engine.clone();
    let sign_in = tokio::spawn(async move {
        engine
            .send(EngineCommand::AddAccount {
                kind_id: "fake".to_string(),
            })
            .await;
    });
    let working = h.wait_for_state(|s| s.account_status.is_working).await;
    assert!(working.account_status.is_working);
    gate.open().await;
    sign_in.await.unwrap();
    let done = h.wait_for_state(|s| !s.account_status.is_working).await;
    assert!(done.revision > working.revision);
    assert_eq!(done.accounts.len(), 2);
}
