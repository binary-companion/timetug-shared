//! `SettingsStorage`: where the host keeps the user's preferences.
use std::sync::Arc;

use timetug_engine::settings::{
    EngineSettings as EngineEngineSettings, SettingsStorage as EngineSettingsStorage,
};

use crate::values::settings::EngineSettings;
use crate::values::{ToEngine, ToMirror};

/// The user's preferences. Both methods are synchronous (the engine's seam is), so they must return quickly.
#[uniffi::export(foreign)]
pub trait SettingsStorage: Send + Sync {
    /// The stored settings, or the defaults when there are none.
    fn load(&self) -> EngineSettings;
    /// Stores the settings.
    fn save(&self, settings: EngineSettings);
}

/// Presents the host's storage to the engine.
pub struct SettingsStorageAdapter {
    host: Arc<dyn SettingsStorage>,
}

impl SettingsStorageAdapter {
    /// Wraps the host's storage.
    pub fn new(host: Arc<dyn SettingsStorage>) -> SettingsStorageAdapter {
        SettingsStorageAdapter { host }
    }
}

impl EngineSettingsStorage for SettingsStorageAdapter {
    fn load(&self) -> EngineEngineSettings {
        self.host.load().to_engine()
    }

    fn save(&self, settings: &EngineEngineSettings) {
        self.host.save(settings.clone().to_mirror());
    }
}
