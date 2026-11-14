//! Port of `Tests/CalendarBridgeTests/ConnectedSourceTests.swift`.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use calendar_core::failure::{Failure, Outcome};
use calendar_core::model::{CalendarDescriptor, CalendarEvent, CalendarService};
use calendar_core::source_types::{
    CalendarChange, CalendarSource as LibrarySource, SourceCapabilities,
    SourceError as LibrarySourceError, SourceNotice, SourceNoticeKind,
};
use calendar_core::time::DateInterval;
use jiff::Timestamp;
use timetug_core::{CalendarSource, SourceChange, SourceError};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::connected_source::ConnectedSource;

/// Swift's `FakeLibrarySource`. Like the Swift one it has a single change stream: the first `changes()` call takes the
/// receiver; the sender stays here so a test can yield changes and see whether the stream ended.
struct FakeLibrarySource {
    id: String,
    calendars_result: Mutex<Outcome<Vec<CalendarDescriptor>>>,
    events_result: Mutex<Outcome<Vec<CalendarEvent>>>,
    sender: UnboundedSender<CalendarChange>,
    receiver: Mutex<Option<UnboundedReceiver<CalendarChange>>>,
}

impl FakeLibrarySource {
    fn new(id: &str) -> Arc<FakeLibrarySource> {
        let (sender, receiver) = unbounded_channel();
        Arc::new(FakeLibrarySource {
            id: id.to_string(),
            calendars_result: Mutex::new(Ok(Vec::new())),
            events_result: Mutex::new(Ok(Vec::new())),
            sender,
            receiver: Mutex::new(Some(receiver)),
        })
    }

    /// `continuation.yield(_:)`.
    fn yield_change(&self, change: CalendarChange) {
        self.sender.send(change).unwrap();
    }

    /// `wasTerminated`: the consumer of the stream is gone.
    fn was_terminated(&self) -> bool {
        self.sender.is_closed()
    }
}

#[async_trait]
impl LibrarySource for FakeLibrarySource {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn display_name(&self) -> String {
        "Fake".to_string()
    }
    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities::default()
    }
    async fn calendars(&self) -> Outcome<Vec<CalendarDescriptor>> {
        self.calendars_result.lock().unwrap().clone()
    }
    async fn events_in(&self, _interval: DateInterval) -> Outcome<Vec<CalendarEvent>> {
        self.events_result.lock().unwrap().clone()
    }
    fn changes(&self) -> UnboundedReceiver<CalendarChange> {
        self.receiver
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| unbounded_channel().1)
    }
}

fn forever() -> DateInterval {
    DateInterval::new(Timestamp::MIN, Timestamp::MAX)
}

fn connected(fake: &Arc<FakeLibrarySource>) -> ConnectedSource {
    ConnectedSource::new(Arc::clone(fake) as Arc<dyn LibrarySource>)
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn forwards_identity_and_maps_calendars_and_events() {
    let fake = FakeLibrarySource::new("google-1");
    let mut calendar = CalendarDescriptor::new("c", "Work", CalendarService::google());
    calendar.account_name = Some("me@x.test".to_string());
    *fake.calendars_result.lock().unwrap() = Ok(vec![calendar]);
    *fake.events_result.lock().unwrap() = Ok(vec![CalendarEvent::new(
        "e",
        "c",
        "T",
        Timestamp::from_second(1_000).unwrap(),
        Timestamp::from_second(2_000).unwrap(),
    )]);
    let source = connected(&fake);
    assert!(source.id() == "google-1" && source.display_name() == "Fake");
    let keys: Vec<String> = source
        .calendars()
        .await
        .unwrap()
        .iter()
        .map(|c| c.key())
        .collect();
    assert_eq!(keys, ["google-1/c"]);
    let events = source.events_in(forever()).await.unwrap();
    assert_eq!(
        events
            .iter()
            .map(|e| e.source_id.as_str())
            .collect::<Vec<_>>(),
        ["google-1"]
    );
    assert_eq!(events.iter().map(|e| e.title()).collect::<Vec<_>>(), ["T"]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn translates_permission_and_auth_errors() {
    let fake = FakeLibrarySource::new("src");
    let source = connected(&fake);
    *fake.calendars_result.lock().unwrap() = Err(LibrarySourceError::NeedsPermission.into());
    let permission = source.calendars().await.unwrap_err();
    assert_eq!(
        SourceError::from_failure(&permission),
        Some(SourceError::NeedsPermission),
        "got {permission:?}"
    );
    assert_eq!(permission, Failure::from(SourceError::NeedsPermission));
    *fake.events_result.lock().unwrap() = Err(LibrarySourceError::AuthExpired.into());
    let auth = source.events_in(forever()).await.unwrap_err();
    assert_eq!(
        SourceError::from_failure(&auth),
        Some(SourceError::AuthExpired),
        "got {auth:?}"
    );
    assert_eq!(auth, Failure::from(SourceError::AuthExpired));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn other_errors_propagate_untranslated() {
    let fake = FakeLibrarySource::new("src");
    *fake.events_result.lock().unwrap() = Err(LibrarySourceError::Server { status: 500 }.into());
    let error = connected(&fake).events_in(forever()).await.unwrap_err();
    assert_eq!(
        error,
        Failure::Source(LibrarySourceError::Server { status: 500 })
    );
    assert_eq!(SourceError::from_failure(&error), None);
    // Not a source error at all: also untouched.
    *fake.calendars_result.lock().unwrap() = Err(Failure::Host("boom".to_string()));
    let host = connected(&fake).calendars().await.unwrap_err();
    assert_eq!(host, Failure::Host("boom".to_string()));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn library_changes_keep_source_and_calendar_scope_including_failure() {
    let fake = FakeLibrarySource::new("src");
    let mut changes = connected(&fake).changes();
    fake.yield_change(CalendarChange::CalendarsChanged);
    fake.yield_change(CalendarChange::EventsChanged {
        calendar_ids: Some(BTreeSet::from(["c".to_string()])),
    });
    fake.yield_change(CalendarChange::SourceFailed(
        LibrarySourceError::AuthExpired,
    ));
    assert_eq!(
        changes.recv().await,
        Some(SourceChange::CalendarsChanged {
            source_id: "src".into()
        })
    );
    assert_eq!(
        changes.recv().await,
        Some(SourceChange::EventsChanged {
            source_id: "src".into(),
            calendar_ids: Some(BTreeSet::from(["c".to_string()]))
        })
    );
    assert_eq!(
        changes.recv().await,
        Some(SourceChange::SourceFailed {
            source_id: "src".into()
        })
    );
}

/// Not a Swift test: an unscoped `eventsChanged` (nil ids) keeps `None`, and the stream ends when the library's does
/// (`continuation.finish()`).
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn unscoped_events_changed_stays_unscoped_and_the_end_of_the_library_stream_ends_ours() {
    let (library_sender, library_receiver) = unbounded_channel();
    let fake = FakeLibrarySource::new("src");
    *fake.receiver.lock().unwrap() = Some(library_receiver);
    let mut changes = connected(&fake).changes();
    library_sender
        .send(CalendarChange::EventsChanged { calendar_ids: None })
        .unwrap();
    drop(library_sender);
    assert_eq!(
        changes.recv().await,
        Some(SourceChange::EventsChanged {
            source_id: "src".into(),
            calendar_ids: None
        })
    );
    assert_eq!(changes.recv().await, None);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn cancelling_the_consumer_ends_the_library_stream() {
    let fake = FakeLibrarySource::new("src");
    let source = connected(&fake);
    let consumer = tokio::spawn(async move {
        let mut changes = source.changes();
        while changes.recv().await.is_some() {}
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    consumer.abort();
    for _ in 0..200 {
        if fake.was_terminated() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(fake.was_terminated());
}

/// Swift's `NoticingLibrarySource`.
struct NoticingLibrarySource;

#[async_trait]
impl LibrarySource for NoticingLibrarySource {
    fn id(&self) -> String {
        "n".to_string()
    }
    fn display_name(&self) -> String {
        "N".to_string()
    }
    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities::default()
    }
    async fn calendars(&self) -> Outcome<Vec<CalendarDescriptor>> {
        Ok(Vec::new())
    }
    async fn events_in(&self, _interval: DateInterval) -> Outcome<Vec<CalendarEvent>> {
        Ok(Vec::new())
    }
    fn changes(&self) -> UnboundedReceiver<CalendarChange> {
        unbounded_channel().1 // `AsyncStream { $0.finish() }`
    }
    async fn notices(&self) -> Vec<SourceNotice> {
        vec![SourceNotice::new(SourceNoticeKind::UnreadableRecurrence, 3)]
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn notices_pass_through_and_default_to_none() {
    let noticing = ConnectedSource::new(Arc::new(NoticingLibrarySource) as Arc<dyn LibrarySource>);
    assert_eq!(
        noticing.notices().await,
        vec![SourceNotice::new(SourceNoticeKind::UnreadableRecurrence, 3)]
    );
    assert!(
        connected(&FakeLibrarySource::new("src"))
            .notices()
            .await
            .is_empty()
    );
}
