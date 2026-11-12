//! Port of `Support/AccountFakes.swift`: fakes for the account manager tests.
//!
//! Swift's mutable class properties (`kind.nextConnectionID = "2"` after the kind is registered) are interior state
//! behind a `Mutex` here, because a registered kind is shared as `Arc<dyn ConnectorKind>`. Swift's
//! `withUnsafeCurrentTask { $0?.cancel() }` is `cancel_current_task()`; `Task.sleep(for: .seconds(30))` that only a
//! cancellation can end is a future that never completes (the sign-in race drops it on cancel).

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use calendar_core::connection::Connection;
use calendar_core::connection::{
    AuthorizationInteraction, AuthorizationMethod, ConnectorKind, ConnectorRegistry,
    CredentialField, CredentialHelp, CredentialSnapshot, CredentialStore, InMemoryCredentialStore,
    InMemorySyncStateStore, OAuthRedirectSession, Platform, SyncStateStore,
};
use calendar_core::external_tokens::AccountIdentityFamily;
use calendar_core::failure::{Failure, Outcome};
use calendar_core::source_types::SourceError as LibrarySourceError;
use calendar_core::time::DateInterval;
use timetug_core::{CalendarInfo, CalendarSource, SourceChange, TimeTugCalendarEvent};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::sync::watch;
use uuid::Uuid;

use crate::accounts::account_manager::{
    AccountManager, SettingsChange, SourceReconcilerUpdate, SourceReconciling, StatusObserver,
};
use crate::accounts::cancellation::cancel_current_task;
use crate::host::{EngineHost, SignInPrompting};
use crate::settings::{EngineSettings, InMemorySettingsStorage, SettingsStorage};

use super::{LockedBox, TempDir};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) struct FakeSource {
    pub(crate) id: String,
}

impl FakeSource {
    pub(crate) fn new(id: &str) -> FakeSource {
        FakeSource { id: id.to_string() }
    }
}

#[async_trait]
impl CalendarSource for FakeSource {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn display_name(&self) -> String {
        self.id.clone()
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

struct ReconcilerState {
    builds: usize,
    stopped: bool,
    stored_at_reconcile: Vec<bool>,
}

/// A reconciler fake: builds a `FakeSource` per connection (and "eventkit" when the system calendar is on).
pub(crate) struct FakeReconciler {
    state: Mutex<ReconcilerState>,
    log: Arc<LockedBox<Vec<String>>>,
    accounts_file: Option<PathBuf>,
}

impl FakeReconciler {
    pub(crate) fn new(log: Arc<LockedBox<Vec<String>>>, accounts_file: Option<PathBuf>) -> Self {
        FakeReconciler {
            state: Mutex::new(ReconcilerState {
                builds: 0,
                stopped: false,
                stored_at_reconcile: Vec::new(),
            }),
            log,
            accounts_file,
        }
    }

    pub(crate) fn builds(&self) -> usize {
        lock(&self.state).builds
    }

    pub(crate) fn stopped(&self) -> bool {
        lock(&self.state).stopped
    }

    /// Whether each reconcile saw the stored account file still listing "1" (for the removal-order test).
    pub(crate) fn stored_at_reconcile(&self) -> Vec<bool> {
        lock(&self.state).stored_at_reconcile.clone()
    }

    fn update(connections: &[Connection], system_on: bool) -> SourceReconcilerUpdate {
        let mut sources: Vec<Arc<dyn CalendarSource>> = Vec::new();
        if system_on {
            sources.push(Arc::new(FakeSource::new("eventkit")));
        }
        for connection in connections {
            sources.push(Arc::new(FakeSource::new(&connection.source_id())));
        }
        SourceReconcilerUpdate {
            sources,
            failures: BTreeMap::new(),
        }
    }
}

#[async_trait]
impl SourceReconciling for FakeReconciler {
    async fn reconcile(
        &self,
        connections: &[Connection],
        system_calendar_enabled: bool,
    ) -> SourceReconcilerUpdate {
        self.log.mutate(|log| log.push("reconcile".to_string()));
        let stored = self.accounts_file.as_ref().and_then(|file| {
            let data = std::fs::read(file).ok()?;
            let list = serde_json::from_slice::<Vec<Connection>>(&data).ok()?;
            Some(list.iter().any(|c| c.connection_id == "1"))
        });
        {
            let mut state = lock(&self.state);
            state.builds += connections.len();
            if let Some(stored) = stored {
                state.stored_at_reconcile.push(stored);
            }
        }
        FakeReconciler::update(connections, system_calendar_enabled)
    }

    async fn rebuild(
        &self,
        _connection: &Connection,
        connections: &[Connection],
        system_calendar_enabled: bool,
    ) -> SourceReconcilerUpdate {
        lock(&self.state).builds += 1;
        FakeReconciler::update(connections, system_calendar_enabled)
    }

    async fn source_id_for_connection(&self, _connection_id: &str) -> Option<String> {
        None
    }

    async fn stop(&self) {
        lock(&self.state).stopped = true;
    }
}

/// `FakeConnectorKind.Outcome`.
#[derive(Clone)]
pub(crate) enum FakeOutcome {
    Succeed,
    /// Stores secrets for `connection_id`, returns the connection, and cancels the signing-in task as it returns.
    SucceedThenCancel {
        connection_id: String,
    },
    /// Rejects the password until it equals `good`.
    Password {
        good: String,
    },
    /// Rust-only: waits forever holding a guard that sets `dropped` when the future is dropped (a sign-in waiting
    /// for a browser whose redirect session closes in its destructor).
    HangHoldingGuard {
        dropped: Arc<AtomicBool>,
    },
}

/// Sets its flag when dropped.
struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Swift's `sourceFactory`.
pub(crate) type SourceFactory = Arc<
    dyn Fn(&Connection) -> Outcome<Arc<dyn calendar_core::source_types::CalendarSource>>
        + Send
        + Sync,
>;

struct Props {
    outcome: FakeOutcome,
    next_email: String,
    next_connection_id: String,
    reauthorize_error: Option<Failure>,
    reauthorize_suspends: bool,
    reauthorize_result: Option<Connection>,
    reauthorize_cancels_and_renames: Option<String>,
    authorize_gate: Option<Arc<Gate>>,
    source_factory: Option<SourceFactory>,
    reauthorized: usize,
    attempts: usize,
}

/// A kind with configurable authorization method and outcome. `FakeFamilyKind` is the same type with `family` set.
pub(crate) struct FakeConnectorKind {
    id: String,
    display_name: String,
    authorization: AuthorizationMethod,
    family: Option<String>,
    props: Mutex<Props>,
}

impl FakeConnectorKind {
    /// `FakeConnectorKind()`: Google, OAuth, succeeds.
    pub(crate) fn new() -> Arc<FakeConnectorKind> {
        FakeConnectorKind::with(
            "google",
            "Google",
            AuthorizationMethod::Oauth,
            FakeOutcome::Succeed,
        )
    }

    pub(crate) fn with_outcome(outcome: FakeOutcome) -> Arc<FakeConnectorKind> {
        FakeConnectorKind::with("google", "Google", AuthorizationMethod::Oauth, outcome)
    }

    pub(crate) fn with(
        id: &str,
        display_name: &str,
        authorization: AuthorizationMethod,
        outcome: FakeOutcome,
    ) -> Arc<FakeConnectorKind> {
        FakeConnectorKind::build(id, display_name, authorization, outcome, None)
    }

    /// `FakeFamilyKind(id:displayName:family:)`: a kind that says it is the same people as another kind's.
    pub(crate) fn family(id: &str, display_name: &str, family: &str) -> Arc<FakeConnectorKind> {
        FakeConnectorKind::build(
            id,
            display_name,
            AuthorizationMethod::Oauth,
            FakeOutcome::Succeed,
            Some(family.to_string()),
        )
    }

    fn build(
        id: &str,
        display_name: &str,
        authorization: AuthorizationMethod,
        outcome: FakeOutcome,
        family: Option<String>,
    ) -> Arc<FakeConnectorKind> {
        Arc::new(FakeConnectorKind {
            id: id.to_string(),
            display_name: display_name.to_string(),
            authorization,
            family,
            props: Mutex::new(Props {
                outcome,
                next_email: "a@x.test".to_string(),
                next_connection_id: "1".to_string(),
                reauthorize_error: None,
                reauthorize_suspends: false,
                reauthorize_result: None,
                reauthorize_cancels_and_renames: None,
                authorize_gate: None,
                source_factory: None,
                reauthorized: 0,
                attempts: 0,
            }),
        })
    }

    pub(crate) fn set_next_email(&self, email: &str) {
        lock(&self.props).next_email = email.to_string();
    }

    pub(crate) fn set_next_connection_id(&self, id: &str) {
        lock(&self.props).next_connection_id = id.to_string();
    }

    pub(crate) fn set_reauthorize_error(&self, error: Failure) {
        lock(&self.props).reauthorize_error = Some(error);
    }

    pub(crate) fn set_reauthorize_suspends(&self, suspends: bool) {
        lock(&self.props).reauthorize_suspends = suspends;
    }

    pub(crate) fn set_reauthorize_result(&self, connection: Connection) {
        lock(&self.props).reauthorize_result = Some(connection);
    }

    /// When set, reauthorize cancels the signing-in task and returns the connection renamed to this.
    pub(crate) fn set_reauthorize_cancels_and_renames(&self, name: &str) {
        lock(&self.props).reauthorize_cancels_and_renames = Some(name.to_string());
    }

    /// When set, `authorize` waits here first (a sign-in in progress).
    pub(crate) fn set_authorize_gate(&self, gate: Arc<Gate>) {
        lock(&self.props).authorize_gate = Some(gate);
    }

    pub(crate) fn set_source_factory(&self, factory: SourceFactory) {
        lock(&self.props).source_factory = Some(factory);
    }

    pub(crate) fn reauthorized(&self) -> usize {
        lock(&self.props).reauthorized
    }

    pub(crate) fn attempts(&self) -> usize {
        lock(&self.props).attempts
    }
}

impl AccountIdentityFamily for FakeConnectorKind {
    fn identity_family(&self) -> String {
        self.family.clone().unwrap_or_else(|| self.id.clone())
    }
}

#[async_trait]
impl ConnectorKind for FakeConnectorKind {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn display_name(&self) -> String {
        self.display_name.clone()
    }

    fn supported_platforms(&self) -> Platform {
        Platform::MACOS | Platform::IOS | Platform::LINUX | Platform::WINDOWS
    }

    fn authorization(&self) -> AuthorizationMethod {
        self.authorization.clone()
    }

    fn as_identity_family(&self) -> Option<&dyn AccountIdentityFamily> {
        if self.family.is_some() {
            Some(self)
        } else {
            None
        }
    }

    async fn authorize(
        &self,
        interaction: &dyn AuthorizationInteraction,
        credentials: &dyn CredentialStore,
    ) -> Outcome<Connection> {
        let (gate, outcome, email, next_id, attempts) = {
            let mut props = lock(&self.props);
            props.attempts += 1;
            (
                props.authorize_gate.clone(),
                props.outcome.clone(),
                props.next_email.clone(),
                props.next_connection_id.clone(),
                props.attempts,
            )
        };
        if let Some(gate) = gate {
            gate.wait().await;
        }
        match outcome {
            FakeOutcome::Succeed => {
                let c = Connection::new(self.id.clone(), next_id.clone(), email.clone())
                    .with_config(&[("email", &email)]);
                credentials
                    .set_secrets(
                        BTreeMap::from([("refresh_token".to_string(), format!("r-{next_id}"))]),
                        &c.connection_id,
                    )
                    .await?;
                Ok(c)
            }
            FakeOutcome::HangHoldingGuard { dropped } => {
                let _guard = DropFlag(dropped);
                std::future::pending::<()>().await;
                Err(Failure::Cancelled)
            }
            FakeOutcome::SucceedThenCancel { connection_id } => {
                credentials
                    .set_secrets(
                        BTreeMap::from([("refresh_token".to_string(), "r".to_string())]),
                        &connection_id,
                    )
                    .await?;
                cancel_current_task();
                Ok(Connection::new(self.id.clone(), connection_id, "c@x.test"))
            }
            FakeOutcome::Password { good } => {
                let AuthorizationMethod::Password { fields } = &self.authorization else {
                    return Err(Failure::Source(LibrarySourceError::InvalidResponse(
                        "not a password kind".to_string(),
                    )));
                };
                let values = interaction.prompt_credentials(fields).await?;
                if values.get("password") != Some(&good) {
                    return Err(Failure::Source(LibrarySourceError::AuthExpired));
                }
                let username = values.get("username").cloned().unwrap_or_default();
                let c = Connection::new(self.id.clone(), format!("p{attempts}"), username.clone())
                    .with_config(&[("username", &username)]);
                credentials
                    .set_secrets(
                        BTreeMap::from([(
                            "password".to_string(),
                            values.get("password").cloned().unwrap_or_default(),
                        )]),
                        &c.connection_id,
                    )
                    .await?;
                Ok(c)
            }
        }
    }

    async fn reauthorize(
        &self,
        connection: &Connection,
        _interaction: &dyn AuthorizationInteraction,
        _credentials: &dyn CredentialStore,
    ) -> Outcome<Connection> {
        if lock(&self.props).reauthorize_suspends {
            // `Task.sleep(for: .seconds(30))`: only a cancellation ends it in the tests.
            std::future::pending::<()>().await;
        }
        let (error, rename, result) = {
            let props = lock(&self.props);
            (
                props.reauthorize_error.clone(),
                props.reauthorize_cancels_and_renames.clone(),
                props.reauthorize_result.clone(),
            )
        };
        if let Some(error) = error {
            return Err(error);
        }
        lock(&self.props).reauthorized += 1;
        if let Some(name) = rename {
            cancel_current_task();
            let mut renamed = connection.clone();
            renamed.display_name = name;
            return Ok(renamed);
        }
        Ok(result.unwrap_or_else(|| connection.clone()))
    }

    fn make_source(
        &self,
        connection: &Connection,
        _credentials: Arc<dyn CredentialStore>,
        _sync_state: Arc<dyn SyncStateStore>,
    ) -> Outcome<Arc<dyn calendar_core::source_types::CalendarSource>> {
        let factory = lock(&self.props).source_factory.clone();
        match factory {
            Some(factory) => factory(connection),
            None => Err(Failure::Source(LibrarySourceError::InvalidResponse(
                "unused".to_string(),
            ))),
        }
    }
}

/// One `prepare` call of the sign-in form.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Armed {
    pub(crate) title: String,
    pub(crate) help: Option<CredentialHelp>,
    pub(crate) values: BTreeMap<String, String>,
    pub(crate) error: Option<String>,
}

struct SignInState {
    armed: Vec<Armed>,
    passwords: VecDeque<String>,
}

/// Plays the host's sign-in form: records what it was armed with and answers prompts from a queue of password
/// attempts.
pub(crate) struct FakeSignIn {
    state: Mutex<SignInState>,
    typed_values: BTreeMap<String, String>,
}

impl FakeSignIn {
    /// `FakeSignIn(passwords:typed:)`; `typed` defaults to `["username": "me"]`.
    pub(crate) fn new(passwords: Vec<String>) -> FakeSignIn {
        FakeSignIn {
            state: Mutex::new(SignInState {
                armed: Vec::new(),
                passwords: passwords.into(),
            }),
            typed_values: BTreeMap::from([("username".to_string(), "me".to_string())]),
        }
    }

    pub(crate) fn prepared(&self) -> Vec<Armed> {
        lock(&self.state).armed.clone()
    }
}

#[async_trait]
impl SignInPrompting for FakeSignIn {
    async fn prepare(
        &self,
        title: String,
        help: Option<CredentialHelp>,
        values: BTreeMap<String, String>,
        error: Option<String>,
    ) {
        lock(&self.state).armed.push(Armed {
            title,
            help,
            values,
            error,
        });
    }

    async fn last_non_secret_values(&self) -> Option<BTreeMap<String, String>> {
        Some(self.typed_values.clone())
    }
}

#[async_trait]
impl AuthorizationInteraction for FakeSignIn {
    async fn begin_oauth_redirect(&self) -> Outcome<Box<dyn OAuthRedirectSession>> {
        Err(Failure::Source(LibrarySourceError::InvalidResponse(
            "unused".to_string(),
        )))
    }

    async fn prompt_credentials(
        &self,
        _fields: &[CredentialField],
    ) -> Outcome<BTreeMap<String, String>> {
        let next = lock(&self.state).passwords.pop_front();
        // The fake form throws a cancellation when it has no answer.
        let Some(next) = next else {
            return Err(Failure::Cancelled);
        };
        let mut values = self.typed_values.clone();
        values.insert("password".to_string(), next);
        Ok(values)
    }
}

/// Swift's `FailingRemovalCredentials`: every removal throws.
pub(crate) struct FailingRemovalCredentials {
    pub(crate) inner: Arc<InMemoryCredentialStore>,
}

#[async_trait]
impl CredentialStore for FailingRemovalCredentials {
    async fn secrets(&self, id: &str) -> Outcome<Option<BTreeMap<String, String>>> {
        self.inner.secrets(id).await
    }
    async fn set_secrets(&self, secrets: BTreeMap<String, String>, id: &str) -> Outcome<()> {
        self.inner.set_secrets(secrets, id).await
    }
    async fn remove_secrets(&self, _id: &str) -> Outcome<()> {
        Err(Failure::Host(
            "FailingRemovalCredentials.Failure".to_string(),
        ))
    }
    async fn credential_snapshot(&self, id: &str) -> Outcome<Option<CredentialSnapshot>> {
        self.inner.credential_snapshot(id).await
    }
    async fn update_refresh_token(&self, token: &str, id: &str, expected: Uuid) -> Outcome<bool> {
        self.inner.update_refresh_token(token, id, expected).await
    }
}

/// Wraps a credential store and logs removals.
pub(crate) struct LoggingCredentials {
    pub(crate) inner: Arc<InMemoryCredentialStore>,
    pub(crate) log: Arc<LockedBox<Vec<String>>>,
}

#[async_trait]
impl CredentialStore for LoggingCredentials {
    async fn secrets(&self, id: &str) -> Outcome<Option<BTreeMap<String, String>>> {
        self.inner.secrets(id).await
    }
    async fn set_secrets(&self, secrets: BTreeMap<String, String>, id: &str) -> Outcome<()> {
        self.inner.set_secrets(secrets, id).await
    }
    async fn remove_secrets(&self, id: &str) -> Outcome<()> {
        self.log.mutate(|log| log.push("secrets".to_string()));
        self.inner.remove_secrets(id).await
    }
    async fn credential_snapshot(&self, id: &str) -> Outcome<Option<CredentialSnapshot>> {
        self.inner.credential_snapshot(id).await
    }
    async fn update_refresh_token(&self, token: &str, id: &str, expected: Uuid) -> Outcome<bool> {
        self.inner.update_refresh_token(token, id, expected).await
    }
}

/// Suspends callers of `wait()` until `open()`; `is_waiting` says a caller has arrived.
pub(crate) struct Gate {
    opened: watch::Sender<bool>,
    waiting: Mutex<bool>,
}

impl Gate {
    pub(crate) fn new() -> Gate {
        Gate {
            opened: watch::Sender::new(false),
            waiting: Mutex::new(false),
        }
    }

    pub(crate) async fn is_waiting(&self) -> bool {
        *lock(&self.waiting)
    }

    /// `is_waiting` for a synchronous polling condition (the engine harness's `wait_until`).
    pub(crate) fn is_waiting_now(&self) -> bool {
        *lock(&self.waiting)
    }

    pub(crate) async fn wait(&self) {
        let mut receiver = self.opened.subscribe();
        if *receiver.borrow() {
            return;
        }
        *lock(&self.waiting) = true;
        let _ = receiver.wait_for(|opened| *opened).await;
    }

    pub(crate) async fn open(&self) {
        self.opened.send_replace(true);
        *lock(&self.waiting) = false;
    }
}

/// Swift's `LoggingSyncState`.
pub(crate) struct LoggingSyncState {
    pub(crate) inner: Arc<InMemorySyncStateStore>,
    pub(crate) log: Arc<LockedBox<Vec<String>>>,
    pub(crate) gate: Option<Arc<Gate>>,
}

#[async_trait]
impl SyncStateStore for LoggingSyncState {
    async fn token(&self, id: &str, scope: &str) -> Option<String> {
        self.inner.token(id, scope).await
    }
    async fn set_token(&self, token: Option<&str>, id: &str, scope: &str) {
        self.inner.set_token(token, id, scope).await;
    }
    async fn remove_all(&self, id: &str) {
        self.log.mutate(|log| log.push("sync".to_string()));
        if let Some(gate) = &self.gate {
            gate.wait().await;
        }
        self.inner.remove_all(id).await;
    }
}

/// The optional arguments of `AccountHarness.init`.
#[derive(Default)]
pub(crate) struct HarnessOptions {
    pub(crate) passwords: Vec<String>,
    pub(crate) failing_removal: bool,
    pub(crate) initial: EngineSettings,
    pub(crate) directory: Option<PathBuf>,
    pub(crate) sync_gate: Option<Arc<Gate>>,
    pub(crate) on_status_change: Option<StatusObserver>,
}

/// An `AccountManager` wired to temp storage, in-memory stores and a recording `applySources`.
pub(crate) struct AccountHarness {
    pub(crate) directory: PathBuf,
    _temp: Option<TempDir>,
    pub(crate) manager: Arc<AccountManager>,
    pub(crate) reconciler: Arc<FakeReconciler>,
    pub(crate) credentials: Arc<InMemoryCredentialStore>,
    pub(crate) sync_state: Arc<InMemorySyncStateStore>,
    pub(crate) settings: Arc<InMemorySettingsStorage>,
    pub(crate) sign_in: Arc<FakeSignIn>,
    pub(crate) applied: Arc<LockedBox<Vec<Vec<String>>>>,
    pub(crate) log: Arc<LockedBox<Vec<String>>>,
}

impl AccountHarness {
    pub(crate) fn new(kinds: Vec<Arc<FakeConnectorKind>>) -> AccountHarness {
        AccountHarness::with(kinds, HarnessOptions::default())
    }

    pub(crate) fn with(
        kinds: Vec<Arc<FakeConnectorKind>>,
        options: HarnessOptions,
    ) -> AccountHarness {
        let (temp, directory) = match options.directory {
            Some(directory) => (None, directory),
            None => {
                let temp = TempDir::make();
                let path = temp.path().to_path_buf();
                (Some(temp), path)
            }
        };
        let log = Arc::new(LockedBox::new(Vec::new()));
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let sync_state = Arc::new(InMemorySyncStateStore::new());
        let settings = Arc::new(InMemorySettingsStorage::new(options.initial));
        let sign_in = Arc::new(FakeSignIn::new(options.passwords));
        let applied = Arc::new(LockedBox::new(Vec::new()));
        let reconciler = Arc::new(FakeReconciler::new(
            log.clone(),
            Some(directory.join("accounts.json")),
        ));
        let mut registry = ConnectorRegistry::new();
        for kind in kinds {
            registry.register(kind);
        }
        let store: Arc<dyn CredentialStore> = if options.failing_removal {
            Arc::new(FailingRemovalCredentials {
                inner: credentials.clone(),
            })
        } else {
            Arc::new(LoggingCredentials {
                inner: credentials.clone(),
                log: log.clone(),
            })
        };
        let mut host = EngineHost::new(
            directory.clone(),
            settings.clone(),
            store,
            sign_in.clone(),
            sign_in.clone(),
            registry,
        );
        host.request_system_access = Some(Arc::new(|| Box::pin(async {})));
        let load_settings = settings.clone();
        let save_settings = settings.clone();
        let applied_sources = applied.clone();
        let update_log = log.clone();
        let manager = AccountManager::new(
            host,
            reconciler.clone(),
            Arc::new(move || {
                let settings = load_settings.clone();
                Box::pin(async move { settings.load() })
            }),
            Arc::new(move |sources| {
                let applied = applied_sources.clone();
                Box::pin(async move {
                    applied.mutate(|all| all.push(sources.iter().map(|s| s.id()).collect()));
                })
            }),
            Arc::new(move |change: SettingsChange| {
                let settings = save_settings.clone();
                let log = update_log.clone();
                Box::pin(async move {
                    let mut s = settings.load();
                    change(&mut s);
                    settings.save(&s);
                    log.mutate(|log| log.push("selections".to_string()));
                })
            }),
        )
        .with_sync_state(Arc::new(LoggingSyncState {
            inner: sync_state.clone(),
            log: log.clone(),
            gate: options.sync_gate,
        }));
        let manager = match options.on_status_change {
            Some(observer) => manager.with_on_status_change(observer),
            None => manager,
        };
        AccountHarness {
            directory,
            _temp: temp,
            manager: Arc::new(manager),
            reconciler,
            credentials,
            sync_state,
            settings,
            sign_in,
            applied,
            log,
        }
    }

    pub(crate) fn accounts_file(&self) -> PathBuf {
        self.directory.join("accounts.json")
    }
}
