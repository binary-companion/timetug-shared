//! Port of `EngineSettings.swift`.

use std::sync::{Mutex, MutexGuard, PoisonError};

use timetug_core::TakeoverSettings;

/// Everything the host lets the engine read about the user's preferences.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineSettings {
    pub takeover: TakeoverSettings,
    pub inference_enabled: bool,
    pub system_calendar_enabled: bool,
}

impl Default for EngineSettings {
    /// Swift's `init` with every argument at its default: `TakeoverSettings()`, `false`, `true`.
    fn default() -> EngineSettings {
        EngineSettings {
            takeover: TakeoverSettings::default(),
            inference_enabled: false,
            system_calendar_enabled: true,
        }
    }
}

/// Where the host keeps settings; the engine reads and writes through this seam.
pub trait SettingsStorage: Send + Sync {
    fn load(&self) -> EngineSettings;
    fn save(&self, settings: &EngineSettings);
}

struct InMemory {
    current: EngineSettings,
    history: Vec<EngineSettings>,
}

/// A settings store held in memory, for tests and hosts without persistence.
pub struct InMemorySettingsStorage {
    state: Mutex<InMemory>,
}

impl InMemorySettingsStorage {
    pub fn new(initial: EngineSettings) -> InMemorySettingsStorage {
        InMemorySettingsStorage {
            state: Mutex::new(InMemory {
                current: initial,
                history: Vec::new(),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, InMemory> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every value passed to `save`, oldest first.
    pub fn saved(&self) -> Vec<EngineSettings> {
        self.lock().history.clone()
    }
}

impl Default for InMemorySettingsStorage {
    fn default() -> InMemorySettingsStorage {
        InMemorySettingsStorage::new(EngineSettings::default())
    }
}

impl SettingsStorage for InMemorySettingsStorage {
    fn load(&self) -> EngineSettings {
        self.lock().current.clone()
    }

    fn save(&self, settings: &EngineSettings) {
        let mut state = self.lock();
        state.current = settings.clone();
        state.history.push(settings.clone());
    }
}
