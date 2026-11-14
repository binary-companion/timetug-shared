//! Port of `Support/EngineHarness.swift` (without `CallGate` and `ScriptedSource`, which are in `scripted_source`).
//!
//! A real `Engine` over a temp data directory, a `ManualClock` at `EPOCH`, in-memory settings, captured diagnostics and
//! events, and a fake connector kind "fake" with one scripted source per stored account.
//!
//! Differences from Swift, forced by the language: the optional arguments of `EngineHarness.init` are the fields of
//! `HarnessOptions` (struct update syntax); `host.calendar` is `host.zone`, so `hostCalendar` is `host_zone` and the
//! UTC `Calendar` is `Zone::utc()`; `waitForState`, `waitUntil` and `settle` take synchronous conditions, because every
//! condition the tests use reads a lock or a counter (Swift needed `await` only to read an actor's property).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use calendar_core::connection::{AuthorizationMethod, Connection, ConnectorRegistry};
use calendar_core::connection::{CredentialStore, InMemoryCredentialStore};
use calendar_core::failure::{Failure, Outcome};
use calendar_core::file_connection_store::FileConnectionStore;
use calendar_core::model::CalendarEvent;
use calendar_core::source_types::{CalendarSource as LibrarySource, SourceError};
use calendar_core::time::{Zone, add_seconds};
use jiff::Timestamp;
use timetug_core::dedup::adjudication::EngineInfo;
use timetug_core::widget::snapshot::WidgetSnapshot;

use super::account_fakes::{FakeConnectorKind, FakeOutcome, FakeSignIn, Gate};
use super::scripted_source::ScriptedSource;
use super::{LockedBox, TempDir, poll};
use crate::clock::EngineClock;
use crate::diagnostic::EngineDiagnostic;
use crate::engine::Engine;
use crate::event::EngineEvent;
use crate::host::EngineHost;
use crate::inference::text_generator::{TextGenerator, TextGeneratorAvailability};
use crate::settings::{EngineSettings, InMemorySettingsStorage, SettingsStorage};
use crate::state::EngineState;

use super::manual_clock::ManualClock;

/// `CalendarCore.CalendarEvent.sample(id:startsIn:duration:)`: a 30-minute meeting on calendar "cal" starting
/// `starts_in` seconds after `EngineHarness::EPOCH`.
pub(crate) fn sample(id: &str, starts_in: f64) -> CalendarEvent {
    sample_for(id, starts_in, 1800.0)
}

pub(crate) fn sample_for(id: &str, starts_in: f64, duration: f64) -> CalendarEvent {
    let start = add_seconds(EngineHarness::epoch(), starts_in);
    CalendarEvent::new(
        id,
        "cal",
        format!("Meeting {id}"),
        start,
        add_seconds(start, duration),
    )
}

/// The optional arguments of `EngineHarness.init`. `events` go to account "c1"; `accounts` maps further connection ids
/// to their events. With `directory`, an existing data directory is reused and `accounts.json` is left as it is.
/// `source_ids` overrides a connection's source id (default "fake-<connection id>"); `clock_start` is where the clock
/// starts; `host_settings` replaces the in-memory settings the host hands the engine (`settings` is then unused).
#[derive(Default)]
pub(crate) struct HarnessOptions {
    pub(crate) events: Vec<CalendarEvent>,
    pub(crate) accounts: BTreeMap<String, Vec<CalendarEvent>>,
    pub(crate) failures: BTreeMap<String, SourceError>,
    pub(crate) directory: Option<PathBuf>,
    pub(crate) text_generator: Option<Arc<dyn TextGenerator>>,
    pub(crate) inference_enabled: bool,
    pub(crate) system_source: Option<Arc<dyn timetug_core::CalendarSource>>,
    pub(crate) source_ids: BTreeMap<String, String>,
    pub(crate) clock_start: Option<Timestamp>,
    pub(crate) host_settings: Option<Arc<dyn SettingsStorage>>,
    pub(crate) system_access_gate: Option<Arc<Gate>>,
    pub(crate) extra_kinds: Vec<Arc<FakeConnectorKind>>,
    pub(crate) unconfigured_kind_ids: Vec<String>,
}

type DrainCheck = Arc<dyn Fn() -> bool + Send + Sync>;

/// Runs with the 1-based number of each `generate` call.
pub(crate) type CallHook = Arc<dyn Fn(usize) + Send + Sync>;

pub(crate) struct EngineHarness {
    pub(crate) clock: Arc<ManualClock>,
    pub(crate) directory: PathBuf,
    _temp: Option<TempDir>,
    pub(crate) settings: Arc<InMemorySettingsStorage>,
    pub(crate) diagnostics: Arc<LockedBox<Vec<EngineDiagnostic>>>,
    pub(crate) emitted: Arc<LockedBox<Vec<EngineEvent>>>,
    /// Keyed by connection id.
    pub(crate) sources: BTreeMap<String, Arc<ScriptedSource>>,
    pub(crate) kind: Arc<FakeConnectorKind>,
    /// What `host.zone` returns; a test changes it to simulate a time zone change.
    pub(crate) host_zone: Arc<LockedBox<Zone>>,
    /// How many times the engine asked the host for system calendar access.
    pub(crate) system_access_requests: Arc<LockedBox<usize>>,
    pub(crate) engine: Arc<Engine>,
    /// Extra drain conditions (for example a `TimelineRecorder` that must have received every event).
    pub(crate) drain_checks: Arc<LockedBox<Vec<DrainCheck>>>,
}

impl EngineHarness {
    /// 2026-10-15 12:00:00 UTC.
    pub(crate) fn epoch() -> Timestamp {
        Timestamp::from_second(1_792_065_600).unwrap()
    }

    pub(crate) fn utc() -> Zone {
        Zone::utc()
    }

    pub(crate) async fn new(events: Vec<CalendarEvent>) -> EngineHarness {
        EngineHarness::with(HarnessOptions {
            events,
            ..HarnessOptions::default()
        })
        .await
    }

    pub(crate) async fn with(options: HarnessOptions) -> EngineHarness {
        let mut all = options.accounts;
        if !all.contains_key("c1") {
            all.insert("c1".to_string(), options.events);
        }
        let source_id = |id: &str| {
            options
                .source_ids
                .get(id)
                .cloned()
                .unwrap_or_else(|| format!("fake-{id}"))
        };
        let (temp, dir) = match &options.directory {
            Some(directory) => (None, directory.clone()),
            None => {
                let temp = TempDir::make();
                let path = temp.path().to_path_buf();
                (Some(temp), path)
            }
        };
        if options.directory.is_none() {
            let store = FileConnectionStore::new(dir.join("accounts.json"));
            for id in all.keys() {
                store
                    .add(&Connection::new("fake", id.clone(), format!("{id}@x.test")))
                    .await
                    .unwrap();
            }
        }
        let mut built: BTreeMap<String, Arc<ScriptedSource>> = BTreeMap::new();
        for (id, list) in &all {
            built.insert(
                id.clone(),
                Arc::new(ScriptedSource::new(
                    &source_id(id),
                    list.clone(),
                    options.failures.get(id).cloned(),
                )),
            );
        }
        let clock = Arc::new(ManualClock::new(
            options.clock_start.unwrap_or_else(EngineHarness::epoch),
        ));
        let mut initial = EngineSettings {
            system_calendar_enabled: false,
            ..EngineSettings::default()
        };
        initial.takeover.takeover_calendar_keys = all
            .keys()
            .map(|id| format!("{}/cal", source_id(id)))
            .collect::<BTreeSet<_>>();
        initial.inference_enabled = options.inference_enabled;
        let settings = Arc::new(InMemorySettingsStorage::new(initial));
        let diagnostics = Arc::new(LockedBox::new(Vec::new()));
        let emitted = Arc::new(LockedBox::new(Vec::new()));
        let kind = FakeConnectorKind::with(
            "fake",
            "Fake",
            AuthorizationMethod::Oauth,
            FakeOutcome::Succeed,
        );
        let scripted = built.clone();
        kind.set_source_factory(Arc::new(move |connection: &Connection| {
            match scripted.get(&connection.connection_id) {
                Some(source) => Ok(source.clone() as Arc<dyn LibrarySource>),
                None => Err(Failure::Source(SourceError::InvalidResponse(
                    "no scripted source".to_string(),
                ))),
            }
        }));
        let mut registry = ConnectorRegistry::new();
        registry.register(kind.clone());
        for extra in options.extra_kinds {
            registry.register(extra);
        }
        let sign_in = Arc::new(FakeSignIn::new(Vec::new()));
        let sink = diagnostics.clone();
        let host_zone = Arc::new(LockedBox::new(EngineHarness::utc()));
        let system_access_requests = Arc::new(LockedBox::new(0usize));
        let credentials: Arc<dyn CredentialStore> = Arc::new(InMemoryCredentialStore::new());
        let host_settings: Arc<dyn SettingsStorage> = options
            .host_settings
            .unwrap_or_else(|| settings.clone() as Arc<dyn SettingsStorage>);
        let mut host = EngineHost::new(
            dir.clone(),
            host_settings,
            credentials,
            sign_in.clone(),
            sign_in,
            registry,
        );
        host.unconfigured_kind_ids = options.unconfigured_kind_ids;
        if let Some(source) = options.system_source {
            host.build_system_source = Some(Arc::new(move || source.clone()));
            let requests = system_access_requests.clone();
            let gate = options.system_access_gate;
            host.request_system_access = Some(Arc::new(move || {
                let requests = requests.clone();
                let gate = gate.clone();
                Box::pin(async move {
                    requests.mutate(|count| *count += 1);
                    if let Some(gate) = gate {
                        gate.wait().await;
                    }
                })
            }));
        }
        host.text_generator = options.text_generator;
        host.diagnostics = Arc::new(move |event| sink.mutate(|list| list.push(event)));
        host.clock = clock.clone() as Arc<dyn EngineClock>;
        let zone = host_zone.clone();
        host.zone = Arc::new(move || zone.get());
        let engine = Engine::new(host);
        let mut stream = engine.events();
        let captured = emitted.clone();
        tokio::spawn(async move {
            while let Some(event) = stream.recv().await {
                captured.mutate(|list| list.push(event));
            }
        });
        EngineHarness {
            clock,
            directory: dir,
            _temp: temp,
            settings,
            diagnostics,
            emitted,
            sources: built,
            kind,
            host_zone,
            system_access_requests,
            engine,
            drain_checks: Arc::new(LockedBox::new(Vec::new())),
        }
    }

    pub(crate) fn source(&self, connection_id: &str) -> &Arc<ScriptedSource> {
        &self.sources[connection_id]
    }

    /// The source of account "c1".
    pub(crate) fn c1(&self) -> &Arc<ScriptedSource> {
        self.source("c1")
    }

    /// Polls `current_state()` until `predicate` holds; fails loudly (with the state) after a generous bound.
    pub(crate) async fn wait_for_state(
        &self,
        predicate: impl Fn(&EngineState) -> bool,
    ) -> EngineState {
        let last = LockedBox::new(self.engine.current_state());
        poll(
            || format!("state {}", self.dump()),
            30.0,
            || async {
                let state = self.engine.current_state();
                let holds = predicate(&state);
                last.set(state);
                holds
            },
        )
        .await;
        last.get()
    }

    /// Polls until `condition` holds; fails loudly (with the engine's state) after a generous bound.
    pub(crate) async fn wait_until(&self, what: &str, mut condition: impl FnMut() -> bool) {
        poll(
            || format!("{what} {}", self.dump()),
            30.0,
            || {
                let holds = condition();
                async move { holds }
            },
        )
        .await;
    }

    /// True when nothing is in flight: no command, refresh, resolution, account message or coordinator pass; every
    /// clock sleep that returned has been handled by the engine; every signal a source sent reached the coordinator;
    /// and every emitted event reached this harness (and each registered recorder).
    pub(crate) fn is_quiescent(&self) -> bool {
        let activity = self.engine.activity();
        let signals: usize = self.sources.values().map(|s| s.signals_sent()).sum();
        activity.is_idle()
            && self.clock.resumed_sleeps() as i64 == activity.wakes_handled()
            && signals as i64 == activity.signals_received()
            && self.emitted.mutate(|list| list.len()) as i64 == activity.events_emitted()
            && self.drain_checks.get().iter().all(|check| check())
    }

    /// Waits until the engine is quiescent (see `is_quiescent`) on three consecutive checks. Every wait for "nothing
    /// more happens" uses this, so it never depends on how many yields the scheduler needed.
    pub(crate) async fn settle(&self) {
        let mut stable = 0;
        self.wait_until("quiescence", || {
            stable = if self.is_quiescent() { stable + 1 } else { 0 };
            stable >= 3
        })
        .await;
    }

    /// The engine's bookkeeping and state, for failure messages.
    pub(crate) fn dump(&self) -> String {
        let state = self.engine.current_state();
        let signals: usize = self.sources.values().map(|s| s.signals_sent()).sum();
        format!(
            "[activity: {}; clock resumed={} pending={}; signals sent={}; events captured={}; revision={}; statuses={:?}; events={:?}]",
            self.engine.activity().summary(),
            self.clock.resumed_sleeps(),
            self.clock.pending_sleeps(),
            signals,
            self.emitted.mutate(|list| list.len()),
            state.revision,
            state.statuses,
            state
                .events
                .iter()
                .map(|e| e.source_event_id().to_string())
                .collect::<Vec<_>>(),
        )
    }

    pub(crate) fn widget_snapshots(&self) -> Vec<WidgetSnapshot> {
        self.emitted.mutate(|list| {
            list.iter()
                .filter_map(|event| match event {
                    EngineEvent::WidgetSnapshot(snapshot) => Some(snapshot.clone()),
                    _ => None,
                })
                .collect()
        })
    }

    pub(crate) fn dedup_file(&self) -> PathBuf {
        self.directory.join("dedup-state.json")
    }

    pub(crate) fn takeover_due_ids(&self) -> Vec<String> {
        self.emitted.mutate(|list| {
            list.iter()
                .filter_map(|event| match event {
                    EngineEvent::TakeoverDue { request, .. } => {
                        Some(request.event.source_event_id().to_string())
                    }
                    _ => None,
                })
                .collect()
        })
    }
}

/// A host text model for engine tests: answers every prompt with `answer` (or fails), counts calls and can run a hook
/// on each call.
pub(crate) struct FakeEngineGenerator {
    pub(crate) availability_value: LockedBox<TextGeneratorAvailability>,
    pub(crate) answer: LockedBox<Option<String>>,
    pub(crate) calls: LockedBox<usize>,
    /// Runs with the 1-based call number before the call answers.
    pub(crate) on_call: LockedBox<Option<CallHook>>,
    availability_reads: LockedBox<usize>,
    /// When set, the availability read with this 1-based number waits at the gate first.
    pub(crate) availability_gate: LockedBox<Option<(usize, Arc<Gate>)>>,
}

impl FakeEngineGenerator {
    pub(crate) fn on_device() -> EngineInfo {
        EngineInfo {
            id: "fake-model".to_string(),
            display_name: "Fake model".to_string(),
            is_on_device: true,
        }
    }

    /// Available on device, answering `answer` (`None` makes every call fail).
    pub(crate) fn new(answer: Option<&str>) -> Arc<FakeEngineGenerator> {
        FakeEngineGenerator::with(
            TextGeneratorAvailability::Available(FakeEngineGenerator::on_device()),
            answer,
        )
    }

    pub(crate) fn with(
        availability: TextGeneratorAvailability,
        answer: Option<&str>,
    ) -> Arc<FakeEngineGenerator> {
        Arc::new(FakeEngineGenerator {
            availability_value: LockedBox::new(availability),
            answer: LockedBox::new(answer.map(str::to_string)),
            calls: LockedBox::new(0),
            on_call: LockedBox::new(None),
            availability_reads: LockedBox::new(0),
            availability_gate: LockedBox::new(None),
        })
    }
}

#[async_trait]
impl TextGenerator for FakeEngineGenerator {
    async fn availability(&self) -> TextGeneratorAvailability {
        let read = self.availability_reads.mutate(|reads| {
            *reads += 1;
            *reads
        });
        if let Some((number, gate)) = self.availability_gate.get()
            && number == read
        {
            gate.wait().await;
        }
        self.availability_value.get()
    }

    async fn generate(&self, _instructions: &str, _prompt: &str) -> Outcome<String> {
        let number = self.calls.mutate(|calls| {
            *calls += 1;
            *calls
        });
        if let Some(hook) = self.on_call.get() {
            hook(number);
        }
        match self.answer.get() {
            Some(answer) => Ok(answer),
            None => Err(Failure::Host("FakeEngineGenerator.Failure".to_string())),
        }
    }
}
