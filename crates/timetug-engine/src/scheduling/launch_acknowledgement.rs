//! Port of `Scheduling/LaunchAcknowledgement.swift`.

use std::collections::{BTreeMap, BTreeSet};

use calendar_core::time::add_seconds;
use jiff::Timestamp;
use timetug_core::{CalendarSnapshot, SourceStatus, TimeTugCalendarEvent};

/// Keeps each source pending until its first successful read after launch or addition.
#[derive(Clone, Debug)]
pub struct LaunchAcknowledgement {
    launched_at: Timestamp,
    registered_initial_sources: bool,
    pending: BTreeMap<String, Timestamp>,
    known: BTreeSet<String>,
}

impl LaunchAcknowledgement {
    pub fn new(launched_at: Timestamp) -> LaunchAcknowledgement {
        LaunchAcknowledgement {
            launched_at,
            registered_initial_sources: false,
            pending: BTreeMap::new(),
            known: BTreeSet::new(),
        }
    }

    pub fn register_sources(&mut self, ids: &BTreeSet<String>, now: Timestamp) {
        let cutoff = if self.registered_initial_sources {
            now
        } else {
            self.launched_at
        };
        self.registered_initial_sources = true;
        for id in ids.difference(&self.known) {
            self.pending.insert(id.clone(), cutoff);
        }
        self.pending.retain(|id, _| ids.contains(id));
        self.known = ids.clone();
    }

    /// Returns already-underway meetings on newly healthy sources, then marks those sources initialized.
    pub fn events_to_acknowledge(
        &mut self,
        snapshot: &CalendarSnapshot,
        now: Timestamp,
        grace: f64,
    ) -> Vec<TimeTugCalendarEvent> {
        let ready: BTreeMap<String, Timestamp> = self
            .pending
            .iter()
            .filter(|(id, _)| snapshot.statuses.get(*id) == Some(&SourceStatus::Ok))
            .map(|(id, cutoff)| (id.clone(), *cutoff))
            .collect();
        for id in ready.keys() {
            self.pending.remove(id);
        }
        if ready.is_empty() {
            return Vec::new();
        }
        snapshot
            .events
            .iter()
            .filter(|event| {
                event.end() > now
                    && event.participants().iter().any(|member| {
                        ready.iter().any(|(source_id, cutoff)| {
                            member.calendar_key.starts_with(&format!("{source_id}/"))
                                && member.start < add_seconds(*cutoff, -grace)
                        })
                    })
            })
            .cloned()
            .collect()
    }
}
