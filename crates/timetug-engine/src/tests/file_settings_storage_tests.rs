//! Port of `Persistence/FileSettingsStorageTests.swift`, plus Rust-only checks of the stored shape.

use std::sync::Arc;

use super::support::{LockedBox, TempDir};
use crate::diagnostic::{Level, ignore_diagnostics};
use crate::persistence::file_settings_storage::FileSettingsStorage;
use crate::settings::{EngineSettings, SettingsStorage};

fn storage(dir: &std::path::Path) -> FileSettingsStorage {
    FileSettingsStorage::new(dir, ignore_diagnostics())
}

#[test]
fn missing_file_loads_defaults() {
    let dir = TempDir::make();
    assert_eq!(storage(dir.path()).load(), EngineSettings::default());
}

#[test]
fn round_trips_across_instances() {
    let dir = TempDir::make();
    let mut settings = EngineSettings::default();
    settings.inference_enabled = true;
    settings.system_calendar_enabled = false;
    settings.takeover.lead_time = 120.0;
    settings.takeover.takeover_calendar_keys = ["a/b".to_string()].into();
    storage(dir.path()).save(&settings);
    assert_eq!(storage(dir.path()).load(), settings);
}

#[test]
fn saves_into_missing_directory() {
    let dir = TempDir::make();
    let nested = dir.path().join("nested/deeper");
    let mut settings = EngineSettings::default();
    settings.inference_enabled = true;
    storage(&nested).save(&settings);
    assert_eq!(storage(&nested).load(), settings);
}

#[test]
fn corrupt_file_loads_defaults_and_reports_once() {
    let dir = TempDir::make();
    std::fs::write(dir.path().join("settings.json"), b"{not json").unwrap();
    let diagnostics = Arc::new(LockedBox::new(Vec::new()));
    let sink = Arc::clone(&diagnostics);
    let storage =
        FileSettingsStorage::new(dir.path(), Arc::new(move |d| sink.mutate(|l| l.push(d))));
    assert_eq!(storage.load(), EngineSettings::default());
    let reported = diagnostics.get();
    assert_eq!(reported.len(), 1);
    assert_eq!(reported[0].level, Level::Warning);
    assert_eq!(reported[0].category, "settings");
}

#[test]
fn file_name_is_fixed() {
    assert_eq!(FileSettingsStorage::FILE_NAME, "settings.json");
}

/// Rust-only: every field decodes with a default (a file written by an older build survives a newly added field),
/// and a `null` counts as absent, as Swift's `decodeIfPresent` has it.
#[test]
fn missing_and_null_fields_decode_to_their_defaults() {
    let dir = TempDir::make();
    std::fs::write(
        dir.path().join("settings.json"),
        br#"{"inferenceEnabled": true, "systemCalendarEnabled": null, "unknown": 1}"#,
    )
    .unwrap();
    let loaded = storage(dir.path()).load();
    assert!(loaded.inference_enabled);
    assert!(loaded.system_calendar_enabled);
    assert_eq!(loaded.takeover, EngineSettings::default().takeover);
}

/// Rust-only: the written file has the three Swift keys.
#[test]
fn the_written_file_has_the_swift_keys() {
    let dir = TempDir::make();
    storage(dir.path()).save(&EngineSettings::default());
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("settings.json")).unwrap()).unwrap();
    let object = value.as_object().unwrap();
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["inferenceEnabled", "systemCalendarEnabled", "takeover"]
    );
    assert_eq!(object["inferenceEnabled"], false);
    assert_eq!(object["systemCalendarEnabled"], true);
}
