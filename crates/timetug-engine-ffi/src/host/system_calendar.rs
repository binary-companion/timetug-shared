//! `SystemCalendarSource`: the host's own calendar store (EventKit on Apple platforms) as an engine source, with its series
//! and write methods and a change handler.
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use calendar_core::failure::Outcome;
use calendar_core::model::{
    CalendarDescriptor as EngineDescriptor, CalendarEvent as EngineEvent,
    ResponseStatus as EngineResponse,
};
use calendar_core::recurrence::set::CalendarSeries as EngineSeries;
use calendar_core::source_types::{
    CalendarChange, CalendarSource as EngineCalendarSource, SeriesSource,
    SourceCapabilities as EngineCapabilities,
};
use calendar_core::time::DateInterval;
use calendar_core::write::event_draft::EventDraft as EngineDraft;
use calendar_core::write::event_patch::EventPatch as EnginePatch;
use calendar_core::write::types::{
    EventRef as EngineRef, NotifyPolicy as EngineNotify, RecurrenceScope as EngineScope,
};
use calendar_core::write::writable_source::WritableCalendarSource;
use jiff::Timestamp;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::errors::{FfiError, SourceError};
use crate::values::model::{
    CalendarDescriptor, CalendarEvent, CalendarSeries, ResponseStatus, SourceCapabilities,
};
use crate::values::write::{EventDraft, EventPatch, EventRef, NotifyPolicy, RecurrenceScope};
use crate::values::{ToEngine, ToMirror};

/// The object the host calls when its calendar store changes. The engine hands it to `set_change_handler`; each call
/// becomes one `CalendarChange` on the source's change stream, in order.
#[derive(uniffi::Object, Default)]
pub struct SystemChangeHandler {
    senders: Mutex<Vec<UnboundedSender<CalendarChange>>>,
}

impl SystemChangeHandler {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<UnboundedSender<CalendarChange>>> {
        self.senders.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn send(&self, change: CalendarChange) {
        // A closed receiver is pruned on the next send.
        self.lock()
            .retain(|sender| sender.send(change.clone()).is_ok());
    }

    /// A new receiver of the changes made from now on.
    pub fn subscribe(&self) -> UnboundedReceiver<CalendarChange> {
        let (sender, receiver) = unbounded_channel();
        self.lock().push(sender);
        receiver
    }
}

#[uniffi::export]
impl SystemChangeHandler {
    /// Events changed in these calendars (`None`: the scope is unknown).
    pub fn events_changed(&self, calendar_ids: Option<Vec<String>>) {
        self.send(CalendarChange::EventsChanged {
            calendar_ids: calendar_ids.map(|ids| ids.into_iter().collect()),
        });
    }

    /// The set of calendars changed; the engine reloads the calendar list and the events.
    pub fn calendars_changed(&self) {
        self.send(CalendarChange::CalendarsChanged);
    }

    /// The store failed for good (access revoked, for example); the change stream ends after this.
    pub fn source_failed(&self, error: SourceError) {
        self.send(CalendarChange::SourceFailed(error.into()));
    }
}

/// The host's calendar store. Errors are the engine's own: throw `FfiError.Source(.needsPermission)` when access is not
/// granted and `FfiError.Write(.forbidden(nil))` for a read-only calendar.
#[uniffi::export(foreign)]
#[async_trait]
pub trait SystemCalendarSource: Send + Sync {
    /// The id the source's events carry (`eventkit`).
    fn id(&self) -> String;
    /// The name shown for the source.
    fn display_name(&self) -> String;
    /// What the source can read and write.
    fn capabilities(&self) -> SourceCapabilities;
    /// Asks the user for access to the store, if it has not been asked.
    async fn request_access(&self);
    /// The calendars of the store.
    async fn calendars(&self) -> Result<Vec<CalendarDescriptor>, FfiError>;
    /// The events that intersect `start...end`, recurring ones expanded.
    async fn events_in(
        &self,
        start: Timestamp,
        end: Timestamp,
    ) -> Result<Vec<CalendarEvent>, FfiError>;
    /// The recurrence of a series.
    async fn series(&self, id: String, calendar_id: String) -> Result<CalendarSeries, FfiError>;
    /// Creates an event.
    async fn create(
        &self,
        draft: EventDraft,
        calendar_id: String,
        notify: NotifyPolicy,
    ) -> Result<CalendarEvent, FfiError>;
    /// Changes an event.
    async fn update(
        &self,
        reference: EventRef,
        patch: EventPatch,
        scope: RecurrenceScope,
        notify: NotifyPolicy,
    ) -> Result<CalendarEvent, FfiError>;
    /// Deletes an event.
    async fn delete(
        &self,
        reference: EventRef,
        scope: RecurrenceScope,
        notify: NotifyPolicy,
    ) -> Result<(), FfiError>;
    /// Answers an invitation.
    async fn respond(
        &self,
        reference: EventRef,
        response: ResponseStatus,
        scope: RecurrenceScope,
        notify: NotifyPolicy,
    ) -> Result<CalendarEvent, FfiError>;
    /// Gives the host the handler to call when the store changes. Called once, before the first read.
    async fn set_change_handler(&self, handler: Arc<SystemChangeHandler>);
}

/// Presents the host's store to the engine as a connector-library source.
pub struct SystemCalendarAdapter {
    host: Arc<dyn SystemCalendarSource>,
    handler: Arc<SystemChangeHandler>,
    registered: tokio::sync::OnceCell<()>,
}

impl SystemCalendarAdapter {
    /// Wraps the host's store.
    pub fn new(host: Arc<dyn SystemCalendarSource>) -> SystemCalendarAdapter {
        SystemCalendarAdapter {
            host,
            handler: Arc::new(SystemChangeHandler::default()),
            registered: tokio::sync::OnceCell::new(),
        }
    }

    /// Gives the host the change handler once, before the first read.
    async fn ensure_registered(&self) {
        self.registered
            .get_or_init(|| async {
                self.host.set_change_handler(self.handler.clone()).await;
            })
            .await;
    }
}

fn host_failure<T>(result: Result<T, FfiError>) -> Outcome<T> {
    result.map_err(Into::into)
}

#[async_trait]
impl EngineCalendarSource for SystemCalendarAdapter {
    fn id(&self) -> String {
        self.host.id()
    }

    fn display_name(&self) -> String {
        self.host.display_name()
    }

    fn capabilities(&self) -> EngineCapabilities {
        self.host.capabilities().to_engine()
    }

    async fn calendars(&self) -> Outcome<Vec<EngineDescriptor>> {
        self.ensure_registered().await;
        host_failure(self.host.calendars().await).map(|calendars| calendars.to_engine())
    }

    async fn events_in(&self, interval: DateInterval) -> Outcome<Vec<EngineEvent>> {
        self.ensure_registered().await;
        host_failure(self.host.events_in(interval.start, interval.end).await)
            .map(|events| events.to_engine())
    }

    fn changes(&self) -> UnboundedReceiver<CalendarChange> {
        self.handler.subscribe()
    }

    fn as_series(&self) -> Option<&dyn SeriesSource> {
        Some(self)
    }

    fn as_writable(&self) -> Option<&dyn WritableCalendarSource> {
        Some(self)
    }
}

#[async_trait]
impl SeriesSource for SystemCalendarAdapter {
    async fn series(&self, id: &str, calendar_id: &str) -> Outcome<EngineSeries> {
        host_failure(
            self.host
                .series(id.to_string(), calendar_id.to_string())
                .await,
        )
        .map(|series| series.to_engine())
    }
}

#[async_trait]
impl WritableCalendarSource for SystemCalendarAdapter {
    async fn create(
        &self,
        draft: &EngineDraft,
        calendar_id: &str,
        notify: EngineNotify,
    ) -> Outcome<EngineEvent> {
        host_failure(
            self.host
                .create(
                    draft.clone().to_mirror(),
                    calendar_id.to_string(),
                    notify.to_mirror(),
                )
                .await,
        )
        .map(|event| event.to_engine())
    }

    async fn update(
        &self,
        reference: &EngineRef,
        patch: &EnginePatch,
        scope: EngineScope,
        notify: EngineNotify,
    ) -> Outcome<EngineEvent> {
        host_failure(
            self.host
                .update(
                    reference.clone().to_mirror(),
                    patch.clone().to_mirror(),
                    scope.to_mirror(),
                    notify.to_mirror(),
                )
                .await,
        )
        .map(|event| event.to_engine())
    }

    async fn delete(
        &self,
        reference: &EngineRef,
        scope: EngineScope,
        notify: EngineNotify,
    ) -> Outcome<()> {
        host_failure(
            self.host
                .delete(
                    reference.clone().to_mirror(),
                    scope.to_mirror(),
                    notify.to_mirror(),
                )
                .await,
        )
    }

    async fn respond(
        &self,
        reference: &EngineRef,
        response: EngineResponse,
        scope: EngineScope,
        notify: EngineNotify,
    ) -> Outcome<EngineEvent> {
        host_failure(
            self.host
                .respond(
                    reference.clone().to_mirror(),
                    response.to_mirror(),
                    scope.to_mirror(),
                    notify.to_mirror(),
                )
                .await,
        )
        .map(|event| event.to_engine())
    }
}
