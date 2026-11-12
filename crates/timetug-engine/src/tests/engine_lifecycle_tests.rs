//! Rust-only: the lifecycle contract of the `Engine` API that the Swift suite reaches only through `AsyncStream`:
//! `stop` ends `states()` and `events()`, a refresh finishing after `stop` publishes nothing, a second `start` is
//! ignored with a warning, and the engine is `Send + Sync` (the C interface shares it between tasks).

use std::sync::Arc;

use timetug_core::SourceStatus;

use super::support::engine_harness::{EngineHarness, sample};
use crate::command::EngineCommand;
use crate::diagnostic::Level;
use crate::engine::Engine;

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn the_engine_can_be_shared_between_tasks() {
    assert_send_sync::<Engine>();
    assert_send_sync::<Arc<Engine>>();
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn stop_ends_the_state_and_event_channels_after_what_was_queued() {
    let h = EngineHarness::new(vec![sample("e1", 3600.0)]).await;
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| s.statuses.get("fake-c1") == Some(&SourceStatus::Ok))
        .await;
    h.settle().await;
    let mut states = h.engine.states();
    let mut events = h.engine.events();
    let last = h.engine.current_state().revision;

    h.engine.send(EngineCommand::Stop).await;
    // The newest state is still delivered, then the channel ends.
    states.changed().await.unwrap();
    assert_eq!(states.borrow_and_update().revision, last);
    assert!(states.changed().await.is_err());
    // Events queued before the stop are drained, then the channel ends.
    while events.recv().await.is_some() {}
    assert!(events.recv().await.is_none());
    // A receiver made after the stop still starts with the current state and then ends.
    let mut late = h.engine.states();
    late.changed().await.unwrap();
    assert_eq!(late.borrow_and_update().revision, last);
    assert!(late.changed().await.is_err());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_refresh_finishing_after_stop_publishes_nothing() {
    let h = EngineHarness::new(vec![sample("e1", 600.0)]).await;
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| s.statuses.get("fake-c1") == Some(&SourceStatus::Ok))
        .await;
    h.settle().await;
    let source = h.c1().clone();
    source.set_gated(true);
    let engine = h.engine.clone();
    let refresh = tokio::spawn(async move { engine.send(EngineCommand::Refresh).await });
    h.wait_until("the refresh at the gate", || source.gate.waiting() == 1)
        .await;

    h.engine.send(EngineCommand::Stop).await;
    let revision = h.engine.current_state().revision;
    let widgets = h.widget_snapshots().len();
    source.set_gated(false);
    source.gate.release_all();
    refresh.await.unwrap();
    h.settle().await;
    assert_eq!(h.engine.current_state().revision, revision);
    assert_eq!(h.widget_snapshots().len(), widgets);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_second_start_is_ignored_with_a_warning() {
    let h = EngineHarness::new(vec![sample("e1", 600.0)]).await;
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| s.statuses.get("fake-c1") == Some(&SourceStatus::Ok))
        .await;
    h.settle().await;
    let calls = h.c1().events_calls();
    h.engine.send(EngineCommand::Start).await;
    h.settle().await;
    assert_eq!(h.c1().events_calls(), calls);
    assert!(h.diagnostics.get().iter().any(|d| d.level == Level::Warning
        && d.category == "engine"
        && d.message == "start was sent more than once; ignored"));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_cancelled_sign_in_ends_and_leaves_the_accounts_alone() {
    use super::support::account_fakes::Gate;
    let h = EngineHarness::new(vec![sample("e1", 3600.0)]).await;
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| s.statuses.get("fake-c1") == Some(&SourceStatus::Ok))
        .await;
    h.settle().await;
    // The sign-in waits at a gate that is never opened; cancelling ends the task and the command returns.
    h.kind.set_authorize_gate(Arc::new(Gate::new()));
    let engine = h.engine.clone();
    let sign_in = tokio::spawn(async move {
        engine
            .send(EngineCommand::AddAccount {
                kind_id: "fake".to_string(),
            })
            .await;
    });
    h.wait_for_state(|s| s.account_status.is_working).await;
    // A second sign-in while one runs is ignored with a warning.
    h.engine
        .send(EngineCommand::AddAccount {
            kind_id: "fake".to_string(),
        })
        .await;
    assert!(
        h.diagnostics
            .get()
            .iter()
            .any(|d| d.category == "accounts"
                && d.message == "a sign-in is already running; ignored")
    );
    h.engine.send(EngineCommand::CancelAuthorization).await;
    sign_in.await.unwrap();
    let state = h.wait_for_state(|s| !s.account_status.is_working).await;
    assert_eq!(state.accounts.len(), 1);
}

/// The task-2 note carried to task 4: the Google and Microsoft kinds close their redirect session only after the flow
/// returns, so a cancelled sign-in must be dropped (not merely abandoned) for the session's `Drop` to run. Through the
/// engine: `CancelAuthorization` ends the engine's sign-in task and drops the pending `authorize` future.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn cancel_authorization_drops_a_pending_sign_in_so_its_guards_run() {
    use std::sync::atomic::{AtomicBool, Ordering};

    use calendar_core::connection::AuthorizationMethod;

    use super::support::account_fakes::{FakeConnectorKind, FakeOutcome};
    use super::support::engine_harness::HarnessOptions;

    let dropped = Arc::new(AtomicBool::new(false));
    let hang = FakeConnectorKind::with(
        "hang",
        "Hang",
        AuthorizationMethod::Oauth,
        FakeOutcome::HangHoldingGuard {
            dropped: dropped.clone(),
        },
    );
    let h = EngineHarness::with(HarnessOptions {
        events: vec![sample("e1", 3600.0)],
        extra_kinds: vec![hang],
        ..HarnessOptions::default()
    })
    .await;
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| s.statuses.get("fake-c1") == Some(&SourceStatus::Ok))
        .await;
    h.settle().await;
    let engine = h.engine.clone();
    let sign_in = tokio::spawn(async move {
        engine
            .send(EngineCommand::AddAccount {
                kind_id: "hang".to_string(),
            })
            .await;
    });
    h.wait_for_state(|s| s.account_status.is_working).await;
    assert!(!dropped.load(Ordering::SeqCst));
    h.engine.send(EngineCommand::CancelAuthorization).await;
    sign_in.await.unwrap();
    assert!(dropped.load(Ordering::SeqCst));
    let state = h.wait_for_state(|s| !s.account_status.is_working).await;
    assert_eq!(state.accounts.len(), 1);
}
