//! Port of `EngineSettings.swift`.

use std::sync::{Mutex, MutexGuard, PoisonError};

use timetug_core::TakeoverSettings;

/// Everything the host lets the engine read about the user's preferences.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineSettings {
    /// When and how takeovers appear.
    pub takeover: TakeoverSettings,
    /// Whether the text model may judge ambiguous duplicate pairs.
    pub inference_enabled: bool,
    /// Whether the system calendar (EventKit on the Mac) is a source.
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
    /// Returns the stored settings, or defaults when there are none.
    fn load(&self) -> EngineSettings;
    /// Stores `settings`.
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
    /// Creates a store that starts out holding `initial`.
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
