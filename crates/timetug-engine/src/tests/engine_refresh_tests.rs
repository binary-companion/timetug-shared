//! Port of `EngineRefreshTests.swift`.

use std::collections::BTreeSet;
use std::sync::Arc;

use calendar_core::source_types::{CalendarChange, SourceError};
use timetug_core::SourceStatus;

use super::support::LockedBox;
use super::support::engine_harness::{EngineHarness, HarnessOptions, sample};
use crate::command::EngineCommand;
use crate::diagnostic::ignore_diagnostics;
use crate::persistence::ledger_store::LedgerStore;

fn cal() -> CalendarChange {
    CalendarChange::EventsChanged {
        calendar_ids: Some(BTreeSet::from(["cal".to_string()])),
    }
}

fn ok(state: &crate::state::EngineState, source: &str) -> bool {
    state.statuses.get(source) == Some(&SourceStatus::Ok)
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn start_publishes_agenda_and_ok_status() {
    let h = EngineHarness::new(vec![sample("e1", 600.0)]).await;
    h.engine.send(EngineCommand::Start).await;
    let state = h.wait_for_state(|s| ok(s, "fake-c1")).await;
    assert_eq!(state.agenda.items.len(), 1);
    assert_eq!(
        state.agenda.next.as_ref().map(|e| e.source_event_id()),
        Some("e1")
    );
    assert!(state.revision > 0);
    let ids: Vec<&str> = state
        .accounts
        .iter()
        .map(|a| a.connection_id.as_str())
        .collect();
    assert_eq!(ids, vec!["c1"]);
    // A new subscriber receives the current state first. Status and tick publications may still land after `state`
    // was read, so the first value lies between that state and the engine's current one.
    let mut states = h.engine.states();
    let current = h.engine.current_state().revision;
    states.changed().await.unwrap();
    let first = states.borrow_and_update().revision;
    assert!(first >= state.revision && first <= current);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn source_signal_refreshes_only_that_source_and_a_burst_causes_one_follow_up() {
    let h = EngineHarness::with(HarnessOptions {
        accounts: [
            ("c1".to_string(), vec![sample("a", 600.0)]),
            ("c2".to_string(), vec![sample("b", 1200.0)]),
        ]
        .into_iter()
        .collect(),
        ..HarnessOptions::default()
    })
    .await;
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| ok(s, "fake-c1") && ok(s, "fake-c2"))
        .await;
    h.settle().await;
    let (c1, c2) = (h.source("c1"), h.source("c2"));
    let (c1_base, c2_base) = (c1.events_calls(), c2.events_calls());

    c1.signal(cal());
    h.settle().await;
    assert_eq!(c1.events_calls(), c1_base + 1);
    assert_eq!(c2.events_calls(), c2_base);

    // A burst of three signals while a refresh is held at the gate runs exactly one follow-up pass. The test releases
    // the gate only once the coordinator has received all three (counted, not yielded for), so the coalescing is
    // deterministic.
    let received = h.engine.activity().signals_received();
    c1.set_gated(true);
    c1.signal(cal());
    h.wait_until("the refresh to reach the gate", || c1.gate.waiting() == 1)
        .await;
    c1.set_events(vec![sample("a", 600.0), sample("late", 900.0)]);
    for _ in 0..3 {
        c1.signal(cal());
    }
    h.wait_until("the coordinator to receive the burst", || {
        h.engine.activity().signals_received() == received + 4
    })
    .await;
    c1.set_gated(false);
    c1.gate.release_all();
    h.settle().await;
    assert_eq!(c1.events_calls(), c1_base + 3);
    assert_eq!(c2.events_calls(), c2_base);
    // The follow-up read the data written during the burst.
    assert!(
        h.engine
            .current_state()
            .events
            .iter()
            .any(|e| e.source_event_id() == "late")
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn periodic_refresh_fires_every_900_seconds() {
    let h = EngineHarness::new(vec![sample("e1", 3600.0)]).await;
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| ok(s, "fake-c1")).await;
    // The periodic deadline is fixed when it is armed (start, then after each periodic refresh), so once the engine is
    // quiescent the next one is +900 from the last refresh whether or not its sleep has registered yet.
    h.settle().await;
    let base = h.c1().events_calls();
    h.clock.advance(899.0).await;
    h.settle().await;
    assert_eq!(h.c1().events_calls(), base);
    h.clock.advance(1.0).await;
    h.wait_until("the periodic refresh", || h.c1().events_calls() == base + 1)
        .await;
    h.settle().await;
    assert_eq!(h.c1().events_calls(), base + 1);
    h.clock.advance(899.0).await;
    h.settle().await;
    assert_eq!(h.c1().events_calls(), base + 1);
    h.clock.advance(1.0).await;
    h.wait_until("the second periodic refresh", || {
        h.c1().events_calls() == base + 2
    })
    .await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn revisions_increase_and_a_stale_publication_is_dropped() {
    let h = EngineHarness::new(vec![sample("old", 600.0)]).await;
    let observed = Arc::new(LockedBox::new(Vec::<u64>::new()));
    let mut states = h.engine.states();
    let seen = observed.clone();
    let observer = tokio::spawn(async move {
        while states.changed().await.is_ok() {
            let revision = states.borrow_and_update().revision;
            seen.mutate(|list| list.push(revision));
        }
    });
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| ok(s, "fake-c1")).await;
    let source = h.c1().clone();

    // Refresh A reads the old events and is held; refresh B reads the new events and is held too.
    source.set_gated(true);
    let engine = h.engine.clone();
    let refresh_a = tokio::spawn(async move { engine.send(EngineCommand::Refresh).await });
    h.wait_until("refresh A at the gate", || source.gate.waiting() == 1)
        .await;
    source.set_events(vec![sample("new", 900.0)]);
    let engine = h.engine.clone();
    let refresh_b = tokio::spawn(async move { engine.send(EngineCommand::Refresh).await });
    h.wait_until("refresh B at the gate", || source.gate.waiting() == 2)
        .await;

    // B completes first, then A (out of order).
    source.gate.release(1);
    refresh_b.await.unwrap();
    let after_b = h.engine.current_state();
    let ids: Vec<&str> = after_b.events.iter().map(|e| e.source_event_id()).collect();
    assert_eq!(ids, vec!["new"]);
    source.gate.release(0);
    refresh_a.await.unwrap();
    let after_a = h.engine.current_state();
    assert_eq!(after_a.revision, after_b.revision);
    let ids: Vec<&str> = after_a.events.iter().map(|e| e.source_event_id()).collect();
    assert_eq!(ids, vec!["new"]);

    // The channel keeps only the newest state, so a lagging consumer may skip revisions: wait until it has seen the
    // engine's current one, then check that whatever it saw only ever increased.
    let last = h.engine.current_state().revision;
    h.wait_until(&format!("the observer to reach revision {last}"), || {
        observed.get().last() == Some(&last)
    })
    .await;
    observer.abort();
    let revisions = observed.get();
    assert!(!revisions.is_empty());
    assert!(revisions.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(last, after_a.revision);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn launch_acknowledgement_marks_meetings_underway_before_the_grace() {
    let h = EngineHarness::new(vec![sample("early", -600.0), sample("recent", -30.0)]).await;
    h.engine.send(EngineCommand::Start).await;
    let state = h.wait_for_state(|s| ok(s, "fake-c1")).await;
    let early = state
        .events
        .iter()
        .find(|e| e.source_event_id() == "early")
        .unwrap();
    let recent = state
        .events
        .iter()
        .find(|e| e.source_event_id() == "recent")
        .unwrap();
    assert!(state.ledger.has_fired(early));
    // "recent" is inside the grace, so it is not acknowledged: it is presented late instead. That takeover can land
    // before this state is read, so the outcome is checked rather than the intermediate ledger.
    h.wait_until("the late takeover", || {
        h.takeover_due_ids().contains(&"recent".to_string())
    })
    .await;
    h.settle().await;
    assert_eq!(h.takeover_due_ids(), vec!["recent".to_string()]);
    assert!(h.engine.current_state().ledger.has_fired(recent));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn failing_source_keeps_events_and_removed_account_is_not_resurrected() {
    let h = EngineHarness::with(HarnessOptions {
        accounts: [
            ("c1".to_string(), vec![sample("a", 600.0)]),
            ("c2".to_string(), vec![sample("b", 1200.0)]),
        ]
        .into_iter()
        .collect(),
        ..HarnessOptions::default()
    })
    .await;
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| ok(s, "fake-c2")).await;
    let c2 = h.source("c2").clone();

    c2.set_failure(Some(SourceError::Network("offline".to_string())));
    h.engine.send(EngineCommand::Refresh).await;
    let mut state = h.engine.current_state();
    assert!(
        matches!(
            state.statuses.get("fake-c2"),
            Some(SourceStatus::Failing(_))
        ),
        "expected failing, got {:?}",
        state.statuses.get("fake-c2")
    );
    assert_eq!(state.statuses.get("fake-c1"), Some(&SourceStatus::Ok));
    let ids: BTreeSet<&str> = state.events.iter().map(|e| e.source_event_id()).collect();
    assert_eq!(ids, BTreeSet::from(["a", "b"]));

    // Remove c2 while its read is suspended; the read then succeeds but must not bring its events back.
    c2.set_failure(None);
    c2.set_gated(true);
    let engine = h.engine.clone();
    let refresh = tokio::spawn(async move { engine.send(EngineCommand::Refresh).await });
    h.wait_until("the c2 read at the gate", || c2.gate.waiting() == 1)
        .await;
    h.engine
        .send(EngineCommand::RemoveAccount {
            connection_id: "c2".to_string(),
        })
        .await;
    c2.gate.release_all();
    refresh.await.unwrap();
    h.settle().await;
    state = h.engine.current_state();
    let ids: Vec<&str> = state.events.iter().map(|e| e.source_event_id()).collect();
    assert_eq!(ids, vec!["a"]);
    assert_eq!(state.statuses.get("fake-c2"), None);
    assert_eq!(state.statuses.get("fake-c1"), Some(&SourceStatus::Ok));
    let accounts: Vec<&str> = state
        .accounts
        .iter()
        .map(|a| a.connection_id.as_str())
        .collect();
    assert_eq!(accounts, vec!["c1"]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn acknowledged_ledger_is_persisted_and_survives_a_new_engine() {
    let first = EngineHarness::new(vec![sample("early", -600.0)]).await;
    first.engine.send(EngineCommand::Start).await;
    let state = first.wait_for_state(|s| ok(s, "fake-c1")).await;
    let early = state
        .events
        .iter()
        .find(|e| e.source_event_id() == "early")
        .unwrap();
    assert!(
        LedgerStore::new(&first.directory, ignore_diagnostics())
            .load()
            .has_fired(early)
    );
    first.engine.send(EngineCommand::Stop).await;

    // The second engine's source never reports `.ok`, so it cannot acknowledge anything itself.
    let second = EngineHarness::with(HarnessOptions {
        events: vec![sample("early", -600.0)],
        failures: [(
            "c1".to_string(),
            SourceError::Network("offline".to_string()),
        )]
        .into_iter()
        .collect(),
        directory: Some(first.directory.clone()),
        ..HarnessOptions::default()
    })
    .await;
    second.engine.send(EngineCommand::Start).await;
    let reloaded = second
        .wait_for_state(|s| matches!(s.statuses.get("fake-c1"), Some(SourceStatus::Failing(_))))
        .await;
    assert!(reloaded.ledger.has_fired(early));
}
