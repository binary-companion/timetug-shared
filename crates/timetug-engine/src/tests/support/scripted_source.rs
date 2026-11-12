//! Port of `CallGate` and `ScriptedSource` from `Support/EngineHarness.swift`.
//!
//! `ScriptedSource` is a connector-library source with scripted events: it can fail, hold each `events_in` call at a
//! gate, count calls, and emit change signals on demand. A gated call returns the events and failure scripted when it
//! began. Swift's single `AsyncStream` is a list of senders here, because the library trait hands out a new receiver on
//! every `changes()` call; `signal` reaches all of them.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use calendar_core::failure::{Failure, Outcome};
use calendar_core::model::{CalendarDescriptor, CalendarEvent, CalendarService};
use calendar_core::source_types::{
    CalendarChange, CalendarSource, SourceCapabilities, SourceError,
};
use calendar_core::time::DateInterval;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

use super::LockedBox;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Suspends callers of `wait()` until released one by one (by arrival index) or all at once.
#[derive(Default)]
pub(crate) struct CallGate {
    waiters: Mutex<Vec<oneshot::Sender<()>>>,
}

impl CallGate {
    pub(crate) fn new() -> CallGate {
        CallGate::default()
    }

    /// How many callers wait now (a caller whose future was dropped no longer counts).
    pub(crate) fn waiting(&self) -> usize {
        let mut waiters = lock(&self.waiters);
        waiters.retain(|waiter| !waiter.is_closed());
        waiters.len()
    }

    pub(crate) async fn wait(&self) {
        let (resume, resumed) = oneshot::channel();
        lock(&self.waiters).push(resume);
        let _ = resumed.await;
    }

    /// Resumes the caller that is `index`-th among those still waiting (0 = earliest).
    pub(crate) fn release(&self, index: usize) {
        let waiter = {
            let mut waiters = lock(&self.waiters);
            waiters.retain(|waiter| !waiter.is_closed());
            waiters.remove(index)
        };
        let _ = waiter.send(());
    }

    pub(crate) fn release_all(&self) {
        let all = std::mem::take(&mut *lock(&self.waiters));
        for waiter in all {
            let _ = waiter.send(());
        }
    }
}

#[derive(Clone)]
struct Script {
    events: Vec<CalendarEvent>,
    failure: Option<SourceError>,
    gated: bool,
    events_calls: usize,
    last_interval: Option<DateInterval>,
}

pub(crate) struct ScriptedSource {
    id: String,
    pub(crate) gate: CallGate,
    script: LockedBox<Script>,
    senders: Mutex<Vec<UnboundedSender<CalendarChange>>>,
    /// Swift makes its stream in `init`, so a signal sent before anyone listens is buffered: the first `changes()` call
    /// gets this receiver.
    first_receiver: Mutex<Option<UnboundedReceiver<CalendarChange>>>,
    sent: AtomicUsize,
}

impl ScriptedSource {
    pub(crate) fn new(
        id: &str,
        events: Vec<CalendarEvent>,
        failure: Option<SourceError>,
    ) -> ScriptedSource {
        let (sender, receiver) = unbounded_channel();
        ScriptedSource {
            id: id.to_string(),
            gate: CallGate::new(),
            script: LockedBox::new(Script {
                events,
                failure,
                gated: false,
                events_calls: 0,
                last_interval: None,
            }),
            senders: Mutex::new(vec![sender]),
            first_receiver: Mutex::new(Some(receiver)),
            sent: AtomicUsize::new(0),
        }
    }

    /// Change signals sent through `signal(_:)`.
    pub(crate) fn signals_sent(&self) -> usize {
        self.sent.load(Ordering::SeqCst)
    }

    /// True once the change listener stopped consuming (cancelled or dropped): every receiver handed out is closed.
    pub(crate) fn listener_ended(&self) -> bool {
        lock(&self.senders).iter().all(UnboundedSender::is_closed)
    }

    pub(crate) fn events_calls(&self) -> usize {
        self.script.get().events_calls
    }

    /// The interval the last `events_in` call asked for.
    pub(crate) fn last_interval(&self) -> Option<DateInterval> {
        self.script.get().last_interval
    }

    pub(crate) fn set_events(&self, events: Vec<CalendarEvent>) {
        self.script.mutate(|script| script.events = events);
    }

    pub(crate) fn set_failure(&self, failure: Option<SourceError>) {
        self.script.mutate(|script| script.failure = failure);
    }

    pub(crate) fn set_gated(&self, gated: bool) {
        self.script.mutate(|script| script.gated = gated);
    }

    pub(crate) fn signal(&self, change: CalendarChange) {
        self.sent.fetch_add(1, Ordering::SeqCst);
        for sender in lock(&self.senders).iter() {
            let _ = sender.send(change.clone());
        }
    }
}

#[async_trait]
impl CalendarSource for ScriptedSource {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn display_name(&self) -> String {
        self.id.clone()
    }

    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities::default()
    }

    async fn calendars(&self) -> Outcome<Vec<CalendarDescriptor>> {
        Ok(vec![CalendarDescriptor::new(
            "cal",
            "Work",
            CalendarService::google(),
        )])
    }

    async fn events_in(&self, interval: DateInterval) -> Outcome<Vec<CalendarEvent>> {
        let current = self.script.mutate(|script| {
            script.events_calls += 1;
            script.last_interval = Some(interval);
            script.clone()
        });
        if current.gated {
            self.gate.wait().await;
        }
        if let Some(failure) = current.failure {
            return Err(Failure::Source(failure));
        }
        Ok(current.events)
    }

    fn changes(&self) -> UnboundedReceiver<CalendarChange> {
        if let Some(receiver) = lock(&self.first_receiver).take() {
            return receiver;
        }
        let (sender, receiver) = unbounded_channel();
        lock(&self.senders).push(sender);
        receiver
    }
}
