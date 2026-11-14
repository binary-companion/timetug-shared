//! Tests of `Sources/CalendarSource.swift`, which no Swift test file covers on its own (the store tests exercise it).

use std::collections::BTreeSet;

use calendar_core::failure::Failure;
use calendar_core::source_types::{SourceError as LibrarySourceError, SourceNotice};
use calendar_core::time::DateInterval;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use crate::model::calendar_info::CalendarInfo;
use crate::model::event::TimeTugCalendarEvent;
use crate::sources::{CalendarSource, SourceChange, SourceError, SourceStatus};

#[test]
fn source_change_reports_its_source_for_every_case() {
    let changes = [
        SourceChange::CalendarsChanged {
            source_id: "a".into(),
        },
        SourceChange::EventsChanged {
            source_id: "a".into(),
            calendar_ids: None,
        },
        SourceChange::EventsChanged {
            source_id: "a".into(),
            calendar_ids: Some(BTreeSet::from(["c".to_string()])),
        },
        SourceChange::SourceFailed {
            source_id: "a".into(),
        },
    ];
    for change in &changes {
        assert_eq!(change.source_id(), "a", "{change:?}");
    }
    // The origin survives a connector that cannot narrow the fetch to a calendar.
    assert_ne!(changes[1], changes[2]);
}

#[test]
fn source_error_is_the_librarys_error_of_the_same_name() {
    assert_eq!(
        Failure::from(SourceError::NeedsPermission),
        Failure::Source(LibrarySourceError::NeedsPermission)
    );
    assert_eq!(
        Failure::from(SourceError::AuthExpired),
        Failure::Source(LibrarySourceError::AuthExpired)
    );
    assert_eq!(
        SourceError::from_failure(&Failure::Source(LibrarySourceError::NeedsPermission)),
        Some(SourceError::NeedsPermission)
    );
    assert_eq!(
        SourceError::from_failure(&Failure::Source(LibrarySourceError::AuthExpired)),
        Some(SourceError::AuthExpired)
    );
    // Every other failure is not one of Core's two actionable statuses.
    for other in [
        Failure::Cancelled,
        Failure::Host("boom".into()),
        Failure::Source(LibrarySourceError::NotFound),
        Failure::Source(LibrarySourceError::Network("down".into())),
    ] {
        assert_eq!(SourceError::from_failure(&other), None, "{other:?}");
    }
}

#[test]
fn source_status_compares_by_case_and_message() {
    assert_eq!(
        SourceStatus::Failing("x".into()),
        SourceStatus::Failing("x".into())
    );
    assert_ne!(
        SourceStatus::Failing("x".into()),
        SourceStatus::Failing("y".into())
    );
    assert_ne!(SourceStatus::Ok, SourceStatus::NeedsPermission);
}

struct Bare;

#[async_trait::async_trait]
impl CalendarSource for Bare {
    fn id(&self) -> String {
        "bare".into()
    }
    fn display_name(&self) -> String {
        "Bare".into()
    }
    async fn calendars(&self) -> Result<Vec<CalendarInfo>, Failure> {
        Ok(Vec::new())
    }
    async fn events_in(
        &self,
        _interval: DateInterval,
    ) -> Result<Vec<TimeTugCalendarEvent>, Failure> {
        Ok(Vec::new())
    }
    fn changes(&self) -> UnboundedReceiver<SourceChange> {
        unbounded_channel().1
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn notices_default_to_none() {
    let source: Box<dyn CalendarSource> = Box::new(Bare);
    let notices: Vec<SourceNotice> = source.notices().await;
    assert!(notices.is_empty());
    assert_eq!(source.id(), "bare");
    assert_eq!(source.display_name(), "Bare");
}
