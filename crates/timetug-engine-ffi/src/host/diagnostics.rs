//! `DiagnosticSink`: where the engine sends its log lines.
use std::sync::Arc;

use timetug_engine::diagnostic::{
    EngineDiagnostic as EngineEngineDiagnostic, EngineDiagnosticSink,
};

use crate::values::ToMirror;
use crate::values::diagnostics::EngineDiagnostic;

/// Receives the engine's diagnostics. Called from the engine thread; return quickly. Diagnostics never contain tokens,
/// passwords, addresses or feed links.
#[uniffi::export(foreign)]
pub trait DiagnosticSink: Send + Sync {
    /// Records one log line.
    fn log(&self, diagnostic: EngineDiagnostic);
}

/// The engine's sink function over the host's sink.
pub fn engine_sink(host: Arc<dyn DiagnosticSink>) -> EngineDiagnosticSink {
    Arc::new(move |diagnostic: EngineEngineDiagnostic| host.log(diagnostic.to_mirror()))
}
