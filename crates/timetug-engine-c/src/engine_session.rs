//! Port of `EngineSession.swift`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use calendar_core::external_tokens::ExternalTokenSource;
use calendar_core::http::HttpTransport;
use calendar_core::time::Zone;
use jiff::Timestamp;
use serde_json::Value;
use timetug_engine::clock::{EngineClock, SystemClock};
use timetug_engine::command::EngineCommand;
use timetug_engine::diagnostic::{EngineDiagnostic, EngineDiagnosticSink, Level};
use timetug_engine::engine::Engine;
use timetug_engine::event::EngineEvent;
use timetug_engine::host::{EngineHost, current_zone};
use timetug_engine::persistence::file_settings_storage::FileSettingsStorage;
use timetug_engine::state::EngineState;
use timetug_engine_protocol::command_mapping::{CommandDecoder, CommandOutcome};
use timetug_engine_protocol::config::WireConfig;
use timetug_engine_protocol::envelope::{Envelope, EnvelopeError, MessageId};
use timetug_engine_protocol::message_catalog::{WireReady, WireReadyCapabilities};
use timetug_engine_protocol::service_messages::ReplyErrorKind;
use timetug_engine_protocol::state_mapping::{
    json_value, wire_diagnostic, wire_message_for_event, wire_message_for_state,
};
use timetug_engine_protocol::wire_types::WireDates;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::connectors::ConnectorSetup;
use crate::delivery::{Delivery, MessageSink, Outbox};
use crate::manual_engine_clock::ManualEngineClock;
use crate::remote_host_services::{
    RemoteAuthorizationInteraction, RemoteCredentialStore, RemoteExternalTokenSource,
    RemoteHttpTransport, RemoteSignInPrompting, RemoteTextGenerator,
};
use crate::request_table::RequestTable;

/// The engine's version as reported in `ready`. The release workflow sets `TIMETUG_ENGINE_VERSION` before it builds the
/// package (`build.rs`); the repository keeps the development value.
pub enum EngineBuild {}

impl EngineBuild {
    pub const VERSION: &'static str = env!("TIMETUG_ENGINE_VERSION");
}

/// How long `shutdown` waits for the engine thread to end (Swift: `stoppedSignal.wait(timeout: 10)`).
const SHUTDOWN_WAIT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Lane {
    General,
    Accounts,
    Settings,
    Countdown,
    Takeover,
    Dedup,
}

fn lane_for(command: &EngineCommand) -> Lane {
    match command {
        EngineCommand::Start
        | EngineCommand::Refresh
        | EngineCommand::System(_)
        | EngineCommand::Stop
        | EngineCommand::SettingsChangedExternally => Lane::General,
        EngineCommand::AddAccount { .. }
        | EngineCommand::ReauthorizeAccount { .. }
        | EngineCommand::RemoveAccount { .. }
        | EngineCommand::CancelAuthorization => Lane::Accounts,
        EngineCommand::ApplySettings(_) => Lane::Settings,
        EngineCommand::SetCountdownVisible(_) => Lane::Countdown,
        EngineCommand::TakeoverPresented { .. }
        | EngineCommand::TakeoverClosed { .. }
        | EngineCommand::Join { .. }
        | EngineCommand::Snooze { .. }
        | EngineCommand::Dismiss { .. }
        | EngineCommand::TestTug { .. } => Lane::Takeover,
        EngineCommand::Merge(..)
        | EngineCommand::Unmerge(_)
        | EngineCommand::Separate { .. }
        | EngineCommand::ForgetCorrections => Lane::Dedup,
    }
}

/// The time zone the engine's calendar uses: the config's or a `timeZoneChanged` zone, else the process zone.
pub struct ZoneOverride {
    zone: Mutex<Option<Zone>>,
}

impl ZoneOverride {
    pub fn new(zone: Option<Zone>) -> ZoneOverride {
        ZoneOverride {
            zone: Mutex::new(zone),
        }
    }

    pub fn current(&self) -> Zone {
        self.zone
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or_else(current_zone)
    }

    pub fn set(&self, zone: Zone) {
        *self.zone.lock().unwrap_or_else(PoisonError::into_inner) = Some(zone);
    }
}

/// A one-shot signal a thread can wait on with a timeout.
pub struct Signal {
    fired: Mutex<bool>,
    changed: Condvar,
}

impl Signal {
    pub fn new() -> Signal {
        Signal {
            fired: Mutex::new(false),
            changed: Condvar::new(),
        }
    }

    pub fn fire(&self) {
        *self.fired.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.changed.notify_all();
    }

    /// Returns true when fired, false after `timeout`.
    pub fn wait(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut fired = self.fired.lock().unwrap_or_else(PoisonError::into_inner);
        while !*fired {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            fired = self
                .changed
                .wait_timeout(fired, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}

impl Default for Signal {
    fn default() -> Self {
        Signal::new()
    }
}

/// Fires its signal when dropped, however the engine thread ends.
struct FireOnDrop(Arc<Signal>);

impl Drop for FireOnDrop {
    fn drop(&mut self) {
        self.0.fire();
    }
}

struct SessionState {
    latest_state: Arc<EngineState>,
    started: bool,
    stopped: bool,
    lanes: BTreeMap<Lane, UnboundedSender<EngineCommand>>,
    tasks: Vec<JoinHandle<()>>,
}

/// What the tasks on the engine thread share: the engine, the outbox and the session's bookkeeping.
struct Shared {
    config: WireConfig,
    engine: Arc<Engine>,
    manual_clock: Option<Arc<ManualEngineClock>>,
    zone: Arc<ZoneOverride>,
    decoder: CommandDecoder,
    outbox: Arc<Outbox>,
    state: Mutex<SessionState>,
}

/// One engine behind the C interface: the `Engine`, its remote host services, the command intake and the delivery to
/// the host. Created by `tt_engine_create`, ended by `tt_engine_destroy` (`shutdown`).
///
/// Threads: one engine thread owns a `tokio` current-thread runtime and runs until shutdown; one delivery thread calls
/// the host. Nothing is delivered until `start`.
///
/// Commands: `send` only enqueues (never blocks). One intake task decodes them in order against the latest published
/// state and hands them to lanes, as the Mac app's command queue does: commands in the same lane reach the engine one
/// after another in send order, and lanes do not wait for each other, so a sign-in waiting for a person never holds up a
/// takeover. `account.cancel` and `test.advanceClock` act at once, because the work they end or unblock may itself be
/// what a lane is waiting on.
///
/// Messages: `ready` first (on the first `start`), then states from the state pump and events from the event pump,
/// through `Outbox` and `Delivery`. Engine diagnostics reach the host through the engine host's diagnostic sink, which
/// sees every diagnostic, including the persistence stores' that never become `.diagnostic` events; the event pump
/// skips `.diagnostic` so none is delivered twice.
pub struct EngineSession {
    pub config: WireConfig,
    pub requests: Arc<RequestTable>,
    shared: Arc<Shared>,
    delivery: Arc<Delivery>,
    intake: Mutex<Option<UnboundedSender<String>>>,
    stop: Mutex<Option<oneshot::Sender<()>>>,
    stopped_signal: Arc<Signal>,
    shut_down: AtomicBool,
}

fn encode(envelope: &Envelope) -> String {
    envelope.to_json()
}

impl EngineSession {
    /// Parses and validates `config_json`, creates the data directory and builds the engine (not started). Returns
    /// `None` for any invalid config; nothing is delivered then.
    pub fn make(config_json: &str, sink: Arc<dyn MessageSink>) -> Option<Arc<EngineSession>> {
        let config = WireConfig::parse(config_json).ok()?;
        let directory = std::path::PathBuf::from(&config.data_directory);
        std::fs::create_dir_all(&directory).ok()?;
        EngineSession::build(config, directory, sink)
    }

    fn build(
        config: WireConfig,
        directory: std::path::PathBuf,
        sink: Arc<dyn MessageSink>,
    ) -> Option<Arc<EngineSession>> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .ok()?;
        let delivery = Delivery::new(sink);
        let outbox = Arc::new(Outbox::new(delivery.clone()));

        let (clock, manual_clock): (Arc<dyn EngineClock>, Option<Arc<ManualEngineClock>>) =
            if config.testing {
                let start = config
                    .test_clock_start
                    .as_deref()
                    .and_then(WireDates::date)
                    .unwrap_or_else(Timestamp::now);
                let manual = Arc::new(ManualEngineClock::new(start));
                (manual.clone(), Some(manual))
            } else {
                (Arc::new(SystemClock), None)
            };
        let zone = Arc::new(ZoneOverride::new(
            config.time_zone.as_deref().and_then(Zone::named),
        ));
        let decoder = CommandDecoder::new(config.testing);

        let request_outbox = outbox.clone();
        let requests = RequestTable::new(
            Arc::new(move |envelope| request_outbox.post(encode(&envelope))),
            clock.clone(),
        );
        let diagnostic_outbox = outbox.clone();
        let diagnostics: EngineDiagnosticSink = Arc::new(move |diagnostic: EngineDiagnostic| {
            diagnostic_outbox.post(encode(&wire_diagnostic(&diagnostic, None)));
        });
        let token_source: Option<Arc<dyn ExternalTokenSource>> =
            config.external_tokens.then(|| {
                Arc::new(RemoteExternalTokenSource::new(
                    requests.clone(),
                    diagnostics.clone(),
                )) as Arc<dyn ExternalTokenSource>
            });
        let transport: Arc<dyn HttpTransport> =
            Arc::new(RemoteHttpTransport::new(requests.clone()));
        let connectors = ConnectorSetup::make(
            &config,
            transport,
            clock.clone(),
            zone.current(),
            token_source,
        );
        let zone_for_host = zone.clone();
        let mut host = EngineHost::new(
            directory.clone(),
            Arc::new(FileSettingsStorage::new(&directory, diagnostics.clone())),
            Arc::new(RemoteCredentialStore::new(requests.clone())),
            Arc::new(RemoteAuthorizationInteraction::new(requests.clone())),
            Arc::new(RemoteSignInPrompting::new(requests.clone())),
            connectors.registry,
        );
        host.unconfigured_kind_ids = connectors.unconfigured_kind_ids;
        host.text_generator = config
            .text_generation
            .then(|| Arc::new(RemoteTextGenerator::new(requests.clone())) as Arc<_>);
        host.diagnostics = diagnostics;
        host.clock = clock;
        host.zone = Arc::new(move || zone_for_host.current());
        // `Engine::new` spawns its status task, so it needs the runtime; the engine thread runs it below.
        let engine = {
            let _enter = runtime.enter();
            Engine::new(host)
        };

        let (intake_tx, intake_rx) = unbounded_channel::<String>();
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let stopped_signal = Arc::new(Signal::new());
        let shared = Arc::new(Shared {
            config: config.clone(),
            engine,
            manual_clock,
            zone,
            decoder,
            outbox,
            state: Mutex::new(SessionState {
                latest_state: Arc::new(EngineState::empty()),
                started: false,
                stopped: false,
                lanes: BTreeMap::new(),
                tasks: Vec::new(),
            }),
        });
        delivery.start().ok()?;
        let thread_shared = shared.clone();
        let fire = FireOnDrop(stopped_signal.clone());
        let spawned = std::thread::Builder::new()
            .name("TimeTug engine".to_string())
            .spawn(move || {
                let _fire = fire;
                runtime.block_on(engine_main(thread_shared, intake_rx, stop_rx));
                drop(runtime);
            });
        if spawned.is_err() {
            delivery.close(false);
            return None;
        }
        Some(Arc::new(EngineSession {
            config,
            requests,
            shared,
            delivery,
            intake: Mutex::new(Some(intake_tx)),
            stop: Mutex::new(Some(stop_tx)),
            stopped_signal,
            shut_down: AtomicBool::new(false),
        }))
    }

    // MARK: - From the host

    /// Enqueues one command; never blocks. Dropped after `shutdown`.
    pub fn send(&self, json: String) {
        let intake = self.intake.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(intake) = intake.as_ref() {
            let _ = intake.send(json);
        }
    }

    pub fn reply(&self, request_id: u64, json: &str) {
        self.requests.fulfill(request_id, json);
    }

    /// Stops the engine. From outside this engine's callback it waits until no callback runs (none starts after it
    /// returns) and, bounded, until the engine has stopped, so the data directory is free for a new engine. From inside
    /// a callback it only cancels and returns at once (the threads end on their own). Idempotent.
    pub fn shutdown(&self) {
        if self.shut_down.swap(true, Ordering::SeqCst) {
            return;
        }
        self.shared.lock().stopped = true;
        let from_callback = self.delivery.is_delivery_thread();
        drop(
            self.intake
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
        self.delivery.close(!from_callback);
        self.requests.fail_all(ReplyErrorKind::Cancelled);
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

/// The engine thread's one future: the intake task, then, on the stop signal, the cancellation of every task and the
/// engine's own `stop`.
async fn engine_main(
    shared: Arc<Shared>,
    mut intake: tokio::sync::mpsc::UnboundedReceiver<String>,
    stop: oneshot::Receiver<()>,
) {
    let intake_shared = shared.clone();
    let intake_task = tokio::spawn(async move {
        while let Some(text) = intake.recv().await {
            intake_shared.handle(&text);
        }
    });
    // Both a signal and a dropped sender end the wait.
    let _ = stop.await;
    intake_task.abort();
    let (tasks, lanes) = {
        let mut state = shared.lock();
        (
            std::mem::take(&mut state.tasks),
            std::mem::take(&mut state.lanes),
        )
    };
    drop(lanes);
    for task in tasks {
        task.abort();
    }
    shared.engine.send(EngineCommand::Stop).await;
}

/// The `id` of a text that is not an envelope, when it is a JSON object with a usable one (Swift's `IDOnly`).
fn id_only(text: &str) -> Option<MessageId> {
    let value: Value = serde_json::from_str(text).ok()?;
    MessageId::from_value(value.as_object()?.get("id")?)
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, SessionState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    // MARK: - Command intake (one task, in order)

    fn handle(self: &Arc<Self>, text: &str) {
        if self.lock().stopped {
            return;
        }
        let envelope = match Envelope::parse(text) {
            Ok(envelope) => envelope,
            Err(error) => {
                let message = match error {
                    EnvelopeError::NotJson => "invalid JSON",
                    EnvelopeError::NotAnObject | EnvelopeError::Member(_) => "invalid envelope",
                };
                self.reject(id_only(text), message);
                return;
            }
        };
        let state = self.lock().latest_state.clone();
        match self.decoder.decode(&envelope, &state) {
            CommandOutcome::Rejected(diagnostic) => self
                .outbox
                .post(encode(&wire_diagnostic(&diagnostic, envelope.id.clone()))),
            CommandOutcome::TestAdvanceClock { seconds } => {
                if let Some(clock) = &self.manual_clock {
                    clock.advance(seconds);
                }
            }
            CommandOutcome::TimeZone(zone, command) => {
                self.zone.set(zone);
                self.dispatch(command);
            }
            CommandOutcome::Engine(EngineCommand::Start) => {
                self.start_once();
                self.dispatch(EngineCommand::Start);
            }
            CommandOutcome::Engine(EngineCommand::CancelAuthorization) => {
                let engine = self.engine.clone();
                self.track(tokio::spawn(async move {
                    engine.send(EngineCommand::CancelAuthorization).await;
                }));
            }
            CommandOutcome::Engine(command) => self.dispatch(command),
        }
    }

    fn reject(&self, id: Option<MessageId>, message: &str) {
        let diagnostic = EngineDiagnostic::new(Level::Warning, "command", message);
        self.outbox.post(encode(&wire_diagnostic(&diagnostic, id)));
    }

    /// The first `start` publishes `ready` and subscribes the pumps before the engine starts, so no state or event of
    /// the start is missed. A later `start` goes to the engine alone (which refuses it with a diagnostic).
    fn start_once(self: &Arc<Self>) {
        {
            let mut state = self.lock();
            if state.started {
                return;
            }
            state.started = true;
        }
        let capabilities = WireReadyCapabilities::with_external_tokens(
            false,
            self.config.text_generation,
            self.config.external_tokens.then_some(true),
        );
        let ready = WireReady::new(
            EngineBuild::VERSION,
            self.config.platform.clone(),
            capabilities,
        );
        self.outbox.open_with(encode(
            &Envelope::new("ready").with_payload(json_value(&ready)),
        ));
        let mut states = self.engine.states();
        let mut events = self.engine.events();
        let state_outbox = self.outbox.clone();
        // The state pump records the latest state for the decoder, then hands it to the outbox. It holds the session
        // weakly (Swift: `[weak self]`), so the tasks the session keeps do not keep it alive.
        let session = Arc::downgrade(self);
        self.track(tokio::spawn(async move {
            while states.changed().await.is_ok() {
                let state = states.borrow_and_update().clone();
                if let Some(session) = session.upgrade() {
                    session.lock().latest_state = Arc::new(state.clone());
                }
                state_outbox.post_state(encode(&wire_message_for_state(&state)));
            }
        }));
        let event_outbox = self.outbox.clone();
        self.track(tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                if matches!(event, EngineEvent::Diagnostic(_)) {
                    continue;
                }
                event_outbox.post(encode(&wire_message_for_event(&event)));
            }
        }));
    }

    fn dispatch(&self, command: EngineCommand) {
        let lane = lane_for(&command);
        let sender = {
            let mut state = self.lock();
            if state.stopped {
                return;
            }
            match state.lanes.get(&lane) {
                Some(existing) => existing.clone(),
                None => {
                    let (sender, mut receiver) = unbounded_channel::<EngineCommand>();
                    let engine = self.engine.clone();
                    state.tasks.push(tokio::spawn(async move {
                        while let Some(command) = receiver.recv().await {
                            engine.send(command).await;
                        }
                    }));
                    state.lanes.insert(lane, sender.clone());
                    sender
                }
            }
        };
        let _ = sender.send(command);
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
}
