//! Port of `TimeTugCoreTests/CalendarStoreInferenceTests.swift`.
//!
//! The Swift adjudicators are `final class ... @unchecked Sendable` over an `NSLock` (or an actor, for the gated one);
//! here they are structs over a `Mutex` and, for the gated one, two `Notify` permits. `async let` is `tokio::join!`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use calendar_core::model::CalendarEvent;
use calendar_core::time::add_seconds;
use tokio::sync::Notify;

use super::calendar_store_tests::{FakeSource, new_store};
use super::dedup_support::ev;
use super::support::{date, day};
use crate::dedup::adjudication::{
    AdjudicationRequest, AdjudicationVerdict, AdjudicatorAvailability, DuplicateAdjudicator,
    EngineInfo, VerdictAnswer, VerdictCache,
};
use crate::dedup::lesson_book::LessonDecision;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::merge_types::{MergeProvenance, MergedMember};
use crate::store::calendar_store::{CalendarSnapshot, CalendarStore, DedupState, InferenceStatus};

fn now() -> jiff::Timestamp {
    date("2026-09-18T09:00:00Z")
}

fn fake_engine() -> EngineInfo {
    EngineInfo::new("fake-ai", "Fake AI", true)
}

/// `FakeAdjudicator`.
struct FakeAdjudicator {
    availability: AdjudicatorAvailability,
    answer: VerdictAnswer,
    seen: Mutex<Vec<AdjudicationRequest>>,
}

impl FakeAdjudicator {
    fn new() -> Arc<FakeAdjudicator> {
        FakeAdjudicator::with(
            AdjudicatorAvailability::Available(fake_engine()),
            VerdictAnswer::Same,
        )
    }
    fn with(availability: AdjudicatorAvailability, answer: VerdictAnswer) -> Arc<FakeAdjudicator> {
        Arc::new(FakeAdjudicator {
            availability,
            answer,
            seen: Mutex::new(Vec::new()),
        })
    }
    fn requests(&self) -> Vec<AdjudicationRequest> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl DuplicateAdjudicator for FakeAdjudicator {
    fn availability(&self) -> AdjudicatorAvailability {
        self.availability.clone()
    }
    async fn judge(&self, requests: &[AdjudicationRequest]) -> Vec<AdjudicationVerdict> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(requests.iter().cloned());
        requests
            .iter()
            .map(|r| AdjudicationVerdict::new(r.id.clone(), self.answer))
            .collect()
    }
}

fn doctor() -> TimeTugCalendarEvent {
    ev("1")
        .title("Scott: Doctor")
        .minutes(60)
        .cal("personal")
        .others(0)
        .build()
}

fn official() -> TimeTugCalendarEvent {
    ev("2")
        .title("Intermountain Health")
        .minutes(60)
        .cal("work")
        .location("1234 Main St")
        .notes("Bring insurance card")
        .build()
}

fn make_store(adjudicator: Option<Arc<dyn DuplicateAdjudicator>>) -> CalendarStore {
    let source = FakeSource::fake();
    source.set_events(Ok(vec![doctor(), official()]));
    CalendarStore::new(
        sources![source],
        super::support::zone("UTC"),
        adjudicator,
        super::support::fixed_clock(),
    )
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn inference_is_off_by_default_and_the_engine_is_never_called() {
    let engine = FakeAdjudicator::new();
    let store = make_store(Some(engine.clone()));
    assert_eq!(store.inference_status(), InferenceStatus::Disabled);
    assert_eq!(store.refresh(now(), 60.0, None, None).await.events.len(), 2);
    assert!(store.resolve_pending(now()).await.is_none());
    assert!(engine.requests().is_empty());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn enabled_engine_merges_after_resolve_pending_without_blocking_refresh() {
    let engine = FakeAdjudicator::new();
    let store = make_store(Some(engine.clone()));
    store.set_inference_enabled(true, now());
    // refresh never waits on the model
    assert_eq!(store.refresh(now(), 60.0, None, None).await.events.len(), 2);
    let merged = store.resolve_pending(now()).await;
    assert_eq!(merged.as_ref().map(|s| s.events.len()), Some(1));
    assert_eq!(
        merged.unwrap().events[0].merge_provenance,
        Some(MergeProvenance::Inference {
            engine_id: "fake-ai".to_string(),
            engine_name: "Fake AI".to_string()
        })
    );
    assert_eq!(engine.requests().len(), 1);
    // cached: no second call
    assert!(store.resolve_pending(now()).await.is_none());
    // cache applied on refresh
    assert_eq!(store.refresh(now(), 60.0, None, None).await.events.len(), 1);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn unavailable_or_off_device_engines_fall_back_to_rules_only() {
    let unavailable = FakeAdjudicator::with(
        AdjudicatorAvailability::Unavailable {
            reason: "not eligible".to_string(),
        },
        VerdictAnswer::Same,
    );
    let mut store = make_store(Some(unavailable.clone()));
    store.set_inference_enabled(true, now());
    store.refresh(now(), 60.0, None, None).await;
    assert_eq!(
        store.inference_status(),
        InferenceStatus::Unavailable {
            reason: "not eligible".to_string()
        }
    );
    assert!(store.resolve_pending(now()).await.is_none());
    assert!(unavailable.requests().is_empty());

    let cloud = FakeAdjudicator::with(
        AdjudicatorAvailability::Available(EngineInfo::new("cloud", "Cloud", false)),
        VerdictAnswer::Same,
    );
    store = make_store(Some(cloud.clone()));
    store.set_inference_enabled(true, now());
    store.refresh(now(), 60.0, None, None).await;
    assert_eq!(store.inference_status(), InferenceStatus::NotOnDevice);
    assert!(store.resolve_pending(now()).await.is_none());
    assert!(cloud.requests().is_empty());

    let none = make_store(None);
    none.set_inference_enabled(true, now());
    assert_eq!(none.inference_status(), InferenceStatus::NoEngine);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn turning_inference_off_ignores_cached_verdicts() {
    let store = make_store(Some(FakeAdjudicator::new()));
    store.set_inference_enabled(true, now());
    store.refresh(now(), 60.0, None, None).await;
    store.resolve_pending(now()).await;
    assert_eq!(store.set_inference_enabled(false, now()).events.len(), 2);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn unmerge_splits_and_remembers_and_manual_merge_joins_again() {
    let store = make_store(Some(FakeAdjudicator::new()));
    store.set_inference_enabled(true, now());
    store.refresh(now(), 60.0, None, None).await;
    let merged = store.resolve_pending(now()).await.unwrap().events.remove(0);

    let split = store.unmerge(&merged, now());
    assert_eq!(split.events.len(), 2);
    assert_eq!(
        store.state().lessons.lessons().first().map(|l| l.decision),
        Some(LessonDecision::Different)
    );
    // survives refresh, engine not re-asked
    assert_eq!(store.refresh(now(), 60.0, None, None).await.events.len(), 2);

    let a = split
        .events
        .iter()
        .find(|e| e.title() == "Scott: Doctor")
        .unwrap();
    let b = split
        .events
        .iter()
        .find(|e| e.title() == "Intermountain Health")
        .unwrap();
    let joined = store.merge(a, b, now());
    assert_eq!(joined.events.len(), 1);
    assert_eq!(
        joined.events[0].merge_provenance,
        Some(MergeProvenance::UserConfirmed)
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn lessons_work_with_inference_off_and_can_be_forgotten() {
    let store = make_store(None);
    let snapshot = store.refresh(now(), 60.0, None, None).await;
    let joined = store.merge(&snapshot.events[0], &snapshot.events[1], now());
    assert_eq!(joined.events.len(), 1);
    assert_eq!(store.forget_lessons(now()).events.len(), 2);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn state_round_trips_through_load() {
    let store = make_store(Some(FakeAdjudicator::new()));
    let snapshot = store.refresh(now(), 60.0, None, None).await;
    store.merge(&snapshot.events[0], &snapshot.events[1], now());
    let state = store.state();

    let fresh = make_store(None);
    fresh.load(state);
    assert_eq!(fresh.refresh(now(), 60.0, None, None).await.events.len(), 1);
}

/// `SlowAdjudicator`: answers `same` after a 50 ms sleep (the test's paused clock advances through it).
struct SlowAdjudicator {
    calls: Mutex<usize>,
}

impl SlowAdjudicator {
    fn call_count(&self) -> usize {
        *self.calls.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl DuplicateAdjudicator for SlowAdjudicator {
    fn availability(&self) -> AdjudicatorAvailability {
        AdjudicatorAvailability::Available(fake_engine())
    }
    async fn judge(&self, requests: &[AdjudicationRequest]) -> Vec<AdjudicationVerdict> {
        *self.calls.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        tokio::time::sleep(Duration::from_millis(50)).await;
        requests
            .iter()
            .map(|r| AdjudicationVerdict::new(r.id.clone(), VerdictAnswer::Same))
            .collect()
    }
}

/// `FailingAdjudicator`: returns no verdicts at all.
struct FailingAdjudicator {
    calls: Mutex<usize>,
}

impl FailingAdjudicator {
    fn call_count(&self) -> usize {
        *self.calls.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl DuplicateAdjudicator for FailingAdjudicator {
    fn availability(&self) -> AdjudicatorAvailability {
        AdjudicatorAvailability::Available(fake_engine())
    }
    async fn judge(&self, _requests: &[AdjudicationRequest]) -> Vec<AdjudicationVerdict> {
        *self.calls.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        Vec::new()
    }
}

/// `private actor GatedAdjudicator`: `judge` waits until `release()`.
#[derive(Default)]
struct GatedAdjudicator {
    request_id: Mutex<String>,
    started: Notify,
    released: Notify,
}

impl GatedAdjudicator {
    async fn wait_until_started(&self) {
        self.started.notified().await;
    }
    fn release(&self) {
        self.released.notify_one();
    }
}

#[async_trait]
impl DuplicateAdjudicator for GatedAdjudicator {
    fn availability(&self) -> AdjudicatorAvailability {
        AdjudicatorAvailability::Available(fake_engine())
    }
    async fn judge(&self, requests: &[AdjudicationRequest]) -> Vec<AdjudicationVerdict> {
        let id = requests[0].id.clone();
        *self
            .request_id
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = id.clone();
        self.started.notify_one();
        self.released.notified().await;
        vec![AdjudicationVerdict::new(id, VerdictAnswer::Same)]
    }
}

fn store_with(adjudicator: Arc<dyn DuplicateAdjudicator>) -> (Arc<FakeSource>, Arc<CalendarStore>) {
    let source = FakeSource::fake();
    source.set_events(Ok(vec![doctor(), official()]));
    let store = CalendarStore::new(
        sources![source],
        super::support::zone("UTC"),
        Some(adjudicator),
        super::support::fixed_clock(),
    );
    (source, Arc::new(store))
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn changed_refresh_input_rejects_suspended_inference_verdict() {
    let engine = Arc::new(GatedAdjudicator::default());
    let (source, store) = store_with(engine.clone());
    store.set_inference_enabled(true, now());
    store.refresh(now(), 60.0, None, None).await;
    let resolving = tokio::spawn({
        let store = store.clone();
        async move { store.resolve_pending(now()).await }
    });
    engine.wait_until_started().await;
    source.set_events(Ok(vec![doctor()]));
    store.refresh(now(), 60.0, None, None).await;
    engine.release();
    assert!(resolving.await.unwrap().is_none());
    assert!(store.state().verdicts.entries().is_empty());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn transient_failures_back_off_per_canonical_key() {
    let engine = Arc::new(FailingAdjudicator {
        calls: Mutex::new(0),
    });
    let (_source, store) = store_with(engine.clone());
    store.set_inference_enabled(true, now());
    store.refresh(now(), 60.0, None, None).await;
    assert!(store.resolve_pending(now()).await.is_none());
    assert_eq!(engine.call_count(), 1);
    // (seconds after `now`, calls expected after asking then)
    for (offset, calls) in [
        (59.0, 1),
        (60.0, 2),
        (179.0, 2),
        (180.0, 3),
        (419.0, 3),
        (420.0, 4),
        (899.0, 4),
        (900.0, 5),
        (1799.0, 5),
        (1800.0, 6),
    ] {
        assert!(
            store
                .resolve_pending(add_seconds(now(), offset))
                .await
                .is_none(),
            "at +{offset}"
        );
        assert_eq!(engine.call_count(), calls, "at +{offset}");
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn disabling_inference_discards_an_in_flight_verdict() {
    let engine = Arc::new(GatedAdjudicator::default());
    let (_source, store) = store_with(engine.clone());
    store.set_inference_enabled(true, now());
    store.refresh(now(), 60.0, None, None).await;
    let pending = tokio::spawn({
        let store = store.clone();
        async move { store.resolve_pending(now()).await }
    });
    engine.wait_until_started().await;
    store.set_inference_enabled(false, now());
    engine.release();
    assert!(pending.await.unwrap().is_none());
    assert!(store.state().verdicts.entries().is_empty());
    assert_eq!(store.set_inference_enabled(true, now()).events.len(), 2);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn concurrent_resolve_pending_runs_only_one_pass() {
    let slow = Arc::new(SlowAdjudicator {
        calls: Mutex::new(0),
    });
    let (_source, store) = store_with(slow.clone());
    store.set_inference_enabled(true, now());
    store.refresh(now(), 60.0, None, None).await;
    let (first, second) = tokio::join!(store.resolve_pending(now()), store.resolve_pending(now()));
    assert_eq!(slow.call_count(), 1);
    assert_eq!([first, second].iter().flatten().count(), 1);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn loaded_state_is_pruned_on_the_next_snapshot() {
    let mut stale = VerdictCache::default();
    stale.store(
        &AdjudicationVerdict::new("old", VerdictAnswer::Same),
        &fake_engine(),
        add_seconds(now(), -3600.0),
        add_seconds(now(), -VerdictCache::RETENTION - 60.0),
    );
    assert!(stale.entry_for("old").is_some());
    let store = make_store(None);
    store.load(DedupState::new(Default::default(), stale));
    store.refresh(now(), 60.0, None, None).await;
    assert!(store.state().verdicts.entry_for("old").is_none());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_pair_that_already_ended_today_is_judged_once() {
    let engine = FakeAdjudicator::new();
    let store = make_store(Some(engine.clone()));
    // both events ended hours ago but are still in today's window
    let later = date("2026-09-18T15:00:00Z");
    store.set_inference_enabled(true, later);
    assert_eq!(store.refresh(later, 60.0, None, None).await.events.len(), 2);
    let mut passes = 0;
    while store.resolve_pending(later).await.is_some() && passes < 5 {
        passes += 1;
    }
    assert_eq!(passes, 1);
    assert!(store.resolve_pending(later).await.is_none());
    assert_eq!(store.refresh(later, 60.0, None, None).await.events.len(), 1);
    assert_eq!(store.refresh(later, 60.0, None, None).await.events.len(), 1);
    assert_eq!(engine.requests().len(), 1);
}

async fn unmerge_exact_duplicates(a: TimeTugCalendarEvent, b: TimeTugCalendarEvent) {
    let source = FakeSource::fake();
    source.set_events(Ok(vec![a, b]));
    let store = new_store(sources![source], "UTC");
    let merged = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(merged.events.len(), 1);
    let split = store.unmerge(&merged.events[0], now());
    assert_eq!(split.events.len(), 2);
    // persists across refresh
    assert_eq!(store.refresh(now(), 60.0, None, None).await.events.len(), 2);
    // Undo stays available: each half offers the other as a manual "Merge with".
    let ids: Vec<String> = split.events.iter().map(|e| e.id()).collect();
    for event in &split.events {
        let expected: Vec<String> = ids
            .iter()
            .filter(|id| **id != event.id())
            .cloned()
            .collect();
        assert_eq!(
            split
                .candidates
                .get(&event.id())
                .map(|list| list.iter().map(|e| e.id()).collect::<Vec<_>>()),
            Some(expected)
        );
    }
    let rejoined = store.merge(&split.events[0], &split.events[1], now());
    assert_eq!(rejoined.events.len(), 1);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn unmerging_all_day_duplicates_on_two_calendars_sticks() {
    unmerge_exact_duplicates(
        super::dedup_support::make_all_day_in(
            "1",
            "UTC",
            day(2026, 9, 18),
            day(2026, 9, 19),
            "Holiday",
            "personal",
        ),
        super::dedup_support::make_all_day_in(
            "2",
            "UTC",
            day(2026, 9, 18),
            day(2026, 9, 19),
            "Holiday",
            "work",
        ),
    )
    .await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn unmerging_same_calendar_exact_duplicates_sticks() {
    unmerge_exact_duplicates(
        ev("1").title("Standup").cal("work").build(),
        ev("2").title("Standup").cal("work").build(),
    )
    .await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn forgetting_lessons_also_forgets_cached_verdicts() {
    let engine = FakeAdjudicator::new();
    let store = make_store(Some(engine.clone()));
    store.set_inference_enabled(true, now());
    store.refresh(now(), 60.0, None, None).await;
    assert_eq!(
        store.resolve_pending(now()).await.map(|s| s.events.len()),
        Some(1)
    );
    let forgotten = store.forget_lessons(now());
    // not silently re-merged from cache
    assert_eq!(forgotten.events.len(), 2);
    assert!(store.state().verdicts.entries().is_empty());
    // pending again, judged again
    assert_eq!(
        store.resolve_pending(now()).await.map(|s| s.events.len()),
        Some(1)
    );
    assert_eq!(engine.requests().len(), 2);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_backlog_larger_than_one_pass_drains_across_passes() {
    // 25 look-alike pairs, 55 minutes apart so different pairs never fall inside each other's time gate.
    let base = date("2026-09-18T00:10:00Z");
    let mut events: Vec<TimeTugCalendarEvent> = Vec::new();
    for k in 0..25 {
        let start = add_seconds(base, f64::from(k * 55 * 60));
        for (calendar_id, title) in [
            ("personal", format!("Errand {k}")),
            ("work", format!("Visit {k}")),
        ] {
            events.push(TimeTugCalendarEvent::new(
                CalendarEvent::new(
                    format!("{calendar_id}{k}"),
                    calendar_id,
                    title,
                    start,
                    add_seconds(start, 20.0 * 60.0),
                ),
                "fake",
            ));
        }
    }
    let engine = FakeAdjudicator::new();
    let source = FakeSource::fake();
    source.set_events(Ok(events));
    let store = CalendarStore::new(
        sources![source],
        super::support::zone("UTC"),
        Some(engine.clone()),
        super::support::fixed_clock(),
    );
    store.set_inference_enabled(true, now());
    assert_eq!(
        store.refresh(now(), 60.0, None, None).await.events.len(),
        50
    );
    let mut passes = 0;
    let mut latest: Option<CalendarSnapshot> = None;
    loop {
        let Some(next) = store.resolve_pending(now()).await else {
            break;
        };
        if passes >= 10 {
            break;
        }
        latest = Some(next);
        passes += 1;
    }
    assert_eq!(passes, 2); // 20 then 5
    assert_eq!(latest.map(|s| s.events.len()), Some(25));
    assert_eq!(engine.requests().len(), 25);
}

fn copy(n: i64) -> TimeTugCalendarEvent {
    ev(&format!("x{n}"))
        .title("Mando's Upcoming Appointment")
        .start("2026-09-18T13:00:00Z")
        .minutes(30)
        .cal(&format!("X{n}"))
        .location("Clinic, 5 Main St")
        .notes("Bring your card")
        .build()
}

fn spem() -> TimeTugCalendarEvent {
    ev("s")
        .title("Mando Spem Collection")
        .start("2026-09-18T12:45:00Z")
        .minutes(60)
        .cal("S")
        .build()
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn three_identical_copies_and_a_placeholder_are_judged_once_per_group_pair() {
    let engine = FakeAdjudicator::new();
    let source = FakeSource::fake();
    source.set_events(Ok(vec![spem(), copy(1), copy(2), copy(3)]));
    let store = CalendarStore::new(
        sources![source],
        super::support::zone("UTC"),
        Some(engine.clone()),
        super::support::fixed_clock(),
    );
    store.set_inference_enabled(true, now());
    assert_eq!(store.refresh(now(), 60.0, None, None).await.events.len(), 2);
    let merged = store.resolve_pending(now()).await;
    assert_eq!(engine.requests().len(), 1);
    assert_eq!(merged.as_ref().map(|s| s.events.len()), Some(1));
    assert_eq!(merged.unwrap().events[0].merged_members.len(), 4);
    assert!(store.resolve_pending(now()).await.is_none());
}

// MARK: - Separating one member of a merged card

async fn placeholder_and_copies(
    engine: Arc<FakeAdjudicator>,
) -> (CalendarStore, TimeTugCalendarEvent) {
    let source = FakeSource::fake();
    source.set_events(Ok(vec![spem(), copy(1), copy(2), copy(3)]));
    let store = CalendarStore::new(
        sources![source],
        super::support::zone("UTC"),
        Some(engine),
        super::support::fixed_clock(),
    );
    store.set_inference_enabled(true, now());
    store.refresh(now(), 60.0, None, None).await;
    let merged = store.resolve_pending(now()).await.unwrap().events.remove(0);
    (store, merged)
}

fn key_sets(events: &[TimeTugCalendarEvent]) -> BTreeSet<BTreeSet<String>> {
    events
        .iter()
        .map(|e| {
            e.participants()
                .into_iter()
                .map(|p| p.calendar_key)
                .collect()
        })
        .collect()
}

fn keys(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn separating_the_placeholder_leaves_the_copies_merged_and_persists() {
    let (store, merged) = placeholder_and_copies(FakeAdjudicator::new()).await;
    assert_eq!(merged.merged_members.len(), 4);
    let placeholder: MergedMember = merged
        .merged_members
        .iter()
        .find(|m| m.calendar_key == "fake/S")
        .unwrap()
        .clone();
    let split = store.separate(&[placeholder], &merged, now());
    assert_eq!(split.events.len(), 2);
    assert!(split.events.iter().any(|e| e.merged_members.len() == 3));
    assert!(
        split
            .events
            .iter()
            .any(|e| e.title() == "Mando Spem Collection" && e.merged_members.is_empty())
    );
    let after = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(after.events.len(), 2);
    assert!(after.events.iter().any(|e| e.merged_members.len() == 3));
    // placeholder vs each of the three copies
    assert_eq!(store.state().lessons.lessons().len(), 3);
    // The split-off event can be offered for a manual re-merge.
    let alone = after
        .events
        .iter()
        .find(|e| e.merged_members.is_empty())
        .unwrap();
    assert!(
        after
            .candidates
            .get(&alone.id())
            .is_some_and(|list| !list.is_empty())
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn separating_one_identical_copy_leaves_the_others_merged() {
    let (store, merged) = placeholder_and_copies(FakeAdjudicator::new()).await;
    let one = merged
        .merged_members
        .iter()
        .find(|m| m.calendar_key == "fake/X1")
        .unwrap()
        .clone();
    let split = store.separate(&[one], &merged, now());
    // a new correction changes the prompt; the old verdict is not reused
    assert_eq!(split.events.len(), 3);
    let resolved = store.resolve_pending(now()).await.unwrap_or(split);
    let mut by_members: BTreeMap<BTreeSet<String>, usize> = BTreeMap::new();
    for event in &resolved.events {
        let set: BTreeSet<String> = event
            .participants()
            .into_iter()
            .map(|p| p.calendar_key)
            .collect();
        *by_members.entry(set).or_default() += 1;
    }
    assert_eq!(by_members.get(&keys(&["fake/X1"])), Some(&1));
    assert_eq!(
        by_members.get(&keys(&["fake/S", "fake/X2", "fake/X3"])),
        Some(&1)
    );
    let after = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(
        key_sets(&after.events),
        BTreeSet::from([keys(&["fake/X1"]), keys(&["fake/S", "fake/X2", "fake/X3"])])
    );
    let alone = after
        .events
        .iter()
        .find(|e| e.participants().len() == 1)
        .unwrap();
    assert!(
        after
            .candidates
            .get(&alone.id())
            .is_some_and(|list| !list.is_empty())
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn separating_all_identical_copies_together_leaves_them_merged_and_the_placeholder_alone() {
    let (store, merged) = placeholder_and_copies(FakeAdjudicator::new()).await;
    let copies: Vec<MergedMember> = merged
        .merged_members
        .iter()
        .filter(|m| m.calendar_key != "fake/S")
        .cloned()
        .collect();
    assert_eq!(copies.len(), 3);
    let split = store.separate(&copies, &merged, now());
    assert_eq!(split.events.len(), 2);
    let expected = BTreeSet::from([keys(&["fake/S"]), keys(&["fake/X1", "fake/X2", "fake/X3"])]);
    assert_eq!(key_sets(&split.events), expected);
    // each copy vs the placeholder; none among the copies
    assert_eq!(store.state().lessons.lessons().len(), 3);
    let after = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(key_sets(&after.events), expected);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn unmerging_the_whole_card_still_works_after_a_separation() {
    let (store, merged) = placeholder_and_copies(FakeAdjudicator::new()).await;
    let placeholder = merged
        .merged_members
        .iter()
        .find(|m| m.calendar_key == "fake/S")
        .unwrap()
        .clone();
    let split = store.separate(&[placeholder], &merged, now());
    let copies = split
        .events
        .iter()
        .find(|e| e.merged_members.len() == 3)
        .unwrap();
    assert_eq!(store.unmerge(copies, now()).events.len(), 4);
}
