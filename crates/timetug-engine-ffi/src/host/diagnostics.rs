//! `DiagnosticSink`: where the engine sends its log lines.
use std::sync::Arc;

use calendar_core::diagnostic_log::{DiagnosticEvent as ConnectorEvent, DiagnosticLog};
use timetug_engine::diagnostic::{
    EngineDiagnostic as EngineEngineDiagnostic, EngineDiagnosticSink,
};

use crate::values::ToMirror;
use crate::values::diagnostics::{DiagnosticEvent, EngineDiagnostic};

/// Receives the engine's diagnostics (`log`) and the structured events its connectors report (`record`). Called from the
/// engine thread; return quickly. Diagnostics never contain tokens, passwords, addresses or feed links; a string field of a
/// connector event is private (`isPrivate`) unless the connector marked it public, and a host's report shows private values
/// as a placeholder by default.
#[uniffi::export(foreign)]
pub trait DiagnosticSink: Send + Sync {
    /// Records one log line.
    fn log(&self, diagnostic: EngineDiagnostic);

    /// Records one structured event of a connector (for example the iCalendar subscription's `fetchCompleted`), as the
    /// connector library's `DiagnosticLog.record` reported it: level, category, name, fields with their privacy flag, and
    /// the engine clock's time. Added in 2.1.0; the Swift package gives it an empty default.
    fn record(&self, event: DiagnosticEvent);
}

/// The engine's sink function over the host's sink.
pub fn engine_sink(host: Arc<dyn DiagnosticSink>) -> EngineDiagnosticSink {
    Arc::new(move |diagnostic: EngineEngineDiagnostic| host.log(diagnostic.to_mirror()))
}

/// The connectors' log over the host's sink: every event goes to `record`, in order, with its private flags.
pub fn connector_log(host: Arc<dyn DiagnosticSink>) -> Arc<dyn DiagnosticLog> {
    Arc::new(ConnectorLog(host))
}

struct ConnectorLog(Arc<dyn DiagnosticSink>);

impl DiagnosticLog for ConnectorLog {
    fn log(&self, event: ConnectorEvent) {
        self.0.record(event.to_mirror());
    }
}
