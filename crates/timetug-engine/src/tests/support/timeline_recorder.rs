//! Port of `Support/TimelineRecorder.swift`.
//!
//! Records the engine's `takeoverDue`, `takeoverWithdrawn` and `widgetSnapshot` events in arrival order. A takeover
//! entry is stamped with the manual clock's time (seconds after the clock's start) when it arrived; a widget entry with
//! its snapshot's `generatedAt`, so a lagging consumer never mis-stamps it. Conformance tests assert the takeover
//! entries exactly and the widget entries only structurally: how many partial publications a refresh makes depends on
//! how the sources' answers interleave. Subscribe before sending `.start`.
//!
//! The entries are compared as values and printed as text (`due(a@540)`, `due(b, late@1440)`, `withdrawn(b@600)`,
//! `widget@0`), exactly as Swift's `description` writes them, because assertion messages are read by people.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use calendar_core::time::seconds_between;
use timetug_core::widget::snapshot::WidgetSnapshot;

use super::LockedBox;
use super::engine_harness::EngineHarness;
use super::manual_clock::ManualClock;
use crate::clock::EngineClock;
use crate::event::EngineEvent;

#[derive(Clone, PartialEq)]
pub(crate) enum Entry {
    /// `id` is the request's `sourceEventID`; `late` is `TakeoverRequest.hasStarted`.
    Due {
        id: String,
        at: f64,
        late: bool,
    },
    /// `id` is the withdrawn event's `sourceEventID`.
    Withdrawn {
        id: String,
        at: f64,
    },
    Widget {
        at: f64,
    },
}

impl Entry {
    /// `.due(id, at:)` with `late` at its default.
    pub(crate) fn due(id: &str, at: f64) -> Entry {
        Entry::Due {
            id: id.to_string(),
            at,
            late: false,
        }
    }

    pub(crate) fn due_late(id: &str, at: f64) -> Entry {
        Entry::Due {
            id: id.to_string(),
            at,
            late: true,
        }
    }

    pub(crate) fn withdrawn(id: &str, at: f64) -> Entry {
        Entry::Withdrawn {
            id: id.to_string(),
            at,
        }
    }
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Entry::Due { id, at, late } => write!(
                f,
                "due({id}{}@{})",
                if *late { ", late" } else { "" },
                *at as i64
            ),
            Entry::Withdrawn { id, at } => write!(f, "withdrawn({id}@{})", *at as i64),
            Entry::Widget { at } => write!(f, "widget@{}", *at as i64),
        }
    }
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

pub(crate) struct TimelineRecorder {
    entries: Arc<LockedBox<Vec<Entry>>>,
    widgets: Arc<LockedBox<Vec<WidgetSnapshot>>>,
}

impl TimelineRecorder {
    pub(crate) fn new(h: &EngineHarness) -> TimelineRecorder {
        let mut stream = h.engine.events();
        // Events emitted from now on reach this recorder; `settle()` waits until it received all of them.
        let activity = h.engine.activity().clone();
        let offset = activity.events_emitted();
        let received = Arc::new(LockedBox::new(0i64));
        let received_by_check = received.clone();
        h.drain_checks.mutate(|checks| {
            checks.push(Arc::new(move || {
                received_by_check.get() == activity.events_emitted() - offset
            }));
        });
        let clock: Arc<ManualClock> = h.clock.clone();
        let origin = h.clock.now();
        let entries = Arc::new(LockedBox::new(Vec::new()));
        let widgets = Arc::new(LockedBox::new(Vec::new()));
        let (entries_task, widgets_task) = (entries.clone(), widgets.clone());
        tokio::spawn(async move {
            while let Some(event) = stream.recv().await {
                let at = seconds_between(origin, clock.now());
                match &event {
                    EngineEvent::TakeoverDue { request, .. } => {
                        entries_task.mutate(|list| {
                            list.push(Entry::Due {
                                id: request.event.source_event_id().to_string(),
                                at,
                                late: request.has_started,
                            });
                        });
                    }
                    EngineEvent::TakeoverWithdrawn { event_id } => {
                        entries_task.mutate(|list| {
                            list.push(Entry::Withdrawn {
                                id: TimelineRecorder::source_event_id_of(event_id),
                                at,
                            });
                        });
                    }
                    EngineEvent::WidgetSnapshot(snapshot) => {
                        widgets_task.mutate(|list| list.push(snapshot.clone()));
                        entries_task.mutate(|list| {
                            list.push(Entry::Widget {
                                at: seconds_between(origin, snapshot.generated_at),
                            });
                        });
                    }
                    EngineEvent::JoinRequested(_) | EngineEvent::Diagnostic(_) => {}
                }
                received.mutate(|count| *count += 1);
            }
        });
        TimelineRecorder { entries, widgets }
    }

    pub(crate) fn entries(&self) -> Vec<Entry> {
        self.entries.get()
    }

    /// The `takeoverDue` and `takeoverWithdrawn` entries, in order.
    pub(crate) fn takeovers(&self) -> Vec<Entry> {
        self.entries()
            .into_iter()
            .filter(|entry| !matches!(entry, Entry::Widget { .. }))
            .collect()
    }

    /// The times at which at least one widget snapshot was generated, as whole-number-exact `f64` keys in seconds.
    pub(crate) fn widget_times(&self) -> BTreeSet<i64> {
        self.entries()
            .into_iter()
            .filter_map(|entry| match entry {
                Entry::Widget { at } => Some(at as i64),
                _ => None,
            })
            .collect()
    }

    /// Every widget snapshot, in arrival order.
    pub(crate) fn widgets(&self) -> Vec<WidgetSnapshot> {
        self.widgets.get()
    }

    /// `TimeTugCalendarEvent.id` is "source/sourceEventID/startEpoch".
    fn source_event_id_of(id: &str) -> String {
        let parts: Vec<&str> = id.split('/').filter(|part| !part.is_empty()).collect();
        if parts.len() >= 3 {
            parts[1..parts.len() - 1].join("/")
        } else {
            id.to_string()
        }
    }

    /// Waits until `count` takeover entries arrived, then (with `settle`) until the engine is quiescent, so an extra
    /// one shows up in the assertion that follows. Pass `settle: false` while the test holds a refresh at a gate.
    pub(crate) async fn wait_for_takeovers(&self, count: usize, h: &EngineHarness, settle: bool) {
        h.wait_until(
            &format!("{count} takeover entries (have {:?})", self.takeovers()),
            || self.takeovers().len() >= count,
        )
        .await;
        if settle {
            h.settle().await;
        }
    }

    /// Waits until a widget snapshot generated at `seconds` arrived (a refresh at that time published), then lets the
    /// rest of that refresh land.
    pub(crate) async fn wait_for_widget(&self, seconds: i64, h: &EngineHarness) {
        h.wait_until(
            &format!(
                "a widget snapshot at {seconds} (have {:?})",
                self.widget_times()
            ),
            || self.widget_times().contains(&seconds),
        )
        .await;
        h.settle().await;
    }
}
