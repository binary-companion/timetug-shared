//! Port of `EngineEvent.swift`.

use calendar_core::url_string::UrlString;
use timetug_core::takeover::request::TakeoverRequest;
use timetug_core::widget::snapshot::WidgetSnapshot;

use crate::diagnostic::EngineDiagnostic;

/// One-shot notifications from the engine to its front end (state itself arrives as `EngineState` snapshots).
// An event is built once and moved once to the host; boxing the request would only change how the plain data of the
// Swift enum is spelled.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum EngineEvent {
    TakeoverDue {
        request: TakeoverRequest,
        calendar_title: Option<String>,
    },
    TakeoverWithdrawn {
        event_id: String,
    },
    WidgetSnapshot(WidgetSnapshot),
    /// The host opens this with its own safe-open policy.
    JoinRequested(UrlString),
    Diagnostic(EngineDiagnostic),
}
