//! Mirror of the one-shot events the engine sends to the host.
#![allow(missing_docs)]

use calendar_core::url_string::UrlString;
use timetug_engine::event::EngineEvent as EngineEngineEvent;

use super::ToMirror;
use super::diagnostics::EngineDiagnostic;
use super::state::TakeoverRequest;
use super::widget::WidgetSnapshot;

/// One-shot notifications from the engine to its front end.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum EngineEvent {
    TakeoverDue {
        request: TakeoverRequest,
        calendar_title: Option<String>,
    },
    TakeoverWithdrawn {
        event_id: String,
    },
    WidgetSnapshot(WidgetSnapshot),
    JoinRequested(UrlString),
    Diagnostic(EngineDiagnostic),
}
impl ToMirror for EngineEngineEvent {
    type Mirror = EngineEvent;
    fn to_mirror(self) -> EngineEvent {
        match self {
            EngineEngineEvent::TakeoverDue {
                request,
                calendar_title,
            } => EngineEvent::TakeoverDue {
                request: request.to_mirror(),
                calendar_title,
            },
            EngineEngineEvent::TakeoverWithdrawn { event_id } => {
                EngineEvent::TakeoverWithdrawn { event_id }
            }
            EngineEngineEvent::WidgetSnapshot(snapshot) => {
                EngineEvent::WidgetSnapshot(snapshot.to_mirror())
            }
            EngineEngineEvent::JoinRequested(url) => EngineEvent::JoinRequested(url),
            EngineEngineEvent::Diagnostic(diagnostic) => {
                EngineEvent::Diagnostic(diagnostic.to_mirror())
            }
        }
    }
}
