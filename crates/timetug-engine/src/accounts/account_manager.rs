//! Port of `AccountManager.swift`.
//!
//! `AccountManager` is Swift's `actor`: its state is a `std::sync::Mutex<Inner>` that is locked, read or changed, and
//! unlocked again before every `.await` (conventions section 4), because a Swift actor is re-entrant at each
//! suspension point. Each `// await point (AccountManager.swift:NNN)` comment names the Swift line of the `await` so
//! a reviewer can check that no lock is held there. Swift's `current { didSet }` reports every change of the status
//! through `on_status_change`; here `modify_status` does, once per Swift statement, while it holds the lock so the
//! reports keep their order. The callback therefore must not call back into the manager.
//!
//! Cancellation: see [`crate::accounts::cancellation`]. `begin_add_account` and `begin_reauthorize` run the operation as
//! a task whose token `cancel_authorization` cancels; a pending sign-in is dropped (its guards run), and the
//! operation's own `Task.isCancelled` / `Task.checkCancellation()` checks read the same token.
//!
//! Failure text: Swift shows `error.localizedDescription` for an error it has no sentence for, and `"\(e)"` for a
//! `SourceError` case. `describe` reproduces the second exactly (`swift_source_error_description`) and uses a fixed
//! sentence for the first where the error has no text of its own; the sites are listed in `docs/rust-port/file-map.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use calendar_core::connection::{
    AuthorizationMethod, Connection, ConnectionId, ConnectorKind, ConnectorRegistry,
    CredentialField, Platform, SyncStateStore,
};
use calendar_core::failure::{Failure, Outcome};
use calendar_core::file_connection_store::{FileConnectionStore, FileStoreError, LoadResult};
use calendar_core::file_sync_state_store::FileSyncStateStore;
use calendar_core::source_types::SourceError;
use timetug_core::CalendarSource;
use timetug_core::text::case_insensitive_eq;

use crate::accounts::cancellation::{
    CancelToken, check_cancellation, current_token, in_current_or_new_task, is_task_cancelled,
    run_in_task,
};
use crate::diagnostic::{EngineDiagnostic, EngineDiagnosticSink, Level};
use crate::host::EngineHost;
use crate::settings::EngineSettings;
use crate::state::{AccountKindInfo, Authorization};

/// A boxed `Send` future: Swift's `async` closure result.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Swift's `@Sendable () async -> EngineSettings`.
pub type SettingsProvider = Arc<dyn Fn() -> BoxFuture<EngineSettings> + Send + Sync>;

/// Swift's `@Sendable ([any CalendarSource]) async -> Void`.
pub type SourceApplier = Arc<dyn Fn(Vec<Arc<dyn CalendarSource>>) -> BoxFuture<()> + Send + Sync>;

/// Swift's `@Sendable (inout EngineSettings) -> Void`.
pub type SettingsChange = Box<dyn FnOnce(&mut EngineSettings) + Send>;

/// Swift's `@Sendable (@Sendable (inout EngineSettings) -> Void) async -> Void`.
pub type SettingsUpdater = Arc<dyn Fn(SettingsChange) -> BoxFuture<()> + Send + Sync>;

/// Swift's `@Sendable (Status) -> Void`.
pub type StatusObserver = Arc<dyn Fn(Status) + Send + Sync>;

/// What a reconcile produced: the full source set to apply and the connections whose source could not be built.
pub struct SourceReconcilerUpdate {
    pub sources: Vec<Arc<dyn CalendarSource>>,
    pub failures: BTreeMap<ConnectionId, String>,
}

/// What `AccountManager` needs from the source reconciler (`SourceReconciler` implements it).
#[async_trait]
pub trait SourceReconciling: Send + Sync {
    async fn reconcile(
        &self,
        connections: &[Connection],
        system_calendar_enabled: bool,
    ) -> SourceReconcilerUpdate;
    async fn rebuild(
        &self,
        connection: &Connection,
        connections: &[Connection],
        system_calendar_enabled: bool,
    ) -> SourceReconcilerUpdate;
    /// Swift's `sourceID(forConnection:)`.
    async fn source_id_for_connection(&self, connection_id: &str) -> Option<String>;
    async fn stop(&self);
}

/// `AccountManager.WaitingStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitingStyle {
    Browser,
    InApp,
}

/// `AccountManager.Status`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub accounts: Vec<Connection>,
    pub is_working: bool,
    pub build_failures: BTreeMap<ConnectionId, String>,
    pub error_message: Option<String>,
    pub waiting_style: WaitingStyle,
}

impl Default for Status {
    fn default() -> Status {
        Status {
            accounts: Vec::new(),
            is_working: false,
            build_failures: BTreeMap::new(),
            error_message: None,
            waiting_style: WaitingStyle::Browser,
        }
    }
}

impl Status {
    /// Swift's `static let empty`.
    pub fn empty() -> Status {
        Status::default()
    }
}

/// What the do-blocks of `addAccount`, `reauthorize` and `removeAccount` can throw: a `Failure`, or the connection
/// store's own error (Swift: `FileStoreError` or a Foundation error, both reaching `describe`'s default arm).
#[derive(Debug)]
enum AccountError {
    Failure(Failure),
    Store(FileStoreError),
}

impl From<Failure> for AccountError {
    fn from(failure: Failure) -> AccountError {
        AccountError::Failure(failure)
    }
}

impl From<FileStoreError> for AccountError {
    fn from(error: FileStoreError) -> AccountError {
        AccountError::Store(error)
    }
}

impl AccountError {
    /// Swift's `catch is CancellationError`.
    fn is_cancellation(&self) -> bool {
        matches!(self, AccountError::Failure(Failure::Cancelled))
    }
}

struct Inner {
    current: Status,
    source_ids: BTreeMap<ConnectionId, String>,
    /// Swift's `authTask`: the token of the task `begin_add_account` / `begin_reauthorize` started.
    auth_task: Option<CancelToken>,
    /// Connection ids whose removal is in flight; sign-ins that produce one are discarded, not applied.
    removing: BTreeSet<ConnectionId>,
}

/// Owns the user's accounts: add, remove, re-sign-in and the system calendar switch. It persists changes, keeps
/// the source set current through the reconciler and forgets a removed account's calendar selections.
pub struct AccountManager {
    host: EngineHost,
    reconciler: Arc<dyn SourceReconciling>,
    connection_store: FileConnectionStore,
    sync_state: Arc<dyn SyncStateStore>,
    settings: SettingsProvider,
    update_settings: SettingsUpdater,
    apply_sources: SourceApplier,
    on_status_change: StatusObserver,
    diagnose: EngineDiagnosticSink,
    /// Every change of `inner.current` (accounts, `is_working`, `error_message`, `build_failures`, `waiting_style`)
    /// is reported through `on_status_change`, so a front end can show sign-in progress while it runs.
    inner: Mutex<Inner>,
}

/// Swift's `defer { current.isWorking = false }`.
struct WorkingGuard<'a>(&'a AccountManager);

impl Drop for WorkingGuard<'_> {
    fn drop(&mut self) {
        self.0.modify_status(|status| status.is_working = false);
    }
}

/// Swift's `defer { removing.remove(connectionID) }`.
struct RemovingGuard<'a> {
    manager: &'a AccountManager,
    connection_id: ConnectionId,
}

impl Drop for RemovingGuard<'_> {
    fn drop(&mut self) {
        self.manager.lock().removing.remove(&self.connection_id);
    }
}

impl AccountManager {
    /// Swift's `init` with `syncState`, `onStatusChange` and `diagnostics` at their defaults (a
    /// `FileSyncStateStore` over `sync-state.json`, a status observer that ignores everything, `host.diagnostics`);
    /// `with_sync_state`, `with_on_status_change` and `with_diagnostics` replace them before the manager is shared.
    pub fn new(
        host: EngineHost,
        reconciler: Arc<dyn SourceReconciling>,
        settings: SettingsProvider,
        apply_sources: SourceApplier,
        update_settings: SettingsUpdater,
    ) -> AccountManager {
        let connection_store = FileConnectionStore::new(host.data_directory.join("accounts.json"));
        let sync_state: Arc<dyn SyncStateStore> = Arc::new(FileSyncStateStore::new(
            host.data_directory.join("sync-state.json"),
        ));
        let diagnose = host.diagnostics.clone();
        AccountManager {
            host,
            reconciler,
            connection_store,
            sync_state,
            settings,
            update_settings,
            apply_sources,
            on_status_change: Arc::new(|_| {}),
            diagnose,
            inner: Mutex::new(Inner {
                current: Status::default(),
                source_ids: BTreeMap::new(),
                auth_task: None,
                removing: BTreeSet::new(),
            }),
        }
    }

    #[must_use]
    pub fn with_sync_state(mut self, sync_state: Arc<dyn SyncStateStore>) -> AccountManager {
        self.sync_state = sync_state;
        self
    }

    #[must_use]
    pub fn with_on_status_change(mut self, on_status_change: StatusObserver) -> AccountManager {
        self.on_status_change = on_status_change;
        self
    }

    #[must_use]
    pub fn with_diagnostics(mut self, diagnostics: EngineDiagnosticSink) -> AccountManager {
        self.diagnose = diagnostics;
        self
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// One Swift statement that assigns into `current`: runs `change`, and reports the new status when it differs
    /// from the old one (Swift's `didSet { if current != oldValue { onStatusChange(current) } }`).
    fn modify_status<R>(&self, change: impl FnOnce(&mut Status) -> R) -> R {
        let mut inner = self.lock();
        let old = inner.current.clone();
        let result = change(&mut inner.current);
        if inner.current != old {
            (self.on_status_change)(inner.current.clone());
        }
        result
    }

    pub fn status(&self) -> Status {
        self.lock().current.clone()
    }

    /// Account kinds offered by "add" (system-permission kinds are a switch, not an account). "Other CalDAV" goes last.
    pub fn available_kind_ids(&self) -> Vec<String> {
        self.available_kinds()
            .iter()
            .map(|kind| kind.id())
            .collect()
    }

    /// The "add account" choices: the registry's kinds for this platform (system kinds excluded) plus the
    /// `unconfigured` kind ids, which are listed with `is_configured` false; sorted by id with CalDAV last.
    pub fn kind_infos(
        registry: &ConnectorRegistry,
        unconfigured: &[String],
    ) -> Vec<AccountKindInfo> {
        let mut infos: BTreeMap<String, AccountKindInfo> = BTreeMap::new();
        for kind in AccountManager::available_kinds_in(registry) {
            let authorization = match kind.authorization() {
                AuthorizationMethod::Oauth => Authorization::Oauth,
                AuthorizationMethod::Password { .. } => Authorization::Password,
                AuthorizationMethod::System => Authorization::System,
            };
            infos.insert(
                kind.id(),
                AccountKindInfo::new(kind.id(), kind.display_name(), authorization, true),
            );
        }
        for id in unconfigured {
            if let Some(info) = infos.get_mut(id) {
                info.is_configured = false;
                continue;
            }
            if let Some(kind) = registry.kind(id)
                && matches!(kind.authorization(), AuthorizationMethod::System)
            {
                continue;
            }
            // The engine has no name for a kind this build did not register; the host may localize it by id.
            let name = registry
                .kind(id)
                .map(|kind| kind.display_name())
                .unwrap_or_else(|| uppercase_first(id));
            infos.insert(
                id.clone(),
                AccountKindInfo::new(id.clone(), name, Authorization::Oauth, false),
            );
        }
        // Swift sorts by id and moves "caldav" last; the map is already ordered by id.
        let sorted: Vec<AccountKindInfo> = infos.into_values().collect();
        let (caldav, others): (Vec<AccountKindInfo>, Vec<AccountKindInfo>) =
            sorted.into_iter().partition(|info| info.id == "caldav");
        others.into_iter().chain(caldav).collect()
    }

    /// The key of this account's status in `CalendarSnapshot.statuses`.
    pub fn status_key(&self, connection: &Connection) -> String {
        self.status_key_in(&self.lock(), connection)
    }

    fn status_key_in(&self, inner: &Inner, connection: &Connection) -> String {
        inner
            .source_ids
            .get(&connection.connection_id)
            .cloned()
            .unwrap_or_else(|| connection.source_id())
    }

    pub async fn start(&self) {
        // await point (AccountManager.swift:109): the store reads the file; no manager state is held.
        match self.connection_store.load().await {
            LoadResult::Loaded(list) => {
                self.modify_status(|status| status.accounts = list);
                // await point (AccountManager.swift:112)
                self.sweep_orphaned_selections().await;
            }
            LoadResult::Missing => {
                self.modify_status(|status| status.accounts = Vec::new());
                // await point (AccountManager.swift:115)
                self.sweep_orphaned_selections().await;
            }
            LoadResult::Unreadable => {
                self.modify_status(|status| status.accounts = Vec::new());
                self.modify_status(|status| {
                    status.error_message = Some(
                        "Your saved accounts could not be read, so none were loaded. Nothing was deleted."
                            .to_string(),
                    );
                });
            }
        }
        // await point (AccountManager.swift:120)
        if (self.settings)().await.system_calendar_enabled {
            self.request_system_access().await;
        }
        // await point (AccountManager.swift:121)
        self.reconcile_and_apply().await;
    }

    /// `await host.requestSystemAccess?()`.
    async fn request_system_access(&self) {
        if let Some(request) = &self.host.request_system_access {
            request().await;
        }
    }

    pub async fn add_account(&self, kind_id: &str) {
        in_current_or_new_task(self.add_account_in_task(kind_id)).await;
    }

    async fn add_account_in_task(&self, kind_id: &str) {
        let Some(kind) = self.host.registry.kind(kind_id) else {
            if self.lock().current.is_working {
                return;
            }
            self.invalid_command(
                "Unknown account type.",
                &format!("addAccount: unknown account type {kind_id}"),
            );
            return;
        };
        // Swift checks `isWorking` and sets it in one actor turn; one lock hold does the same, so two tasks started
        // by `begin_add_account` on a multi-threaded runtime cannot both pass the check.
        if !self.try_begin_work() {
            return;
        }
        self.modify_status(|status| status.error_message = None);
        let _working = WorkingGuard(self);
        let mut authorized: Option<Connection> = None;
        // await point (AccountManager.swift:136 to 154), inside `add_account_attempt`.
        let outcome = self.add_account_attempt(&kind, &mut authorized).await;
        match outcome {
            Ok(()) => {}
            Err(error) if error.is_cancellation() => {
                if let Some(authorized) = authorized {
                    // await point (AccountManager.swift:156)
                    self.discard_secrets_if_unowned(&authorized.connection_id)
                        .await;
                }
            }
            Err(error) => {
                if let Some(authorized) = authorized {
                    // await point (AccountManager.swift:158)
                    self.discard_secrets_if_unowned(&authorized.connection_id)
                        .await;
                }
                let message = describe_account_error(&error);
                self.modify_status(|status| status.error_message = Some(message));
            }
        }
    }

    /// `guard !current.isWorking` and `current.isWorking = true` as one step: false when a sign-in is already running.
    fn try_begin_work(&self) -> bool {
        let mut inner = self.lock();
        if inner.current.is_working {
            return false;
        }
        inner.current.is_working = true;
        (self.on_status_change)(inner.current.clone());
        true
    }

    /// The `do { ... }` block of `addAccount`. Returns `Ok(())` for the paths that `return` from inside it.
    async fn add_account_attempt(
        &self,
        kind: &Arc<dyn ConnectorKind>,
        authorized: &mut Option<Connection>,
    ) -> Result<(), AccountError> {
        let interaction = &self.host.interaction;
        let credentials = &self.host.credentials;
        // await point (AccountManager.swift:136): the sign-in; no lock is held.
        let connection = self
            .sign_in(kind, BTreeMap::new(), || {
                kind.authorize(&**interaction, &**credentials)
            })
            .await?;
        *authorized = Some(connection.clone());
        // A cancel that races the end of the sign-in must not leave a half-added account behind.
        check_cancellation()?;
        if self.lock().removing.contains(&connection.connection_id) {
            // await point (AccountManager.swift:143)
            self.discard_secrets_if_unowned(&connection.connection_id)
                .await;
            self.modify_status(|status| {
                status.error_message = Some("That account is being removed.".to_string());
            });
            return Ok(());
        }
        if self.is_already_added(&connection) {
            // await point (AccountManager.swift:148)
            self.discard_secrets_if_unowned(&connection.connection_id)
                .await;
            let message = format!("{} is already added.", connection.display_name);
            self.modify_status(|status| status.error_message = Some(message));
            return Ok(());
        }
        // await point (AccountManager.swift:152)
        self.connection_store.add(&connection).await?;
        self.modify_status(|status| status.accounts.push(connection));
        // await point (AccountManager.swift:154)
        self.reconcile_and_apply().await;
        Ok(())
    }

    /// `current.accounts.contains(where: { sameAccount($0, connection) })`, on one snapshot of the accounts.
    fn is_already_added(&self, added: &Connection) -> bool {
        let accounts = self.lock().current.accounts.clone();
        accounts
            .iter()
            .any(|existing| self.same_account(existing, added))
    }

    /// Two connections are one account when their kinds are the same people (the same identity family; a kind
    /// without one is its own family) and their names, the email, are equal ignoring case.
    fn same_account(&self, existing: &Connection, added: &Connection) -> bool {
        let family = |kind_id: &str| -> String {
            self.host
                .registry
                .kind(kind_id)
                .and_then(|kind| kind.as_identity_family().map(|f| f.identity_family()))
                .unwrap_or_else(|| kind_id.to_string())
        };
        family(&existing.kind_id) == family(&added.kind_id)
            && case_insensitive_eq(&existing.display_name, &added.display_name)
    }

    /// Deletes a discarded sign-in's secrets, unless a stored account has the same connection id.
    async fn discard_secrets_if_unowned(&self, connection_id: &str) {
        if self
            .lock()
            .current
            .accounts
            .iter()
            .any(|account| account.connection_id == connection_id)
        {
            return;
        }
        // await point (AccountManager.swift:176): `try?`, so a failure is ignored.
        let _ = self.host.credentials.remove_secrets(connection_id).await;
    }

    /// Starts `add_account` as a cancellable task (`cancel_authorization` stops it). Needs a `tokio` runtime.
    pub fn begin_add_account(self: &Arc<Self>, kind_id: &str) {
        let token = CancelToken::new();
        self.lock().auth_task = Some(token.clone());
        let manager = self.clone();
        let kind_id = kind_id.to_string();
        // await point (AccountManager.swift:181): the task runs `add_account` on its own.
        tokio::spawn(run_in_task(token, async move {
            manager.add_account(&kind_id).await;
        }));
    }

    pub fn begin_reauthorize(self: &Arc<Self>, connection_id: &str) {
        if self.lock().current.is_working {
            return;
        }
        let token = CancelToken::new();
        self.lock().auth_task = Some(token.clone());
        let manager = self.clone();
        let connection_id = connection_id.to_string();
        // await point (AccountManager.swift:186): the task runs `reauthorize` on its own.
        tokio::spawn(run_in_task(token, async move {
            manager.reauthorize(&connection_id).await;
        }));
    }

    pub fn cancel_authorization(&self) {
        let token = self.lock().auth_task.clone();
        if let Some(token) = token {
            token.cancel();
        }
    }

    /// Runs one sign-in. A `.password` kind signs in through the host's credential form: when a submitted form
    /// fails, the form opens again with the error and what the user typed (never the password), until it succeeds
    /// or the user cancels.
    ///
    /// Cancelling the task ends a pending `attempt` (the future is dropped, so its guards run) with
    /// `Failure::Cancelled`, as Swift's cancellation ends the `await` inside the connector.
    async fn sign_in<F, Fut>(
        &self,
        kind: &Arc<dyn ConnectorKind>,
        prefill: BTreeMap<String, String>,
        mut attempt: F,
    ) -> Outcome<Connection>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Outcome<Connection>>,
    {
        let AuthorizationMethod::Password { fields } = kind.authorization() else {
            self.modify_status(|status| status.waiting_style = WaitingStyle::Browser);
            // await point (AccountManager.swift:199)
            return until_cancelled(attempt()).await;
        };
        self.modify_status(|status| status.waiting_style = WaitingStyle::InApp);
        let help = kind
            .as_credential_prompt_help()
            .and_then(|prompt| prompt.credential_help());
        let mut values = prefill;
        let mut message: Option<String> = None;
        loop {
            // await point (AccountManager.swift:206)
            self.host
                .sign_in
                .prepare(
                    format!("{} account", kind.display_name()),
                    help.clone(),
                    values.clone(),
                    message.clone(),
                )
                .await;
            // await point (AccountManager.swift:208)
            match until_cancelled(attempt()).await {
                Ok(connection) => return Ok(connection),
                Err(Failure::Cancelled) => return Err(Failure::Cancelled),
                Err(error) => {
                    if is_task_cancelled() {
                        return Err(Failure::Cancelled);
                    }
                    // await point (AccountManager.swift:213)
                    let Some(typed) = self.host.sign_in.last_non_secret_values().await else {
                        return Err(error);
                    };
                    values = typed;
                    message = Some(AccountManager::describe_sign_in(&error, &fields));
                }
            }
        }
    }

    /// "Apple ID or app-specific password was not accepted." for a rejected password, in the kind's own words;
    /// "iCal link was not accepted." for a kind whose only field is secret; the usual description otherwise.
    pub fn describe_sign_in(error: &Failure, fields: &[CredentialField]) -> String {
        let Failure::Source(SourceError::AuthExpired) = error else {
            return describe(error);
        };
        let Some(secret) = fields.iter().find(|field| field.is_secret) else {
            return describe(error);
        };
        let name = fields
            .iter()
            .find(|field| !field.is_secret && field.key == "username")
            .or_else(|| fields.iter().find(|field| !field.is_secret));
        let Some(name) = name else {
            return format!("{} was not accepted.", secret.label);
        };
        format!(
            "{} or {} was not accepted.",
            name.label,
            lowercase_first(&secret.label)
        )
    }

    /// Order matters so an interruption never leaves a half-removed account that looks alive: the stored connection
    /// goes first, the source is then stopped through the reconciler, the calendar selections are forgotten, and
    /// secrets and sync state are deleted last, best effort.
    pub async fn remove_account(&self, connection_id: &str) {
        let connection = self
            .lock()
            .current
            .accounts
            .iter()
            .find(|account| account.connection_id == connection_id)
            .cloned();
        let Some(connection) = connection else {
            self.invalid_command(
                "That account no longer exists.",
                &format!("removeAccount: no account {connection_id}"),
            );
            return;
        };
        if !self.lock().removing.insert(connection_id.to_string()) {
            return;
        }
        let _removing = RemovingGuard {
            manager: self,
            connection_id: connection_id.to_string(),
        };
        let source_id = self.status_key(&connection);
        // await point (AccountManager.swift:241)
        if let Err(error) = self.connection_store.remove(connection_id).await {
            let message = describe_account_error(&AccountError::Store(error));
            self.modify_status(|status| status.error_message = Some(message));
            return;
        }
        self.modify_status(|status| {
            status
                .accounts
                .retain(|account| account.connection_id != connection_id);
        });
        // await point (AccountManager.swift:244)
        self.reconcile_and_apply().await;
        // await point (AccountManager.swift:245)
        (self.update_settings)(Box::new(move |settings: &mut EngineSettings| {
            settings.takeover.remove_calendars_for_source_id(&source_id);
        }))
        .await;
        // await point (AccountManager.swift:246): the gate of the sync-state fake suspends here in the tests.
        self.sync_state.remove_all(connection_id).await;
        // await point (AccountManager.swift:247)
        if let Err(error) = self.host.credentials.remove_secrets(connection_id).await {
            let message = format!(
                "The account was removed, but its saved sign-in could not be deleted: {}",
                describe(&error)
            );
            self.modify_status(|status| status.error_message = Some(message));
        }
    }

    pub async fn reauthorize(&self, connection_id: &str) {
        in_current_or_new_task(self.reauthorize_in_task(connection_id)).await;
    }

    async fn reauthorize_in_task(&self, connection_id: &str) {
        let connection = self
            .lock()
            .current
            .accounts
            .iter()
            .find(|account| account.connection_id == connection_id)
            .cloned();
        let Some(connection) = connection else {
            self.invalid_command(
                "That account no longer exists.",
                &format!("reauthorizeAccount: no account {connection_id}"),
            );
            return;
        };
        let Some(kind) = self.host.registry.kind(&connection.kind_id) else {
            self.invalid_command(
                "Unknown account type.",
                &format!(
                    "reauthorizeAccount: unknown account type {}",
                    connection.kind_id
                ),
            );
            return;
        };
        if self.lock().removing.contains(connection_id) {
            self.modify_status(|status| {
                status.error_message = Some("That account is being removed.".to_string());
            });
            return;
        }
        self.modify_status(|status| status.is_working = true);
        self.modify_status(|status| status.error_message = None);
        let _working = WorkingGuard(self);
        // await point (AccountManager.swift:269 to 287), inside `reauthorize_attempt`.
        match self.reauthorize_attempt(&kind, &connection).await {
            Ok(()) => {}
            Err(error) if error.is_cancellation() => {}
            Err(error) => {
                let message = describe_account_error(&error);
                self.modify_status(|status| status.error_message = Some(message));
            }
        }
    }

    /// The `do { ... }` block of `reauthorize`.
    async fn reauthorize_attempt(
        &self,
        kind: &Arc<dyn ConnectorKind>,
        connection: &Connection,
    ) -> Result<(), AccountError> {
        let interaction = &self.host.interaction;
        let credentials = &self.host.credentials;
        // await point (AccountManager.swift:269): the sign-in; no lock is held.
        let updated = self
            .sign_in(kind, connection.config.clone(), || {
                kind.reauthorize(connection, &**interaction, &**credentials)
            })
            .await?;
        check_cancellation()?;
        if updated.connection_id != connection.connection_id {
            // await point (AccountManager.swift:274)
            self.discard_secrets_if_unowned(&updated.connection_id)
                .await;
            self.modify_status(|status| {
                status.error_message =
                    Some("Sign-in failed: signed in as a different account.".to_string());
            });
            return Ok(());
        }
        if self.lock().removing.contains(&connection.connection_id) {
            self.modify_status(|status| {
                status.error_message = Some("That account is being removed.".to_string());
            });
            return Ok(());
        }
        // Persist first, so memory never runs ahead of the file.
        // await point (AccountManager.swift:283)
        self.connection_store.add(&updated).await?;
        self.modify_status(|status| {
            if let Some(index) = status
                .accounts
                .iter()
                .position(|account| account.connection_id == connection.connection_id)
            {
                status.accounts[index] = updated.clone();
            }
        });
        let accounts = self.lock().current.accounts.clone();
        // await point (AccountManager.swift:286): the settings read inside the argument list.
        let enabled = (self.settings)().await.system_calendar_enabled;
        // await point (AccountManager.swift:285)
        let update = self.reconciler.rebuild(&updated, &accounts, enabled).await;
        // await point (AccountManager.swift:287)
        self.finish(update).await;
        Ok(())
    }

    /// Turning it off hides the calendars but keeps their stored selections, so turning it back on is lossless.
    pub async fn set_system_calendar_enabled(&self, enabled: bool) {
        // await point (AccountManager.swift:296)
        (self.update_settings)(Box::new(move |settings: &mut EngineSettings| {
            settings.system_calendar_enabled = enabled;
        }))
        .await;
        if enabled {
            // await point (AccountManager.swift:297)
            self.request_system_access().await;
        }
        // await point (AccountManager.swift:298)
        self.reconcile_and_apply().await;
    }

    /// The engine's own settings already hold the new value: requests access when it is on, then rebuilds the
    /// sources from `settings()` as they are now, so a later external change is never overwritten.
    pub async fn system_calendar_setting_changed(&self) {
        // await point (AccountManager.swift:304)
        if (self.settings)().await.system_calendar_enabled {
            self.request_system_access().await;
        }
        // await point (AccountManager.swift:305)
        self.reconcile_and_apply().await;
    }

    /// A command naming an account or account type that does not exist: shown to the user and diagnosed (ids only).
    fn invalid_command(&self, message: &str, diagnostic: &str) {
        self.modify_status(|status| status.error_message = Some(message.to_string()));
        (self.diagnose)(EngineDiagnostic::new(
            Level::Warning,
            "accounts",
            diagnostic,
        ));
    }

    pub async fn stop(&self) {
        let token = self.lock().auth_task.clone();
        if let Some(token) = token {
            token.cancel();
        }
        // await point (AccountManager.swift:316)
        self.reconciler.stop().await;
    }

    async fn reconcile_and_apply(&self) {
        // await point (AccountManager.swift:320)
        let enabled = (self.settings)().await.system_calendar_enabled;
        let accounts = self.lock().current.accounts.clone();
        // await point (AccountManager.swift:321)
        let update = self.reconciler.reconcile(&accounts, enabled).await;
        // await point (AccountManager.swift:322)
        self.finish(update).await;
    }

    async fn finish(&self, update: SourceReconcilerUpdate) {
        self.modify_status(|status| status.build_failures = update.failures.clone());
        let accounts = self.lock().current.accounts.clone();
        let mut ids: BTreeMap<ConnectionId, String> = BTreeMap::new();
        for account in &accounts {
            // await point (AccountManager.swift:329)
            if let Some(id) = self
                .reconciler
                .source_id_for_connection(&account.connection_id)
                .await
            {
                ids.insert(account.connection_id.clone(), id);
            }
        }
        self.lock().source_ids = ids;
        for (id, message) in &update.failures {
            (self.diagnose)(EngineDiagnostic::new(
                Level::Warning,
                "accounts",
                format!("source for {id} failed: {message}"),
            ));
        }
        // await point (AccountManager.swift:335)
        (self.apply_sources)(update.sources).await;
    }

    /// Drops stored selections of account-based sources that no stored account owns (an interrupted removal).
    /// Never runs on an unreadable file and never touches system calendar (`eventkit/...`) keys.
    async fn sweep_orphaned_selections(&self) {
        let prefixes: Vec<String> = self
            .available_kinds()
            .iter()
            .map(|kind| format!("{}-", kind.id()))
            .collect();
        let known: BTreeSet<String> = {
            let inner = self.lock();
            inner
                .current
                .accounts
                .iter()
                .map(|account| self.status_key_in(&inner, account))
                .chain(
                    inner
                        .current
                        .accounts
                        .iter()
                        .map(|account| account.source_id()),
                )
                .collect()
        };
        // await point (AccountManager.swift:343)
        (self.update_settings)(Box::new(move |settings: &mut EngineSettings| {
            settings.takeover.remove_calendars_where_source_id(|id| {
                prefixes
                    .iter()
                    .any(|prefix| id.starts_with(prefix.as_str()))
                    && !known.contains(id)
            });
        }))
        .await;
    }

    fn available_kinds(&self) -> Vec<Arc<dyn ConnectorKind>> {
        AccountManager::available_kinds_in(&self.host.registry)
    }

    fn available_kinds_in(registry: &ConnectorRegistry) -> Vec<Arc<dyn ConnectorKind>> {
        let kinds: Vec<Arc<dyn ConnectorKind>> = registry
            .kinds_for(Platform::current())
            .into_iter()
            .filter(|kind| !matches!(kind.authorization(), AuthorizationMethod::System))
            .collect();
        let (caldav, others): (Vec<_>, Vec<_>) =
            kinds.into_iter().partition(|kind| kind.id() == "caldav");
        others.into_iter().chain(caldav).collect()
    }
}

/// Races `attempt` against the cancellation of the current task. A pending `attempt` is dropped when the task is
/// cancelled, so a sign-in waiting for a browser ends and its drop guards (the redirect session) run. When `attempt`
/// is ready first, its result stands, and the caller's `check_cancellation` decides about a cancel that came with it.
async fn until_cancelled<T>(attempt: impl Future<Output = Outcome<T>>) -> Outcome<T> {
    match current_token() {
        Some(token) => {
            tokio::select! {
                biased;
                result = attempt => result,
                () = token.cancelled() => Err(Failure::Cancelled),
            }
        }
        None => attempt.await,
    }
}

/// `prefix(1).lowercased() + dropFirst()`, on the first `char` (see the Rust-only test for combining and emoji text).
fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `prefix(1).uppercased() + dropFirst()`.
fn uppercase_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Swift's `"\(error)"` for a `CalendarCore.SourceError`: the case name, then its payload as Swift prints it.
fn swift_source_error_description(error: &SourceError) -> String {
    match error {
        SourceError::AuthExpired => "authExpired".to_string(),
        SourceError::Network(message) => format!("network({message:?})"),
        SourceError::RateLimited { retry_after: None } => {
            "rateLimited(retryAfter: nil)".to_string()
        }
        SourceError::RateLimited {
            retry_after: Some(seconds),
        } => format!("rateLimited(retryAfter: Optional({seconds:?}))"),
        SourceError::Server { status } => format!("server(status: {status})"),
        SourceError::InvalidResponse(message) => format!("invalidResponse({message:?})"),
        SourceError::NeedsPermission => "needsPermission".to_string(),
        SourceError::NotFound => "notFound".to_string(),
    }
}

/// The sentence for an error Swift shows through `localizedDescription` (Foundation's wording for an error with no
/// text of its own; the real wording names the error type and a number, which no Rust value has).
const OPERATION_FAILED: &str = "The operation couldn\u{2019}t be completed.";

/// Swift's `describe(_:)`.
fn describe(error: &Failure) -> String {
    match error {
        Failure::Source(SourceError::AuthExpired) => "Sign-in was not completed.".to_string(),
        Failure::Source(SourceError::InvalidResponse(message)) => {
            format!("Sign-in failed: {message}.")
        }
        Failure::Source(other) => {
            format!(
                "Sign-in failed ({}).",
                swift_source_error_description(other)
            )
        }
        // `localizedDescription` of the host's own error: the text it reported.
        Failure::Host(message) => message.clone(),
        Failure::Write(_) | Failure::Cancelled => OPERATION_FAILED.to_string(),
    }
}

fn describe_account_error(error: &AccountError) -> String {
    match error {
        AccountError::Failure(failure) => describe(failure),
        // The connection store's errors: Foundation's text names the file and the system call; a fixed sentence.
        AccountError::Store(FileStoreError::Unreadable) => {
            "Your saved accounts could not be read, so nothing was changed.".to_string()
        }
        AccountError::Store(FileStoreError::Io(_)) => {
            "Your accounts could not be saved.".to_string()
        }
    }
}

/// Swift's `String(describing: error)` for a failed source build (kept in `buildFailures` and diagnosed).
pub(crate) fn describe_for_log(error: &Failure) -> String {
    match error {
        Failure::Source(source) => swift_source_error_description(source),
        Failure::Write(write) => write.to_string(),
        Failure::Cancelled => "CancellationError()".to_string(),
        Failure::Host(message) => message.clone(),
    }
}
