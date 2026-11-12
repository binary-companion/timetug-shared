//! Port of `EngineTakeoverTests.swift`: takeover scheduling, the fire guard, snooze, join, dismiss and the test tug.
//! `ManualClock` drives every timer.

use calendar_core::model::{
    ConferenceInfo, ConferenceOrigin, ConferenceProvider, Participation, ResponseStatus,
};
use calendar_core::time::add_seconds;
use calendar_core::url_string::UrlString;
use timetug_core::takeover::ledger::TakeoverLedger;
use timetug_core::takeover::request::TakeoverRequest;
use timetug_core::{SourceStatus, TimeTugCalendarEvent};

use super::support::engine_harness::{EngineHarness, sample, sample_for};
use crate::clock::EngineClock;
use crate::command::EngineCommand;
use crate::diagnostic::ignore_diagnostics;
use crate::event::EngineEvent;
use crate::persistence::ledger_store::LedgerStore;

// MARK: Helpers

/// Starts the engine and waits until the first refresh has published the source as healthy.
async fn started(h: &EngineHarness) {
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| s.statuses.get("fake-c1") == Some(&SourceStatus::Ok))
        .await;
}

fn event(h: &EngineHarness, source_event_id: &str) -> Option<TimeTugCalendarEvent> {
    h.engine
        .current_state()
        .events
        .into_iter()
        .find(|e| e.source_event_id() == source_event_id)
}

fn requests(h: &EngineHarness) -> Vec<TakeoverRequest> {
    h.emitted.mutate(|list| {
        list.iter()
            .filter_map(|event| match event {
                EngineEvent::TakeoverDue { request, .. } => Some(request.clone()),
                _ => None,
            })
            .collect()
    })
}

fn withdrawn_ids(h: &EngineHarness) -> Vec<String> {
    h.emitted.mutate(|list| {
        list.iter()
            .filter_map(|event| match event {
                EngineEvent::TakeoverWithdrawn { event_id } => Some(event_id.clone()),
                _ => None,
            })
            .collect()
    })
}

fn join_urls(h: &EngineHarness) -> Vec<UrlString> {
    h.emitted.mutate(|list| {
        list.iter()
            .filter_map(|event| match event {
                EngineEvent::JoinRequested(url) => Some(url.clone()),
                _ => None,
            })
            .collect()
    })
}

fn diagnostic_messages(h: &EngineHarness) -> Vec<String> {
    h.diagnostics
        .mutate(|list| list.iter().map(|d| d.message.clone()).collect())
}

/// Diagnostics never carry calendar content: every sample title starts with "Meeting ".
fn expect_no_titles_in_diagnostics(h: &EngineHarness) {
    assert!(
        !diagnostic_messages(h)
            .iter()
            .any(|message| message.contains("Meeting ")),
        "a diagnostic carries a title: {:?}",
        diagnostic_messages(h)
    );
}

fn persisted_ledger(h: &EngineHarness) -> TakeoverLedger {
    LedgerStore::new(&h.directory, ignore_diagnostics()).load()
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

fn meet_link() -> ConferenceInfo {
    ConferenceInfo::new(
        UrlString::parse("https://meet.google.com/abc-defg-hij").unwrap(),
        ConferenceProvider::Meet,
        ConferenceOrigin::Structured,
    )
}

// MARK: 1. Lead time

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn fire_at_lead_time_emits_once_and_persists_the_ledger() {
    let h = EngineHarness::new(vec![sample("e1", 600.0)]).await;
    started(&h).await;
    let armed = h.engine.current_state().next_takeover;
    assert_eq!(
        armed
            .as_ref()
            .map(|a| a.event.source_event_id().to_string()),
        Some("e1".to_string())
    );
    assert_eq!(
        armed.map(|a| a.fire_at),
        Some(add_seconds(EngineHarness::epoch(), 540.0))
    );

    h.clock.advance(539.0).await;
    h.settle().await;
    assert!(h.takeover_due_ids().is_empty());

    h.clock.advance(1.0).await;
    h.wait_until("the takeover", || h.takeover_due_ids() == ids(&["e1"]))
        .await;
    h.settle().await;
    assert_eq!(h.takeover_due_ids(), ids(&["e1"]));
    let e1 = event(&h, "e1").unwrap();
    let state = h.engine.current_state();
    assert!(state.ledger.has_fired(&e1));
    assert!(state.next_takeover.is_none());
    assert!(persisted_ledger(&h).has_fired(&e1));
    assert!(!requests(&h)[0].has_started);
    assert!(
        diagnostic_messages(&h)
            .iter()
            .any(|m| m.contains("lead time"))
    );
    expect_no_titles_in_diagnostics(&h);
}

// MARK: 2. Re-check before firing

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn event_edited_out_before_the_timer_never_takes_over() {
    let h = EngineHarness::new(vec![sample("e1", 600.0)]).await;
    started(&h).await;
    let e1 = event(&h, "e1").unwrap();
    h.c1().set_events(Vec::new());
    h.engine.send(EngineCommand::Refresh).await;
    h.wait_for_state(|s| s.events.is_empty()).await;
    h.wait_until("the withdrawal", || withdrawn_ids(&h) == vec![e1.id()])
        .await;
    h.clock.advance(600.0).await;
    h.settle().await;
    assert!(h.takeover_due_ids().is_empty());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn declined_before_the_timer_never_takes_over() {
    let h = EngineHarness::new(vec![sample("e1", 600.0)]).await;
    started(&h).await;
    let mut declined = sample("e1", 600.0);
    declined.participation = Some(Participation::Invited(ResponseStatus::Declined));
    h.c1().set_events(vec![declined]);
    h.engine.send(EngineCommand::Refresh).await;
    h.wait_for_state(|s| {
        s.events.first().map(|e| e.response_status) == Some(Some(ResponseStatus::Declined))
    })
    .await;
    h.clock.advance(600.0).await;
    h.settle().await;
    assert!(h.takeover_due_ids().is_empty());
    assert!(h.engine.current_state().next_takeover.is_none());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn opted_out_before_the_timer_never_takes_over() {
    let h = EngineHarness::new(vec![sample("e1", 600.0)]).await;
    started(&h).await;
    h.engine
        .send(EngineCommand::ApplySettings(Box::new(|s| {
            s.takeover.takeover_calendar_keys.clear();
        })))
        .await;
    assert!(h.engine.current_state().next_takeover.is_none());
    h.clock.advance(600.0).await;
    h.settle().await;
    assert!(h.takeover_due_ids().is_empty());
}

// MARK: 3. Review Focus 3: a second takeover waits while one is visible

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn second_takeover_waits_while_one_is_visible_and_fires_on_close() {
    let h = EngineHarness::new(vec![sample("a", 600.0), sample("b", 600.0)]).await;
    started(&h).await;
    h.clock.advance(540.0).await;
    h.wait_until("the first takeover", || h.takeover_due_ids().len() == 1)
        .await;
    let first = requests(&h)[0].event.clone();
    h.engine
        .send(EngineCommand::TakeoverPresented {
            event_id: first.id(),
            on_screen: true,
        })
        .await;
    h.clock.advance(30.0).await;
    h.settle().await;

    let second_id = if first.source_event_id() == "a" {
        "b"
    } else {
        "a"
    };
    let second = event(&h, second_id).unwrap();
    let mut state = h.engine.current_state();
    assert_eq!(h.takeover_due_ids().len(), 1);
    assert!(state.takeover_visible);
    assert!(state.next_takeover.is_none());
    assert!(!state.ledger.has_fired(&second));
    assert!(!persisted_ledger(&h).has_fired(&second));

    h.engine
        .send(EngineCommand::TakeoverClosed {
            event_id: first.id(),
        })
        .await;
    h.wait_until("the second takeover", || h.takeover_due_ids().len() == 2)
        .await;
    h.settle().await;
    assert_eq!(
        h.takeover_due_ids(),
        vec![first.source_event_id().to_string(), second_id.to_string()]
    );
    state = h.engine.current_state();
    assert!(state.ledger.has_fired(&second));
    assert!(!state.takeover_visible);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_toast_fallback_also_holds_the_second_takeover() {
    let h = EngineHarness::new(vec![sample("a", 600.0), sample("b", 900.0)]).await;
    started(&h).await;
    h.clock.advance(540.0).await;
    h.wait_until("the first takeover", || h.takeover_due_ids() == ids(&["a"]))
        .await;
    let a = event(&h, "a").unwrap();
    h.engine
        .send(EngineCommand::TakeoverPresented {
            event_id: a.id(),
            on_screen: false,
        })
        .await;
    h.clock.advance(300.0).await;
    h.settle().await;
    assert_eq!(h.takeover_due_ids(), ids(&["a"]));
    assert!(h.engine.current_state().takeover_visible);
    h.engine
        .send(EngineCommand::Dismiss { event_id: a.id() })
        .await;
    h.wait_until("the second takeover", || {
        h.takeover_due_ids() == ids(&["a", "b"])
    })
    .await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_close_for_another_event_does_not_release_the_guard() {
    let h = EngineHarness::new(vec![sample("a", 600.0), sample("b", 600.0)]).await;
    started(&h).await;
    h.clock.advance(540.0).await;
    h.wait_until("the first takeover", || h.takeover_due_ids().len() == 1)
        .await;
    let first = requests(&h)[0].event.clone();
    h.engine
        .send(EngineCommand::TakeoverPresented {
            event_id: first.id(),
            on_screen: true,
        })
        .await;
    h.engine
        .send(EngineCommand::TakeoverClosed {
            event_id: "stale/other/0".to_string(),
        })
        .await;
    h.settle().await;
    assert_eq!(h.takeover_due_ids().len(), 1);
    assert!(h.engine.current_state().takeover_visible);
}

// MARK: 4. Review Focus 4: wake after sleeping past a meeting's start

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn wake_three_hours_late_presents_once_and_never_for_an_ended_event() {
    let h = EngineHarness::new(vec![
        sample_for("ended", 600.0, 1800.0),
        sample_for("long", 600.0, 5.0 * 3600.0),
    ])
    .await;
    started(&h).await;
    h.clock.advance(3.0 * 3600.0).await;
    h.wait_until("the late takeover", || {
        h.takeover_due_ids() == ids(&["long"])
    })
    .await;
    // The host's wake handling refreshes; doing it twice must not present again.
    h.engine.send(EngineCommand::Refresh).await;
    h.engine.send(EngineCommand::Refresh).await;
    h.clock.advance(0.0).await;
    h.settle().await;
    assert_eq!(h.takeover_due_ids(), ids(&["long"]));
    assert!(requests(&h)[0].has_started);
    assert!(
        diagnostic_messages(&h)
            .iter()
            .any(|m| m.contains("late after wake"))
    );
    expect_no_titles_in_diagnostics(&h);
}

// MARK: 5. Snooze

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn snooze_rearms_and_fires_again_when_it_expires() {
    let h = EngineHarness::new(vec![sample_for("e1", 600.0, 400.0)]).await;
    started(&h).await;
    h.clock.advance(540.0).await;
    h.wait_until("the takeover", || h.takeover_due_ids() == ids(&["e1"]))
        .await;
    let request = requests(&h)[0].clone();
    // The meeting ends at +1000; at +540 a 600-second snooze would outlast it.
    assert_eq!(request.snooze_options, vec![60.0, 300.0]);
    h.engine
        .send(EngineCommand::TakeoverPresented {
            event_id: request.event.id(),
            on_screen: true,
        })
        .await;

    h.engine
        .send(EngineCommand::Snooze {
            event_id: request.event.id(),
            seconds: 300.0,
        })
        .await;
    let e1 = event(&h, "e1").unwrap();
    let mut state = h.engine.current_state();
    assert!(state.ledger.is_snoozed(&e1));
    assert!(persisted_ledger(&h).is_snoozed(&e1));
    assert!(!state.takeover_visible);
    assert_eq!(
        state.next_takeover.as_ref().map(|n| n.fire_at),
        Some(add_seconds(EngineHarness::epoch(), 840.0))
    );

    h.clock.advance(299.0).await;
    h.settle().await;
    assert_eq!(h.takeover_due_ids(), ids(&["e1"]));
    h.clock.advance(1.0).await;
    h.wait_until("the snoozed takeover", || {
        h.takeover_due_ids() == ids(&["e1", "e1"])
    })
    .await;
    state = h.engine.current_state();
    assert!(state.ledger.has_fired(&e1));
    assert!(
        diagnostic_messages(&h)
            .iter()
            .any(|m| m.contains("snooze expired"))
    );
}

// MARK: 6. Join and dismiss

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn join_requests_the_url_and_closes() {
    let mut meeting = sample("e1", 600.0);
    let url = UrlString::parse("https://meet.google.com/abc-defg-hij").unwrap();
    meeting.conferences = vec![meet_link()];
    let h = EngineHarness::new(vec![meeting, sample("later", 3600.0)]).await;
    started(&h).await;
    h.clock.advance(540.0).await;
    h.wait_until("the takeover", || h.takeover_due_ids() == ids(&["e1"]))
        .await;
    let request = requests(&h)[0].clone();
    assert_eq!(request.join_url, Some(url.clone()));
    h.engine
        .send(EngineCommand::TakeoverPresented {
            event_id: request.event.id(),
            on_screen: true,
        })
        .await;
    h.engine
        .send(EngineCommand::Join {
            event_id: request.event.id(),
        })
        .await;
    h.wait_until("the join request", || join_urls(&h) == vec![url.clone()])
        .await;
    let state = h.engine.current_state();
    assert!(!state.takeover_visible);
    assert_eq!(
        state
            .next_takeover
            .as_ref()
            .map(|n| n.event.source_event_id().to_string()),
        Some("later".to_string())
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn dismiss_closes_without_joining() {
    let mut meeting = sample("e1", 600.0);
    meeting.conferences = vec![meet_link()];
    let h = EngineHarness::new(vec![meeting, sample("later", 3600.0)]).await;
    started(&h).await;
    h.clock.advance(540.0).await;
    h.wait_until("the takeover", || h.takeover_due_ids() == ids(&["e1"]))
        .await;
    let id = requests(&h)[0].event.id();
    h.engine
        .send(EngineCommand::TakeoverPresented {
            event_id: id.clone(),
            on_screen: true,
        })
        .await;
    h.engine.send(EngineCommand::Dismiss { event_id: id }).await;
    h.settle().await;
    assert!(join_urls(&h).is_empty());
    let state = h.engine.current_state();
    assert!(!state.takeover_visible);
    assert_eq!(
        state
            .next_takeover
            .as_ref()
            .map(|n| n.event.source_event_id().to_string()),
        Some("later".to_string())
    );
}

// MARK: 7. Test tug

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn test_tug_emits_the_sample_and_leaves_the_ledger_untouched() {
    let h = EngineHarness::new(vec![sample("e1", 600.0)]).await;
    started(&h).await;
    let file = h.directory.join(LedgerStore::FILE_NAME);
    let before = std::fs::read(&file).unwrap();
    let ledger_before = h.engine.current_state().ledger;

    let test_tug = || EngineCommand::TestTug {
        title: "Host sample".to_string(),
        calendar_title: "Host calendar".to_string(),
    };
    h.engine.send(test_tug()).await;
    h.wait_until("the test takeover", || requests(&h).len() == 1)
        .await;
    let request = requests(&h)[0].clone();
    // The host supplies every display string; the engine adds none.
    assert_eq!(request.event.title(), "Host sample");
    let calendar_titles: Vec<String> = h.emitted.mutate(|list| {
        list.iter()
            .filter_map(|event| match event {
                EngineEvent::TakeoverDue { calendar_title, .. } => calendar_title.clone(),
                _ => None,
            })
            .collect()
    });
    assert_eq!(calendar_titles, vec!["Host calendar".to_string()]);
    assert_eq!(request.event.source_event_id(), "test");
    assert_eq!(request.event.calendar_id(), "test");
    assert_eq!(request.event.source_id, "test");
    assert_eq!(request.event.start(), add_seconds(h.clock.now(), 60.0));
    h.engine
        .send(EngineCommand::TakeoverPresented {
            event_id: request.event.id(),
            on_screen: true,
        })
        .await;
    h.engine
        .send(EngineCommand::Snooze {
            event_id: request.event.id(),
            seconds: 300.0,
        })
        .await;
    h.engine.send(test_tug()).await;
    h.wait_until("the second test takeover", || requests(&h).len() == 2)
        .await;
    h.engine
        .send(EngineCommand::Join {
            event_id: request.event.id(),
        })
        .await;
    h.settle().await;

    assert!(join_urls(&h).is_empty());
    assert_eq!(std::fs::read(&file).unwrap(), before);
    assert_eq!(h.engine.current_state().ledger, ledger_before);
    assert_eq!(
        h.engine
            .current_state()
            .next_takeover
            .map(|n| n.event.source_event_id().to_string()),
        Some("e1".to_string())
    );
}

// MARK: 8. Settings change

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn lead_time_zero_rearms_at_the_start() {
    let h = EngineHarness::new(vec![sample("e1", 600.0)]).await;
    started(&h).await;
    h.engine
        .send(EngineCommand::ApplySettings(Box::new(|s| {
            s.takeover.lead_time = 0.0;
        })))
        .await;
    let state = h.engine.current_state();
    assert_eq!(
        state.next_takeover.map(|n| n.fire_at),
        Some(add_seconds(EngineHarness::epoch(), 600.0))
    );
    assert!(withdrawn_ids(&h).is_empty());
    h.clock.advance(540.0).await;
    h.settle().await;
    assert!(h.takeover_due_ids().is_empty());
    h.clock.advance(60.0).await;
    h.wait_until("the takeover at the start", || {
        h.takeover_due_ids() == ids(&["e1"])
    })
    .await;
    assert!(requests(&h)[0].has_started);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn disabling_takeovers_withdraws_the_pending_one() {
    let h = EngineHarness::new(vec![sample("e1", 600.0)]).await;
    started(&h).await;
    let e1 = event(&h, "e1").unwrap();
    h.engine
        .send(EngineCommand::ApplySettings(Box::new(|s| {
            s.takeover.enabled = false;
        })))
        .await;
    h.wait_until("the withdrawal", || withdrawn_ids(&h) == vec![e1.id()])
        .await;
    assert!(h.engine.current_state().next_takeover.is_none());
    h.clock.advance(600.0).await;
    h.settle().await;
    assert!(h.takeover_due_ids().is_empty());
}

// MARK: Review fixes

/// F1: a presented report for an event that is not outstanding (a late duplicate after close) is ignored.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_duplicate_presented_after_close_does_not_block_the_next_takeover() {
    let h = EngineHarness::new(vec![sample("a", 600.0), sample("b", 900.0)]).await;
    started(&h).await;
    h.clock.advance(540.0).await;
    h.wait_until("the first takeover", || h.takeover_due_ids() == ids(&["a"]))
        .await;
    let a = event(&h, "a").unwrap();
    let presented = || EngineCommand::TakeoverPresented {
        event_id: a.id(),
        on_screen: true,
    };
    h.engine.send(presented()).await;
    h.engine
        .send(EngineCommand::TakeoverClosed { event_id: a.id() })
        .await;
    h.engine.send(presented()).await;
    assert!(!h.engine.current_state().takeover_visible);
    h.clock.advance(300.0).await;
    h.wait_until("the second takeover", || {
        h.takeover_due_ids() == ids(&["a", "b"])
    })
    .await;
}

/// F2: a fire held back by an outstanding takeover leaves no stale `nextTakeover` behind.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_fire_held_by_the_guard_clears_next_takeover() {
    let h = EngineHarness::new(vec![sample("e1", 600.0)]).await;
    started(&h).await;
    h.engine
        .send(EngineCommand::TestTug {
            title: "Host sample".to_string(),
            calendar_title: "Host calendar".to_string(),
        })
        .await;
    assert_eq!(
        h.engine
            .current_state()
            .next_takeover
            .map(|n| n.event.source_event_id().to_string()),
        Some("e1".to_string())
    );
    h.clock.advance(540.0).await;
    h.wait_until("the held fire", || {
        h.diagnostics.mutate(|list| {
            list.iter()
                .any(|d| d.message == "takeover suppressed: overlayVisible")
        })
    })
    .await;
    assert!(h.engine.current_state().next_takeover.is_none());
    assert_eq!(h.takeover_due_ids(), ids(&["test"]));
    // Closing the test tug re-arms, and the held meeting fires at once.
    let test_id = requests(&h)[0].event.id();
    h.engine
        .send(EngineCommand::Dismiss { event_id: test_id })
        .await;
    h.wait_until("the held takeover", || {
        h.takeover_due_ids() == ids(&["test", "e1"])
    })
    .await;
}

/// F3: the armed meeting disappearing while a takeover is outstanding is still withdrawn, exactly once.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn withdrawal_is_reported_while_a_takeover_is_outstanding() {
    let h = EngineHarness::new(vec![sample("e1", 600.0)]).await;
    started(&h).await;
    let e1 = event(&h, "e1").unwrap();
    h.engine
        .send(EngineCommand::TestTug {
            title: "Host sample".to_string(),
            calendar_title: "Host calendar".to_string(),
        })
        .await;
    h.c1().set_events(Vec::new());
    h.engine.send(EngineCommand::Refresh).await;
    h.wait_until("the withdrawal", || withdrawn_ids(&h) == vec![e1.id()])
        .await;
    h.engine.send(EngineCommand::Refresh).await;
    h.settle().await;
    assert_eq!(withdrawn_ids(&h), vec![e1.id()]);
}

// MARK: Stop

/// A refresh still in flight when `.stop` arrives must not arm, fire or persist anything afterwards: a fired entry
/// written then would suppress, after a relaunch, a takeover the user never saw.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_refresh_finishing_after_stop_arms_and_persists_nothing() {
    let h = EngineHarness::new(vec![sample("e1", 600.0)]).await;
    started(&h).await;
    let event = h.engine.current_state().events.into_iter().next().unwrap();
    let source = h.c1().clone();
    source.set_gated(true);
    let engine = h.engine.clone();
    let refresh = tokio::spawn(async move { engine.send(EngineCommand::Refresh).await });
    h.wait_until("the refresh at the gate", || source.gate.waiting() == 1)
        .await;

    h.engine.send(EngineCommand::Stop).await;
    source.set_gated(false);
    source.gate.release_all();
    refresh.await.unwrap();
    h.settle().await;
    // Nothing is armed: no takeover, tick or periodic sleep is pending.
    assert_eq!(h.clock.pending_sleeps(), 0);

    h.clock.advance(600.0).await;
    h.settle().await;
    assert!(h.takeover_due_ids().is_empty());
    assert!(!persisted_ledger(&h).has_fired(&event));
}
