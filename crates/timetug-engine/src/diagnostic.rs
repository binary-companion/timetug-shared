//! Port of `EngineDiagnostic.swift`.

use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    Info,
    Warning,
    Error,
}

impl Level {
    /// The Swift raw value.
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Warning => "warning",
            Level::Error => "error",
        }
    }

    /// The case with the Swift raw value `raw`.
    pub fn from_raw_value(raw: &str) -> Option<Level> {
        match raw {
            "info" => Some(Level::Info),
            "warning" => Some(Level::Warning),
            "error" => Some(Level::Error),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineDiagnostic {
    pub level: Level,
    pub category: String,
    /// Never contains event titles or other calendar content.
    pub message: String,
}

impl EngineDiagnostic {
    pub fn new(
        level: Level,
        category: impl Into<String>,
        message: impl Into<String>,
    ) -> EngineDiagnostic {
        EngineDiagnostic {
            level,
            category: category.into(),
            message: message.into(),
        }
    }
}

/// Swift's `@Sendable (EngineDiagnostic) -> Void`.
pub type EngineDiagnosticSink = Arc<dyn Fn(EngineDiagnostic) + Send + Sync>;

/// Swift's default argument `{ _ in }`: a sink that drops every diagnostic.
pub fn ignore_diagnostics() -> EngineDiagnosticSink {
    Arc::new(|_| {})
}
