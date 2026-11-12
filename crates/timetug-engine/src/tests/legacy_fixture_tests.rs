//! `LegacyFixtureTests.swift`: the files the Mac app wrote (`fixtures/engine`, found through
//! `CARGO_MANIFEST_DIR`) load through the persistence layer, are honoured, are never rewritten by a load, and corrupt
//! copies load empty with a diagnostic (the tests above the `Engine` line); then an engine started over a data directory
//! the Mac app wrote (Review Focus 1 and 2 of the Swift suite; Review Focus 4 of the Rust plan) reads the Mac files,
//! honours what they say, never rewrites what it only reads, and starts even when a file is corrupt (the five tests
//! below it, which need `EngineHarness`).

use std::collections::BTreeSet;
use std::sync::Arc;

use calendar_core::connection::Connection;
use calendar_core::file_connection_store::FileConnectionStore;
use calendar_core::model::CalendarEvent;
use calendar_core::time::add_seconds;
use timetug_core::takeover::ledger::TakeoverLedger;
use timetug_core::{DedupState, SourceStatus, TakeoverSettings, TimeTugCalendarEvent};

use super::support::engine_harness::{EngineHarness, HarnessOptions, sample};
use super::support::{Fixtures, LockedBox, TempDir, ts};
use crate::command::EngineCommand;
use crate::diagnostic::{EngineDiagnostic, Level, ignore_diagnostics};
use crate::persistence::dedup_state_store::DedupStateStore;
use crate::persistence::file_settings_storage::FileSettingsStorage;
use crate::persistence::ledger_store::LedgerStore;
use crate::settings::{EngineSettings, SettingsStorage};

/// The fixtures' events start at 2027-01-15 08:00 UTC (Unix 1_800_000_000) and end 30 minutes later.
fn meeting(id: &str, title: &str) -> TimeTugCalendarEvent {
    TimeTugCalendarEvent::new(
        CalendarEvent::new(id, "cal", title, ts(1_800_000_000), ts(1_800_001_800)),
        "s",
    )
}

fn collecting_sink() -> (
    Arc<LockedBox<Vec<EngineDiagnostic>>>,
    crate::diagnostic::EngineDiagnosticSink,
) {
    let box_ = Arc::new(LockedBox::new(Vec::new()));
    let sink = Arc::clone(&box_);
    (box_, Arc::new(move |d| sink.mutate(|l| l.push(d))))
}

#[test]
fn the_fixture_directory_holds_the_three_mac_files() {
    for name in [
        "takeover-ledger.json",
        "dedup-state.json",
        "settings-legacy.json",
    ] {
        assert!(Fixtures::path(name).is_file(), "{name} is missing");
    }
}

#[test]
fn the_mac_ledger_loads_and_is_honoured() {
    let dir = TempDir::make_copying("takeover-ledger.json");
    let ledger = LedgerStore::new(dir.path(), ignore_diagnostics()).load();
    // The ledger's keys are "s/a/<start>" (fired) and "s/b/<start>" (snoozed until 08:00 minus 5 minutes), each also
    // under a content key.
    assert_eq!(ledger.key_count(), 4);
    let a = meeting("a", "Sample meeting A");
    let b = meeting("b", "Sample meeting B");
    assert!(ledger.has_fired(&a));
    assert!(!ledger.has_fired(&b));
    assert!(ledger.is_snoozed(&b));
    assert!(!ledger.is_snoozed(&a));
}

#[test]
fn the_mac_dedup_state_loads_and_its_lesson_is_honoured() {
    let dir = TempDir::make_copying("dedup-state.json");
    let state = DedupStateStore::new(dir.path(), ignore_diagnostics()).load();
    assert_ne!(state, DedupState::default());
    // The lesson says "sample a" on s/x and "sample b" on s/y are the same meeting.
    let lessons = state.lessons.lessons();
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0].title_a, "sample a");
    assert_eq!(lessons[0].title_b, "sample b");
    assert_eq!(lessons[0].calendar_key_a, "s/x");
    assert_eq!(lessons[0].calendar_key_b, "s/y");
}

#[test]
fn loading_never_rewrites_the_files_it_only_reads() {
    let dir = TempDir::make();
    for name in [
        "takeover-ledger.json",
        "dedup-state.json",
        "settings-legacy.json",
    ] {
        std::fs::copy(Fixtures::path(name), dir.path().join(name)).unwrap();
    }
    let before: Vec<Vec<u8>> = [
        "takeover-ledger.json",
        "dedup-state.json",
        "settings-legacy.json",
    ]
    .iter()
    .map(|name| std::fs::read(dir.path().join(name)).unwrap())
    .collect();
    let _ = LedgerStore::new(dir.path(), ignore_diagnostics()).load();
    let _ = DedupStateStore::new(dir.path(), ignore_diagnostics()).load();
    let _ = FileSettingsStorage::new(dir.path(), ignore_diagnostics()).load();
    let after: Vec<Vec<u8>> = [
        "takeover-ledger.json",
        "dedup-state.json",
        "settings-legacy.json",
    ]
    .iter()
    .map(|name| std::fs::read(dir.path().join(name)).unwrap())
    .collect();
    assert_eq!(before, after);
    // Only the three files exist: nothing was written next to them.
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
}

#[test]
fn legacy_settings_decode() {
    let data = std::fs::read(Fixtures::path("settings-legacy.json")).unwrap();
    let takeover: TakeoverSettings = serde_json::from_slice(&data).unwrap();
    assert!(!takeover.enabled);
    assert!(takeover.require_other_attendees);
    assert_eq!(takeover.lead_time, 120.0);
}

#[test]
fn corrupt_ledger_and_dedup_state_load_empty_with_diagnostics() {
    let dir = TempDir::make();
    std::fs::write(dir.path().join("takeover-ledger.json"), b"{not json").unwrap();
    std::fs::write(dir.path().join("dedup-state.json"), b"{\"lessons\": 7").unwrap();
    let (diagnostics, sink) = collecting_sink();
    let ledger = LedgerStore::new(dir.path(), Arc::clone(&sink)).load();
    let state = DedupStateStore::new(dir.path(), sink).load();
    assert_eq!(ledger, TakeoverLedger::new());
    assert_eq!(state, DedupState::default());
    let reported = diagnostics.get();
    assert!(
        reported
            .iter()
            .any(|d| d.level == Level::Warning && d.category == "ledger")
    );
    assert!(
        reported
            .iter()
            .any(|d| d.level == Level::Warning && d.category == "dedup")
    );
}

// MARK: The engine over a Mac data directory

/// Settings storage over a Mac-format settings file: `load` decodes it (legacy keys included), `save` counts and
/// writes. Stands in for a host that keeps its settings on disk.
struct LegacyFixtureSettingsStorage {
    path: std::path::PathBuf,
    saves: LockedBox<usize>,
}

impl SettingsStorage for LegacyFixtureSettingsStorage {
    fn load(&self) -> EngineSettings {
        let takeover = std::fs::read(&self.path)
            .ok()
            .and_then(|data| serde_json::from_slice::<TakeoverSettings>(&data).ok())
            .unwrap_or_default();
        EngineSettings {
            takeover,
            inference_enabled: false,
            system_calendar_enabled: false,
        }
    }

    fn save(&self, settings: &EngineSettings) {
        self.saves.mutate(|saves| *saves += 1);
        let _ = std::fs::write(&self.path, serde_json::to_vec(&settings.takeover).unwrap());
    }
}

fn meeting_start() -> jiff::Timestamp {
    ts(1_800_000_000)
}

fn library_meeting(id: &str, title: &str, calendar: &str, starts_in: f64) -> CalendarEvent {
    let start = add_seconds(meeting_start(), starts_in);
    CalendarEvent::new(id, calendar, title, start, add_seconds(start, 1800.0))
}

/// A data directory holding the given fixtures and an `accounts.json` with one "fake" account, "c1".
async fn data_directory(copying: &[&str]) -> TempDir {
    let dir = TempDir::make();
    for name in copying {
        std::fs::copy(Fixtures::path(name), dir.path().join(name)).unwrap();
    }
    FileConnectionStore::new(dir.path().join("accounts.json"))
        .add(&Connection::new("fake", "c1", "c1@x.test"))
        .await
        .unwrap();
    dir
}

fn bytes(dir: &TempDir, name: &str) -> Vec<u8> {
    std::fs::read(dir.path().join(name)).unwrap()
}

// MARK: Mac files load and are honoured

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn mac_ledger_dedup_state_and_accounts_load_and_are_honoured() {
    let dir = data_directory(&["takeover-ledger.json", "dedup-state.json"]).await;
    let accounts_before = bytes(&dir, "accounts.json");
    // The ledger's keys are "s/a/<start>" (fired) and "s/b/<start>" (snoozed until 07:55); the dedup lesson says
    // "sample a" on s/x and "sample b" on s/y are the same meeting.
    let h = EngineHarness::with(HarnessOptions {
        events: vec![
            library_meeting("a", "Sample meeting A", "cal", 0.0),
            library_meeting("b", "Sample meeting B", "cal", 0.0),
            library_meeting("x1", "Sample A", "x", 3600.0),
            library_meeting("y1", "Sample B", "y", 3600.0),
        ],
        directory: Some(dir.path().to_path_buf()),
        source_ids: [("c1".to_string(), "s".to_string())].into_iter().collect(),
        clock_start: Some(add_seconds(meeting_start(), -600.0)),
        ..HarnessOptions::default()
    })
    .await;
    h.engine.send(EngineCommand::Start).await;
    let mut state = h
        .wait_for_state(|s| s.statuses.get("s") == Some(&SourceStatus::Ok))
        .await;

    // Accounts load.
    let ids: Vec<&str> = state
        .accounts
        .iter()
        .map(|a| a.connection_id.as_str())
        .collect();
    assert_eq!(ids, vec!["c1"]);
    assert!(state.account_status.error_message.is_none());
    assert!(
        h.diagnostics
            .get()
            .iter()
            .any(|d| d.category == "ledger" && d.message == "ledger loaded with 4 keys")
    );

    // The dedup lesson merges the two look-alikes (without it they are only manual candidates).
    let merged = state
        .events
        .iter()
        .find(|e| e.participants().len() == 2)
        .unwrap();
    let titles: BTreeSet<String> = merged
        .participants()
        .iter()
        .map(|m| m.title.clone())
        .collect();
    assert_eq!(
        titles,
        BTreeSet::from(["Sample A".to_string(), "Sample B".to_string()])
    );
    assert_eq!(state.events.len(), 3);

    // The fired meeting never takes over again; the snoozed one is armed for its snooze end and fires then.
    let a = state
        .events
        .iter()
        .find(|e| e.source_event_id() == "a")
        .unwrap()
        .clone();
    assert!(state.ledger.has_fired(&a));
    assert_eq!(
        state
            .next_takeover
            .as_ref()
            .map(|n| n.event.source_event_id().to_string()),
        Some("b".to_string())
    );
    assert_eq!(
        state.next_takeover.as_ref().map(|n| n.fire_at),
        Some(add_seconds(meeting_start(), -300.0))
    );
    h.clock.advance(300.0).await;
    h.wait_until("the snoozed takeover", || {
        h.takeover_due_ids() == vec!["b".to_string()]
    })
    .await;
    let b = state
        .events
        .iter()
        .find(|e| e.source_event_id() == "b")
        .unwrap()
        .clone();
    h.engine
        .send(EngineCommand::Dismiss { event_id: b.id() })
        .await;
    h.clock.advance(600.0).await;
    h.settle().await;
    assert_eq!(h.takeover_due_ids(), vec!["b".to_string()]);
    state = h.engine.current_state();
    assert!(state.ledger.has_fired(&a) && state.ledger.has_fired(&b));

    // accounts.json is only read: byte-identical after start, takeovers and stop.
    h.engine.send(EngineCommand::Stop).await;
    assert_eq!(bytes(&dir, "accounts.json"), accounts_before);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn without_the_mac_dedup_state_the_look_alikes_stay_separate() {
    let dir = data_directory(&[]).await;
    let h = EngineHarness::with(HarnessOptions {
        events: vec![
            library_meeting("x1", "Sample A", "x", 3600.0),
            library_meeting("y1", "Sample B", "y", 3600.0),
        ],
        directory: Some(dir.path().to_path_buf()),
        source_ids: [("c1".to_string(), "s".to_string())].into_iter().collect(),
        clock_start: Some(add_seconds(meeting_start(), -600.0)),
        ..HarnessOptions::default()
    })
    .await;
    h.engine.send(EngineCommand::Start).await;
    let state = h
        .wait_for_state(|s| s.statuses.get("s") == Some(&SourceStatus::Ok))
        .await;
    assert_eq!(state.events.len(), 2);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn legacy_settings_decode_and_are_never_written_back_on_start() {
    let dir = data_directory(&["settings-legacy.json"]).await;
    let settings_before = bytes(&dir, "settings-legacy.json");
    let accounts_before = bytes(&dir, "accounts.json");
    let storage = Arc::new(LegacyFixtureSettingsStorage {
        path: dir.path().join("settings-legacy.json"),
        saves: LockedBox::new(0),
    });
    let h = EngineHarness::with(HarnessOptions {
        events: vec![sample("e1", 600.0)],
        directory: Some(dir.path().to_path_buf()),
        host_settings: Some(storage.clone() as Arc<dyn SettingsStorage>),
        ..HarnessOptions::default()
    })
    .await;
    h.engine.send(EngineCommand::Start).await;
    let state = h
        .wait_for_state(|s| s.statuses.get("fake-c1") == Some(&SourceStatus::Ok))
        .await;

    assert!(!state.settings.takeover.enabled);
    assert!(state.settings.takeover.require_other_attendees);
    assert_eq!(state.settings.takeover.lead_time, 120.0);
    assert!(state.next_takeover.is_none());
    h.engine.send(EngineCommand::Stop).await;
    assert_eq!(storage.saves.get(), 0);
    assert_eq!(bytes(&dir, "settings-legacy.json"), settings_before);
    assert_eq!(bytes(&dir, "accounts.json"), accounts_before);
}

// MARK: Review Focus 2: corrupt or unreadable files

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn corrupt_ledger_and_dedup_state_load_empty_with_diagnostics_and_the_engine_still_starts() {
    let dir = data_directory(&[]).await;
    std::fs::write(dir.path().join("takeover-ledger.json"), b"{not json").unwrap();
    std::fs::write(dir.path().join("dedup-state.json"), b"{\"lessons\": 7").unwrap();
    let h = EngineHarness::with(HarnessOptions {
        events: vec![sample("e1", 600.0)],
        directory: Some(dir.path().to_path_buf()),
        ..HarnessOptions::default()
    })
    .await;
    h.engine.send(EngineCommand::Start).await;
    let state = h
        .wait_for_state(|s| s.statuses.get("fake-c1") == Some(&SourceStatus::Ok))
        .await;

    let diagnostics = h.diagnostics.get();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.level == Level::Warning && d.category == "ledger")
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.level == Level::Warning && d.category == "dedup")
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.category == "ledger" && d.message == "ledger loaded with 0 keys")
    );
    let ids: Vec<&str> = state.events.iter().map(|e| e.source_event_id()).collect();
    assert_eq!(ids, vec!["e1"]);
    assert_eq!(state.ledger.key_count(), 0);

    // The empty ledger is used: the meeting takes over at its lead time and is then recorded on disk.
    h.clock.advance(540.0).await;
    h.wait_until("the takeover", || {
        h.takeover_due_ids() == vec!["e1".to_string()]
    })
    .await;
    let e1 = state.events.first().unwrap();
    assert!(
        LedgerStore::new(dir.path(), ignore_diagnostics())
            .load()
            .has_fired(e1)
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn an_unreadable_accounts_file_is_never_overwritten() {
    let dir = TempDir::make();
    let accounts = dir.path().join("accounts.json");
    let garbage = b"not json".to_vec();
    std::fs::write(&accounts, &garbage).unwrap();
    let h = EngineHarness::with(HarnessOptions {
        events: vec![sample("e1", 600.0)],
        directory: Some(dir.path().to_path_buf()),
        ..HarnessOptions::default()
    })
    .await;
    h.engine.send(EngineCommand::Start).await;
    let mut state = h.engine.current_state();

    assert!(state.accounts.is_empty());
    assert!(state.events.is_empty());
    assert_eq!(
        state.account_status.error_message.as_deref(),
        Some("Your saved accounts could not be read, so none were loaded. Nothing was deleted.")
    );
    assert_eq!(std::fs::read(&accounts).unwrap(), garbage);

    // Adding, removing and refreshing never write over the unreadable file.
    h.engine
        .send(EngineCommand::AddAccount {
            kind_id: "fake".to_string(),
        })
        .await;
    h.engine
        .send(EngineCommand::RemoveAccount {
            connection_id: "c1".to_string(),
        })
        .await;
    h.engine.send(EngineCommand::Refresh).await;
    h.engine.send(EngineCommand::Stop).await;
    state = h.engine.current_state();
    assert!(state.accounts.is_empty());
    assert!(state.account_status.error_message.is_some());
    assert_eq!(std::fs::read(&accounts).unwrap(), garbage);
}
