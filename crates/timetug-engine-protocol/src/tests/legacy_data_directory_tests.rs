//! Review Focus 4: a data directory written by engine 1.x loads, and the wire state reports it.
//!
//! `src/tests/data/legacy-data-dir/` holds vendored copies (never edited): `accounts.json` and `sync-state.json` are
//! `Tests/PersistedFormats/1.2.0/connections.json` and `sync-state.json` of calendar-connectors, saved under the names
//! the Swift engine uses on disk (`Engine.swift` and `AccountManager.swift`: `accounts.json`, `sync-state.json`); the
//! other three are byte copies of `Tests/TimeTugEngineTests/Fixtures`.
//!
//! The engine itself (`Engine`, `AccountManager`) is ported in phase 3 tasks 2 and 4, so this test builds the state the
//! engine would publish from the persistence layer the engine uses (`FileConnectionStore`, `FileSyncStateStore`,
//! `LedgerStore`, `DedupStateStore`, the settings the host supplies) and maps it to the wire. The version that starts an
//! engine over the directory belongs to task 4 (see the note in `docs/rust-port/file-map.md`).

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use calendar_core::connection::SyncStateStore;
use calendar_core::file_connection_store::FileConnectionStore;
use calendar_core::file_sync_state_store::FileSyncStateStore;
use timetug_core::{DedupState, TakeoverSettings};
use timetug_engine::diagnostic::ignore_diagnostics;
use timetug_engine::persistence::dedup_state_store::DedupStateStore;
use timetug_engine::persistence::ledger_store::LedgerStore;
use timetug_engine::settings::EngineSettings;
use timetug_engine::state::EngineState;

use super::support::{TempDir, ts};
use crate::state_mapping::wire_message_for_state;
use crate::wire_types::WireState;

const FILES: [&str; 5] = [
    "accounts.json",
    "sync-state.json",
    "settings-legacy.json",
    "takeover-ledger.json",
    "dedup-state.json",
];

fn vendored(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/tests/data/legacy-data-dir")
        .join(name)
}

/// Runs a future that never waits on anything (the connector file stores are `async` for their trait, not for I/O).
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
    }
}

fn legacy_directory() -> TempDir {
    let dir = TempDir::make();
    for name in FILES {
        std::fs::copy(vendored(name), dir.path().join(name)).unwrap();
    }
    dir
}

fn bytes(dir: &Path, name: &str) -> Vec<u8> {
    std::fs::read(dir.join(name)).unwrap()
}

#[test]
fn the_three_engine_files_are_byte_copies_of_the_swift_fixtures() {
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Tests/TimeTugEngineTests/Fixtures");
    for name in [
        "settings-legacy.json",
        "takeover-ledger.json",
        "dedup-state.json",
    ] {
        assert_eq!(
            std::fs::read(vendored(name)).unwrap(),
            std::fs::read(fixtures.join(name)).unwrap(),
            "{name} drifted from Tests/TimeTugEngineTests/Fixtures"
        );
    }
}

#[test]
fn a_legacy_data_directory_loads_and_the_wire_state_reports_it() {
    let dir = legacy_directory();
    let before: Vec<Vec<u8>> = FILES.iter().map(|n| bytes(dir.path(), n)).collect();

    // Accounts: the brokered Microsoft account of the 1.2.0 format.
    let accounts =
        block_on(FileConnectionStore::new(dir.path().join("accounts.json")).connections());
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].kind_id, "microsoft-brokered");
    assert_eq!(
        accounts[0].connection_id,
        "7E3A9C21-4B58-4D0F-A6E2-91C8B5D40F37"
    );

    // Sync state: both calendar delta tokens survive the load.
    let sync = FileSyncStateStore::new(dir.path().join("sync-state.json"));
    let token = block_on(sync.token("7E3A9C21-4B58-4D0F-A6E2-91C8B5D40F37", "AAMkAGI2TCCC="));
    assert!(
        token
            .as_deref()
            .is_some_and(|t| t.starts_with("1800000000|https://graph.microsoft.com/"))
    );
    assert!(block_on(sync.token("7E3A9C21-4B58-4D0F-A6E2-91C8B5D40F37", "_calendars")).is_some());

    // Settings (the host's own storage, in the Mac app's legacy shape), ledger and lessons.
    let takeover: TakeoverSettings =
        serde_json::from_slice(&bytes(dir.path(), "settings-legacy.json")).unwrap();
    let settings = EngineSettings {
        takeover,
        ..EngineSettings::default()
    };
    let ledger = LedgerStore::new(dir.path(), ignore_diagnostics()).load();
    let dedup = DedupStateStore::new(dir.path(), ignore_diagnostics()).load();
    assert_eq!(ledger.key_count(), 4);
    assert_ne!(dedup, DedupState::default());
    let lessons = dedup.lessons.lessons();
    assert_eq!(lessons.len(), 1);
    assert_eq!(
        (lessons[0].title_a.as_str(), lessons[0].title_b.as_str()),
        ("sample a", "sample b")
    );

    // The state the engine publishes, on the wire.
    let mut state = EngineState::empty();
    state.accounts = accounts.clone();
    state.settings = settings.clone();
    state.ledger = ledger.clone();
    let wire: WireState =
        serde_json::from_value(wire_message_for_state(&state).payload.unwrap()).unwrap();
    assert_eq!(wire.accounts.len(), 1);
    assert_eq!(wire.accounts[0].kind_id, "microsoft-brokered");
    assert_eq!(wire.accounts[0].display_name, "dave@contoso.com");
    assert_eq!(wire.accounts[0].config["authMode"], "external");
    assert_eq!(wire.accounts[0].config["email"], "dave@contoso.com");
    assert!(!wire.settings.takeover.enabled);
    assert!(wire.settings.takeover.require_other_attendees);
    assert_eq!(wire.settings.takeover.lead_time, 120.0);
    assert_eq!(wire.ledger.count, 4);

    // Loading rewrote nothing, and nothing was added to the directory.
    let after: Vec<Vec<u8>> = FILES.iter().map(|n| bytes(dir.path(), n)).collect();
    assert_eq!(before, after);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), FILES.len());

    // The first save loses nothing: the ledger and the lessons written back read the same, and the accounts and
    // sync state are still there after the connector stores write them.
    assert!(LedgerStore::new(dir.path(), ignore_diagnostics()).save(&ledger, ts(1_800_000_000)));
    assert_eq!(
        LedgerStore::new(dir.path(), ignore_diagnostics())
            .load()
            .key_count(),
        4
    );
    assert!(DedupStateStore::new(dir.path(), ignore_diagnostics()).save(&dedup));
    assert_eq!(
        DedupStateStore::new(dir.path(), ignore_diagnostics()).load(),
        dedup
    );
    let store = FileConnectionStore::new(dir.path().join("accounts.json"));
    block_on(store.save(&accounts)).unwrap();
    assert_eq!(block_on(store.connections()), accounts);
    block_on(sync.set_token(Some("1800090000|x"), "other", "scope"));
    let reread = FileSyncStateStore::new(dir.path().join("sync-state.json"));
    assert!(
        block_on(reread.token("7E3A9C21-4B58-4D0F-A6E2-91C8B5D40F37", "AAMkAGI2TCCC=")).is_some()
    );
    assert_eq!(
        block_on(reread.token("other", "scope")).as_deref(),
        Some("1800090000|x")
    );
}

// MARK: - An Engine over the directory (Review Focus 4, end to end)

mod engine_over_the_directory {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use calendar_core::connection::{
        AuthorizationInteraction, ConnectorRegistry, CredentialField, InMemoryCredentialStore,
        OAuthRedirectSession,
    };
    use calendar_core::failure::{Failure, Outcome};
    use timetug_core::TakeoverSettings;
    use timetug_engine::command::EngineCommand;
    use timetug_engine::diagnostic::EngineDiagnostic;
    use timetug_engine::engine::Engine;
    use timetug_engine::host::{EngineHost, SignInPrompting};
    use timetug_engine::settings::{EngineSettings, InMemorySettingsStorage};

    use super::{FILES, bytes, legacy_directory};
    use crate::state_mapping::wire_message_for_state;
    use crate::wire_types::WireState;

    /// The host never shows UI in this test: every sign-in is cancelled.
    struct NoInteraction;

    #[async_trait]
    impl AuthorizationInteraction for NoInteraction {
        async fn begin_oauth_redirect(&self) -> Outcome<Box<dyn OAuthRedirectSession>> {
            Err(Failure::Cancelled)
        }

        async fn prompt_credentials(
            &self,
            _fields: &[CredentialField],
        ) -> Outcome<BTreeMap<String, String>> {
            Err(Failure::Cancelled)
        }
    }

    struct NoSignIn;

    #[async_trait]
    impl SignInPrompting for NoSignIn {
        async fn prepare(
            &self,
            _title: String,
            _help: Option<calendar_core::connection::CredentialHelp>,
            _values: BTreeMap<String, String>,
            _error: Option<String>,
        ) {
        }

        async fn last_non_secret_values(&self) -> Option<BTreeMap<String, String>> {
            None
        }
    }

    /// An engine built over a copy of the 1.x data directory with fakes (no connector kinds, in-memory settings holding
    /// the Mac app's legacy values), started: the wire state reports the accounts, settings and ledger, the lessons are
    /// loaded, and nothing on disk is lost.
    #[tokio::test(flavor = "current_thread")]
    async fn a_started_engine_reports_the_legacy_data_directory() {
        let dir = legacy_directory();
        let accounts_before = bytes(dir.path(), "accounts.json");
        let sync_before = bytes(dir.path(), "sync-state.json");
        let takeover: TakeoverSettings =
            serde_json::from_slice(&bytes(dir.path(), "settings-legacy.json")).unwrap();
        let settings = Arc::new(InMemorySettingsStorage::new(EngineSettings {
            takeover,
            ..EngineSettings::default()
        }));
        let diagnostics: Arc<Mutex<Vec<EngineDiagnostic>>> = Arc::default();
        let sink = Arc::clone(&diagnostics);
        let mut host = EngineHost::new(
            dir.path(),
            settings.clone(),
            Arc::new(InMemoryCredentialStore::new()),
            Arc::new(NoInteraction),
            Arc::new(NoSignIn),
            ConnectorRegistry::new(),
        );
        host.diagnostics = Arc::new(move |d| sink.lock().unwrap().push(d));
        let engine = Engine::new(host);
        engine.send(EngineCommand::Start).await;
        let state = engine.states().borrow().clone();
        engine.send(EngineCommand::Stop).await;

        let wire: WireState =
            serde_json::from_value(wire_message_for_state(&state).payload.unwrap()).unwrap();
        assert_eq!(wire.accounts.len(), 1);
        assert_eq!(wire.accounts[0].kind_id, "microsoft-brokered");
        assert_eq!(
            wire.accounts[0].connection_id,
            "7E3A9C21-4B58-4D0F-A6E2-91C8B5D40F37"
        );
        assert_eq!(wire.accounts[0].config["authMode"], "external");
        assert!(!wire.settings.takeover.enabled);
        assert!(wire.settings.takeover.require_other_attendees);
        assert_eq!(wire.settings.takeover.lead_time, 120.0);
        assert_eq!(wire.ledger.count, 4);
        assert!(
            diagnostics
                .lock()
                .unwrap()
                .iter()
                .any(|d| d.category == "ledger" && d.message == "ledger loaded with 4 keys")
        );
        // Nothing was lost: the account list and the sync state are byte for byte as they were, and the data directory
        // holds the five files plus nothing else the engine should not write.
        assert_eq!(bytes(dir.path(), "accounts.json"), accounts_before);
        assert_eq!(bytes(dir.path(), "sync-state.json"), sync_before);
        for name in FILES {
            assert!(dir.path().join(name).is_file(), "{name} is gone");
        }
    }
}
