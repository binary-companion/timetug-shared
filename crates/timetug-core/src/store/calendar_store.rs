//! Port of `Store/CalendarStore.swift`.
//!
//! `CalendarStore` is a Swift actor; here it is a struct over a `std::sync::Mutex<State>` (conventions section 4).
//! Every Swift method body that has no `await` is one lock hold. In `refresh` and `resolve_pending` the lock is
//! released at each Swift `await` and the state is read again after it, exactly where Swift reads it again: a
//! generation, revision or inference check after the suspension looks at the current state, never at a copy taken
//! before. No lock is held across an `.await` (the compiler enforces it: a `MutexGuard` is not `Send`).
//!
//! Differences from Swift, all forced by Core having no clock and no environment:
//! - `calendar: Calendar` is `zone: Zone` (`set_calendar` is `set_zone`), and the `.current` default is gone.
//! - `passClock` has no default (`Date.init` reads the clock): the constructor takes it.
//! - Methods whose Swift body has no `await` are plain functions; Swift made them `async` only because actors are
//!   isolated.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use calendar_core::all_day::AllDay;
use calendar_core::civil::{self, Unit};
use calendar_core::failure::Outcome;
use calendar_core::source_types::SourceNotice;
use calendar_core::time::{DateInterval, Now, Zone, add_seconds, seconds_between};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::dedup::adjudication::{
    AdjudicationRequest, AdjudicatorAvailability, DuplicateAdjudicator, EngineInfo, VerdictCache,
};
use crate::dedup::lesson_book::{LessonBook, LessonDecision};
use crate::dedup::resolver::DuplicateResolver;
use crate::model::calendar_info::CalendarInfo;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::merge_types::MergedMember;
use crate::sources::{CalendarSource, SourceError, SourceStatus};
use crate::store::join_all::{BoxFuture, Completions};
use crate::widget::snapshot::WidgetSnapshot;

#[derive(Clone, Debug, PartialEq)]
pub struct CalendarSnapshot {
    pub events: Vec<TimeTugCalendarEvent>,
    /// Deduplicated events spanning today and the following two local days, for widgets only.
    pub widget_events: Vec<TimeTugCalendarEvent>,
    pub calendars: Vec<CalendarInfo>,
    /// Keyed by `CalendarSource.id`.
    pub statuses: BTreeMap<String, SourceStatus>,
    /// `CalendarSource.id` -> `displayName`, for front ends that show source problems.
    pub source_names: BTreeMap<String, String>,
    pub fetched_at: Timestamp,
    /// Increases only when the store accepts a new publication, including a local settings/lesson change.
    pub revision: u64,
    /// Quiet hints per source (a warning icon), keyed by `CalendarSource.id`; a source with none has no entry.
    pub notices: BTreeMap<String, Vec<SourceNotice>>,
    /// Look-alike events kept separate (event id -> other events), for a manual "Merge".
    pub candidates: BTreeMap<String, Vec<TimeTugCalendarEvent>>,
}

impl CalendarSnapshot {
    /// Swift's `init` with `candidates`, `revision`, `widgetEvents` and `notices` at their defaults (empty, 0, empty,
    /// empty); assign the public fields for the rest.
    pub fn new(
        events: Vec<TimeTugCalendarEvent>,
        calendars: Vec<CalendarInfo>,
        statuses: BTreeMap<String, SourceStatus>,
        source_names: BTreeMap<String, String>,
        fetched_at: Timestamp,
    ) -> CalendarSnapshot {
        CalendarSnapshot {
            events,
            widget_events: Vec::new(),
            calendars,
            statuses,
            source_names,
            fetched_at,
            revision: 0,
            notices: BTreeMap::new(),
            candidates: BTreeMap::new(),
        }
    }

    /// Swift's `static let empty`; `.distantPast` is `Timestamp::MIN` (as in the resolver).
    pub fn empty() -> CalendarSnapshot {
        CalendarSnapshot::new(
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            Timestamp::MIN,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InferenceStatus {
    Disabled,
    NoEngine,
    Unavailable { reason: String },
    NotOnDevice,
    Active(EngineInfo),
}

/// What the app persists between launches so decisions and verdicts survive a relaunch.
///
/// Persisted (family A): `{"lessons": ..., "verdicts": ...}`; both keys are required, as in Swift's synthesized
/// decoding.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DedupState {
    pub lessons: LessonBook,
    pub verdicts: VerdictCache,
}

impl DedupState {
    pub fn new(lessons: LessonBook, verdicts: VerdictCache) -> DedupState {
        DedupState { lessons, verdicts }
    }
}

/// Swift's `onPublication: (@Sendable (CalendarSnapshot) async -> Void)?`.
pub type OnPublication<'a> =
    &'a (dyn Fn(CalendarSnapshot) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync);

/// What one source's fetch brings back: calendars, events and notices.
type Fetched = Outcome<(
    Vec<CalendarInfo>,
    Vec<TimeTugCalendarEvent>,
    Vec<SourceNotice>,
)>;

/// What the first turn of `refresh` decided.
enum Prepared {
    /// No source to read: the snapshot of the cached state.
    Nothing(CalendarSnapshot),
    Fetch {
        started_generation: u64,
        started_refresh: u64,
        query_window: DateInterval,
        /// (source, its id, its request revision)
        requests: Vec<(Arc<dyn CalendarSource>, String, u64)>,
    },
}

/// `Calendar.date(byAdding: .day, ...)!`: Swift traps outside the range of a `Date`; 24 hours per day instead (the
/// widget snapshot does the same).
fn add_days(days: i64, to: Timestamp, zone: &Zone) -> Timestamp {
    civil::add(Unit::Day, days, to, zone).unwrap_or_else(|| add_seconds(to, 86_400.0 * days as f64))
}

/// Everything the Swift actor keeps in stored properties (except the constants `adjudicator` and `passClock`'s
/// owner, which sit beside it).
struct State {
    sources: Vec<Arc<dyn CalendarSource>>,
    generation: u64,
    refresh_revision: u64,
    source_request_revisions: BTreeMap<String, u64>,
    requested_query_window: Option<DateInterval>,
    publication_revision: u64,
    latest_snapshot: CalendarSnapshot,
    zone: Zone,
    last_events: BTreeMap<String, Vec<TimeTugCalendarEvent>>,
    last_calendars: BTreeMap<String, Vec<CalendarInfo>>,
    statuses: BTreeMap<String, SourceStatus>,
    last_notices: BTreeMap<String, Vec<SourceNotice>>,
    adjudicator: Option<Arc<dyn DuplicateAdjudicator>>,
    inference_enabled: bool,
    lessons: LessonBook,
    verdicts: VerdictCache,
    pending: Vec<AdjudicationRequest>,
    inference_generation: u64,
    /// Request id -> (attempts, retry after).
    failures: BTreeMap<String, (i64, Timestamp)>,
    last_window: Option<DateInterval>,
    last_widget_window: Option<DateInterval>,
    is_resolving: bool,
}

impl State {
    fn fetch_window(&self, now: Timestamp, lead_time: f64) -> DateInterval {
        let day_start = civil::start_of_day(now, &self.zone);
        let next_day_start = add_days(1, day_start, &self.zone);
        DateInterval::new(
            day_start,
            add_seconds(next_day_start, lead_time + CalendarStore::FETCH_BUFFER),
        )
    }

    fn active_engine(&self) -> Option<EngineInfo> {
        if !self.inference_enabled {
            return None;
        }
        match self.adjudicator.as_ref()?.availability() {
            AdjudicatorAvailability::Available(engine) if engine.is_on_device => Some(engine),
            _ => None,
        }
    }

    fn invalidate_inference(&mut self) {
        self.inference_generation = self.inference_generation.wrapping_add(1);
        self.failures.clear();
    }

    fn make_snapshot(&mut self, now: Timestamp) -> CalendarSnapshot {
        self.publication_revision = self.publication_revision.wrapping_add(1);
        let window = self.last_window.unwrap_or(DateInterval::new(now, now)); // DateInterval(start: now, duration: 0)
        let source_ids: Vec<String> = self.sources.iter().map(|s| s.id()).collect();
        let ids: BTreeSet<&String> = source_ids.iter().collect();
        let calendars: Vec<CalendarInfo> = source_ids
            .iter()
            .flat_map(|id| self.last_calendars.get(id).cloned().unwrap_or_default())
            .collect();
        let widget_window = self.last_widget_window.unwrap_or(window);
        let first_date = AllDay::date_of(widget_window.start, &self.zone);
        let last_date = AllDay::date_of(add_seconds(widget_window.end, -1.0), &self.zone);
        let mut raw: Vec<TimeTugCalendarEvent> = source_ids
            .iter()
            .flat_map(|id| self.last_events.get(id).into_iter().flatten())
            .filter(|event| {
                if let Some((first, end_exclusive)) = event.all_day_dates() {
                    return end_exclusive > first_date && first <= last_date;
                }
                event.end() > widget_window.start && event.start() < widget_window.end
            })
            .cloned()
            .collect();
        // `uniquingKeysWith: { first, _ in first }`
        let mut info_by_key: BTreeMap<String, &CalendarInfo> = BTreeMap::new();
        for info in &calendars {
            info_by_key.entry(info.key()).or_insert(info);
        }
        for event in &mut raw {
            let info = info_by_key.get(&event.calendar_key());
            event.calendar_service = info
                .and_then(|i| i.service.as_ref())
                .map(|s| s.raw_value().to_string());
            event.calendar_provider = info
                .and_then(|i| i.provider.as_ref())
                .map(|p| p.raw_value().to_string());
        }
        let current_first_date = AllDay::date_of(window.start, &self.zone);
        let current_last_date = AllDay::date_of(add_seconds(window.end, -1.0), &self.zone);
        let current: Vec<TimeTugCalendarEvent> = raw
            .iter()
            .filter(|event| {
                if let Some((first, end_exclusive)) = event.all_day_dates() {
                    return end_exclusive > current_first_date && first <= current_last_date;
                }
                event.end() > window.start && event.start() < window.end
            })
            .cloned()
            .collect();
        let engine = self.active_engine();
        let engine_id = engine.as_ref().map_or("unknown", |e| e.id.as_str());
        let verdicts = engine.as_ref().map(|_| &self.verdicts);
        let resolution = DuplicateResolver::resolve_with_engine(
            &current,
            &calendars,
            &self.lessons,
            verdicts,
            engine_id,
        );
        let widget_resolution = DuplicateResolver::resolve_with_engine(
            &raw,
            &calendars,
            &self.lessons,
            verdicts,
            engine_id,
        );
        self.lessons.touch(&resolution.used_lesson_keys, now);
        self.lessons.prune(now);
        self.verdicts.prune(now);
        let new_ids: Vec<&String> = resolution.pending.iter().map(|r| &r.id).collect();
        if self
            .pending
            .iter()
            .map(|r| &r.id)
            .ne(new_ids.iter().copied())
        {
            self.inference_generation = self.inference_generation.wrapping_add(1);
            self.failures
                .retain(|key, _| resolution.pending.iter().any(|r| &r.id == key));
        }
        self.pending = resolution.pending;
        // `Dictionary(uniqueKeysWithValues:)` traps on a repeated id in Swift; the first one wins here.
        let mut source_names: BTreeMap<String, String> = BTreeMap::new();
        for source in &self.sources {
            source_names
                .entry(source.id())
                .or_insert_with(|| source.display_name());
        }
        let mut snapshot = CalendarSnapshot::new(
            resolution.events,
            calendars,
            self.statuses
                .iter()
                .filter(|(key, _)| ids.contains(key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            source_names,
            now,
        );
        snapshot.candidates = resolution.candidates;
        snapshot.revision = self.publication_revision;
        snapshot.widget_events = widget_resolution.events;
        snapshot.notices = self
            .last_notices
            .iter()
            .filter(|(key, value)| ids.contains(key) && !value.is_empty())
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        self.latest_snapshot = snapshot.clone();
        snapshot
    }
}

/// Clears `is_resolving` when a `resolve_pending` pass is dropped mid-flight (Swift's `defer`, also run when the task
/// is cancelled). A pass that runs to its end clears the flag itself, under the same lock hold as its last state
/// change, and disarms this.
struct ResolvingGuard<'a> {
    store: &'a CalendarStore,
    armed: bool,
}

impl Drop for ResolvingGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.store.lock().is_resolving = false;
        }
    }
}

/// Merges events from all sources over the fetch window. A failing source keeps its last good
/// events so a network blip never causes a missed meeting.
pub struct CalendarStore {
    state: Mutex<State>,
    pass_clock: Now,
}

impl CalendarStore {
    /// Extra time past the lead time so events just after midnight are already loaded.
    pub const FETCH_BUFFER: f64 = 300.0;
    /// The widest gap between two zones, so an all-day event on one of today's dates in a far zone is still fetched.
    pub const SOURCE_QUERY_MARGIN: f64 = 26.0 * 3600.0;

    const MAX_PENDING_PER_PASS: usize = 20;

    pub fn new(
        sources: Vec<Arc<dyn CalendarSource>>,
        zone: Zone,
        adjudicator: Option<Arc<dyn DuplicateAdjudicator>>,
        pass_clock: Now,
    ) -> CalendarStore {
        CalendarStore {
            state: Mutex::new(State {
                sources,
                generation: 0,
                refresh_revision: 0,
                source_request_revisions: BTreeMap::new(),
                requested_query_window: None,
                publication_revision: 0,
                latest_snapshot: CalendarSnapshot::empty(),
                zone,
                last_events: BTreeMap::new(),
                last_calendars: BTreeMap::new(),
                statuses: BTreeMap::new(),
                last_notices: BTreeMap::new(),
                adjudicator,
                inference_enabled: false,
                lessons: LessonBook::default(),
                verdicts: VerdictCache::default(),
                pending: Vec::new(),
                inference_generation: 0,
                failures: BTreeMap::new(),
                last_window: None,
                last_widget_window: None,
                is_resolving: false,
            }),
            pass_clock,
        }
    }

    /// One lock hold is one Swift actor turn. A poisoned lock only means another thread panicked; the state is still
    /// consistent between turns, so it is used.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Replaces the source set. Removed sources lose their cached events, calendars and status. Bumps the
    /// generation so a `refresh` that was suspended when this ran discards its results instead of restoring them.
    pub fn set_sources(&self, new_sources: Vec<Arc<dyn CalendarSource>>) {
        let mut state = self.lock();
        let keep: BTreeSet<String> = new_sources.iter().map(|s| s.id()).collect();
        state.last_events.retain(|key, _| keep.contains(key));
        state.last_calendars.retain(|key, _| keep.contains(key));
        state.statuses.retain(|key, _| keep.contains(key));
        state
            .source_request_revisions
            .retain(|key, _| keep.contains(key));
        state.last_notices.retain(|key, _| keep.contains(key));
        state.sources = new_sources;
        state.generation = state.generation.wrapping_add(1);
        state.invalidate_inference();
        state.refresh_revision = state.refresh_revision.wrapping_add(1);
        if state.last_window.is_some() {
            let fetched_at = state.latest_snapshot.fetched_at;
            state.make_snapshot(fetched_at);
        }
    }

    /// Changes the zone used for day windows and all-day filtering (Swift's `setCalendar`, which took a `Calendar`).
    /// Any refresh started in the previous context must discard its results.
    pub fn set_zone(&self, zone: Zone) {
        let mut state = self.lock();
        state.zone = zone;
        state.refresh_revision = state.refresh_revision.wrapping_add(1);
        state.invalidate_inference();
    }

    /// Local midnight today through next local midnight + lead time + buffer.
    pub fn fetch_window(&self, now: Timestamp, lead_time: f64) -> DateInterval {
        self.lock().fetch_window(now, lead_time)
    }

    /// Refreshes only named sources when the fetch window is unchanged. A new day/window forces a full read.
    ///
    /// Sources are read concurrently on this task; each result is applied, and published through `on_publication`,
    /// as it arrives, unless the store's sources, zone, window or that source's request have moved on in the
    /// meantime (checked against the state at that moment).
    pub async fn refresh(
        &self,
        now: Timestamp,
        lead_time: f64,
        source_ids: Option<&BTreeSet<String>>,
        on_publication: Option<OnPublication<'_>>,
    ) -> CalendarSnapshot {
        // Everything up to the first `await` is one actor turn.
        let prepared = {
            let mut state = self.lock();
            let window = state.fetch_window(now, lead_time);
            let widget_end = add_days(WidgetSnapshot::HORIZON_DAYS, window.start, &state.zone);
            let widget_window = DateInterval::new(window.start, widget_end);
            let query_window = DateInterval::new(
                add_seconds(window.start, -CalendarStore::SOURCE_QUERY_MARGIN),
                add_seconds(
                    window.end.max(widget_window.end),
                    CalendarStore::SOURCE_QUERY_MARGIN,
                ),
            );
            let started_generation = state.generation;
            let context_changed = state.requested_query_window != Some(query_window);
            if context_changed {
                state.refresh_revision = state.refresh_revision.wrapping_add(1);
                state.requested_query_window = Some(query_window);
            }
            let started_refresh = state.refresh_revision;
            let current: Vec<Arc<dyn CalendarSource>> = state
                .sources
                .iter()
                .filter(|source| {
                    context_changed || source_ids.is_none_or(|ids| ids.contains(&source.id()))
                })
                .cloned()
                .collect();
            state.last_window = Some(window);
            state.last_widget_window = Some(widget_window);
            if current.is_empty() {
                Prepared::Nothing(state.make_snapshot(now))
            } else {
                let mut requests = Vec::with_capacity(current.len());
                for source in current {
                    let source_id = source.id();
                    let revision = state
                        .source_request_revisions
                        .get(&source_id)
                        .copied()
                        .unwrap_or(0)
                        .wrapping_add(1);
                    state
                        .source_request_revisions
                        .insert(source_id.clone(), revision);
                    requests.push((source, source_id, revision));
                }
                Prepared::Fetch {
                    started_generation,
                    started_refresh,
                    query_window,
                    requests,
                }
            }
        };
        let (started_generation, started_refresh, query_window, requests) = match prepared {
            Prepared::Nothing(snapshot) => {
                if let Some(on_publication) = on_publication {
                    on_publication(snapshot.clone()).await;
                }
                return snapshot;
            }
            Prepared::Fetch {
                started_generation,
                started_refresh,
                query_window,
                requests,
            } => (started_generation, started_refresh, query_window, requests),
        };

        let fetches: Vec<BoxFuture<(String, u64, Fetched)>> = requests
            .into_iter()
            .map(|(source, source_id, revision)| {
                let fetch: BoxFuture<(String, u64, Fetched)> = Box::pin(async move {
                    let result: Fetched = async {
                        let calendars = source.calendars().await?;
                        let events = source.events_in(query_window).await?;
                        Ok((calendars, events, source.notices().await))
                    }
                    .await;
                    (source_id, revision, result)
                });
                fetch
            })
            .collect();
        let mut group = Completions::new(fetches);
        while let Some((source_id, revision, result)) = group.next().await {
            // Each result is one actor turn: the checks read the state as it is now.
            let snapshot = {
                let mut state = self.lock();
                if state.generation != started_generation
                    || state.refresh_revision != started_refresh
                    || state.source_request_revisions.get(&source_id) != Some(&revision)
                {
                    continue;
                }
                match result {
                    Ok((calendars, events, notices)) => {
                        state.last_calendars.insert(source_id.clone(), calendars);
                        state.last_events.insert(source_id.clone(), events);
                        state.last_notices.insert(source_id.clone(), notices);
                        state.statuses.insert(source_id, SourceStatus::Ok);
                    }
                    Err(failure) => {
                        state.last_notices.remove(&source_id);
                        let status = match SourceError::from_failure(&failure) {
                            Some(SourceError::NeedsPermission) => SourceStatus::NeedsPermission,
                            Some(SourceError::AuthExpired) => SourceStatus::AuthExpired,
                            None => SourceStatus::Failing(failure.to_string()),
                        };
                        state.statuses.insert(source_id, status);
                    }
                }
                state.make_snapshot(now)
            };
            if let Some(on_publication) = on_publication {
                on_publication(snapshot).await;
            }
        }
        self.lock().latest_snapshot.clone()
    }

    pub fn inference_status(&self) -> InferenceStatus {
        let state = self.lock();
        if !state.inference_enabled {
            return InferenceStatus::Disabled;
        }
        let Some(adjudicator) = &state.adjudicator else {
            return InferenceStatus::NoEngine;
        };
        match adjudicator.availability() {
            AdjudicatorAvailability::Unavailable { reason } => {
                InferenceStatus::Unavailable { reason }
            }
            AdjudicatorAvailability::Available(engine) => {
                if engine.is_on_device {
                    InferenceStatus::Active(engine)
                } else {
                    InferenceStatus::NotOnDevice
                }
            }
        }
    }

    pub fn set_inference_enabled(&self, enabled: bool, now: Timestamp) -> CalendarSnapshot {
        let mut state = self.lock();
        if state.inference_enabled != enabled {
            state.invalidate_inference();
        }
        state.inference_enabled = enabled;
        state.make_snapshot(now)
    }

    /// Asks the engine about the pairs still waiting for a verdict (one bounded pass). Returns a new
    /// snapshot only when a verdict was recorded; call again until `None` to drain a backlog.
    pub async fn resolve_pending(&self, now: Timestamp) -> Option<CalendarSnapshot> {
        // First turn: claim the pass and choose the batch.
        let (adjudicator, engine, batch, started_generation) = {
            let mut state = self.lock();
            let adjudicator = state.adjudicator.clone()?;
            let engine = state.active_engine()?;
            if state.pending.is_empty() || state.is_resolving {
                return None;
            }
            state.is_resolving = true;
            let batch: Vec<AdjudicationRequest> = state
                .pending
                .iter()
                .filter(|request| {
                    now >= state
                        .failures
                        .get(&request.id)
                        .map_or(Timestamp::MIN, |failure| failure.1)
                })
                .take(CalendarStore::MAX_PENDING_PER_PASS)
                .cloned()
                .collect();
            if batch.is_empty() {
                state.is_resolving = false;
                return None;
            }
            (adjudicator, engine, batch, state.inference_generation)
        };
        let mut guard = ResolvingGuard {
            store: self,
            armed: true,
        };
        let started_at = (self.pass_clock)();
        if seconds_between(started_at, (self.pass_clock)()) >= 30.0 {
            return None; // the guard clears `is_resolving`
        }
        let returned = adjudicator.judge(&batch).await;

        // Second turn, after the engine answered: everything is read again.
        let mut state = self.lock();
        let result = Self::record_verdicts(
            &mut state,
            &engine,
            &batch,
            &returned,
            started_generation,
            now,
        );
        state.is_resolving = false;
        guard.armed = false;
        result
    }

    /// The part of `resolvePending` after `judge` returns.
    fn record_verdicts(
        state: &mut State,
        engine: &EngineInfo,
        batch: &[AdjudicationRequest],
        returned: &[crate::dedup::adjudication::AdjudicationVerdict],
        started_generation: u64,
        now: Timestamp,
    ) -> Option<CalendarSnapshot> {
        // Swift also checks `!Task.isCancelled`; a cancelled Rust future is dropped and never gets here.
        if state.inference_generation != started_generation
            || state.active_engine().map(|e| e.id) != Some(engine.id.clone())
        {
            return None;
        }
        // `Dictionary(_, uniquingKeysWith: { first, _ in first })`
        let mut by_id: BTreeMap<&str, &AdjudicationRequest> = BTreeMap::new();
        for request in batch {
            by_id.entry(request.id.as_str()).or_insert(request);
        }
        let returned_ids: BTreeSet<&str> = returned.iter().map(|v| v.request_id.as_str()).collect();
        for request in batch
            .iter()
            .filter(|request| !returned_ids.contains(request.id.as_str()))
        {
            let attempts = (state.failures.get(&request.id).map_or(0, |f| f.0) + 1).min(5);
            let delay = (60 * (1_i64 << (attempts - 1))).min(900);
            state.failures.insert(
                request.id.clone(),
                (attempts, add_seconds(now, delay as f64)),
            );
        }
        let mut changed = false;
        for verdict in returned {
            let Some(request) = by_id.get(verdict.request_id.as_str()) else {
                continue;
            };
            state.failures.remove(&verdict.request_id);
            state.verdicts.store(
                verdict,
                engine,
                request.first.end.max(request.second.end),
                now,
            );
            changed = true;
        }
        if !changed {
            return None;
        }
        state.verdicts.prune(now);
        Some(state.make_snapshot(now))
    }

    /// The user says this merged event is not one meeting: remember every pair of its participants.
    /// `LessonBook::record` skips same-calendar pairs unless they are exact duplicates.
    pub fn unmerge(&self, event: &TimeTugCalendarEvent, now: Timestamp) -> CalendarSnapshot {
        let mut state = self.lock();
        state.invalidate_inference();
        let parts = event.participants();
        for (index, a) in parts.iter().enumerate() {
            for b in &parts[index + 1..] {
                state.lessons.record(a, b, LessonDecision::Different, now);
            }
        }
        state.make_snapshot(now)
    }

    /// The user says these copies of a merged event are not the same meeting as the rest: remember a "different"
    /// lesson between each given copy and every other participant not in `members`. Pairs among the given
    /// copies are not recorded, so they stay together. (Swift: `separate(_:from:now:)`.)
    pub fn separate(
        &self,
        members: &[MergedMember],
        event: &TimeTugCalendarEvent,
        now: Timestamp,
    ) -> CalendarSnapshot {
        let mut state = self.lock();
        state.invalidate_inference();
        let key = |m: &MergedMember| format!("{}|{}", m.calendar_key, m.content_key);
        let chosen: BTreeSet<String> = members.iter().map(key).collect();
        let rest: Vec<MergedMember> = event
            .participants()
            .into_iter()
            .filter(|m| !chosen.contains(&key(m)))
            .collect();
        for member in members {
            for other in &rest {
                state
                    .lessons
                    .record(member, other, LessonDecision::Different, now);
            }
        }
        state.make_snapshot(now)
    }

    /// The user says these two displayed events are one meeting.
    pub fn merge(
        &self,
        a: &TimeTugCalendarEvent,
        b: &TimeTugCalendarEvent,
        now: Timestamp,
    ) -> CalendarSnapshot {
        let mut state = self.lock();
        state.invalidate_inference();
        for x in a.participants() {
            for y in b.participants() {
                state.lessons.record(&x, &y, LessonDecision::Same, now);
            }
        }
        state.make_snapshot(now)
    }

    /// Also clears cached model verdicts, so a pair the model merged is asked about again.
    pub fn forget_lessons(&self, now: Timestamp) -> CalendarSnapshot {
        let mut state = self.lock();
        state.invalidate_inference();
        state.lessons = LessonBook::default();
        state.verdicts = VerdictCache::default();
        state.make_snapshot(now)
    }

    pub fn state(&self) -> DedupState {
        let state = self.lock();
        DedupState::new(state.lessons.clone(), state.verdicts.clone())
    }

    pub fn load(&self, dedup_state: DedupState) {
        let mut state = self.lock();
        state.invalidate_inference();
        state.lessons = dedup_state.lessons;
        state.verdicts = dedup_state.verdicts;
        state.verdicts.discard_incompatible();
    }
}
