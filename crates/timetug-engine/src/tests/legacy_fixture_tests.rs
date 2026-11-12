//! The part of `LegacyFixtureTests.swift` that does not need the `Engine`: the files the Mac app wrote
//! (`Tests/TimeTugEngineTests/Fixtures`, found through `CARGO_MANIFEST_DIR`) load through the persistence layer, are
//! honoured, are never rewritten by a load, and corrupt copies load empty with a diagnostic.
//!
//! The five engine-level tests of the Swift suite (`macLedgerDedupStateAndAccountsLoadAndAreHonoured` and its four
//! siblings) need `Engine` and `EngineHarness` and are ported with phase 3 task 4 (their rows in `test-parity.md`
//! stay `pending` until then). Review Focus 4 adds the end-to-end data-directory test in task 5.

use std::sync::Arc;

use calendar_core::model::CalendarEvent;
use timetug_core::takeover::ledger::TakeoverLedger;
use timetug_core::{DedupState, TakeoverSettings, TimeTugCalendarEvent};

use super::support::{Fixtures, LockedBox, TempDir, ts};
use crate::diagnostic::{EngineDiagnostic, Level, ignore_diagnostics};
use crate::persistence::dedup_state_store::DedupStateStore;
use crate::persistence::file_settings_storage::FileSettingsStorage;
use crate::persistence::ledger_store::LedgerStore;
use crate::settings::SettingsStorage;

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
