//! Port of `SourceReconciler.swift`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use calendar_core::connection::{Connection, ConnectionId};
use calendar_core::failure::Outcome;
use timetug_core::{CalendarSource, SourceChange};
use uuid::Uuid;

use crate::accounts::account_manager::{
    BoxFuture, SourceReconcilerUpdate, SourceReconciling, describe_for_log,
};
use crate::accounts::cancellation::CancelToken;
use crate::host::BuildSystemSource;

/// Swift's `@Sendable (Connection) throws -> any CalendarSource`.
pub type BuildAccount = Arc<dyn Fn(&Connection) -> Outcome<Arc<dyn CalendarSource>> + Send + Sync>;

/// Swift's `@Sendable (SourceChange) async -> Void`.
pub type OnChange = Arc<dyn Fn(SourceChange) -> BoxFuture<()> + Send + Sync>;

const SYSTEM_KEY: &str = "\u{0}system";

struct Entry {
    source: Arc<dyn CalendarSource>,
    /// Swift's `listener: Task<Void, Never>`; cancelling the token ends the listener.
    listener: CancelToken,
}

/// Decides which sources exist (the system calendar if enabled, then one per stored account), keeps each running
/// source's change listener alive, and replaces sources when asked. Sources are identified by their own `id`.
///
/// Swift's actor: every method is synchronous inside the actor (no `await`), so the whole `apply` runs under the
/// `std` mutex. The listeners are spawned tasks, so `reconcile` and `rebuild` need a `tokio` runtime.
pub struct SourceReconciler {
    build_account: BuildAccount,
    build_system_source: BuildSystemSource,
    on_change: OnChange,
    /// Keyed by connection id, or `SYSTEM_KEY`.
    entries: Mutex<BTreeMap<String, Entry>>,
    /// Every running listener, reachable without the lock so `cancel_all()` can run from a destructor.
    listeners: Arc<TaskBag>,
}

impl SourceReconciler {
    pub fn new(
        build_account: BuildAccount,
        build_system_source: BuildSystemSource,
        on_change: OnChange,
    ) -> SourceReconciler {
        SourceReconciler {
            build_account,
            build_system_source,
            on_change,
            entries: Mutex::new(BTreeMap::new()),
            listeners: Arc::new(TaskBag::default()),
        }
    }

    fn entries(&self) -> MutexGuard<'_, BTreeMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Synchronous teardown for an owner that is going away without `stop()`.
    pub fn cancel_all(&self) {
        self.listeners.cancel_all();
    }

    fn apply(
        &self,
        connections: &[Connection],
        system_calendar_enabled: bool,
        rebuilding: Option<&str>,
    ) -> SourceReconcilerUpdate {
        let mut entries = self.entries();
        let mut wanted: Vec<String> = Vec::new();
        let mut failures: BTreeMap<ConnectionId, String> = BTreeMap::new();
        if system_calendar_enabled {
            wanted.push(SYSTEM_KEY.to_string());
            if !entries.contains_key(SYSTEM_KEY) {
                let entry = self.start((self.build_system_source)());
                entries.insert(SYSTEM_KEY.to_string(), entry);
            }
        }
        for connection in connections {
            let key = connection.connection_id.clone();
            if Some(key.as_str()) == rebuilding {
                // Build the replacement first: a failed build must not take down the source that still works.
                match (self.build_account)(connection) {
                    Ok(source) => {
                        let replacement = self.start(source);
                        if let Some(old) = entries.get(&key) {
                            old.listener.cancel();
                        }
                        entries.insert(key.clone(), replacement);
                    }
                    Err(error) => {
                        failures.insert(key.clone(), describe_for_log(&error));
                    }
                }
            } else if !entries.contains_key(&key) {
                match (self.build_account)(connection) {
                    Ok(source) => {
                        let entry = self.start(source);
                        entries.insert(key.clone(), entry);
                    }
                    Err(error) => {
                        failures.insert(key.clone(), describe_for_log(&error));
                        continue;
                    }
                }
            }
            if !entries.contains_key(&key) {
                continue;
            }
            wanted.push(key);
        }
        let keep: BTreeSet<&String> = wanted.iter().collect();
        entries.retain(|key, entry| {
            if keep.contains(key) {
                true
            } else {
                entry.listener.cancel();
                false
            }
        });
        SourceReconcilerUpdate {
            sources: wanted
                .iter()
                .filter_map(|key| entries.get(key).map(|entry| entry.source.clone()))
                .collect(),
            failures,
        }
    }

    fn start(&self, source: Arc<dyn CalendarSource>) -> Entry {
        let on_change = self.on_change.clone();
        let listeners = self.listeners.clone();
        let id = Uuid::new_v4();
        let listener = CancelToken::new();
        let token = listener.clone();
        let listening = source.clone();
        tokio::spawn(async move {
            let mut changes = listening.changes();
            loop {
                // Cancelling ends the loop even while it waits for the next change (Swift's `for await` over an
                // `AsyncStream` stops when its task is cancelled), and wins over a buffered change.
                let change = tokio::select! {
                    biased;
                    () = token.cancelled() => break,
                    change = changes.recv() => change,
                };
                let Some(change) = change else { break };
                // A removed source's listener may still see a buffered change before cancellation lands.
                if token.is_cancelled() {
                    break;
                }
                // await point (SourceReconciler.swift:90): no lock is held; `on_change` runs to completion.
                on_change(change).await;
            }
            listeners.remove(id);
        });
        self.listeners.insert(listener.clone(), id);
        Entry { source, listener }
    }
}

#[async_trait]
impl SourceReconciling for SourceReconciler {
    async fn reconcile(
        &self,
        connections: &[Connection],
        system_calendar_enabled: bool,
    ) -> SourceReconcilerUpdate {
        self.apply(connections, system_calendar_enabled, None)
    }

    /// After a successful re-sign-in: builds a fresh source for `connection` and restarts its listener (a source's
    /// change stream ends after `.sourceFailed`, so an unchanged id would otherwise never poll again).
    async fn rebuild(
        &self,
        connection: &Connection,
        connections: &[Connection],
        system_calendar_enabled: bool,
    ) -> SourceReconcilerUpdate {
        self.apply(
            connections,
            system_calendar_enabled,
            Some(&connection.connection_id),
        )
    }

    async fn source_id_for_connection(&self, connection_id: &str) -> Option<String> {
        self.entries().get(connection_id).map(|e| e.source.id())
    }

    async fn stop(&self) {
        let mut entries = self.entries();
        for entry in entries.values() {
            entry.listener.cancel();
        }
        entries.clear();
        drop(entries);
        self.listeners.cancel_all();
    }
}

#[derive(Default)]
struct TaskBagState {
    tasks: BTreeMap<Uuid, CancelToken>,
    finished: BTreeSet<Uuid>,
}

/// Lock-protected listeners that can be cancelled from any context, including a destructor.
#[derive(Default)]
pub struct TaskBag {
    state: Mutex<TaskBagState>,
}

impl TaskBag {
    fn lock(&self) -> MutexGuard<'_, TaskBagState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn insert(&self, task: CancelToken, id: Uuid) {
        let mut state = self.lock();
        // The task may already have ended (and removed itself) before it was inserted.
        if !state.finished.remove(&id) {
            state.tasks.insert(id, task);
        }
    }

    pub fn remove(&self, id: Uuid) {
        let mut state = self.lock();
        if state.tasks.remove(&id).is_none() {
            state.finished.insert(id);
        }
    }

    pub fn cancel_all(&self) {
        let all: Vec<CancelToken> = std::mem::take(&mut self.lock().tasks)
            .into_values()
            .collect();
        for token in all {
            token.cancel();
        }
    }
}
