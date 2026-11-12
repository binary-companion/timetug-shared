//! Port of `ConformanceTests.swift`: scripted timelines run through one engine each. Every test asserts the exact
//! ordered list of `takeoverDue` and `takeoverWithdrawn` events (ids, clock times, the late flag), that a widget snapshot
//! was generated at every refresh time, what the last widget snapshot holds, and the final `EngineState`.
//!
//! Determinism: the manual clock moves only when a test advances it, and each advance wakes at most one sleeper that
//! emits takeover events (the periodic refresh at +900 s steps, or one takeover). How many widget snapshots one
//! refresh produces depends on how its sources' answers interleave with the engine (one per partial publication), so
//! their count is never asserted.

use std::collections::BTreeSet;
use std::sync::Arc;

use calendar_core::source_types::SourceError;
use calendar_core::time::add_seconds;
use timetug_core::agenda::day_agenda::State as AgendaState;
use timetug_core::{InferenceStatus, SourceStatus, TimeTugCalendarEvent};

use super::support::engine_harness::{
    EngineHarness, FakeEngineGenerator, HarnessOptions, sample, sample_for,
};
use super::support::timeline_recorder::{Entry, TimelineRecorder};
use crate::command::{EngineCommand, SystemEvent};
use crate::inference::text_generator::{TextGenerator, TextGeneratorAvailability};
use crate::settings::SettingsStorage;
use crate::state::EngineState;

async fn started(h: &EngineHarness, accounts: usize) {
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| {
        s.statuses.len() == accounts
            && s.statuses
                .values()
                .all(|status| *status == SourceStatus::Ok)
    })
    .await;
}

fn event(h: &EngineHarness, source_event_id: &str) -> TimeTugCalendarEvent {
    h.engine
        .current_state()
        .events
        .into_iter()
        .find(|e| e.source_event_id() == source_event_id)
        .unwrap_or_else(|| panic!("no event {source_event_id}"))
}

fn agenda(state: &EngineState, item_state: AgendaState) -> Vec<String> {
    state
        .agenda
        .items
        .iter()
        .filter(|item| item.state == item_state)
        .map(|item| item.event.source_event_id().to_string())
        .collect()
}

/// Advances the clock and waits for the refresh it triggers (a widget snapshot at the new time).
async fn advance_to_refresh(h: &EngineHarness, r: &TimelineRecorder, by: f64, refresh_at: i64) {
    h.clock.advance(by).await;
    r.wait_for_widget(refresh_at, h).await;
}

/// Advances the clock and waits until the timeline holds `count` takeover entries.
async fn advance_to_takeovers(h: &EngineHarness, r: &TimelineRecorder, by: f64, count: usize) {
    h.clock.advance(by).await;
    r.wait_for_takeovers(count, h, true).await;
}

/// A widget snapshot was generated at each of `times` (seconds after the start); others may exist too.
fn expect_widgets(r: &TimelineRecorder, times: &[i64]) {
    let expected: BTreeSet<i64> = times.iter().copied().collect();
    assert!(
        expected.is_subset(&r.widget_times()),
        "widgets at {:?}",
        r.widget_times()
    );
}

/// Waits until the last widget snapshot's titles are `titles`.
async fn expect_last_widget(h: &EngineHarness, r: &TimelineRecorder, titles: &[&str]) {
    let titles: Vec<String> = titles.iter().map(|t| t.to_string()).collect();
    h.wait_until(
        &format!("the last widget snapshot to show {titles:?}"),
        || {
            r.widgets().last().map(|w| {
                w.events
                    .iter()
                    .map(|e| e.title.clone())
                    .collect::<Vec<String>>()
            }) == Some(titles.clone())
        },
    )
    .await;
}

fn two(
    a: Vec<calendar_core::model::CalendarEvent>,
    b: Vec<calendar_core::model::CalendarEvent>,
) -> std::collections::BTreeMap<String, Vec<calendar_core::model::CalendarEvent>> {
    [("c1".to_string(), a), ("c2".to_string(), b)]
        .into_iter()
        .collect()
}

// MARK: Two accounts, overlapping meetings

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn two_accounts_with_overlapping_meetings() {
    let h = EngineHarness::with(HarnessOptions {
        accounts: two(vec![sample("a", 600.0)], vec![sample("b", 1200.0)]),
        ..HarnessOptions::default()
    })
    .await;
    let r = TimelineRecorder::new(&h);
    started(&h, 2).await;
    r.wait_for_widget(0, &h).await;

    // a fires at its lead time; the host shows it.
    advance_to_takeovers(&h, &r, 540.0, 1).await;
    let a = event(&h, "a");
    h.engine
        .send(EngineCommand::TakeoverPresented {
            event_id: a.id(),
            on_screen: true,
        })
        .await;

    // The periodic refresh at +900 republishes; a is still on screen.
    advance_to_refresh(&h, &r, 360.0, 900).await;

    // b's lead time passes while a is on screen: b is held, not fired.
    h.clock.advance(240.0).await;
    h.settle().await;
    assert_eq!(r.takeovers().len(), 1);
    assert!(h.engine.current_state().next_takeover.is_none());

    // Closing a releases b at once; the host snoozes it for 5 minutes, then it fires again.
    h.engine
        .send(EngineCommand::TakeoverClosed { event_id: a.id() })
        .await;
    r.wait_for_takeovers(2, &h, true).await;
    let b = event(&h, "b");
    h.engine
        .send(EngineCommand::Snooze {
            event_id: b.id(),
            seconds: 300.0,
        })
        .await;
    advance_to_takeovers(&h, &r, 300.0, 3).await;
    h.engine
        .send(EngineCommand::Dismiss { event_id: b.id() })
        .await;

    // The next periodic refresh, at +1800, while both meetings are underway.
    advance_to_refresh(&h, &r, 360.0, 1800).await;

    assert_eq!(
        r.takeovers(),
        vec![
            Entry::due("a", 540.0),
            Entry::due("b", 1140.0),
            Entry::due_late("b", 1440.0),
        ]
    );
    expect_widgets(&r, &[0, 900, 1800]);
    expect_last_widget(&h, &r, &["Meeting a", "Meeting b"]).await;
    let state = h.engine.current_state();
    let ids: Vec<&str> = state.events.iter().map(|e| e.source_event_id()).collect();
    assert_eq!(ids, vec!["a", "b"]);
    assert_eq!(
        state.statuses,
        [
            ("fake-c1".to_string(), SourceStatus::Ok),
            ("fake-c2".to_string(), SourceStatus::Ok)
        ]
        .into_iter()
        .collect()
    );
    let mut accounts: Vec<&str> = state
        .accounts
        .iter()
        .map(|a| a.connection_id.as_str())
        .collect();
    accounts.sort();
    assert_eq!(accounts, vec!["c1", "c2"]);
    assert!(state.ledger.has_fired(&a) && state.ledger.has_fired(&b));
    assert!(state.next_takeover.is_none());
    assert!(!state.takeover_visible);
}

// MARK: Wake gap

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_three_hour_wake_gap_presents_the_meeting_underway_once_and_skips_the_ended_one() {
    let h = EngineHarness::new(vec![
        sample_for("long", 1800.0, 5.0 * 3600.0),
        sample("short", 3600.0),
        sample("later", 4.0 * 3600.0),
    ])
    .await;
    let r = TimelineRecorder::new(&h);
    started(&h, 1).await;
    r.wait_for_widget(0, &h).await;

    // The machine sleeps for three hours. Every sleeper wakes at once: the overdue takeover fires first, then the held
    // periodic refresh (gated so its publication cannot overtake the takeover) and the host's wake.
    h.c1().set_gated(true);
    h.clock.advance(3.0 * 3600.0).await;
    r.wait_for_takeovers(1, &h, false).await;
    h.wait_until("the periodic refresh at the gate", || {
        h.c1().gate.waiting() == 1
    })
    .await;
    h.c1().set_gated(false);
    h.c1().gate.release_all();
    r.wait_for_widget(10800, &h).await;
    h.engine
        .send(EngineCommand::System(SystemEvent::Wake))
        .await;
    h.settle().await;

    let long = event(&h, "long");
    h.engine
        .send(EngineCommand::Dismiss {
            event_id: long.id(),
        })
        .await;
    // Periodic refreshes every 900 s from the wake, then "later" at its lead time.
    advance_to_refresh(&h, &r, 900.0, 11700).await;
    advance_to_refresh(&h, &r, 900.0, 12600).await;
    advance_to_refresh(&h, &r, 900.0, 13500).await;
    advance_to_takeovers(&h, &r, 840.0, 2).await;

    assert_eq!(
        r.takeovers(),
        vec![
            Entry::due_late("long", 10800.0),
            Entry::due("later", 14340.0)
        ]
    );
    expect_widgets(&r, &[0, 10800, 11700, 12600, 13500]);
    let state = h.engine.current_state();
    let short = event(&h, "short");
    assert!(!state.ledger.has_fired(&short));
    assert!(state.ledger.has_fired(&long));
    assert!(state.ledger.has_fired(&event(&h, "later")));
    assert!(state.next_takeover.is_none());
    assert_eq!(
        agenda(&state, AgendaState::Current),
        vec!["long".to_string()]
    );
}

// MARK: Account removal mid-day

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn removing_an_account_mid_day_withdraws_its_armed_meeting() {
    let h = EngineHarness::with(HarnessOptions {
        accounts: two(vec![sample("a", 2400.0)], vec![sample("b", 1200.0)]),
        ..HarnessOptions::default()
    })
    .await;
    let r = TimelineRecorder::new(&h);
    started(&h, 2).await;
    r.wait_for_widget(0, &h).await;
    assert_eq!(
        h.engine
            .current_state()
            .next_takeover
            .map(|n| n.event.source_event_id().to_string()),
        Some("b".to_string())
    );

    h.clock.advance(600.0).await;
    h.settle().await;
    h.engine
        .send(EngineCommand::RemoveAccount {
            connection_id: "c2".to_string(),
        })
        .await;
    // The refresh without c2 withdraws b; forgetting c2's calendar selections publishes again.
    r.wait_for_takeovers(1, &h, true).await;
    r.wait_for_widget(600, &h).await;

    advance_to_refresh(&h, &r, 300.0, 900).await;
    advance_to_refresh(&h, &r, 900.0, 1800).await;
    advance_to_takeovers(&h, &r, 540.0, 2).await;

    assert_eq!(
        r.takeovers(),
        vec![Entry::withdrawn("b", 600.0), Entry::due("a", 2340.0)]
    );
    expect_widgets(&r, &[0, 600, 900, 1800]);
    expect_last_widget(&h, &r, &["Meeting a"]).await;
    let state = h.engine.current_state();
    let accounts: Vec<&str> = state
        .accounts
        .iter()
        .map(|a| a.connection_id.as_str())
        .collect();
    assert_eq!(accounts, vec!["c1"]);
    assert_eq!(
        state.statuses,
        [("fake-c1".to_string(), SourceStatus::Ok)]
            .into_iter()
            .collect()
    );
    let ids: Vec<&str> = state.events.iter().map(|e| e.source_event_id()).collect();
    assert_eq!(ids, vec!["a"]);
    let keys: BTreeSet<String> = ["fake-c1/cal".to_string()].into_iter().collect();
    assert_eq!(state.settings.takeover.takeover_calendar_keys, keys);
    assert_eq!(h.settings.load().takeover.takeover_calendar_keys, keys);
    assert!(state.ledger.has_fired(&event(&h, "a")));
}

// MARK: Settings toggles

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn lead_time_change_and_disabling_and_reenabling_takeovers() {
    let h = EngineHarness::new(vec![sample("a", 1200.0)]).await;
    let r = TimelineRecorder::new(&h);
    started(&h, 1).await;
    r.wait_for_widget(0, &h).await;
    assert_eq!(
        h.engine.current_state().next_takeover.map(|n| n.fire_at),
        Some(add_seconds(EngineHarness::epoch(), 1140.0))
    );

    // A longer lead time re-arms the same meeting earlier (no withdrawal).
    h.engine
        .send(EngineCommand::ApplySettings(Box::new(|s| {
            s.takeover.lead_time = 300.0;
        })))
        .await;
    h.settle().await;
    assert!(r.takeovers().is_empty());
    assert_eq!(
        h.engine.current_state().next_takeover.map(|n| n.fire_at),
        Some(add_seconds(EngineHarness::epoch(), 900.0))
    );

    // Disabling withdraws it; nothing fires at +900, only the periodic refresh publishes.
    h.engine
        .send(EngineCommand::ApplySettings(Box::new(|s| {
            s.takeover.enabled = false;
        })))
        .await;
    r.wait_for_takeovers(1, &h, true).await;
    advance_to_refresh(&h, &r, 900.0, 900).await;
    assert_eq!(r.takeovers().len(), 1);

    // Re-enabling inside the lead time fires at once.
    h.clock.advance(60.0).await;
    h.settle().await;
    h.engine
        .send(EngineCommand::ApplySettings(Box::new(|s| {
            s.takeover.enabled = true;
        })))
        .await;
    r.wait_for_takeovers(2, &h, true).await;

    assert_eq!(
        r.takeovers(),
        vec![Entry::withdrawn("a", 0.0), Entry::due("a", 960.0)]
    );
    // Every settings change emits a widget snapshot, as does the refresh at +900.
    expect_widgets(&r, &[0, 900, 960]);
    let state = h.engine.current_state();
    assert_eq!(state.settings.takeover.lead_time, 300.0);
    assert!(state.settings.takeover.enabled);
    assert_eq!(h.settings.load(), state.settings);
    assert_eq!(h.settings.saved().len(), 3);
    assert!(state.ledger.has_fired(&event(&h, "a")));
    assert!(state.next_takeover.is_none());
}

// MARK: Inference becomes available

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn inference_becoming_available_merges_the_look_alikes_and_the_merged_meeting_takes_over_once()
 {
    let generator = FakeEngineGenerator::with(
        TextGeneratorAvailability::TemporarilyUnavailable {
            reason: "low power".to_string(),
        },
        Some("same"),
    );
    let h = EngineHarness::with(HarnessOptions {
        accounts: two(vec![sample("a1", 3600.0)], vec![sample("a2", 3600.0)]),
        text_generator: Some(generator.clone() as Arc<dyn TextGenerator>),
        inference_enabled: true,
        ..HarnessOptions::default()
    })
    .await;
    let r = TimelineRecorder::new(&h);
    started(&h, 2).await;
    r.wait_for_widget(0, &h).await;
    assert_eq!(
        h.engine.current_state().inference_status,
        InferenceStatus::Unavailable {
            reason: "temporary: low power".to_string()
        }
    );

    // The model comes back; the next refresh re-snapshots with it and its verdict merges the pair.
    h.clock.advance(600.0).await;
    h.settle().await;
    generator
        .availability_value
        .set(TextGeneratorAvailability::Available(
            FakeEngineGenerator::on_device(),
        ));
    h.engine.send(EngineCommand::Refresh).await;
    h.wait_for_state(|s| s.events.len() == 1).await;
    h.settle().await;
    assert_eq!(generator.calls.get(), 1);

    advance_to_refresh(&h, &r, 300.0, 900).await;
    advance_to_refresh(&h, &r, 900.0, 1800).await;
    advance_to_refresh(&h, &r, 900.0, 2700).await;
    advance_to_takeovers(&h, &r, 840.0, 1).await;

    let merged = h.engine.current_state().events.into_iter().next().unwrap();
    assert_eq!(
        r.takeovers(),
        vec![Entry::due(merged.source_event_id(), 3540.0)]
    );
    expect_widgets(&r, &[0, 600, 900, 1800, 2700]);
    h.wait_until("a merged last widget snapshot", || {
        r.widgets().last().map(|w| w.events.len()) == Some(1)
    })
    .await;
    let state = h.engine.current_state();
    assert_eq!(state.events.len(), 1);
    assert_eq!(merged.participants().len(), 2);
    assert_eq!(
        state.inference_status,
        InferenceStatus::Active(FakeEngineGenerator::on_device())
    );
    assert!(state.ledger.has_fired(&merged));
    assert_eq!(generator.calls.get(), 1);
    assert!(h.dedup_file().exists());
}

// MARK: A failing source recovers

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_failing_source_recovers_and_its_meeting_takes_over() {
    let h = EngineHarness::with(HarnessOptions {
        accounts: two(vec![sample("a", 1200.0)], vec![sample("b", 600.0)]),
        failures: [(
            "c2".to_string(),
            SourceError::Network("offline".to_string()),
        )]
        .into_iter()
        .collect(),
        ..HarnessOptions::default()
    })
    .await;
    let r = TimelineRecorder::new(&h);
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| {
        s.statuses.get("fake-c1") == Some(&SourceStatus::Ok) && s.statuses.contains_key("fake-c2")
    })
    .await;
    r.wait_for_widget(0, &h).await;
    let mut state = h.engine.current_state();
    let ids: Vec<&str> = state.events.iter().map(|e| e.source_event_id()).collect();
    assert_eq!(ids, vec!["a"]);
    assert_eq!(
        state
            .next_takeover
            .as_ref()
            .map(|n| n.event.source_event_id().to_string()),
        Some("a".to_string())
    );

    h.clock.advance(300.0).await;
    h.settle().await;
    h.source("c2").set_failure(None);
    h.engine
        .send(EngineCommand::System(SystemEvent::NetworkReturned))
        .await;
    r.wait_for_widget(300, &h).await;
    assert_eq!(
        h.engine
            .current_state()
            .next_takeover
            .map(|n| n.event.source_event_id().to_string()),
        Some("b".to_string())
    );

    advance_to_takeovers(&h, &r, 240.0, 1).await;

    assert_eq!(r.takeovers(), vec![Entry::due("b", 540.0)]);
    expect_widgets(&r, &[0, 300]);
    expect_last_widget(&h, &r, &["Meeting b", "Meeting a"]).await;
    state = h.engine.current_state();
    assert_eq!(
        state.statuses,
        [
            ("fake-c1".to_string(), SourceStatus::Ok),
            ("fake-c2".to_string(), SourceStatus::Ok)
        ]
        .into_iter()
        .collect()
    );
    let ids: Vec<&str> = state.events.iter().map(|e| e.source_event_id()).collect();
    assert_eq!(ids, vec!["b", "a"]);
    assert!(state.ledger.has_fired(&event(&h, "b")));
    assert!(!state.ledger.has_fired(&event(&h, "a")));
    assert!(!state.takeover_visible);
}

// MARK: The day rolls over

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_day_rolls_over_to_tomorrows_meetings() {
    let midnight: f64 = 12.0 * 3600.0;
    let h = EngineHarness::new(vec![
        sample("today", 600.0),
        sample("tomorrow", midnight + 9.0 * 3600.0),
    ])
    .await;
    let r = TimelineRecorder::new(&h);
    started(&h, 1).await;
    r.wait_for_widget(0, &h).await;
    assert_eq!(
        agenda(&h.engine.current_state(), AgendaState::Upcoming),
        vec!["today".to_string()]
    );

    advance_to_takeovers(&h, &r, 540.0, 1).await;
    let today = event(&h, "today");
    h.engine
        .send(EngineCommand::Dismiss {
            event_id: today.id(),
        })
        .await;

    // Past midnight in one step: the overdue periodic refresh, then the host's day change.
    advance_to_refresh(&h, &r, midnight - 540.0, midnight as i64).await;
    h.engine
        .send(EngineCommand::System(SystemEvent::DayChanged))
        .await;
    h.settle().await;

    assert_eq!(r.takeovers(), vec![Entry::due("today", 540.0)]);
    expect_widgets(&r, &[0, midnight as i64]);
    h.wait_until("tomorrow in the last widget snapshot", || {
        r.widgets()
            .last()
            .and_then(|w| w.events.first().map(|e| e.title.clone()))
            == Some("Meeting tomorrow".to_string())
    })
    .await;
    let state = h.engine.current_state();
    let ids: Vec<&str> = state.events.iter().map(|e| e.source_event_id()).collect();
    assert_eq!(ids, vec!["tomorrow"]);
    assert_eq!(
        agenda(&state, AgendaState::Upcoming),
        vec!["tomorrow".to_string()]
    );
    assert_eq!(state.ledger.key_count(), 0);
    assert_eq!(
        state
            .next_takeover
            .as_ref()
            .map(|n| n.event.source_event_id().to_string()),
        Some("tomorrow".to_string())
    );
    assert_eq!(
        state.next_takeover.map(|n| n.fire_at),
        Some(add_seconds(
            EngineHarness::epoch(),
            midnight + 9.0 * 3600.0 - 60.0
        ))
    );
}
