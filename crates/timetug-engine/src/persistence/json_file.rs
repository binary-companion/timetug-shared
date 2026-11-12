//! Port of `Persistence/JSONFile.swift`.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use calendar_core::files::write_atomic;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::diagnostic::{EngineDiagnostic, EngineDiagnosticSink, Level};

/// One serde value persisted as a JSON file. Corruption and write failures are reported through the diagnostic sink
/// (with the error's type name only, never its description or the path) and never returned as errors.
///
/// Swift reports `String(describing: type(of: error))`. The Rust names are fixed by what failed: `IoError` for a read
/// or a write the file system refused, `DecodingError` for a file that is not the expected JSON, `EncodingError` for a
/// value that cannot be written as JSON.
pub struct JsonFile<Value> {
    pub url: PathBuf,
    pub diagnostics: EngineDiagnosticSink,
    /// Used as the diagnostic category and in messages, for example "ledger".
    pub label: String,
    value: PhantomData<fn() -> Value>,
}

impl<Value: Serialize + DeserializeOwned> JsonFile<Value> {
    pub fn new(
        url: impl Into<PathBuf>,
        diagnostics: EngineDiagnosticSink,
        label: impl Into<String>,
    ) -> JsonFile<Value> {
        JsonFile {
            url: url.into(),
            diagnostics,
            label: label.into(),
            value: PhantomData,
        }
    }

    /// `None` for a missing, unreadable or corrupt file.
    pub fn load(&self) -> Option<Value> {
        if !self.url.exists() {
            return None;
        }
        let read = std::fs::read(&self.url)
            .map_err(|_| "IoError")
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|_| "DecodingError"));
        match read {
            Ok(value) => Some(value),
            Err(kind) => {
                (self.diagnostics)(EngineDiagnostic::new(
                    Level::Warning,
                    self.label.as_str(),
                    format!("{} could not be read ({kind})", self.label),
                ));
                None
            }
        }
    }

    /// Creates the directory and writes atomically. `false` (and an `error` diagnostic) on failure.
    pub fn save(&self, value: &Value) -> bool {
        let written = self.write(value);
        if let Err(kind) = written {
            (self.diagnostics)(EngineDiagnostic::new(
                Level::Error,
                self.label.as_str(),
                format!("{} could not be written ({kind})", self.label),
            ));
        }
        written.is_ok()
    }

    fn write(&self, value: &Value) -> Result<(), &'static str> {
        if let Some(parent) = self.url.parent() {
            std::fs::create_dir_all(parent).map_err(|_| "IoError")?;
        }
        let bytes = serde_json::to_vec(value).map_err(|_| "EncodingError")?;
        write_atomic(&self.url, &bytes).map_err(|_| "IoError")
    }
}

/// The path of `name` inside `directory` (`directory.appendingPathComponent(name)`).
pub(crate) fn file_in(directory: &Path, name: &str) -> PathBuf {
    directory.join(name)
}
