//! Port of `Engine.swift` (803 lines), split into six files that follow its `// MARK:` sections (the one exception to
//! "one Swift file, one Rust module" that the phase 3 plan names). The ranges are in `docs/rust-port/file-map.md`:
//!
//! | Rust file | Swift lines | Sections |
//! |---|---|---|
//! | `mod.rs` | 1-237, 774-803 | the type, `init`, `deinit`, Public API (`states`, `events`, `currentState`, `start`, `send`), and the private helper types |
//! | `refresh.rs` | 238-294, 523-588, 657-673 | Refresh and apply, Inference, System events |
//! | `commands.rs` | 295-355, 625-656, 706-729 | Accounts, Settings, Stop |
//! | `takeover.rs` | 356-522 | Takeover scheduling (ledger, fire guard, scheduling loop) |
//! | `dedup.rs` | 589-624 | Dedup actions |
//! | `publish.rs` | 674-705, 730-773 | UI ticks, Publication |
//!
//! # The actor rule
//!
//! Swift's `actor Engine` is re-entrant at every `await`. Here the state is `Mutex<EngineInner>` (a `std` mutex). A
//! handler locks, reads or changes the state, and unlocks before every `.await`; the compiler enforces it, because a
//! `MutexGuard` held across an `.await` makes the future non-`Send` and every test and the C interface spawn these
//! futures. Swift code that has no `await` is one lock hold: the helpers take `&mut EngineInner` and never lock, so a
//! handler's synchronous stretch cannot be interleaved by another handler, exactly as on the actor. Each `await` of
//! `Engine.swift` is marked `// await point (Engine.swift:NNN)` at the place it stands (or stood, where the Rust callee is
//! synchronous: `CalendarStore` and `AccountManager::status` only had `await` because Swift actors are isolated).
//!
//! Locks are never held while the host runs for long: the diagnostics sink and `states`/`events` consumers must not
//! call back into the engine synchronously (they cannot in Swift either; they would have to `await`).
//!
//! # Tasks
//!
//! The engine creates no runtime and no thread: `Engine::new` and everything it spawns need a `tokio` runtime, which the
//! caller (the C interface, the bindings, a test) provides. Swift's `Task { }` that the engine cancels and restarts is a
//! [`TaskSlot`]: `cancel()` is cooperative (a cancelled sleep ends; a sleep that already returned still proceeds, as in
//! Swift, so the wake accounting of `EngineActivity` stays exact), and `Drop for Engine` aborts every slot, which is
//! Swift's `deinit`.
//!
//! # Publication
//!
//! `publish` is the only place that sends on the `watch` channel; `revision` increases by one per published state.
//! Events go to `tokio::sync::mpsc` unbounded channels, one per `events()` call, pruned when their receiver is gone.

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use async_trait::async_trait;
use calendar_core::connection::{Connection, SyncStateStore};
use calendar_core::failure::{Failure, Outcome};
use calendar_core::file_sync_state_store::FileSyncStateStore;
use calendar_core::source_types::SourceError as LibrarySourceError;
use calendar_core::time::{DateInterval, Now};
use jiff::Timestamp;
use timetug_bridge::connected_source::ConnectedSource;
use timetug_core::agenda::day_agenda::DayAgenda;
use timetug_core::dedup::adjudication::DuplicateAdjudicator;
use timetug_core::takeover::ledger::TakeoverLedger;
use timetug_core::takeover::scheduler::ScheduledTakeover;
use timetug_core::{
    CalendarInfo, CalendarSnapshot, CalendarSource, CalendarStore, DedupState, InferenceStatus,
    SourceChange, TimeTugCalendarEvent,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::accounts::account_manager::{
    AccountManager, BoxFuture, SettingsChange, SourceReconciling, Status,
};
use crate::accounts::cancellation::{CancelToken, current_token, run_in_task};
use crate::accounts::source_reconciler::SourceReconciler;
use crate::accounts::source_refresh_coordinator::{RefreshFn, SourceRefreshCoordinator};
use crate::activity::{EngineActivity, Kind};
use crate::clock::EngineClock;
use crate::command::EngineCommand;
use crate::diagnostic::{EngineDiagnostic, Level};
use crate::event::EngineEvent;
use crate::host::EngineHost;
use crate::inference::text_generator_adjudicator::TextGeneratorAdjudicator;
use crate::persistence::dedup_state_store::DedupStateStore;
use crate::persistence::ledger_store::LedgerStore;
use crate::scheduling::launch_acknowledgement::LaunchAcknowledgement;
use crate::settings::EngineSettings;
use crate::state::{AccountKindInfo, EngineState};

mod commands;
mod dedup;
mod publish;
mod refresh;
mod takeover;

/// The platform-neutral "what and when" of TimeTug: sources, the merged calendar, the takeover ledger and the state every
/// front end renders. Front ends send `EngineCommand`s and observe `states` and `events`; presentation is theirs. Ported
/// from the Mac app's `AppCoordinator`.
///
/// Reentrancy: every `await` inside the engine (store refreshes, account calls) lets other commands and publications
/// run. `apply` is synchronous and guards on `CalendarSnapshot::revision`, so a late publication from an older refresh is
/// dropped and the published `EngineState::revision` only ever increases.
pub struct Engine {
    host: EngineHost,
    store: CalendarStore,
    adjudicator: Option<Arc<TextGeneratorAdjudicator>>,
    ledger_store: LedgerStore,
    dedup_store: DedupStateStore,
    account_manager: Arc<AccountManager>,
    reconciler: Arc<SourceReconciler>,
    /// In-flight work, for tests that wait for quiescence.
    activity: Arc<EngineActivity>,
    /// Delivers the account manager's status changes in order (Swift's `statusTask` and `statusContinuation`).
    status_poster: Arc<StatusPoster>,
    status_task: JoinHandle<()>,
    /// Swift's `ref.engine` (a weak reference the helpers built before `self` existed call through).
    weak: Weak<Engine>,
    inner: Mutex<EngineInner>,
}

/// Everything Swift's `actor Engine` keeps in `var`s.
pub(crate) struct EngineInner {
    // Model
    settings: EngineSettings,
    ledger: TakeoverLedger,
    launch_acknowledgement: LaunchAcknowledgement,
    snapshot: CalendarSnapshot,
    agenda: DayAgenda,
    account_status: Status,
    inference_status: InferenceStatus,
    last_saved_dedup_state: Option<DedupState>,
    available_account_kinds: Vec<AccountKindInfo>,

    // Takeover scheduling
    next_takeover: Option<ScheduledTakeover>,
    /// As reported by the host (`takeoverPresented`), on screen or as a toast.
    takeover_visible: bool,
    fire_task: Option<TaskSlot>,
    /// Bumped on every re-arm; a sleep from an older arming that still wakes does nothing.
    fire_generation: u64,
    /// The takeover emitted and not yet closed (the plan's `pendingTakeoverEventID`); commands name it by `id`.
    outstanding_takeover: Option<TimeTugCalendarEvent>,
    /// True while the outstanding takeover is the test tug's sample.
    outstanding_is_test: bool,

    // Background work
    started: bool,
    periodic_task: Option<TaskSlot>,
    /// Inference resolution off the refresh path; at most one runs at a time.
    resolve_task: Option<TaskSlot>,
    /// Set when resolution is asked for; a running resolution runs once more when it is set again.
    resolve_requested: bool,
    /// UI tick for countdowns and agenda boundaries.
    tick_task: Option<TaskSlot>,
    /// Bumped on every tick arming; a sleep from an older arming that still wakes does nothing.
    tick_generation: u64,
    /// Whether a front end shows a countdown (second-level wakes near a meeting).
    countdown_visible: bool,
    stopped: bool,
    /// The running sign-in (add or reauthorize); `cancelAuthorization` cancels it.
    auth_task: Option<TaskSlot>,

    // Publication
    revision: u64,
    state: EngineState,
    /// `None` after `stop`: dropping the sender ends every `states()` receiver.
    state_tx: Option<watch::Sender<EngineState>>,
    event_senders: Vec<UnboundedSender<EngineEvent>>,
}

impl EngineInner {
    /// True from the moment a takeover is emitted until the host closes it, and while the host reports one visible (on
    /// screen or as a toast). The host reports presentation asynchronously, so the emitted-but-unreported window counts
    /// too: a second takeover waits for the first to close.
    ///
    /// Host contract: for every `takeoverDue` the host MUST eventually send exactly one of `takeoverClosed`, `join`,
    /// `snooze` or `dismiss` with that request's `event.id`, on every path (including when it could not present). Until
    /// it does, every later takeover waits.
    fn takeover_outstanding(&self) -> bool {
        self.takeover_visible || self.outstanding_takeover.is_some()
    }
}

/// Swift's `Task<Void, Never>` the engine keeps to cancel it: a cooperative cancel token (the task reads it through
/// `is_task_cancelled`, and `sleep_until_cancellable` ends on it), the `JoinHandle` (aborted when the engine is dropped),
/// and a flag that turns true when the task body ends, for `await task.value`.
pub(crate) struct TaskSlot {
    token: CancelToken,
    handle: JoinHandle<()>,
    done: watch::Receiver<bool>,
}

/// Sends `true` when the task body ends, however it ends (finished, or dropped by an abort).
struct DoneOnDrop(watch::Sender<bool>);

impl Drop for DoneOnDrop {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

impl TaskSlot {
    /// `Task { ... }`. Inside the body `is_task_cancelled()` reads this slot's token.
    pub(crate) fn spawn<F>(body: F) -> TaskSlot
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let token = CancelToken::new();
        let (done_tx, done) = watch::channel(false);
        let handle = tokio::spawn(run_in_task(token.clone(), async move {
            let _done = DoneOnDrop(done_tx);
            body.await;
        }));
        TaskSlot {
            token,
            handle,
            done,
        }
    }

    /// `Task.cancel()`.
    pub(crate) fn cancel(&self) {
        self.token.cancel();
    }

    /// For `Drop for Engine` (Swift's `deinit` cancels, and a dropped actor's tasks end).
    fn abort(&self) {
        self.token.cancel();
        self.handle.abort();
    }

    /// What `await task.value` waits on.
    pub(crate) fn finished(&self) -> TaskFinished {
        TaskFinished(self.done.clone())
    }
}

/// Completes when the task body has ended.
pub(crate) struct TaskFinished(watch::Receiver<bool>);

impl TaskFinished {
    pub(crate) async fn wait(mut self) {
        let _ = self.0.wait_for(|done| *done).await;
    }
}

/// `try await clock.sleep(until:)` inside a [`TaskSlot`] body: ends with `Err(Cancelled)` when the slot is cancelled
/// while the sleep is pending. The sleep is polled first, so a sleep that already returned wins over a cancel that
/// arrived after it, as a Swift `sleep` that was resumed before `cancel()` returns normally.
pub(crate) async fn sleep_until_cancellable(
    clock: &Arc<dyn EngineClock>,
    deadline: Timestamp,
) -> Outcome<()> {
    let Some(token) = current_token() else {
        return clock.sleep_until(deadline).await;
    };
    tokio::select! {
        biased;
        outcome = clock.sleep_until(deadline) => outcome,
        () = token.cancelled() => Err(Failure::Cancelled),
    }
}

/// `defer { activity.end(kind) }`.
pub(crate) struct ActivityEnd {
    activity: Arc<EngineActivity>,
    kind: Kind,
}

impl ActivityEnd {
    pub(crate) fn begin(activity: &Arc<EngineActivity>, kind: Kind) -> ActivityEnd {
        activity.begin(kind);
        ActivityEnd {
            activity: activity.clone(),
            kind,
        }
    }

    /// For a `begin` that already happened.
    pub(crate) fn for_begun(activity: &Arc<EngineActivity>, kind: Kind) -> ActivityEnd {
        ActivityEnd {
            activity: activity.clone(),
            kind,
        }
    }
}

impl Drop for ActivityEnd {
    fn drop(&mut self) {
        self.activity.end(self.kind);
    }
}

/// What the account manager reports to the engine, in order.
enum AccountMessage {
    Status(Status),
    Diagnostic(EngineDiagnostic),
}

/// Swift's `AsyncStream<AccountMessage>.Continuation` plus `Engine.post`: hands an account manager message to the
/// engine's ordered consumer, counted until it is handled. After `finish` (the engine stopped) a post is `.terminated`
/// and ends its own count at once.
struct StatusPoster {
    tx: UnboundedSender<AccountMessage>,
    activity: Arc<EngineActivity>,
    finished: AtomicBool,
}

impl StatusPoster {
    fn post(&self, message: AccountMessage) {
        self.activity.begin(Kind::AccountMessage);
        if self.finished.load(Ordering::SeqCst) || self.tx.send(message).is_err() {
            self.activity.end(Kind::AccountMessage);
        }
    }

    fn finish(&self) {
        self.finished.store(true, Ordering::SeqCst);
    }
}

/// Placeholder for a host without a system calendar source; never reached (see `effective_account_settings`).
struct NoSystemSource;

#[async_trait]
impl CalendarSource for NoSystemSource {
    fn id(&self) -> String {
        "eventkit".to_string()
    }

    fn display_name(&self) -> String {
        String::new()
    }

    async fn calendars(&self) -> Outcome<Vec<CalendarInfo>> {
        Ok(Vec::new())
    }

    async fn events_in(&self, _interval: DateInterval) -> Outcome<Vec<TimeTugCalendarEvent>> {
        Ok(Vec::new())
    }

    /// `AsyncStream { $0.finish() }`: a stream that has ended.
    fn changes(&self) -> UnboundedReceiver<SourceChange> {
        unbounded_channel().1
    }
}

impl Engine {
    /// Swift's `static let periodicRefresh`. Backstop only: change signals (source polls, wake, network return) drive the
    /// real refreshes.
    pub(crate) const PERIODIC_REFRESH: f64 = 900.0;
    /// Meetings that started longer than this before launch are treated as already handled.
    pub(crate) const LAUNCH_GRACE: f64 = 120.0;
    /// Bounded backstop for one inference resolution: each pass drains a batch, but a verdict that fails to stick must
    /// never loop forever.
    pub(crate) const MAX_RESOLVE_PASSES: usize = 10;

    /// Builds the engine and starts the task that delivers the account manager's status changes, so it must be called
    /// from within a `tokio` runtime (Swift's `init` made a `Task` too). Nothing else runs until `start`.
    pub fn new(host: EngineHost) -> Arc<Engine> {
        Arc::new_cyclic(|weak| Engine::build(host, weak.clone()))
    }

    fn build(host: EngineHost, weak: Weak<Engine>) -> Engine {
        let settings = host.settings.load();
        let launch_acknowledgement = LaunchAcknowledgement::new(host.clock.now());
        let ledger_store = LedgerStore::new(&host.data_directory, host.diagnostics.clone());
        let dedup_store = DedupStateStore::new(&host.data_directory, host.diagnostics.clone());
        let adjudicator = host.text_generator.clone().map(|generator| {
            Arc::new(TextGeneratorAdjudicator::new(
                generator,
                host.clock.clone(),
                host.zone.clone(),
                host.diagnostics.clone(),
            ))
        });
        let clock = host.clock.clone();
        let pass_clock: Now = Arc::new(move || clock.now());
        let store = CalendarStore::new(
            Vec::new(),
            (host.zone)(),
            adjudicator
                .clone()
                .map(|adjudicator| adjudicator as Arc<dyn DuplicateAdjudicator>),
            pass_clock,
        );

        // The trio below calls back into the engine through `weak`, which Swift filled once `self` existed.
        let activity = Arc::new(EngineActivity::new());
        let refresh_weak = weak.clone();
        let refresh: RefreshFn = Arc::new(move |ids| {
            let weak = refresh_weak.clone();
            Box::pin(async move {
                if let Some(engine) = weak.upgrade() {
                    engine.refresh(ids).await;
                }
            })
        });
        let coordinator = Arc::new(SourceRefreshCoordinator::new(
            Some(activity.clone()),
            refresh,
        ));
        let registry = host.registry.clone();
        let credentials = host.credentials.clone();
        let sync_state: Arc<dyn SyncStateStore> = Arc::new(FileSyncStateStore::new(
            host.data_directory.join("sync-state.json"),
        ));
        let build_sync_state = sync_state.clone();
        let system_source = host.build_system_source.clone();
        let (status_tx, mut status_rx) = unbounded_channel::<AccountMessage>();
        let status_poster = Arc::new(StatusPoster {
            tx: status_tx,
            activity: activity.clone(),
            finished: AtomicBool::new(false),
        });
        let reconciler = Arc::new(SourceReconciler::new(
            Arc::new(move |connection: &Connection| {
                let Some(kind) = registry.kind(&connection.kind_id) else {
                    return Err(Failure::Source(LibrarySourceError::InvalidResponse(
                        "unknown account type".to_string(),
                    )));
                };
                let source =
                    kind.make_source(connection, credentials.clone(), build_sync_state.clone())?;
                Ok(Arc::new(ConnectedSource::new(source)) as Arc<dyn CalendarSource>)
            }),
            // Never called without a host source: `effective_account_settings` reports the switch off then.
            Arc::new(move || match &system_source {
                Some(build) => build(),
                None => Arc::new(NoSystemSource) as Arc<dyn CalendarSource>,
            }),
            Arc::new(move |change| {
                let coordinator = coordinator.clone();
                Box::pin(async move { coordinator.signal(change).await })
            }),
        ));
        let settings_weak = weak.clone();
        let apply_weak = weak.clone();
        let update_weak = weak.clone();
        let status_post = status_poster.clone();
        let diagnostic_post = status_poster.clone();
        let account_manager = Arc::new(
            AccountManager::new(
                host.clone(),
                reconciler.clone() as Arc<dyn SourceReconciling>,
                Arc::new(move || {
                    let weak = settings_weak.clone();
                    Box::pin(async move {
                        match weak.upgrade() {
                            Some(engine) => engine.effective_account_settings(),
                            None => EngineSettings {
                                system_calendar_enabled: false,
                                ..EngineSettings::default()
                            },
                        }
                    })
                }),
                Arc::new(move |sources| {
                    let weak = apply_weak.clone();
                    Box::pin(async move {
                        if let Some(engine) = weak.upgrade() {
                            engine.apply_sources(sources).await;
                        }
                    })
                }),
                Arc::new(move |change: SettingsChange| {
                    let weak = update_weak.clone();
                    Box::pin(async move {
                        if let Some(engine) = weak.upgrade() {
                            engine.update_settings(change);
                        }
                    }) as BoxFuture<()>
                }),
            )
            .with_sync_state(sync_state)
            .with_on_status_change(Arc::new(move |status| {
                status_post.post(AccountMessage::Status(status));
            }))
            .with_diagnostics(Arc::new(move |diagnostic| {
                diagnostic_post.post(AccountMessage::Diagnostic(diagnostic));
            })),
        );
        let available_account_kinds =
            AccountManager::kind_infos(&host.registry, &host.unconfigured_kind_ids);

        let consumer_weak = weak.clone();
        let consumer_activity = activity.clone();
        let status_task = tokio::spawn(async move {
            while let Some(message) = status_rx.recv().await {
                if let Some(engine) = consumer_weak.upgrade() {
                    match message {
                        AccountMessage::Status(status) => engine.account_status_changed(status),
                        AccountMessage::Diagnostic(diagnostic) => engine.forward(&diagnostic),
                    }
                }
                consumer_activity.end(Kind::AccountMessage);
            }
        });

        let mut state = EngineState::empty();
        state.settings = settings.clone();
        let (state_tx, _) = watch::channel(state.clone());
        let inner = EngineInner {
            settings,
            ledger: TakeoverLedger::new(),
            launch_acknowledgement,
            snapshot: CalendarSnapshot::empty(),
            agenda: DayAgenda::empty(),
            account_status: Status::empty(),
            inference_status: InferenceStatus::Disabled,
            last_saved_dedup_state: None,
            available_account_kinds,
            next_takeover: None,
            takeover_visible: false,
            fire_task: None,
            fire_generation: 0,
            outstanding_takeover: None,
            outstanding_is_test: false,
            started: false,
            periodic_task: None,
            resolve_task: None,
            resolve_requested: false,
            tick_task: None,
            tick_generation: 0,
            countdown_visible: false,
            stopped: false,
            auth_task: None,
            revision: 0,
            state,
            state_tx: Some(state_tx),
            event_senders: Vec::new(),
        };
        Engine {
            host,
            store,
            adjudicator,
            ledger_store,
            dedup_store,
            account_manager,
            reconciler,
            activity,
            status_poster,
            status_task,
            weak,
            inner: Mutex::new(inner),
        }
    }

    fn lock(&self) -> MutexGuard<'_, EngineInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    // MARK: - Public API

    /// The latest state. Each new receiver first sees the current state as changed (`changed()` returns at once); a slow
    /// receiver sees only the newest. The channel ends when the engine stops.
    pub fn states(&self) -> watch::Receiver<EngineState> {
        let inner = self.lock();
        let mut receiver = match &inner.state_tx {
            Some(sender) => sender.subscribe(),
            None => watch::channel(inner.state.clone()).1,
        };
        receiver.mark_changed();
        receiver
    }

    /// One-shot notifications, buffered without limit. Every receiver gets the events emitted after it was made. Each
    /// call returns a new receiver; the channel ends when the engine stops.
    pub fn events(&self) -> UnboundedReceiver<EngineEvent> {
        let (sender, receiver) = unbounded_channel();
        self.lock().event_senders.push(sender);
        receiver
    }

    /// A copy of the most recently published state.
    pub fn current_state(&self) -> EngineState {
        self.lock().state.clone()
    }

    /// In-flight work counters, for tests that wait for quiescence; the engine itself never reads them.
    pub fn activity(&self) -> &Arc<EngineActivity> {
        &self.activity
    }

    /// Loads the persisted state, builds the sources and refreshes once. Returns after that first refresh.
    pub async fn start(&self) {
        {
            let mut inner = self.lock();
            if inner.started {
                self.diagnose(
                    &mut inner,
                    Level::Warning,
                    "engine",
                    "start was sent more than once; ignored",
                );
                return;
            }
            inner.started = true;
            let now = self.host.clock.now();
            inner.ledger = self.ledger_store.load();
            inner.ledger.prune(now);
            let keys = inner.ledger.key_count();
            self.diagnose(
                &mut inner,
                Level::Info,
                "ledger",
                &format!("ledger loaded with {keys} keys"),
            );
            self.persist_ledger(&mut inner);
            let dedup = self.dedup_store.load();
            inner.last_saved_dedup_state = Some(dedup.clone());
            // await point (Engine.swift:196): `store.load` is synchronous in Rust.
            self.store.load(dedup);
        }
        if let Some(adjudicator) = &self.adjudicator {
            // await point (Engine.swift:197): no lock is held.
            adjudicator.refresh_availability().await;
        }
        {
            let mut inner = self.lock();
            // await points (Engine.swift:198, 199): `store.setInferenceEnabled` and `inferenceStatus` are synchronous.
            let _ = self
                .store
                .set_inference_enabled(inner.settings.inference_enabled, self.host.clock.now());
            inner.inference_status = self.store.inference_status();
            self.arm_periodic_refresh(&mut inner);
        }
        // Requests system calendar access if enabled, builds the sources and refreshes (through `apply_sources`).
        // await point (Engine.swift:202)
        self.account_manager.start().await;
        // await point (Engine.swift:203)
        self.accounts_changed().await;
    }

    /// Runs one command to completion. While it runs it counts as `Kind::Command` activity.
    pub async fn send(&self, command: EngineCommand) {
        let _command = ActivityEnd::begin(&self.activity, Kind::Command);
        match command {
            // await point (Engine.swift:210)
            EngineCommand::Start => self.start().await,
            // await point (Engine.swift:211)
            EngineCommand::Refresh => self.refresh(None).await,
            // await point (Engine.swift:212)
            EngineCommand::AddAccount { kind_id } => {
                self.run_authorization(SignIn::Add(kind_id)).await;
            }
            EngineCommand::RemoveAccount { connection_id } => {
                // await point (Engine.swift:214)
                self.account_manager.remove_account(&connection_id).await;
                // await point (Engine.swift:215)
                self.accounts_changed().await;
            }
            // await point (Engine.swift:216)
            EngineCommand::ReauthorizeAccount { connection_id } => {
                self.run_authorization(SignIn::Reauthorize(connection_id))
                    .await;
            }
            EngineCommand::CancelAuthorization => {
                if let Some(task) = &self.lock().auth_task {
                    task.cancel();
                }
                // await point (Engine.swift:219): `cancelAuthorization` is synchronous in Rust.
                self.account_manager.cancel_authorization();
            }
            // await point (Engine.swift:220)
            EngineCommand::ApplySettings(change) => self.apply_settings(change).await,
            // await point (Engine.swift:221)
            EngineCommand::SettingsChangedExternally => self.settings_changed_externally().await,
            EngineCommand::SetCountdownVisible(visible) => {
                let mut inner = self.lock();
                self.set_countdown_visible(&mut inner, visible);
            }
            EngineCommand::TakeoverPresented {
                event_id,
                on_screen,
            } => {
                let mut inner = self.lock();
                self.takeover_presented(&mut inner, &event_id, on_screen);
            }
            EngineCommand::TakeoverClosed { event_id } => {
                let mut inner = self.lock();
                self.takeover_closed(&mut inner, &event_id);
            }
            EngineCommand::Join { event_id } => {
                let mut inner = self.lock();
                self.join(&mut inner, &event_id);
            }
            EngineCommand::Snooze { event_id, seconds } => {
                let mut inner = self.lock();
                self.snooze(&mut inner, &event_id, seconds);
            }
            EngineCommand::Dismiss { event_id } => {
                let mut inner = self.lock();
                self.dismiss(&mut inner, &event_id);
            }
            EngineCommand::TestTug {
                title,
                calendar_title,
            } => {
                let mut inner = self.lock();
                self.test_tug(&mut inner, &title, &calendar_title);
            }
            // await points (Engine.swift:229 to 232): the dedup actions are synchronous in Rust (the store is).
            EngineCommand::Merge(a, b) => self.merge(&a, &b),
            EngineCommand::Unmerge(event) => self.unmerge(&event),
            EngineCommand::Separate { event, members } => self.separate(&event, &members),
            // await point (Engine.swift:232)
            EngineCommand::ForgetCorrections => self.forget_corrections().await,
            // await point (Engine.swift:233)
            EngineCommand::System(event) => self.handle_system_event(event).await,
            // await point (Engine.swift:234)
            EngineCommand::Stop => self.stop().await,
        }
    }
}

/// The two sign-ins `runAuthorization` runs (Swift passes the body as a closure).
enum SignIn {
    Add(String),
    Reauthorize(String),
}

/// Dropping the engine without `.stop` still ends every task it started (sleeps, listeners, sign-in).
impl Drop for Engine {
    fn drop(&mut self) {
        self.status_task.abort();
        let inner = self.inner.get_mut().unwrap_or_else(PoisonError::into_inner);
        for slot in [
            inner.periodic_task.take(),
            inner.fire_task.take(),
            inner.resolve_task.take(),
            inner.tick_task.take(),
            inner.auth_task.take(),
        ]
        .into_iter()
        .flatten()
        {
            slot.abort();
        }
        self.reconciler.cancel_all();
    }
}

/// `Set<String>` of source ids, for `register_sources`.
fn source_ids(sources: &[Arc<dyn CalendarSource>]) -> BTreeSet<String> {
    sources.iter().map(|source| source.id()).collect()
}
