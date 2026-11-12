//! Port of `Accounts/SourceRefreshCoordinatorTests.swift` (2 tests).

use std::collections::BTreeSet;
use std::sync::Arc;

use timetug_core::SourceChange;

use crate::accounts::account_manager::BoxFuture;
use crate::accounts::source_refresh_coordinator::SourceRefreshCoordinator;
use crate::activity::EngineActivity;

use super::support::account_fakes::Gate;
use super::support::{LockedBox, poll};

type Calls = Arc<LockedBox<Vec<Option<BTreeSet<String>>>>>;

fn ids(items: &[&str]) -> Option<BTreeSet<String>> {
    Some(items.iter().map(|s| (*s).to_string()).collect())
}

fn events_changed(source: &str, calendars: Option<&[&str]>) -> SourceChange {
    SourceChange::EventsChanged {
        source_id: source.to_string(),
        calendar_ids: calendars.map(|c| c.iter().map(|s| (*s).to_string()).collect()),
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn burst_during_suspended_refresh_runs_one_follow_up_with_all_dirty_sources() {
    let calls: Calls = Arc::new(LockedBox::new(Vec::new()));
    let gate = Arc::new(Gate::new());
    let activity = Arc::new(EngineActivity::new());
    let (refresh_calls, refresh_gate) = (calls.clone(), gate.clone());
    let coordinator = SourceRefreshCoordinator::new(
        Some(activity.clone()),
        Arc::new(move |batch| -> BoxFuture<()> {
            let (calls, gate) = (refresh_calls.clone(), refresh_gate.clone());
            Box::pin(async move {
                let count = calls.mutate(|calls| {
                    calls.push(batch);
                    calls.len()
                });
                if count == 1 {
                    gate.wait().await;
                }
            })
        }),
    );
    coordinator
        .signal(events_changed("a", Some(&["one"])))
        .await;
    // The first pass is held at the gate; every later signal is awaited, so all of them arrive while it is held.
    poll(
        || "the first pass at the gate".to_string(),
        30.0,
        || gate.is_waiting(),
    )
    .await;
    assert_eq!(calls.get(), vec![ids(&["a"])]);
    for _ in 0..100 {
        coordinator
            .signal(events_changed("a", Some(&["one"])))
            .await;
    }
    coordinator
        .signal(SourceChange::SourceFailed {
            source_id: "b".to_string(),
        })
        .await;
    gate.open().await;
    // Wait for the worker to finish (it ends after its last pass), so a stray third pass would show up below.
    poll(
        || "the worker to finish".to_string(),
        30.0,
        || async { activity.is_idle() && calls.get().len() >= 2 },
    )
    .await;
    assert_eq!(calls.get(), vec![ids(&["a"]), ids(&["a", "b"])]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn unknown_calendar_scope_requests_full_refresh() {
    let calls: Calls = Arc::new(LockedBox::new(Vec::new()));
    let activity = Arc::new(EngineActivity::new());
    let refresh_calls = calls.clone();
    let coordinator = SourceRefreshCoordinator::new(
        Some(activity.clone()),
        Arc::new(move |batch| -> BoxFuture<()> {
            let calls = refresh_calls.clone();
            Box::pin(async move { calls.mutate(|calls| calls.push(batch)) })
        }),
    );
    coordinator.signal(events_changed("a", None)).await;
    poll(
        || "the worker to finish".to_string(),
        30.0,
        || async { activity.is_idle() && !calls.get().is_empty() },
    )
    .await;
    let all = calls.get();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0], None);
}
