//! Port of `Persistence/EngineSettingsTests.swift`.

use timetug_core::TakeoverSettings;

use super::support::Fixtures;
use crate::settings::{EngineSettings, InMemorySettingsStorage, SettingsStorage};

#[test]
fn legacy_settings_keys_carry_forward() {
    let data = std::fs::read(Fixtures::path("settings-legacy.json")).unwrap();
    let settings: TakeoverSettings = serde_json::from_slice(&data).unwrap();
    assert!(!settings.enabled);
    assert!(settings.require_other_attendees);
    assert_eq!(settings.lead_time, 120.0);
}

#[test]
fn defaults_match_the_spec() {
    let s = EngineSettings::default();
    assert_eq!(s.takeover, TakeoverSettings::default());
    assert!(!s.inference_enabled);
    assert!(s.system_calendar_enabled);
}

#[test]
fn in_memory_storage_returns_initial_value_and_records_saves() {
    let storage = InMemorySettingsStorage::new(EngineSettings {
        inference_enabled: true,
        ..EngineSettings::default()
    });
    assert!(storage.load().inference_enabled);
    assert!(storage.saved().is_empty());
    let mut changed = storage.load();
    changed.system_calendar_enabled = false;
    storage.save(&changed);
    assert_eq!(storage.load(), changed);
    assert_eq!(storage.saved(), vec![changed]);
}
