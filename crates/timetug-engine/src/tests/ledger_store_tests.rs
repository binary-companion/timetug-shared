//! Port of `Persistence/LedgerStoreTests.swift`.

use std::path::Path;
use std::sync::Arc;

use timetug_core::takeover::ledger::TakeoverLedger;

use super::support::{LockedBox, TempDir, standup, ts};
use crate::diagnostic::{EngineDiagnostic, Level, ignore_diagnostics};
use crate::persistence::ledger_store::LedgerStore;

fn store(dir: &Path, events: Option<&Arc<LockedBox<Vec<EngineDiagnostic>>>>) -> LedgerStore {
    let events = events.cloned();
    LedgerStore::new(
        dir,
        Arc::new(move |e| {
            if let Some(events) = &events {
                events.mutate(|list| list.push(e));
            }
        }),
    )
}

#[test]
fn file_name_is_fixed() {
    assert_eq!(LedgerStore::FILE_NAME, "takeover-ledger.json");
}

#[test]
fn missing_file_loads_empty_ledger() {
    let dir = TempDir::make();
    assert_eq!(store(dir.path(), None).load(), TakeoverLedger::new());
}

#[test]
fn corrupt_file_loads_empty_ledger() {
    let dir = TempDir::make();
    std::fs::write(dir.path().join("takeover-ledger.json"), b"not json {").unwrap();
    assert_eq!(store(dir.path(), None).load(), TakeoverLedger::new());
}

#[test]
fn loads_ledger_written_by_the_mac_app() {
    let dir = TempDir::make_copying("takeover-ledger.json");
    let ledger = LedgerStore::new(dir.path(), ignore_diagnostics()).load();
    // One fired and one snoozed event, each under an id key and a content key.
    assert_eq!(ledger.key_count(), 4);
}

#[test]
fn corrupt_ledger_loads_empty_and_emits_diagnostic() {
    let dir = TempDir::make();
    std::fs::write(dir.path().join("takeover-ledger.json"), b"{not json").unwrap();
    let events = Arc::new(LockedBox::new(Vec::new()));
    let ledger = store(dir.path(), Some(&events)).load();
    assert_eq!(ledger.key_count(), 0);
    assert!(
        events
            .get()
            .iter()
            .any(|e| e.level == Level::Warning && e.category == "ledger")
    );
}

#[test]
fn round_trip_creates_directory_and_keeps_fired_event_fired() {
    let dir = TempDir::make();
    let nested = dir.path().join("nested");
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&standup("e1"), ts(1_799_999_000));
    assert!(store(&nested, None).save(&ledger, ts(1_799_999_500)));
    assert!(nested.join("takeover-ledger.json").exists());

    let loaded = store(&nested, None).load();
    assert_eq!(loaded, ledger);
    assert!(loaded.has_fired(&standup("e1")));
    assert!(loaded.has_fired(&standup("resynced-id")));
}

#[test]
fn save_failure_returns_false_and_emits_error() {
    // A path under a regular file cannot be created.
    let dir = TempDir::make();
    let file = dir.path().join("plain");
    std::fs::write(&file, b"").unwrap();
    let events = Arc::new(LockedBox::new(Vec::new()));
    let ok = store(&file.join("x"), Some(&events)).save(&TakeoverLedger::new(), ts(1_800_000_000));
    assert!(!ok);
    assert!(
        events
            .get()
            .iter()
            .any(|e| e.level == Level::Error && e.category == "ledger")
    );
}

#[test]
fn save_prunes_expired_entries_before_writing() {
    let dir = TempDir::make();
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&standup("e1"), ts(1_799_999_000));
    let after_end = ts(1_800_000_000 + 1800 + 1);
    assert!(store(dir.path(), None).save(&ledger, after_end));
    assert_eq!(store(dir.path(), None).load(), TakeoverLedger::new());
}

#[test]
fn load_of_file_with_expired_entries_is_pruned_by_launch_prune() {
    let dir = TempDir::make();
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&standup("e1"), ts(1_799_999_000));
    assert!(store(dir.path(), None).save(&ledger, ts(1_799_999_500)));

    let mut loaded = store(dir.path(), None).load();
    assert!(loaded.has_fired(&standup("e1")));
    let pruned = loaded.prune(ts(1_800_000_000 + 1800 + 1));
    assert!(pruned);
    assert_eq!(loaded, TakeoverLedger::new());
}

/// Rust-only (conventions section 10, item 12): the diagnostic names the kind of error and nothing else, so no path
/// of the host's data directory reaches the host's log.
#[test]
fn diagnostics_carry_no_path_and_no_error_text() {
    let dir = TempDir::make();
    let file = dir.path().join("plain");
    std::fs::write(&file, b"").unwrap();
    let events = Arc::new(LockedBox::new(Vec::new()));
    let target = file.join("x");
    assert!(!store(&target, Some(&events)).save(&TakeoverLedger::new(), ts(1_800_000_000)));
    std::fs::write(dir.path().join("takeover-ledger.json"), b"{not json").unwrap();
    let _ = store(dir.path(), Some(&events)).load();
    let messages: Vec<String> = events.get().into_iter().map(|e| e.message).collect();
    assert_eq!(
        messages,
        vec![
            "ledger could not be written (IoError)".to_string(),
            "ledger could not be read (DecodingError)".to_string()
        ]
    );
}
