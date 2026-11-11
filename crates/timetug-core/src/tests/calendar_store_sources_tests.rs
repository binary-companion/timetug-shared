//! Port of `TimeTugCoreTests/CalendarStoreSourcesTests.swift`, plus the Review Focus 4 test of the phase 2 plan (three
//! sources, the middle one failing twice in a row), which has no Swift counterpart.
//!
//! Swift's `Task { await store.refresh(...) }` is `tokio::spawn` on the test's current-thread runtime, and its
//! `CheckedContinuation`s are `oneshot` channels and `Notify`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use calendar_core::failure::{Failure, Outcome};
use calendar_core::time::DateInterval;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::sync::{oneshot, watch};

use super::calendar_store_tests::{FakeSource, new_store};
use super::dedup_support::ev;
use super::support::{date, day, make_all_day, zone};
use crate::model::calendar_info::CalendarInfo;
use crate::model::event::TimeTugCalendarEvent;
use crate::sources::{CalendarSource, SourceChange, SourceError, SourceStatus};
use crate::store::calendar_store::CalendarSnapshot;

fn now() -> jiff::Timestamp {
    date("2026-09-18T10:00:00Z")
}

fn ids(events: &[TimeTugCalendarEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| e.source_event_id().to_string())
        .collect()
}

fn id_set(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// `private actor SnapshotRecorder`.
#[derive(Default)]
struct SnapshotRecorder {
    snapshots: Mutex<Vec<CalendarSnapshot>>,
}

impl SnapshotRecorder {
    fn record(&self, snapshot: CalendarSnapshot) {
        self.snapshots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(snapshot);
    }
    fn contains(&self, id: &str) -> bool {
        self.snapshots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|s| s.events.iter().any(|e| e.source_event_id() == id))
    }
}

/// A source whose `events_in` waits until `release()` so a test can interleave `set_sources`.
pub(super) struct GateSource {
    id: String,
    started: AtomicBool,
    open: watch::Sender<bool>,
    stored: Vec<TimeTugCalendarEvent>,
}

impl GateSource {
    pub(super) fn new(id: &str, events: Vec<TimeTugCalendarEvent>) -> Arc<GateSource> {
        Arc::new(GateSource {
            id: id.to_string(),
            started: AtomicBool::new(false),
            open: watch::channel(false).0,
            stored: events,
        })
    }
    fn started(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }
    fn release(&self) {
        self.open.send_replace(true);
    }
}

#[async_trait]
impl CalendarSource for GateSource {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn display_name(&self) -> String {
        "Gate".to_string()
    }
    async fn calendars(&self) -> Outcome<Vec<CalendarInfo>> {
        Ok(Vec::new())
    }
    async fn events_in(&self, _interval: DateInterval) -> Outcome<Vec<TimeTugCalendarEvent>> {
        self.started.store(true, Ordering::SeqCst);
        let mut open = self.open.subscribe();
        open.wait_for(|is_open| *is_open)
            .await
            .map_err(|_| Failure::Cancelled)?;
        Ok(self.stored.clone())
    }
    fn changes(&self) -> UnboundedReceiver<SourceChange> {
        unbounded_channel().1
    }
}

/// `private actor OrderedSource`: every `events_in` call waits for the test to finish it by request number.
struct OrderedSource {
    inner: Mutex<OrderedInner>,
}

#[derive(Default)]
struct OrderedInner {
    waiters: Vec<Option<oneshot::Sender<Vec<TimeTugCalendarEvent>>>>,
    milestones: BTreeMap<usize, oneshot::Sender<()>>,
}

impl OrderedSource {
    fn new() -> Arc<OrderedSource> {
        Arc::new(OrderedSource {
            inner: Mutex::new(OrderedInner::default()),
        })
    }
    fn inner(&self) -> std::sync::MutexGuard<'_, OrderedInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
    async fn wait_for_request(&self, count: usize) {
        let receiver = {
            let mut inner = self.inner();
            if inner.waiters.len() >= count {
                return;
            }
            let (sender, receiver) = oneshot::channel();
            inner.milestones.insert(count, sender);
            receiver
        };
        let _ = receiver.await;
    }
    fn finish(&self, request: usize, events: Vec<TimeTugCalendarEvent>) {
        let sender = self.inner().waiters[request].take();
        if let Some(sender) = sender {
            let _ = sender.send(events);
        }
    }
}

#[async_trait]
impl CalendarSource for OrderedSource {
    fn id(&self) -> String {
        "ordered".to_string()
    }
    fn display_name(&self) -> String {
        "Ordered".to_string()
    }
    async fn calendars(&self) -> Outcome<Vec<CalendarInfo>> {
        Ok(Vec::new())
    }
    async fn events_in(&self, _interval: DateInterval) -> Outcome<Vec<TimeTugCalendarEvent>> {
        let (sender, receiver) = oneshot::channel();
        {
            let mut inner = self.inner();
            inner.waiters.push(Some(sender));
            let count = inner.waiters.len();
            if let Some(milestone) = inner.milestones.remove(&count) {
                let _ = milestone.send(());
            }
        }
        receiver.await.map_err(|_| Failure::Cancelled)
    }
    fn changes(&self) -> UnboundedReceiver<SourceChange> {
        unbounded_channel().1
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn targeted_refresh_reads_only_the_named_source() {
    let (a, b, c) = (
        FakeSource::new("a"),
        FakeSource::new("b"),
        FakeSource::new("c"),
    );
    let store = new_store(sources![a, b, c], "UTC");
    store.refresh(now(), 60.0, None, None).await;
    a.set_events(Ok(vec![ev("changed").build()]));
    let snapshot = store
        .refresh(now(), 60.0, Some(&id_set(&["a"])), None)
        .await;
    assert_eq!(ids(&snapshot.events), ["changed"]);
    assert_eq!(a.requested_intervals().len(), 2);
    assert_eq!(b.requested_intervals().len(), 1);
    assert_eq!(c.requested_intervals().len(), 1);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn healthy_source_publishes_while_another_source_is_stalled() {
    let a = GateSource::new("a", vec![ev("slow").build()]);
    let b = FakeSource::new("b");
    b.set_events(Ok(vec![
        ev("fast")
            .title("Fast")
            .start("2026-09-18T12:00:00Z")
            .build(),
    ]));
    let store = Arc::new(new_store(sources![a, b], "UTC"));
    let recorder = Arc::new(SnapshotRecorder::default());
    let refreshing = tokio::spawn({
        let (store, recorder) = (store.clone(), recorder.clone());
        async move {
            let on_publication = |snapshot: CalendarSnapshot| {
                let recorder = recorder.clone();
                let future: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> =
                    Box::pin(async move { recorder.record(snapshot) });
                future
            };
            store
                .refresh(now(), 60.0, None, Some(&on_publication))
                .await
        }
    });
    let mut observed = false;
    for _ in 0..500 {
        if recorder.contains("fast") {
            observed = true;
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(observed);
    a.release();
    let final_snapshot = refreshing.await.unwrap();
    assert_eq!(
        final_snapshot
            .events
            .iter()
            .map(|e| e.source_event_id().to_string())
            .collect::<BTreeSet<_>>(),
        id_set(&["slow", "fast"])
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn older_targeted_request_cannot_overwrite_newer_source_data() {
    let source = OrderedSource::new();
    let store = Arc::new(new_store(sources![source], "UTC"));
    let only = id_set(&["ordered"]);
    let old = tokio::spawn({
        let (store, only) = (store.clone(), only.clone());
        async move { store.refresh(now(), 60.0, Some(&only), None).await }
    });
    source.wait_for_request(1).await;
    let new = tokio::spawn({
        let (store, only) = (store.clone(), only.clone());
        async move { store.refresh(now(), 60.0, Some(&only), None).await }
    });
    source.wait_for_request(2).await;
    source.finish(1, vec![ev("new").title("New").build()]);
    assert_eq!(ids(&new.await.unwrap().events), ["new"]);
    source.finish(0, vec![ev("old").title("Old").build()]);
    old.await.unwrap();
    assert_eq!(
        ids(&store.set_inference_enabled(false, now()).events),
        ["new"]
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn older_refresh_cannot_overwrite_newer_refresh_or_window() {
    let source = OrderedSource::new();
    let store = Arc::new(new_store(sources![source], "UTC"));
    let old = tokio::spawn({
        let store = store.clone();
        async move {
            store
                .refresh(date("2026-09-18T23:59:00Z"), 60.0, None, None)
                .await
        }
    });
    source.wait_for_request(1).await;
    let new = tokio::spawn({
        let store = store.clone();
        async move {
            store
                .refresh(date("2026-09-19T00:01:00Z"), 60.0, None, None)
                .await
        }
    });
    source.wait_for_request(2).await;
    source.finish(
        1,
        vec![ev("new").title("NEW").start("2026-09-19T12:00:00Z").build()],
    );
    let titles = |events: &[TimeTugCalendarEvent]| -> Vec<String> {
        events.iter().map(|e| e.title().to_string()).collect()
    };
    assert_eq!(titles(&new.await.unwrap().events), ["NEW"]);
    source.finish(
        0,
        vec![ev("old").title("OLD").start("2026-09-18T12:00:00Z").build()],
    );
    old.await.unwrap();
    let final_snapshot = store.set_inference_enabled(false, date("2026-09-19T00:01:00Z"));
    assert_eq!(titles(&final_snapshot.events), ["NEW"]);
    assert_eq!(
        store.fetch_window(date("2026-09-19T00:01:00Z"), 60.0).start,
        date("2026-09-19T00:00:00Z")
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn timezone_change_during_refresh_keeps_new_dst_day_and_far_zone_all_day() {
    let source = OrderedSource::new();
    let store = Arc::new(new_store(sources![source], "UTC"));
    let instant = date("2026-03-08T10:00:00Z");
    let old = tokio::spawn({
        let store = store.clone();
        async move { store.refresh(instant, 60.0, None, None).await }
    });
    source.wait_for_request(1).await;
    store.set_zone(zone("America/Denver"));
    let current = tokio::spawn({
        let store = store.clone();
        async move { store.refresh(instant, 60.0, None, None).await }
    });
    source.wait_for_request(2).await;
    let far_zone_day = make_all_day("Pacific/Midway", day(2026, 3, 8), day(2026, 3, 9));
    source.finish(
        1,
        vec![
            ev("new").start("2026-03-08T12:00:00Z").build(),
            far_zone_day,
        ],
    );
    let accepted = current.await.unwrap();
    let id_set_of = |events: &[TimeTugCalendarEvent]| -> BTreeSet<String> {
        events
            .iter()
            .map(|e| e.source_event_id().to_string())
            .collect()
    };
    assert_eq!(id_set_of(&accepted.events), id_set(&["new", "d"]));
    source.finish(0, vec![ev("old").start("2026-03-08T02:00:00Z").build()]);
    old.await.unwrap();
    let final_snapshot = store.set_inference_enabled(false, instant);
    assert_eq!(id_set_of(&final_snapshot.events), id_set(&["new", "d"]));
    assert_eq!(
        store.fetch_window(instant, 60.0).start,
        date("2026-03-08T07:00:00Z")
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn set_sources_drops_removed_source_state() {
    let (a, b) = (FakeSource::new("a"), FakeSource::new("b"));
    a.set_events(Ok(vec![ev("1").build()]));
    b.set_events(Ok(vec![
        ev("2").title("Other").start("2026-09-18T12:00:00Z").build(),
    ]));
    let store = new_store(sources![a, b], "UTC");
    store.refresh(now(), 60.0, None, None).await;
    store.set_sources(sources![a]);
    // set_inference_enabled returns a snapshot built from cached state without refreshing.
    let snapshot = store.set_inference_enabled(false, now());
    assert_eq!(ids(&snapshot.events), ["1"]);
    assert_eq!(
        snapshot.statuses.keys().cloned().collect::<BTreeSet<_>>(),
        id_set(&["a"])
    );
    assert_eq!(
        snapshot
            .source_names
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        id_set(&["a"])
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn set_sources_adds_a_source_for_the_next_refresh() {
    let (a, b) = (FakeSource::new("a"), FakeSource::new("b"));
    b.set_events(Ok(vec![
        ev("2").title("Other").start("2026-09-18T12:00:00Z").build(),
    ]));
    let store = new_store(sources![a], "UTC");
    store.set_sources(sources![a, b]);
    let snapshot = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(ids(&snapshot.events), ["2"]);
    assert_eq!(
        snapshot.statuses.keys().cloned().collect::<BTreeSet<_>>(),
        id_set(&["a", "b"])
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn refresh_in_flight_during_set_sources_does_not_resurrect_a_removed_source() {
    let gate = GateSource::new("eventkit", vec![ev("1").build()]);
    let store = Arc::new(new_store(sources![gate], "UTC"));
    let refreshing = tokio::spawn({
        let store = store.clone();
        async move { store.refresh(now(), 60.0, None, None).await }
    });
    for _ in 0..400 {
        if gate.started() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(gate.started());
    store.set_sources(vec![]);
    gate.release();
    let snapshot = refreshing.await.unwrap();
    assert!(snapshot.events.is_empty());
    assert!(snapshot.statuses.is_empty());
    // Toggled back on: nothing stale shows until its next refresh.
    store.set_sources(sources![gate]);
    assert!(store.set_inference_enabled(false, now()).events.is_empty());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_removed_then_re_added_source_shows_nothing_until_its_next_refresh() {
    let (a, b) = (FakeSource::new("a"), FakeSource::new("b"));
    b.set_events(Ok(vec![
        ev("2").title("Other").start("2026-09-18T12:00:00Z").build(),
    ]));
    let store = new_store(sources![a, b], "UTC");
    store.refresh(now(), 60.0, None, None).await;
    store.set_sources(sources![a]);
    store.set_sources(sources![a, b]);
    let snapshot = store.set_inference_enabled(false, now());
    assert!(snapshot.events.is_empty());
    assert!(!snapshot.statuses.contains_key("b"));
}

// Review Focus 4 (phase 2 plan; Rust only): a source that fails keeps its last good events, also when it fails twice in
// a row, and the sources around it are unaffected.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_middle_of_three_sources_failing_twice_keeps_its_last_good_events() {
    let (first, middle, last) = (
        FakeSource::new("first"),
        FakeSource::new("middle"),
        FakeSource::new("last"),
    );
    let mut e1 = ev("e-first")
        .title("One")
        .start("2026-09-18T11:00:00Z")
        .build();
    e1.source_id = "first".to_string();
    let mut e2 = ev("e-middle")
        .title("Two")
        .start("2026-09-18T12:00:00Z")
        .build();
    e2.source_id = "middle".to_string();
    let mut e3 = ev("e-last")
        .title("Three")
        .start("2026-09-18T13:00:00Z")
        .build();
    e3.source_id = "last".to_string();
    first.set_events(Ok(vec![e1]));
    middle.set_events(Ok(vec![e2]));
    last.set_events(Ok(vec![e3]));
    let store = new_store(sources![first, middle, last], "UTC");
    let all = ["e-first", "e-middle", "e-last"];

    let healthy = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(ids(&healthy.events), all);

    middle.set_events(Err(Failure::Host("offline".to_string())));
    for attempt in 1..=2 {
        let snapshot = store.refresh(now(), 60.0, None, None).await;
        assert_eq!(ids(&snapshot.events), all, "failure {attempt}: events kept");
        assert!(
            matches!(
                snapshot.statuses.get("middle"),
                Some(SourceStatus::Failing(_))
            ),
            "failure {attempt}: the middle source reports failing"
        );
        assert_eq!(snapshot.statuses.get("first"), Some(&SourceStatus::Ok));
        assert_eq!(snapshot.statuses.get("last"), Some(&SourceStatus::Ok));
    }

    // An actionable failure on the third attempt still keeps them, and the status changes with it.
    middle.set_events(Err(SourceError::AuthExpired.into()));
    let expired = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(ids(&expired.events), all);
    assert_eq!(
        expired.statuses.get("middle"),
        Some(&SourceStatus::AuthExpired)
    );

    // It recovers with its next good answer.
    middle.set_events(Ok(vec![]));
    let recovered = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(ids(&recovered.events), ["e-first", "e-last"]);
    assert_eq!(recovered.statuses.get("middle"), Some(&SourceStatus::Ok));
}

// Rust-only. In the Swift tests the per-source request revision alone rejects the stale result (a removed source loses
// its revision; a newer request raises it). These two keep that revision untouched: `set_sources` with the same source
// moves the generation and the refresh revision (Swift bumps both, so either one rejects the result), and `set_zone`
// moves only the refresh revision, which pins that guard on its own.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn replacing_the_source_set_with_the_same_source_still_discards_an_in_flight_refresh() {
    let gate = GateSource::new("eventkit", vec![ev("1").build()]);
    let store = Arc::new(new_store(sources![gate], "UTC"));
    let refreshing = tokio::spawn({
        let store = store.clone();
        async move { store.refresh(now(), 60.0, None, None).await }
    });
    while !gate.started() {
        tokio::task::yield_now().await;
    }
    store.set_sources(sources![gate]); // same id: its request revision survives, the generation moves
    gate.release();
    refreshing.await.unwrap();
    assert!(store.set_inference_enabled(false, now()).events.is_empty());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn changing_the_zone_discards_an_in_flight_refresh_even_without_a_newer_one() {
    let gate = GateSource::new("eventkit", vec![ev("1").build()]);
    let store = Arc::new(new_store(sources![gate], "UTC"));
    let refreshing = tokio::spawn({
        let store = store.clone();
        async move { store.refresh(now(), 60.0, None, None).await }
    });
    while !gate.started() {
        tokio::task::yield_now().await;
    }
    store.set_zone(zone("America/Denver")); // the request revision is untouched, the refresh revision moves
    gate.release();
    refreshing.await.unwrap();
    assert!(store.set_inference_enabled(false, now()).events.is_empty());
}
