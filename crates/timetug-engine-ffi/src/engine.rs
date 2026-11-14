//! The `Engine` object: what a Swift or Kotlin host creates, sends commands to and shuts down.
//!
//! Threads, as behind the C interface: one engine thread owns a `tokio` current-thread runtime and runs until shutdown; one
//! delivery thread calls the observer. The exported async methods never run on the host's executor: each hands its work to
//! the engine runtime and awaits a `oneshot`, so the host's executor only holds a waker (`docs/bindings-spike.md`).
//!
//! Commands: `send` converts the mirror, hands the command to its lane (commands in one lane reach the engine one after
//! another in send order; lanes do not wait for each other, so a sign-in that waits for a person never holds up a takeover)
//! and returns when the engine finished the command. `CancelAuthorization` acts at once, because the work it ends may be
//! what its lane is waiting on.
//!
//! Delivery: the first observer call after `start` is a state summary; summaries coalesce (newest wins), events are never
//! dropped; nothing is delivered after `shutdown()` returns. `shutdown()` called from inside an observer call returns at
//! once and no further observer call starts after that one returns.
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use calendar_core::external_tokens::ExternalTokenSource as EngineTokenSource;
use calendar_core::http::HttpTransport as EngineTransport;
use calendar_core::time::Zone;
use timetug_bridge::ConnectedSource;
use timetug_engine::clock::{EngineClock, SystemClock};
use timetug_engine::command::EngineCommand as EngineEngineCommand;
use timetug_engine::connector_setup;
use timetug_engine::diagnostic::{
    EngineDiagnostic as EngineEngineDiagnostic, EngineDiagnosticSink,
};
use timetug_engine::engine::Engine as CoreEngine;
use timetug_engine::event::EngineEvent as EngineEngineEvent;
use timetug_engine::host::{BuildSystemSource, EngineHost, RequestSystemAccess};
use timetug_engine::host_delivery::{Delivery, DeliverySink};
use timetug_engine::manual_clock::ManualEngineClock;
use timetug_engine::state::EngineState as EngineEngineState;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::config::{EngineConfig, ValidConfig};
use crate::errors::FfiError;
use crate::host::authorization::{AuthorizationInteraction, AuthorizationInteractionAdapter};
use crate::host::credentials::{CredentialStore, CredentialStoreAdapter};
use crate::host::diagnostics::DiagnosticSink;
use crate::host::external_tokens::{ExternalTokenSource, ExternalTokenSourceAdapter};
use crate::host::http::{HttpTransport, HttpTransportAdapter};
use crate::host::observer::EngineObserver;
use crate::host::settings_storage::{SettingsStorage, SettingsStorageAdapter};
use crate::host::sign_in::{SignInPrompting, SignInPromptingAdapter};
use crate::host::system_calendar::{SystemCalendarAdapter, SystemCalendarSource};
use crate::host::text_generator::{TextGenerator, TextGeneratorAdapter};
use crate::runtime::{FireOnDrop, SHUTDOWN_WAIT, Signal};
use crate::values::commands::EngineCommand;
use crate::values::events::EngineEvent;
use crate::values::model::TimeTugCalendarEvent;
use crate::values::state::{DayAgenda, EngineState};
use crate::values::summary::{
    EngineStateSummary, PartRevisions, agenda_of, candidates_of, events_of,
};
use crate::values::{ToEngine, ToMirror};

/// The host objects the engine calls. The first five are required; the others are passed when the configuration says the
/// host has them (`textGeneration`, `externalTokens`, `systemCalendar`).
#[derive(uniffi::Record)]
pub struct EngineHostInterfaces {
    /// Where secrets are kept.
    pub credentials: Arc<dyn CredentialStore>,
    /// The sign-in interactions (browser and form).
    pub authorization: Arc<dyn AuthorizationInteraction>,
    /// The password sign-in form.
    pub sign_in: Arc<dyn SignInPrompting>,
    /// Where the user's preferences are kept.
    pub settings: Arc<dyn SettingsStorage>,
    /// Sends every HTTP request.
    pub http: Arc<dyn HttpTransport>,
    /// Receives the engine's log lines; without it they arrive as `EngineEvent.diagnostic`.
    pub diagnostics: Option<Arc<dyn DiagnosticSink>>,
    /// The host's text model (`textGeneration`).
    pub text_generator: Option<Arc<dyn TextGenerator>>,
    /// The host's account broker (`externalTokens`).
    pub external_tokens: Option<Arc<dyn ExternalTokenSource>>,
    /// The host's calendar store (`systemCalendar`).
    pub system_calendar: Option<Arc<dyn SystemCalendarSource>>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Lane {
    General,
    Accounts,
    Settings,
    Countdown,
    Takeover,
    Dedup,
}

fn lane_for(command: &EngineEngineCommand) -> Lane {
    match command {
        EngineEngineCommand::Start
        | EngineEngineCommand::Refresh
        | EngineEngineCommand::System(_)
        | EngineEngineCommand::Stop
        | EngineEngineCommand::SettingsChangedExternally => Lane::General,
        EngineEngineCommand::AddAccount { .. }
        | EngineEngineCommand::ReauthorizeAccount { .. }
        | EngineEngineCommand::RemoveAccount { .. }
        | EngineEngineCommand::CancelAuthorization => Lane::Accounts,
        EngineEngineCommand::ApplySettings(_) => Lane::Settings,
        EngineEngineCommand::SetCountdownVisible(_) => Lane::Countdown,
        EngineEngineCommand::TakeoverPresented { .. }
        | EngineEngineCommand::TakeoverClosed { .. }
        | EngineEngineCommand::Join { .. }
        | EngineEngineCommand::Snooze { .. }
        | EngineEngineCommand::Dismiss { .. }
        | EngineEngineCommand::TestTug { .. } => Lane::Takeover,
        EngineEngineCommand::Merge(..)
        | EngineEngineCommand::Unmerge(_)
        | EngineEngineCommand::Separate { .. }
        | EngineEngineCommand::ForgetCorrections => Lane::Dedup,
    }
}

/// What the delivery thread hands to the observer. Built once and moved once; boxing a variant would only add an allocation.
#[allow(clippy::large_enum_variant)]
enum Outgoing {
    State(EngineStateSummary),
    Event(EngineEvent),
}

struct ObserverSink(Arc<dyn EngineObserver>);

impl DeliverySink<Outgoing> for ObserverSink {
    fn deliver(&self, item: Outgoing) {
        match item {
            Outgoing::State(summary) => self.0.state_changed(summary),
            Outgoing::Event(event) => self.0.event(event),
        }
    }
}

/// The time zone the engine's calendar uses: the configured one, or the one the host last set.
struct ZoneCell(Mutex<Zone>);

impl ZoneCell {
    fn current(&self) -> Zone {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    fn set(&self, zone: Zone) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = zone;
    }
}

/// The latest published state and the revisions of its large parts.
struct Latest {
    state: Arc<EngineEngineState>,
    parts: PartRevisions,
}

struct SessionState {
    latest: Latest,
    started: bool,
    stopped: bool,
    lanes: BTreeMap<Lane, UnboundedSender<(EngineEngineCommand, oneshot::Sender<()>)>>,
    tasks: Vec<JoinHandle<()>>,
}

struct Session {
    engine: Arc<CoreEngine>,
    handle: tokio::runtime::Handle,
    delivery: Arc<Delivery<Outgoing>>,
    zone: Arc<ZoneCell>,
    #[cfg_attr(not(any(test, feature = "test-clock")), allow(dead_code))]
    manual_clock: Option<Arc<ManualEngineClock>>,
    state: Mutex<SessionState>,
    stop: Mutex<Option<oneshot::Sender<()>>>,
    stopped_signal: Arc<Signal>,
    shut_down: AtomicBool,
}

/// The engine behind the bindings.
#[derive(uniffi::Object)]
pub struct Engine {
    session: Arc<Session>,
}

/// Which clock an engine reads: the system's, or a manual one that starts at the given time (tests).
#[derive(Clone, Copy)]
pub(crate) enum ClockChoice {
    System,
    #[cfg_attr(not(any(test, feature = "test-clock")), allow(dead_code))]
    Manual(jiff::Timestamp),
}

fn unconfigured(flag: bool, name: &str, given: bool, interface: &str) -> Result<(), FfiError> {
    match (flag, given) {
        (true, false) => Err(FfiError::InvalidConfig {
            reason: format!("{name} is on but no {interface} was passed"),
        }),
        (false, true) => Err(FfiError::InvalidConfig {
            reason: format!("a {interface} was passed but {name} is off"),
        }),
        _ => Ok(()),
    }
}

impl Engine {
    /// Validates the configuration, builds the engine (not started) and its two threads.
    pub(crate) fn build(
        config: EngineConfig,
        host: EngineHostInterfaces,
        observer: Arc<dyn EngineObserver>,
        clock_choice: ClockChoice,
    ) -> Result<Arc<Engine>, FfiError> {
        let valid: ValidConfig = config.validate()?;
        unconfigured(
            config.text_generation,
            "textGeneration",
            host.text_generator.is_some(),
            "TextGenerator",
        )?;
        unconfigured(
            config.external_tokens,
            "externalTokens",
            host.external_tokens.is_some(),
            "ExternalTokenSource",
        )?;
        unconfigured(
            config.system_calendar,
            "systemCalendar",
            host.system_calendar.is_some(),
            "SystemCalendarSource",
        )?;
        std::fs::create_dir_all(&valid.data_directory).map_err(|error| {
            FfiError::InvalidConfig {
                reason: format!("cannot create the data directory: {error}"),
            }
        })?;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|error| FfiError::Host {
                message: format!("cannot start the engine runtime: {error}"),
            })?;
        let delivery: Arc<Delivery<Outgoing>> = Delivery::new(Arc::new(ObserverSink(observer)));
        let (clock, manual_clock): (Arc<dyn EngineClock>, Option<Arc<ManualEngineClock>>) =
            match clock_choice {
                ClockChoice::System => (Arc::new(SystemClock), None),
                ClockChoice::Manual(start) => {
                    let manual = Arc::new(ManualEngineClock::new(start));
                    (manual.clone(), Some(manual))
                }
            };
        let zone = Arc::new(ZoneCell(Mutex::new(valid.zone.clone())));

        // Engine diagnostics reach the host's sink when it gave one, else the observer as events.
        let diagnostics: EngineDiagnosticSink = match &host.diagnostics {
            Some(sink) => crate::host::diagnostics::engine_sink(sink.clone()),
            None => {
                let delivery = delivery.clone();
                Arc::new(move |diagnostic: EngineEngineDiagnostic| {
                    delivery.enqueue(Outgoing::Event(EngineEvent::Diagnostic(
                        diagnostic.to_mirror(),
                    )));
                })
            }
        };
        let token_source: Option<Arc<dyn EngineTokenSource>> =
            host.external_tokens.clone().map(|source| {
                Arc::new(ExternalTokenSourceAdapter::new(source)) as Arc<dyn EngineTokenSource>
            });
        let transport: Arc<dyn EngineTransport> =
            Arc::new(HttpTransportAdapter::new(host.http.clone()));
        let connectors = connector_setup::make(
            &valid.connectors,
            transport,
            clock.clone(),
            zone.current(),
            token_source,
        );
        let zone_for_host = zone.clone();
        let mut engine_host = EngineHost::new(
            valid.data_directory.clone(),
            Arc::new(SettingsStorageAdapter::new(host.settings.clone())),
            Arc::new(CredentialStoreAdapter::new(host.credentials.clone())),
            Arc::new(AuthorizationInteractionAdapter::new(
                host.authorization.clone(),
            )),
            Arc::new(SignInPromptingAdapter::new(host.sign_in.clone())),
            connectors.registry,
        );
        engine_host.unconfigured_kind_ids = connectors.unconfigured_kind_ids;
        engine_host.text_generator = host
            .text_generator
            .clone()
            .map(|generator| Arc::new(TextGeneratorAdapter::new(generator)) as Arc<_>);
        if let Some(system) = host.system_calendar.clone() {
            let for_build = system.clone();
            let build: BuildSystemSource = Arc::new(move || {
                Arc::new(ConnectedSource::new(Arc::new(SystemCalendarAdapter::new(
                    for_build.clone(),
                ))))
            });
            engine_host.build_system_source = Some(build);
            let request: RequestSystemAccess = Arc::new(move || {
                let system = system.clone();
                Box::pin(async move { system.request_access().await })
            });
            engine_host.request_system_access = Some(request);
        }
        engine_host.diagnostics = diagnostics;
        engine_host.clock = clock;
        engine_host.zone = Arc::new(move || zone_for_host.current());
        engine_host.platform = valid.platform;

        // `Engine::new` spawns its status task, so it needs the runtime; the engine thread runs it below.
        let handle = runtime.handle().clone();
        let engine = {
            let _enter = runtime.enter();
            CoreEngine::new(engine_host)
        };
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let stopped_signal = Arc::new(Signal::new());
        let session = Arc::new(Session {
            engine,
            handle,
            delivery: delivery.clone(),
            zone,
            manual_clock,
            state: Mutex::new(SessionState {
                latest: Latest {
                    state: Arc::new(EngineEngineState::empty()),
                    parts: PartRevisions::default(),
                },
                started: false,
                stopped: false,
                lanes: BTreeMap::new(),
                tasks: Vec::new(),
            }),
            stop: Mutex::new(Some(stop_tx)),
            stopped_signal: stopped_signal.clone(),
            shut_down: AtomicBool::new(false),
        });
        delivery.start().map_err(|error| FfiError::Host {
            message: format!("cannot start the delivery thread: {error}"),
        })?;
        let thread_session = session.clone();
        let fire = FireOnDrop(stopped_signal);
        let spawned = std::thread::Builder::new()
            .name("TimeTug engine".to_string())
            .spawn(move || {
                let _fire = fire;
                runtime.block_on(engine_main(thread_session, stop_rx));
                drop(runtime);
            });
        if let Err(error) = spawned {
            delivery.close(false);
            return Err(FfiError::Host {
                message: format!("cannot start the engine thread: {error}"),
            });
        }
        Ok(Arc::new(Engine { session }))
    }
}

/// The engine thread's one future: waits for the stop signal, cancels every task, then stops the engine.
async fn engine_main(session: Arc<Session>, stop: oneshot::Receiver<()>) {
    // Both a signal and a dropped sender end the wait.
    let _ = stop.await;
    let (tasks, lanes) = {
        let mut state = session.lock();
        (
            std::mem::take(&mut state.tasks),
            std::mem::take(&mut state.lanes),
        )
    };
    drop(lanes);
    for task in tasks {
        task.abort();
    }
    session.engine.send(EngineEngineCommand::Stop).await;
}

impl Session {
    fn lock(&self) -> MutexGuard<'_, SessionState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Subscribes the pumps once, before the engine starts, so no state or event of the start is missed.
    fn start_once(self: &Arc<Self>) {
        {
            let mut state = self.lock();
            if state.started || state.stopped {
                return;
            }
            state.started = true;
        }
        let mut states = self.engine.states();
        let mut events = self.engine.events();
        let delivery = self.delivery.clone();
        let session = Arc::downgrade(self);
        let state_task = self.handle.spawn(async move {
            while states.changed().await.is_ok() {
                let state = states.borrow_and_update().clone();
                let Some(session) = session.upgrade() else {
                    break;
                };
                let summary = {
                    let mut shared = session.lock();
                    let parts =
                        PartRevisions::after(&shared.latest.state, shared.latest.parts, &state);
                    shared.latest = Latest {
                        state: Arc::new(state.clone()),
                        parts,
                    };
                    EngineStateSummary::of(&state, parts)
                };
                delivery.enqueue_state(Outgoing::State(summary));
            }
        });
        let delivery = self.delivery.clone();
        let event_task = self.handle.spawn(async move {
            while let Some(event) = events.recv().await {
                // The engine host's diagnostic sink sees every diagnostic, including the persistence stores' that never
                // become events, and routes them; delivering the event as well would deliver each twice.
                if matches!(event, EngineEngineEvent::Diagnostic(_)) {
                    continue;
                }
                delivery.enqueue(Outgoing::Event(event.to_mirror()));
            }
        });
        self.track(state_task);
        self.track(event_task);
    }

    fn track(&self, task: JoinHandle<()>) {
        let mut state = self.lock();
        if state.stopped {
            drop(state);
            task.abort();
            return;
        }
        state.tasks.retain(|task| !task.is_finished());
        state.tasks.push(task);
    }

    /// Hands `command` to its lane; `done` resolves when the engine finished it (or is dropped when the engine stops).
    fn dispatch(self: &Arc<Self>, command: EngineEngineCommand, done: oneshot::Sender<()>) {
        if matches!(command, EngineEngineCommand::Start) {
            self.start_once();
        }
        if matches!(command, EngineEngineCommand::CancelAuthorization) {
            let engine = self.engine.clone();
            let task = self.handle.spawn(async move {
                engine.send(EngineEngineCommand::CancelAuthorization).await;
                let _ = done.send(());
            });
            self.track(task);
            return;
        }
        let lane = lane_for(&command);
        let sender = {
            let mut state = self.lock();
            if state.stopped {
                return;
            }
            match state.lanes.get(&lane) {
                Some(existing) => existing.clone(),
                None => {
                    let (sender, mut receiver) =
                        unbounded_channel::<(EngineEngineCommand, oneshot::Sender<()>)>();
                    let engine = self.engine.clone();
                    state.tasks.push(self.handle.spawn(async move {
                        while let Some((command, done)) = receiver.recv().await {
                            engine.send(command).await;
                            let _ = done.send(());
                        }
                    }));
                    state.lanes.insert(lane, sender.clone());
                    sender
                }
            }
        };
        let _ = sender.send((command, done));
    }

    fn latest(&self) -> (Arc<EngineEngineState>, PartRevisions) {
        let state = self.lock();
        (state.latest.state.clone(), state.latest.parts)
    }

    fn shutdown(&self) {
        if self.shut_down.swap(true, Ordering::SeqCst) {
            return;
        }
        self.lock().stopped = true;
        let from_callback = self.delivery.is_delivery_thread();
        self.delivery.close(!from_callback);
        let stop = self
            .stop
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(stop) = stop {
            let _ = stop.send(());
        }
        if !from_callback {
            self.stopped_signal.wait(SHUTDOWN_WAIT);
        }
    }
}

#[uniffi::export]
impl Engine {
    /// Creates the engine from its configuration, the host objects and the observer. Nothing is delivered until `start`.
    /// Throws `FfiError.InvalidConfig` for a configuration that is not acceptable.
    #[uniffi::constructor]
    pub fn new(
        config: EngineConfig,
        host: EngineHostInterfaces,
        observer: Arc<dyn EngineObserver>,
    ) -> Result<Arc<Engine>, FfiError> {
        Engine::build(config, host, observer, ClockChoice::System)
    }

    /// Loads the saved state, builds the sources and refreshes once; returns when the engine finished starting. The
    /// observer's first call, a state summary, comes with it.
    pub async fn start(&self) {
        self.send(EngineCommand::Start).await;
    }

    /// Sends a command and returns when the engine finished it. Commands of one kind of work (accounts, settings,
    /// takeovers, ...) run one after another in send order; different kinds do not wait for each other. After
    /// `shutdown()` it returns at once.
    pub async fn send(&self, command: EngineCommand) {
        let (done, finished) = oneshot::channel();
        self.session.dispatch(command.to_engine(), done);
        // A dropped sender (the engine stopped, or the command was dropped after shutdown) also ends the wait.
        let _ = finished.await;
    }

    /// The latest summary of the state (the same value the observer last got, or the empty state before `start`).
    pub fn current_summary(&self) -> EngineStateSummary {
        let (state, parts) = self.session.latest();
        EngineStateSummary::of(&state, parts)
    }

    /// The latest state in full (the summary plus events, agenda and candidates). Large: prefer the summary and fetch the
    /// parts whose revision changed.
    pub fn current_state(&self) -> EngineState {
        self.session.latest().0.as_ref().clone().to_mirror()
    }

    /// The deduplicated events of the latest state.
    pub fn current_events(&self) -> Vec<TimeTugCalendarEvent> {
        events_of(&self.session.latest().0)
    }

    /// The day agenda of the latest state.
    pub fn current_agenda(&self) -> DayAgenda {
        agenda_of(&self.session.latest().0)
    }

    /// The merge candidates of the latest state.
    pub fn current_candidates(&self) -> HashMap<String, Vec<TimeTugCalendarEvent>> {
        candidates_of(&self.session.latest().0)
    }

    /// Records the current time zone identifier. Follow it with `send(.system(.timeZoneChanged))` so the engine
    /// republishes. Throws `FfiError.InvalidConfig` for an identifier the engine does not know (the zone is unchanged).
    pub fn set_time_zone(&self, identifier: String) -> Result<(), FfiError> {
        self.session
            .zone
            .set(crate::custom_types::zone_from_identifier(&identifier)?);
        Ok(())
    }

    /// Stops the engine. Idempotent. From outside an observer call it returns when no observer call is running (none
    /// starts after it returns) and the engine has stopped, so the data directory is free for a new engine; from inside
    /// an observer call it returns at once and the engine ends on its own.
    pub fn shutdown(&self) {
        self.session.shutdown();
    }
}

#[cfg(feature = "test-clock")]
#[uniffi::export]
impl Engine {
    /// Like `new`, with a manual clock that starts at `clock_start` and moves only with `advance_clock`. Only the Swift
    /// tests' own build of the library has it.
    #[uniffi::constructor]
    pub fn new_for_testing(
        config: EngineConfig,
        host: EngineHostInterfaces,
        observer: Arc<dyn EngineObserver>,
        clock_start: jiff::Timestamp,
    ) -> Result<Arc<Engine>, FfiError> {
        Engine::build(config, host, observer, ClockChoice::Manual(clock_start))
    }

    /// Moves the manual clock forward and wakes what sleeps until then.
    pub fn advance_clock(&self, seconds: f64) {
        self.advance_manual_clock(seconds);
    }
}

#[cfg(any(test, feature = "test-clock"))]
impl Engine {
    /// Moves the manual clock of an engine built with one (`ClockChoice::Manual`).
    pub(crate) fn advance_manual_clock(&self, seconds: f64) {
        if let Some(clock) = &self.session.manual_clock {
            clock.advance(seconds);
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.session.shutdown();
    }
}
