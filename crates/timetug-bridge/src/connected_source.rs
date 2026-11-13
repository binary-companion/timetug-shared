use std::sync::Arc;

use async_trait::async_trait;
use calendar_core::failure::{Failure, Outcome};
use calendar_core::source_types::{CalendarChange, CalendarSource as LibrarySource, SourceNotice};
use calendar_core::time::DateInterval;
use timetug_core::{CalendarInfo, CalendarSource, SourceChange, SourceError, TimeTugCalendarEvent};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use crate::event_mapper::EventMapper;

/// Adapts a connector-library source to Core's `CalendarSource`, so `CalendarStore` never sees library types.
pub struct ConnectedSource {
    source: Arc<dyn LibrarySource>,
    mapper: EventMapper,
}

impl ConnectedSource {
    /// Swift's `init(_:mapper:)` with the default mapper.
    pub fn new(source: Arc<dyn LibrarySource>) -> ConnectedSource {
        ConnectedSource::with_mapper(source, EventMapper::new())
    }

    /// Wraps `source`, using `mapper` to convert its calendars and events to Core's model.
    pub fn with_mapper(source: Arc<dyn LibrarySource>, mapper: EventMapper) -> ConnectedSource {
        ConnectedSource { source, mapper }
    }

    /// Swift's `translate(_:)`: the two actionable library errors become Core's `SourceError`; anything else is
    /// returned as it came.
    fn translate(error: Failure) -> Failure {
        match SourceError::from_failure(&error) {
            Some(translated) => translated.into(),
            None => error,
        }
    }
}

#[async_trait]
impl CalendarSource for ConnectedSource {
    fn id(&self) -> String {
        self.source.id()
    }

    fn display_name(&self) -> String {
        self.source.display_name()
    }

    async fn calendars(&self) -> Outcome<Vec<CalendarInfo>> {
        let id = self.id();
        match self.source.calendars().await {
            Ok(descriptors) => Ok(descriptors
                .iter()
                .map(|d| self.mapper.calendar_info(d, &id))
                .collect()),
            Err(error) => Err(ConnectedSource::translate(error)),
        }
    }

    async fn events_in(&self, interval: DateInterval) -> Outcome<Vec<TimeTugCalendarEvent>> {
        let id = self.id();
        match self.source.events_in(interval).await {
            Ok(events) => Ok(events
                .into_iter()
                .filter_map(|e| self.mapper.event(e, &id))
                .collect()),
            Err(error) => Err(ConnectedSource::translate(error)),
        }
    }

    async fn notices(&self) -> Vec<SourceNotice> {
        self.source.notices().await
    }

    /// Yields once for every library change. `SourceFailed` yields too, so the next refresh surfaces the status.
    /// Dropping the returned receiver ends the forwarding task, which drops the library's receiver and so ends the
    /// library stream (and its polling). The stream ends when the library's does.
    ///
    /// Spawns the forwarding task with `tokio::spawn`: call it from within the engine's runtime.
    fn changes(&self) -> UnboundedReceiver<SourceChange> {
        let source = Arc::clone(&self.source);
        let (sender, receiver) = unbounded_channel();
        let mut library = source.changes();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Swift's `onTermination = { task.cancel() }`.
                    () = sender.closed() => break,
                    change = library.recv() => {
                        let Some(change) = change else { break };
                        let source_id = source.id();
                        let mapped = match change {
                            CalendarChange::CalendarsChanged => {
                                SourceChange::CalendarsChanged { source_id }
                            }
                            CalendarChange::EventsChanged { calendar_ids } => {
                                SourceChange::EventsChanged { source_id, calendar_ids }
                            }
                            CalendarChange::SourceFailed(_) => {
                                SourceChange::SourceFailed { source_id }
                            }
                        };
                        if sender.send(mapped).is_err() {
                            break;
                        }
                    }
                }
            }
            // Dropping `library` here ends the library stream; dropping `sender` finishes ours.
        });
        receiver
    }
}
