//! Port of `Persistence/LedgerStore.swift`.

use std::path::Path;

use jiff::Timestamp;
use timetug_core::takeover::ledger::TakeoverLedger;

use super::json_file::{JsonFile, file_in};
use crate::diagnostic::EngineDiagnosticSink;

/// Persists the takeover ledger as JSON so a relaunch never repeats a takeover. The file format is the one the Mac app
/// writes.
pub struct LedgerStore {
    file: JsonFile<TakeoverLedger>,
}

impl LedgerStore {
    /// Name of the file inside the data directory.
    pub const FILE_NAME: &'static str = "takeover-ledger.json";

    /// Creates a store for `takeover-ledger.json` in `directory`; problems are reported to `diagnostics`.
    pub fn new(directory: impl AsRef<Path>, diagnostics: EngineDiagnosticSink) -> LedgerStore {
        LedgerStore {
            file: JsonFile::new(
                file_in(directory.as_ref(), Self::FILE_NAME),
                diagnostics,
                "ledger",
            ),
        }
    }

    /// An empty ledger for a missing or unreadable file (corruption is reported, never fatal).
    pub fn load(&self) -> TakeoverLedger {
        self.file.load().unwrap_or_default()
    }

    /// Prunes expired entries before encoding so the file never holds them. `false` when the file could not be
    /// written.
    pub fn save(&self, ledger: &TakeoverLedger, now: Timestamp) -> bool {
        let mut pruned = ledger.clone();
        pruned.prune(now);
        self.file.save(&pruned)
    }
}
