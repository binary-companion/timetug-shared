//! Port of `Persistence/DedupStateStoreTests.swift`.

use std::path::Path;
use std::sync::Arc;

use timetug_core::DedupState;
use timetug_core::dedup::adjudication::{AdjudicationVerdict, EngineInfo, VerdictAnswer};
use timetug_core::dedup::lesson_book::LessonDecision;
use timetug_core::model::merge_types::MergedMember;

use super::support::{LockedBox, TempDir, ts};
use crate::diagnostic::{EngineDiagnostic, Level};
use crate::persistence::dedup_state_store::DedupStateStore;

fn store(dir: &Path, events: Option<&Arc<LockedBox<Vec<EngineDiagnostic>>>>) -> DedupStateStore {
    let events = events.cloned();
    DedupStateStore::new(
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
    assert_eq!(DedupStateStore::FILE_NAME, "dedup-state.json");
}

#[test]
fn missing_file_loads_empty_state() {
    let dir = TempDir::make();
    assert_eq!(store(dir.path(), None).load(), DedupState::default());
}

#[test]
fn round_trips_lessons_and_verdicts() {
    let dir = TempDir::make();
    let s = store(&dir.path().join("nested"), None);
    let mut state = DedupState::default();
    let now = ts(1_800_000_000);
    state.lessons.record(
        &MergedMember::new("A", "s/x", "k1", "bare", ts(0), ts(3600)),
        &MergedMember::new("B", "s/y", "k2", "bare", ts(0), ts(3600)),
        LessonDecision::Same,
        now,
    );
    state.verdicts.store(
        &AdjudicationVerdict::new("r", VerdictAnswer::Same),
        &EngineInfo {
            id: "e".into(),
            display_name: "E".into(),
            is_on_device: true,
        },
        ts(1_800_003_600),
        now,
    );
    assert!(s.save(&state));
    assert_eq!(s.load(), state);
}

#[test]
fn loads_state_written_by_the_mac_app() {
    let dir = TempDir::make_copying("dedup-state.json");
    let state = store(dir.path(), None).load();
    assert_ne!(state, DedupState::default());
}

#[test]
fn corrupt_file_loads_empty_state_and_emits_diagnostic() {
    let dir = TempDir::make();
    std::fs::write(dir.path().join("dedup-state.json"), b"not json").unwrap();
    let events = Arc::new(LockedBox::new(Vec::new()));
    assert_eq!(
        store(dir.path(), Some(&events)).load(),
        DedupState::default()
    );
    assert!(
        events
            .get()
            .iter()
            .any(|e| e.level == Level::Warning && e.category == "dedup")
    );
}

#[test]
fn save_failure_returns_false_and_emits_error() {
    let dir = TempDir::make();
    let file = dir.path().join("plain");
    std::fs::write(&file, b"").unwrap();
    let events = Arc::new(LockedBox::new(Vec::new()));
    assert!(!store(&file.join("x"), Some(&events)).save(&DedupState::default()));
    assert!(
        events
            .get()
            .iter()
            .any(|e| e.level == Level::Error && e.category == "dedup")
    );
}
