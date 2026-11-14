//! Port of `Accounts/SourceReconcilerTests.swift` (9 tests). `settle()` sleeps 80 ms on the paused test clock.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use calendar_core::connection::Connection;
use calendar_core::connection::ConnectorRegistry;
use calendar_core::failure::{Failure, Outcome};
use calendar_core::time::DateInterval;
use timetug_core::{CalendarInfo, CalendarSource, SourceChange, SourceError, TimeTugCalendarEvent};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::accounts::account_manager::{
    AccountManager, BoxFuture, SettingsChange, SourceReconciling,
};
use crate::accounts::source_reconciler::SourceReconciler;
use crate::host::EngineHost;
use crate::settings::{EngineSettings, InMemorySettingsStorage, SettingsStorage};

use super::support::LockedBox;
use super::support::account_fakes::{FakeConnectorKind, FakeSignIn};

/// Swift's `ListeningSource`: one stream, handed to the first `changes()` call; `yield_change` and `finish` are the
/// stream's continuation.
struct ListeningSource {
    id: String,
    sender: Mutex<Option<UnboundedSender<SourceChange>>>,
    receiver: Mutex<Option<UnboundedReceiver<SourceChange>>>,
    started: LockedBox<usize>,
}

impl ListeningSource {
    fn new(id: &str) -> Arc<ListeningSource> {
        let (sender, receiver) = unbounded_channel();
        Arc::new(ListeningSource {
            id: id.to_string(),
            sender: Mutex::new(Some(sender)),
            receiver: Mutex::new(Some(receiver)),
            started: LockedBox::new(0),
        })
    }

    fn listener_started(&self) -> usize {
        self.started.get()
    }

    fn yield_change(&self, change: SourceChange) {
        if let Some(sender) = self.sender.lock().unwrap().as_ref() {
            let _ = sender.send(change);
        }
    }

    fn finish(&self) {
        *self.sender.lock().unwrap() = None;
    }
}

#[async_trait]
impl CalendarSource for ListeningSource {
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
    fn changes(&self) -> UnboundedReceiver<SourceChange> {
        self.started.mutate(|count| *count += 1);
        match self.receiver.lock().unwrap().take() {
            Some(receiver) => receiver,
            None => unbounded_channel().1,
        }
    }
}

/// The identity of a source, for Swift's `===`.
fn same(a: &Arc<dyn CalendarSource>, b: &Arc<ListeningSource>) -> bool {
    std::ptr::eq(Arc::as_ptr(a) as *const (), Arc::as_ptr(b) as *const ())
}

struct ReconcilerRig {
    built: Arc<LockedBox<BTreeMap<String, Arc<ListeningSource>>>>,
    change_count: Arc<LockedBox<usize>>,
    fail_next: Arc<LockedBox<BTreeSet<String>>>,
    reconciler: SourceReconciler,
}

impl ReconcilerRig {
    fn new() -> ReconcilerRig {
        ReconcilerRig::failing(&[])
    }

    fn failing(failing: &[&str]) -> ReconcilerRig {
        let built = Arc::new(LockedBox::new(BTreeMap::new()));
        let change_count = Arc::new(LockedBox::new(0));
        let fail_next = Arc::new(LockedBox::new(BTreeSet::new()));
        let failing: BTreeSet<String> = failing.iter().map(|s| (*s).to_string()).collect();
        let (build_map, build_fail) = (built.clone(), fail_next.clone());
        let counter = change_count.clone();
        let reconciler = SourceReconciler::new(
            Arc::new(move |c: &Connection| {
                if failing.contains(&c.connection_id) || build_fail.get().contains(&c.connection_id)
                {
                    return Err(Failure::from(SourceError::AuthExpired));
                }
                let source = ListeningSource::new(&c.source_id());
                build_map.mutate(|map| map.insert(c.connection_id.clone(), source.clone()));
                Ok(source as Arc<dyn CalendarSource>)
            }),
            Arc::new(|| ListeningSource::new("eventkit") as Arc<dyn CalendarSource>),
            Arc::new(move |_| -> BoxFuture<()> {
                let counter = counter.clone();
                Box::pin(async move { counter.mutate(|count| *count += 1) })
            }),
        );
        ReconcilerRig {
            built,
            change_count,
            fail_next,
            reconciler,
        }
    }

    fn source(&self, connection_id: &str) -> Arc<ListeningSource> {
        self.built.get()[connection_id].clone()
    }
}

fn a() -> Connection {
    Connection::new("google", "1", "a@x.test")
}

fn b() -> Connection {
    Connection::new("google", "2", "b@x.test")
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(80)).await;
}

fn source_ids(sources: &[Arc<dyn CalendarSource>]) -> Vec<String> {
    sources.iter().map(|s| s.id()).collect()
}

fn events_changed(source: &ListeningSource) -> SourceChange {
    SourceChange::EventsChanged {
        source_id: source.id.clone(),
        calendar_ids: None,
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn builds_system_source_first_then_accounts_and_keys_by_source_id() {
    let rig = ReconcilerRig::new();
    let update = rig.reconciler.reconcile(&[a(), b()], true).await;
    assert_eq!(
        source_ids(&update.sources),
        ["eventkit", "google-1", "google-2"]
    );
    assert_eq!(
        rig.reconciler.source_id_for_connection("1").await,
        Some(a().source_id())
    );
    rig.reconciler.stop().await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn system_source_disabled_is_left_out() {
    let rig = ReconcilerRig::new();
    let update = rig.reconciler.reconcile(&[a()], false).await;
    assert_eq!(source_ids(&update.sources), ["google-1"]);
    rig.reconciler.stop().await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn unchanged_sources_keep_their_instance_and_listener() {
    let rig = ReconcilerRig::new();
    rig.reconciler.reconcile(&[a()], false).await;
    let first = rig.source("1");
    let update = rig.reconciler.reconcile(&[a(), b()], false).await;
    let kept = update
        .sources
        .iter()
        .find(|s| s.id() == "google-1")
        .unwrap();
    assert!(same(kept, &first));
    settle().await;
    assert_eq!(first.listener_started(), 1);
    rig.reconciler.stop().await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn removed_source_stops_its_listener() {
    let rig = ReconcilerRig::new();
    rig.reconciler.reconcile(&[a()], false).await;
    let source = rig.source("1");
    settle().await;
    rig.reconciler.reconcile(&[], false).await;
    source.yield_change(events_changed(&source));
    settle().await;
    assert_eq!(rig.change_count.get(), 0);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_change_from_a_source_triggers_on_change() {
    let rig = ReconcilerRig::new();
    rig.reconciler.reconcile(&[a()], false).await;
    settle().await;
    let source = rig.source("1");
    source.yield_change(events_changed(&source));
    settle().await;
    assert_eq!(rig.change_count.get(), 1);
    rig.reconciler.stop().await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn failing_build_is_reported_and_does_not_stop_others() {
    let rig = ReconcilerRig::failing(&["1"]);
    let update = rig.reconciler.reconcile(&[a(), b()], false).await;
    assert_eq!(source_ids(&update.sources), ["google-2"]);
    assert!(update.failures.contains_key("1"));
    rig.reconciler.stop().await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn rebuild_replaces_the_instance_and_restarts_the_listener() {
    let rig = ReconcilerRig::new();
    rig.reconciler.reconcile(&[a()], false).await;
    let old = rig.source("1");
    old.finish(); // like a source whose stream ended after .sourceFailed
    let update = rig.reconciler.rebuild(&a(), &[a()], false).await;
    let new = rig.source("1");
    assert!(!Arc::ptr_eq(&old, &new));
    assert!(same(update.sources.first().unwrap(), &new));
    settle().await;
    new.yield_change(events_changed(&new));
    settle().await;
    assert_eq!(rig.change_count.get(), 1);
    rig.reconciler.stop().await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_failed_rebuild_keeps_the_working_source_and_its_listener() {
    let rig = ReconcilerRig::new();
    rig.reconciler.reconcile(&[a()], false).await;
    let old = rig.source("1");
    settle().await;
    rig.fail_next.set(BTreeSet::from(["1".to_string()]));
    let update = rig.reconciler.rebuild(&a(), &[a()], false).await;
    assert_eq!(update.sources.len(), 1);
    assert!(same(update.sources.first().unwrap(), &old));
    assert!(update.failures.contains_key("1"));
    old.yield_change(events_changed(&old));
    settle().await;
    assert_eq!(rig.change_count.get(), 1);
    rig.reconciler.stop().await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn account_manager_works_end_to_end_with_the_real_reconciler() {
    let dir = super::support::TempDir::make();
    let credentials = Arc::new(calendar_core::connection::InMemoryCredentialStore::new());
    let sign_in = Arc::new(FakeSignIn::new(Vec::new()));
    let settings = Arc::new(InMemorySettingsStorage::new(EngineSettings::default()));
    let mut registry = ConnectorRegistry::new();
    registry.register(FakeConnectorKind::new());
    let mut host = EngineHost::new(
        dir.path().to_path_buf(),
        settings.clone(),
        credentials,
        sign_in.clone(),
        sign_in,
        registry,
    );
    host.request_system_access = Some(Arc::new(|| Box::pin(async {})));
    let applied: Arc<LockedBox<Vec<Vec<String>>>> = Arc::new(LockedBox::new(Vec::new()));
    let reconciler = Arc::new(SourceReconciler::new(
        Arc::new(|c: &Connection| {
            Ok(ListeningSource::new(&c.source_id()) as Arc<dyn CalendarSource>)
        }),
        Arc::new(|| ListeningSource::new("eventkit") as Arc<dyn CalendarSource>),
        Arc::new(|_| -> BoxFuture<()> { Box::pin(async {}) }),
    ));
    let (load_settings, save_settings, applied_sources) =
        (settings.clone(), settings.clone(), applied.clone());
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
            Box::pin(async move {
                let mut s = settings.load();
                change(&mut s);
                settings.save(&s);
            })
        }),
    );
    manager.add_account("google").await;
    assert_eq!(
        applied.get().last(),
        Some(&vec!["eventkit".to_string(), "google-1".to_string()])
    );
    assert_eq!(
        reconciler.source_id_for_connection("1").await,
        Some("google-1".to_string())
    );
    manager.remove_account("1").await;
    assert_eq!(applied.get().last(), Some(&vec!["eventkit".to_string()]));
    assert_eq!(reconciler.source_id_for_connection("1").await, None);
    reconciler.stop().await;
}
