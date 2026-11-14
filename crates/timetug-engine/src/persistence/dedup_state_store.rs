//! Port of `Persistence/DedupStateStore.swift`.

use std::path::Path;

use timetug_core::DedupState;

use super::json_file::{JsonFile, file_in};
use crate::diagnostic::EngineDiagnosticSink;

/// Persists lessons and model verdicts as JSON so corrections survive a relaunch.
pub struct DedupStateStore {
    file: JsonFile<DedupState>,
}

impl DedupStateStore {
    /// Name of the file inside the data directory.
    pub const FILE_NAME: &'static str = "dedup-state.json";

    /// Creates a store for `dedup-state.json` in `directory`; problems are reported to `diagnostics`.
    pub fn new(directory: impl AsRef<Path>, diagnostics: EngineDiagnosticSink) -> DedupStateStore {
        DedupStateStore {
            file: JsonFile::new(
                file_in(directory.as_ref(), Self::FILE_NAME),
                diagnostics,
                "dedup",
            ),
        }
    }

    /// Empty state for a missing or unreadable file (corruption is reported, never fatal).
    pub fn load(&self) -> DedupState {
        self.file.load().unwrap_or_default()
    }

    /// `false` when the file could not be written.
    pub fn save(&self, state: &DedupState) -> bool {
        self.file.save(state)
    }
}
