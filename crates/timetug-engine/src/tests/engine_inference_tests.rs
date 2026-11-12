//! Port of `EngineInferenceTests.swift`.
//!
//! One deviation, recorded in `docs/rust-port/file-map.md`: the diagnostic for a throwing generator is
//! `Judgment failed: <VariantName>` here (Rust has no error type names, see `text_generator_adjudicator.rs`), so
//! `a_throwing_generator_...` asserts `contains("Judgment failed: HostError")` where Swift asserts
//! `contains("Failure")`. The fake generator fails with `Failure::Host`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use calendar_core::model::CalendarEvent;
use timetug_core::{InferenceStatus, SourceStatus, TimeTugCalendarEvent};

use super::support::LockedBox;
use super::support::account_fakes::Gate;
use super::support::engine_harness::{EngineHarness, FakeEngineGenerator, HarnessOptions, sample};
use crate::command::EngineCommand;
use crate::inference::text_generator::{TextGenerator, TextGeneratorAvailability};
use crate::state::EngineState;

/// One meeting per account, all at the same time with different titles: every pair is a look-alike only a model can
/// decide (different calendars, nothing shared, nothing conflicting).
fn look_alikes(count: usize) -> BTreeMap<String, Vec<CalendarEvent>> {
    (1..=count)
        .map(|n| (format!("c{n}"), vec![sample(&format!("a{n}"), 3600.0)]))
        .collect()
}

fn all_ok(state: &EngineState, count: usize) -> bool {
    state.statuses.len() == count && state.statuses.values().all(|s| *s == SourceStatus::Ok)
}

/// Records, for every generate call, whether the dedup file existed when it began. The file is written once, at the
/// end of a resolution round, so the first call that sees it is the first call of the next round.
struct RoundProbe {
    file: PathBuf,
    seen: LockedBox<BTreeMap<usize, bool>>,
}

impl RoundProbe {
    fn new(file: PathBuf) -> Arc<RoundProbe> {
        Arc::new(RoundProbe {
            file,
            seen: LockedBox::new(BTreeMap::new()),
        })
    }

    fn record(&self, call: usize) {
        let exists = self.file.exists();
        self.seen.mutate(|seen| {
            seen.insert(call, exists);
        });
    }

    fn file_at(&self, call: usize) -> Option<bool> {
        self.seen.get().get(&call).copied()
    }

    /// Calls made before the first write, i.e. in the first round that judged anything.
    fn calls_before_first_write(&self) -> Option<usize> {
        let calls = self.seen.get();
        calls
            .iter()
            .find(|(_, exists)| **exists)
            .map(|(call, _)| call - 1)
    }
}

fn hook(generator: &FakeEngineGenerator, probe: &Arc<RoundProbe>) {
    let probe = probe.clone();
    generator
        .on_call
        .set(Some(Arc::new(move |call| probe.record(call))));
}

// Scenario 1 (Review Focus 6)
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn look_alike_pairs_resolve_in_at_most_ten_passes_and_dedup_is_persisted_once_at_the_end() {
    // 21 look-alikes make 210 pairs; a pass judges at most 20, so 10 passes leave 10 pairs for later.
    let generator = FakeEngineGenerator::new(Some("different"));
    let h = EngineHarness::with(HarnessOptions {
        accounts: look_alikes(21),
        text_generator: Some(generator.clone() as Arc<dyn TextGenerator>),
        ..HarnessOptions::default()
    })
    .await;
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| all_ok(s, 21)).await;
    h.settle().await;
    assert_eq!(generator.calls.get(), 0);
    assert_eq!(
        h.engine.current_state().inference_status,
        InferenceStatus::Disabled
    );

    let dedup_file = h.dedup_file();
    let probe = RoundProbe::new(dedup_file.clone());
    hook(&generator, &probe);
    let widgets_before = h.widget_snapshots().len();

    h.engine
        .send(EngineCommand::ApplySettings(Box::new(|s| {
            s.inference_enabled = true;
        })))
        .await;

    // Round one made exactly 10 passes of 20 calls: the dedup file, written once at the end of a round, did not exist
    // on the second pass or the 200th call, and did on the first call after round one (checked below, once that call
    // has happened). (If an earlier round was still in flight when the setting changed, the engine runs one more
    // round at once; see the next test. The total then reaches 210 here, but round one is still 200 calls.)
    assert_eq!(probe.file_at(21), Some(false));
    assert_eq!(probe.file_at(200), Some(false));
    assert!([200, 210].contains(&generator.calls.get()));
    assert!(dedup_file.exists());
    let state = h.engine.current_state();
    assert_eq!(
        state.inference_status,
        InferenceStatus::Active(FakeEngineGenerator::on_device())
    );
    assert_eq!(state.events.len(), 21);
    // Each pass's verdicts republish: at least one widget snapshot per pass (10), plus the settings change and the
    // re-snapshot with inference on.
    h.wait_until("the widget snapshots", || {
        h.widget_snapshots().len() >= widgets_before + 12
    })
    .await;

    // Nothing retries on its own; the next refresh judges whatever is left.
    h.engine.send(EngineCommand::Refresh).await;
    h.wait_until("the remaining pairs", || generator.calls.get() == 210)
        .await;
    h.settle().await;
    assert_eq!(generator.calls.get(), 210);
    assert_eq!(probe.calls_before_first_write(), Some(200));
}

/// The CI interleaving of the test above, made deterministic: the round started by the launch refresh (inference still
/// off) is held in flight when inference is turned on. The settings change joins that round and asks for one more, so
/// round one judges with inference on (10 passes, 200 calls, then one write) and round two judges the 10 left. Verdict
/// publications never request a round themselves, so it stops there.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_setting_change_during_an_in_flight_round_runs_exactly_one_more_round() {
    let generator = FakeEngineGenerator::new(Some("different"));
    let gate = Arc::new(Gate::new());
    // Read 1 is `start`'s own availability check; read 2 is the launch refresh's resolution round.
    generator.availability_gate.set(Some((2, gate.clone())));
    let h = EngineHarness::with(HarnessOptions {
        accounts: look_alikes(21),
        text_generator: Some(generator.clone() as Arc<dyn TextGenerator>),
        ..HarnessOptions::default()
    })
    .await;
    let probe = RoundProbe::new(h.dedup_file());
    hook(&generator, &probe);
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| all_ok(s, 21)).await;
    h.wait_until("the launch round at the gate", || gate.is_waiting_now())
        .await;
    let widgets_before = h.widget_snapshots().len();

    let engine = h.engine.clone();
    let turn_on = tokio::spawn(async move {
        engine
            .send(EngineCommand::ApplySettings(Box::new(|s| {
                s.inference_enabled = true;
            })))
            .await;
    });
    // The settings change and the re-snapshot both emitted: the change is now waiting on the held round.
    h.wait_until("the settings change to join the round", || {
        h.widget_snapshots().len() >= widgets_before + 2
    })
    .await;
    assert_eq!(generator.calls.get(), 0);
    gate.open().await;
    turn_on.await.unwrap();

    assert_eq!(probe.calls_before_first_write(), Some(200));
    assert_eq!(generator.calls.get(), 210);
    h.settle().await;
    assert_eq!(generator.calls.get(), 210);
}

// Scenario 2
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn temporarily_unavailable_keeps_pairs_pending_until_the_next_refresh() {
    let generator = FakeEngineGenerator::with(
        TextGeneratorAvailability::TemporarilyUnavailable {
            reason: "low power".to_string(),
        },
        Some("same"),
    );
    let h = EngineHarness::with(HarnessOptions {
        accounts: look_alikes(2),
        text_generator: Some(generator.clone() as Arc<dyn TextGenerator>),
        inference_enabled: true,
        ..HarnessOptions::default()
    })
    .await;
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| all_ok(s, 2)).await;
    h.settle().await;
    let mut state = h.engine.current_state();
    assert_eq!(generator.calls.get(), 0);
    assert_eq!(
        state.inference_status,
        InferenceStatus::Unavailable {
            reason: "temporary: low power".to_string()
        }
    );
    assert_eq!(state.events.len(), 2);
    assert!(!h.dedup_file().exists());

    // Becoming available does not start a retry loop on its own.
    generator
        .availability_value
        .set(TextGeneratorAvailability::Available(
            FakeEngineGenerator::on_device(),
        ));
    h.clock.advance(60.0).await;
    h.settle().await;
    assert_eq!(generator.calls.get(), 0);

    h.engine.send(EngineCommand::Refresh).await;
    state = h.wait_for_state(|s| s.events.len() == 1).await;
    assert_eq!(
        state.inference_status,
        InferenceStatus::Active(FakeEngineGenerator::on_device())
    );
    h.settle().await;
    assert_eq!(generator.calls.get(), 1);
}

// Scenario 3
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_throwing_generator_records_no_verdict_and_retries_only_on_the_next_refresh() {
    let generator = FakeEngineGenerator::new(None);
    let h = EngineHarness::with(HarnessOptions {
        accounts: look_alikes(2),
        text_generator: Some(generator.clone() as Arc<dyn TextGenerator>),
        inference_enabled: true,
        ..HarnessOptions::default()
    })
    .await;
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| all_ok(s, 2)).await;
    h.wait_until("the first judgment", || generator.calls.get() == 1)
        .await;
    h.settle().await;
    assert_eq!(generator.calls.get(), 1);
    assert_eq!(h.engine.current_state().events.len(), 2);
    let diagnostics = h.diagnostics.get();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.category == "inference" && d.message.contains("Judgment failed: HostError")),
        "{diagnostics:?}"
    );
    assert!(!diagnostics.iter().any(|d| d.message.contains("Meeting ")));

    // No timer retries it: once the backoff has passed, still only one call until something refreshes.
    h.clock.advance(60.0).await;
    h.settle().await;
    assert_eq!(generator.calls.get(), 1);

    generator.answer.set(Some("same".to_string()));
    h.engine.send(EngineCommand::Refresh).await;
    h.wait_for_state(|s| s.events.len() == 1).await;
    h.settle().await;
    assert_eq!(generator.calls.get(), 2);
}

// Scenario 4
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn dedup_actions_publish_and_persist_only_when_the_state_changed() {
    let h = EngineHarness::new(vec![
        sample("a", 600.0),
        sample("b", 900.0),
        sample("c", 7200.0),
    ])
    .await;
    h.engine.send(EngineCommand::Start).await;
    let mut state = h.wait_for_state(|s| s.events.len() == 3).await;
    fn event(state: &EngineState, id: &str) -> TimeTugCalendarEvent {
        state
            .events
            .iter()
            .find(|e| e.source_event_id() == id)
            .cloned()
            .unwrap()
    }
    let file = h.dedup_file();
    assert!(!file.exists());

    async fn run(
        h: &EngineHarness,
        state: &mut EngineState,
        file: &std::path::Path,
        command: EngineCommand,
        persists: bool,
    ) {
        let before = state.revision;
        h.engine.send(command).await;
        *state = h.engine.current_state();
        assert!(state.revision > before);
        assert_eq!(file.exists(), persists);
        let _ = std::fs::remove_file(file);
    }

    let command = EngineCommand::Merge(event(&state, "a"), event(&state, "b"));
    run(&h, &mut state, &file, command, true).await;
    assert_eq!(state.events.len(), 2);
    let diagnostics: Vec<_> = h
        .diagnostics
        .get()
        .into_iter()
        .filter(|d| d.category == "dedup")
        .collect();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("2 copies") && d.message.contains("2 events"))
    );
    assert!(
        !h.diagnostics
            .get()
            .iter()
            .any(|d| d.message.contains("Meeting "))
    );

    // Unmerging an event that was never merged teaches nothing.
    let command = EngineCommand::Unmerge(event(&state, "c"));
    run(&h, &mut state, &file, command, false).await;
    assert_eq!(state.events.len(), 2);

    let merged = state
        .events
        .iter()
        .find(|e| e.merged_members.len() == 2)
        .cloned()
        .unwrap();
    run(
        &h,
        &mut state,
        &file,
        EngineCommand::Separate {
            event: merged.clone(),
            members: vec![merged.merged_members[0].clone()],
        },
        true,
    )
    .await;
    assert_eq!(state.events.len(), 3);

    let command = EngineCommand::Merge(event(&state, "a"), event(&state, "b"));
    run(&h, &mut state, &file, command, true).await;
    let merged_again = state
        .events
        .iter()
        .find(|e| e.merged_members.len() == 2)
        .cloned()
        .unwrap();
    run(
        &h,
        &mut state,
        &file,
        EngineCommand::Unmerge(merged_again),
        true,
    )
    .await;
    assert_eq!(state.events.len(), 3);

    run(
        &h,
        &mut state,
        &file,
        EngineCommand::ForgetCorrections,
        true,
    )
    .await;
    // Nothing left to forget.
    run(
        &h,
        &mut state,
        &file,
        EngineCommand::ForgetCorrections,
        false,
    )
    .await;
}
