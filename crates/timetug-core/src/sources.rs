use std::collections::BTreeSet;

use async_trait::async_trait;
use calendar_core::failure::{Failure, Outcome};
use calendar_core::source_types::{SourceError as LibrarySourceError, SourceNotice};
use calendar_core::time::DateInterval;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::model::calendar_info::CalendarInfo;
use crate::model::event::TimeTugCalendarEvent;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceStatus {
    Ok,
    NeedsPermission,
    AuthExpired,
    Failing(String),
}

/// Sources throw these to report a status the user can act on.
///
/// Swift's `throws` is `Result<_, Failure>` here, so this error travels as the library's `SourceError` of the same
/// name inside [`Failure::Source`] (`From<SourceError> for Failure`); [`SourceError::from_failure`] reads it back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    #[error("access not granted")]
    NeedsPermission,
    #[error("authorization expired")]
    AuthExpired,
}

impl From<SourceError> for Failure {
    fn from(error: SourceError) -> Failure {
        match error {
            SourceError::NeedsPermission => Failure::Source(LibrarySourceError::NeedsPermission),
            SourceError::AuthExpired => Failure::Source(LibrarySourceError::AuthExpired),
        }
    }
}

impl SourceError {
    /// Swift's `catch SourceError.needsPermission` and `catch SourceError.authExpired`: the actionable status a failure
    /// stands for, or `None` for every other failure.
    pub fn from_failure(failure: &Failure) -> Option<SourceError> {
        match failure {
            Failure::Source(LibrarySourceError::NeedsPermission) => {
                Some(SourceError::NeedsPermission)
            }
            Failure::Source(LibrarySourceError::AuthExpired) => Some(SourceError::AuthExpired),
            _ => None,
        }
    }
}

/// A source notification retains its origin even when the connector cannot narrow the fetch to a calendar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceChange {
    CalendarsChanged {
        source_id: String,
    },
    EventsChanged {
        source_id: String,
        calendar_ids: Option<BTreeSet<String>>,
    },
    SourceFailed {
        source_id: String,
    },
}

impl SourceChange {
    pub fn source_id(&self) -> &str {
        match self {
            SourceChange::CalendarsChanged { source_id }
            | SourceChange::EventsChanged { source_id, .. }
            | SourceChange::SourceFailed { source_id } => source_id,
        }
    }
}

/// A calendar backend. Returns only Core's normalized model; source-specific types never leak. (This is Core's own
/// protocol; files that also see the library's `calendar_core::CalendarSource` import that one as `LibrarySource`.)
#[async_trait]
pub trait CalendarSource: Send + Sync {
    fn id(&self) -> String;
    fn display_name(&self) -> String;
    async fn calendars(&self) -> Outcome<Vec<CalendarInfo>>;
    async fn events_in(&self, interval: DateInterval) -> Outcome<Vec<TimeTugCalendarEvent>>;
    /// A new receiver for whenever the source's data may have changed; every call returns its own. Changes are never
    /// dropped.
    fn changes(&self) -> UnboundedReceiver<SourceChange>;
    /// What the source noticed in the data it last loaded (for example a warning icon). Optional: the default is none.
    async fn notices(&self) -> Vec<SourceNotice> {
        Vec::new()
    }
}
