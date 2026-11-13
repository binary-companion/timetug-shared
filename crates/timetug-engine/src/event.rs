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
    /// A meeting is due: the host presents the takeover.
    TakeoverDue {
        /// The takeover to present.
        request: TakeoverRequest,
        /// Title of the event's calendar, when it is known.
        calendar_title: Option<String>,
    },
    /// A takeover announced earlier is no longer going to happen (the meeting moved, ended or was removed).
    TakeoverWithdrawn {
        /// Id of the event whose takeover is withdrawn.
        event_id: String,
    },
    /// A fresh snapshot for widgets, emitted when the state they show changed.
    WidgetSnapshot(WidgetSnapshot),
    /// The host opens this with its own safe-open policy.
    JoinRequested(UrlString),
    /// A log line for the host to record.
    Diagnostic(EngineDiagnostic),
}
