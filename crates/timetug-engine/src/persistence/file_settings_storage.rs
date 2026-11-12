//! Port of `Persistence/FileSettingsStorage.swift`.

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use timetug_core::TakeoverSettings;

use super::json_file::{JsonFile, file_in};
use crate::diagnostic::EngineDiagnosticSink;
use crate::settings::{EngineSettings, SettingsStorage};

/// Settings kept in `settings.json` inside the data directory, for hosts with no preference store of their own
/// (the C interface). Writes are atomic (through `JsonFile`); a missing file loads defaults and an unreadable one
/// loads defaults and reports a diagnostic. Calls are serialized, so one instance is safe from any thread.
pub struct FileSettingsStorage {
    file: JsonFile<Stored>,
    lock: Mutex<()>,
}

/// The persisted shape. `EngineSettings` itself is not serialized (it is not a wire or file format), and every field
/// decodes with a default so a file written by an older build survives a newly added field. A `null` counts as
/// absent, as Swift's `decodeIfPresent` has it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    takeover: TakeoverSettings,
    inference_enabled: bool,
    system_calendar_enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredFields {
    #[serde(default)]
    takeover: Option<TakeoverSettings>,
    #[serde(default)]
    inference_enabled: Option<bool>,
    #[serde(default)]
    system_calendar_enabled: Option<bool>,
}

impl<'de> Deserialize<'de> for Stored {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Stored, D::Error> {
        let fields = StoredFields::deserialize(deserializer)?;
        let defaults = EngineSettings::default();
        Ok(Stored {
            takeover: fields.takeover.unwrap_or(defaults.takeover),
            inference_enabled: fields
                .inference_enabled
                .unwrap_or(defaults.inference_enabled),
            system_calendar_enabled: fields
                .system_calendar_enabled
                .unwrap_or(defaults.system_calendar_enabled),
        })
    }
}

impl FileSettingsStorage {
    pub const FILE_NAME: &'static str = "settings.json";

    pub fn new(
        directory: impl AsRef<Path>,
        diagnostics: EngineDiagnosticSink,
    ) -> FileSettingsStorage {
        FileSettingsStorage {
            file: JsonFile::new(
                file_in(directory.as_ref(), Self::FILE_NAME),
                diagnostics,
                "settings",
            ),
            lock: Mutex::new(()),
        }
    }
}

impl SettingsStorage for FileSettingsStorage {
    fn load(&self) -> EngineSettings {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        match self.file.load() {
            Some(stored) => EngineSettings {
                takeover: stored.takeover,
                inference_enabled: stored.inference_enabled,
                system_calendar_enabled: stored.system_calendar_enabled,
            },
            None => EngineSettings::default(),
        }
    }

    fn save(&self, settings: &EngineSettings) {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let stored = Stored {
            takeover: settings.takeover.clone(),
            inference_enabled: settings.inference_enabled,
            system_calendar_enabled: settings.system_calendar_enabled,
        };
        let _ = self.file.save(&stored);
    }
}
