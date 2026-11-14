//! Mirrors of the diagnostics the engine and the connectors report.
#![allow(missing_docs)]

use calendar_core::diagnostic_log as log;
use jiff::Timestamp;
use timetug_engine::diagnostic as diag;

use super::{ToEngine, ToMirror, mirror_record, mirror_unit_enum};

mirror_unit_enum!(EngineDiagnosticLevel <=> diag::Level { Info, Warning, Error });
mirror_record!(EngineDiagnostic <=> diag::EngineDiagnostic {
    level: EngineDiagnosticLevel,
    category: String,
    message: String,
});

mirror_unit_enum!(DiagnosticLevel <=> log::DiagnosticLevel { Debug, Info, Notice, Warning, Error });

/// A diagnostic field's value.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum DiagnosticFieldValue {
    Int(i64),
    Double(f64),
    Bool(bool),
    String(String),
}
impl ToMirror for log::DiagnosticFieldValue {
    type Mirror = DiagnosticFieldValue;
    fn to_mirror(self) -> DiagnosticFieldValue {
        match self {
            log::DiagnosticFieldValue::Int(value) => DiagnosticFieldValue::Int(value),
            log::DiagnosticFieldValue::Double(value) => DiagnosticFieldValue::Double(value),
            log::DiagnosticFieldValue::Bool(value) => DiagnosticFieldValue::Bool(value),
            log::DiagnosticFieldValue::String(value) => DiagnosticFieldValue::String(value),
        }
    }
}
impl ToEngine for DiagnosticFieldValue {
    type Engine = log::DiagnosticFieldValue;
    fn to_engine(self) -> log::DiagnosticFieldValue {
        match self {
            DiagnosticFieldValue::Int(value) => log::DiagnosticFieldValue::Int(value),
            DiagnosticFieldValue::Double(value) => log::DiagnosticFieldValue::Double(value),
            DiagnosticFieldValue::Bool(value) => log::DiagnosticFieldValue::Bool(value),
            DiagnosticFieldValue::String(value) => log::DiagnosticFieldValue::String(value),
        }
    }
}

mirror_record!(DiagnosticField <=> log::DiagnosticField {
    name: String,
    value: DiagnosticFieldValue,
    is_private: bool,
});
mirror_record!(DiagnosticEvent <=> log::DiagnosticEvent {
    level: DiagnosticLevel,
    category: String,
    name: String,
    fields: Vec<DiagnosticField>,
    date: Timestamp,
});
