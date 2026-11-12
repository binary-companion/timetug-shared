//! Rust-only: Review Focus 5 of the phase 3 plan, at the engine. The machine sleeps for hours, or the clock or the zone
//! changes, while a takeover is pending: the takeover fires late once, or is withdrawn, as the conformance scenarios
//! `late-takeover-after-wake`, `takeover-after-clock` and `time-zone-change` say. (`engine_clock_tests.rs` holds the
//! `SystemClock` half: a paused `tokio` clock that advances 26 hours in one step.)

use calendar_core::time::Zone;
use timetug_core::SourceStatus;

use super::support::engine_harness::{EngineHarness, sample, sample_for};
use crate::command::{EngineCommand, SystemEvent};
use crate::event::EngineEvent;

async fn started(h: &EngineHarness) {
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| s.statuses.get("fake-c1") == Some(&SourceStatus::Ok))
        .await;
    h.settle().await;
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

fn withdrawn(h: &EngineHarness) -> Vec<String> {
    h.emitted.mutate(|list| {
        list.iter()
            .filter_map(|event| match event {
                EngineEvent::TakeoverWithdrawn { event_id } => Some(event_id.clone()),
                _ => None,
            })
            .collect()
    })
}

fn messages(h: &EngineHarness) -> Vec<String> {
    h.diagnostics
        .mutate(|list| list.iter().map(|d| d.message.clone()).collect())
}

/// Scenario `late-takeover-after-wake`: 26 hours pass in one step, the host sends `wake`; the meeting is still underway.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_26_hour_sleep_fires_a_pending_takeover_late_exactly_once() {
    let h = EngineHarness::new(vec![sample_for("long", 600.0, 30.0 * 3600.0)]).await;
    started(&h).await;
    assert_eq!(
        h.engine
            .current_state()
            .next_takeover
            .map(|n| n.event.source_event_id().to_string()),
        Some("long".to_string())
    );

    h.clock.advance(26.0 * 3600.0).await;
    h.engine
        .send(EngineCommand::System(SystemEvent::Wake))
        .await;
    h.wait_until("the late takeover", || {
        h.takeover_due_ids() == ids(&["long"])
    })
    .await;
    // The host's wake, clock and refresh signals arrive in any number; none presents again.
    h.engine
        .send(EngineCommand::System(SystemEvent::ClockChanged))
        .await;
    h.engine
        .send(EngineCommand::System(SystemEvent::Wake))
        .await;
    h.engine.send(EngineCommand::Refresh).await;
    h.settle().await;
    assert_eq!(h.takeover_due_ids(), ids(&["long"]));
    assert!(messages(&h).iter().any(|m| m.contains("late after wake")));
    assert!(!messages(&h).iter().any(|m| m.contains("Meeting ")));
    assert!(withdrawn(&h).is_empty());
}

/// A meeting that ended during the sleep is never presented and is not reported as withdrawn either (nothing was shown).
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_26_hour_sleep_never_presents_a_meeting_that_ended_meanwhile() {
    let h = EngineHarness::new(vec![sample("short", 600.0)]).await;
    started(&h).await;
    h.clock.advance(26.0 * 3600.0).await;
    h.engine
        .send(EngineCommand::System(SystemEvent::Wake))
        .await;
    h.settle().await;
    assert!(h.takeover_due_ids().is_empty());
    assert!(h.engine.current_state().next_takeover.is_none());
    assert_eq!(h.engine.current_state().ledger.key_count(), 0);
}

/// Scenario `takeover-after-clock`: the clock jumps to the lead time with no system event; the armed sleep fires on its own.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_clock_jump_to_the_lead_time_fires_without_any_system_event() {
    let h = EngineHarness::new(vec![sample("e1", 960.0)]).await;
    started(&h).await;
    h.clock.advance(900.0).await;
    h.wait_until("the takeover", || h.takeover_due_ids() == ids(&["e1"]))
        .await;
    h.settle().await;
    assert_eq!(h.takeover_due_ids(), ids(&["e1"]));
    assert!(h.engine.current_state().next_takeover.is_none());
}

/// Scenario `time-zone-change`: Tokyo moves today's window so the evening meeting is no longer in it; the pending takeover
/// is withdrawn once, and returns when the zone does.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_time_zone_change_withdraws_the_pending_takeover_once_and_restores_it() {
    let h = EngineHarness::new(vec![sample("late-call", 4.0 * 3600.0)]).await;
    started(&h).await;
    let armed = h.engine.current_state().next_takeover.unwrap();
    assert_eq!(armed.event.source_event_id(), "late-call");
    let event_id = armed.event.id();

    h.host_zone.set(Zone::named("Asia/Tokyo").unwrap());
    h.engine
        .send(EngineCommand::System(SystemEvent::TimeZoneChanged))
        .await;
    h.settle().await;
    assert_eq!(withdrawn(&h), vec![event_id.clone()]);
    let state = h.engine.current_state();
    assert!(state.events.is_empty());
    assert!(state.next_takeover.is_none());

    // Another refresh in the same zone does not withdraw again.
    h.engine
        .send(EngineCommand::System(SystemEvent::NetworkReturned))
        .await;
    h.settle().await;
    assert_eq!(withdrawn(&h), vec![event_id.clone()]);

    h.host_zone.set(Zone::utc());
    h.engine
        .send(EngineCommand::System(SystemEvent::TimeZoneChanged))
        .await;
    h.settle().await;
    let state = h.engine.current_state();
    assert_eq!(
        state
            .next_takeover
            .map(|n| n.event.source_event_id().to_string()),
        Some("late-call".to_string())
    );
    assert_eq!(withdrawn(&h), vec![event_id]);
    assert!(h.takeover_due_ids().is_empty());
}
