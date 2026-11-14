use std::collections::BTreeSet;

use calendar_core::model::ConferenceOrigin;
use calendar_core::time::seconds_between;

use crate::model::event::TimeTugCalendarEvent;
use crate::model::merge_types::MergedMember;
use crate::text::{fold, graphemes, is_letter_or_number};

/// Why the pair rules merge two events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MergeReason {
    /// Same title, start and end (the content key).
    ExactMatch,
    /// The events share an external UID.
    ExternalUid,
    /// The events share a conference link.
    ConferenceLink,
    /// The events share an attendee email and nothing conflicts.
    SharedAttendee,
    /// The events have the same location and nothing conflicts.
    SameLocation,
    /// The titles are the same after normalization and nothing conflicts.
    SameTitle,
}

impl MergeReason {
    /// The Swift raw value (`exactMatch`, `externalUID`, ...).
    pub fn as_str(&self) -> &'static str {
        match self {
            MergeReason::ExactMatch => "exactMatch",
            MergeReason::ExternalUid => "externalUID",
            MergeReason::ConferenceLink => "conferenceLink",
            MergeReason::SharedAttendee => "sharedAttendee",
            MergeReason::SameLocation => "sameLocation",
            MergeReason::SameTitle => "sameTitle",
        }
    }
}

/// Why the pair rules keep two events apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SeparateReason {
    /// Both events are on the same calendar.
    SameCalendar,
    /// At least one event is all-day.
    AllDay,
    /// The events do not overlap closely enough: see [`DuplicateRules::within_time_gate`].
    OutsideTimeGate,
    /// The locations differ.
    ConflictingLocation,
    /// Both have conference links and they share none.
    ConflictingConference,
    /// Both have attendees and they share none.
    ConflictingAttendees,
}

impl SeparateReason {
    /// The Swift raw value (`sameCalendar`, `outsideTimeGate`, ...).
    pub fn as_str(&self) -> &'static str {
        match self {
            SeparateReason::SameCalendar => "sameCalendar",
            SeparateReason::AllDay => "allDay",
            SeparateReason::OutsideTimeGate => "outsideTimeGate",
            SeparateReason::ConflictingLocation => "conflictingLocation",
            SeparateReason::ConflictingConference => "conflictingConference",
            SeparateReason::ConflictingAttendees => "conflictingAttendees",
        }
    }
}

/// What the pair rules decide about two events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PairDecision {
    /// Treat the events as one meeting, for the given reason.
    Merge(MergeReason),
    /// Keep the events apart, for the given reason.
    Separate(SeparateReason),
    /// Nothing matched and nothing conflicted: the only case a model may be asked about.
    Ambiguous,
}

/// Deterministic pair rules. Pure; no model involved.
pub struct DuplicateRules;

#[derive(PartialEq)]
enum LocationRelation {
    Unknown,
    Same,
    Conflict,
}

impl DuplicateRules {
    /// The widest difference in start times, in seconds, inside the time gate (30 minutes).
    pub const MAX_START_DIFFERENCE: f64 = 30.0 * 60.0;
    /// The widest difference in end times, in seconds, inside the time gate (60 minutes).
    pub const MAX_END_DIFFERENCE: f64 = 60.0 * 60.0;
    const MIN_CONTAINED_LOCATION_LENGTH: usize = 6;

    /// Different calendars, timed, and inside the time gate.
    pub fn is_candidate(a: &TimeTugCalendarEvent, b: &TimeTugCalendarEvent) -> bool {
        a.calendar_key() != b.calendar_key()
            && !a.is_all_day()
            && !b.is_all_day()
            && Self::within_time_gate(a, b)
    }

    /// What the user may merge by hand: two timed events whose ranges actually overlap, on any calendar (the same
    /// one included) and with none of the automatic gate's limits. Back-to-back events (one ends as the other
    /// starts) do not overlap. The context menu and the resolver's use of a user's "same" lesson both use this,
    /// so the menu only offers pairs the resolver will honor.
    pub fn is_manual_candidate(a: &TimeTugCalendarEvent, b: &TimeTugCalendarEvent) -> bool {
        !a.is_all_day() && !b.is_all_day() && a.start() < b.end() && b.start() < a.end()
    }

    /// Overlapping, starts within 30 min, ends within 60 min (end times are uncertain and may include travel).
    pub fn within_time_gate(a: &TimeTugCalendarEvent, b: &TimeTugCalendarEvent) -> bool {
        a.start() < b.end()
            && b.start() < a.end()
            && seconds_between(b.start(), a.start()).abs() <= Self::MAX_START_DIFFERENCE
            && seconds_between(b.end(), a.end()).abs() <= Self::MAX_END_DIFFERENCE
    }

    /// Applies the pair rules to `a` and `b` in order: identical content merges, then same calendar, all-day and the
    /// time gate separate, then shared UID or conference link merge, then conflicts separate, then shared attendee,
    /// location or title merge. A pair that nothing decides is [`PairDecision::Ambiguous`].
    pub fn decide(a: &TimeTugCalendarEvent, b: &TimeTugCalendarEvent) -> PairDecision {
        if a.content_key() == b.content_key() {
            return PairDecision::Merge(MergeReason::ExactMatch);
        }
        if a.calendar_key() == b.calendar_key() {
            return PairDecision::Separate(SeparateReason::SameCalendar);
        }
        if a.is_all_day() || b.is_all_day() {
            return PairDecision::Separate(SeparateReason::AllDay);
        }
        if !Self::within_time_gate(a, b) {
            return PairDecision::Separate(SeparateReason::OutsideTimeGate);
        }

        if let Some(key) = a.uid_match_key()
            && Some(&key) == b.uid_match_key().as_ref()
        {
            return PairDecision::Merge(MergeReason::ExternalUid);
        }
        let conf_a = Self::conference_identities(a);
        let conf_b = Self::conference_identities(b);
        if !conf_a.is_disjoint(&conf_b) {
            return PairDecision::Merge(MergeReason::ConferenceLink);
        }
        // Vetoes run before the weaker merge signals: a shared person or place merges two events
        // only when nothing conflicts (user decision 2026-09-19).
        let location = Self::location_relation(a.location(), b.location());
        if location == LocationRelation::Conflict {
            return PairDecision::Separate(SeparateReason::ConflictingLocation);
        }
        // Both have links and none is shared (the shared case merged above).
        if !conf_a.is_empty() && !conf_b.is_empty() {
            return PairDecision::Separate(SeparateReason::ConflictingConference);
        }
        let emails_a = Self::emails(a);
        let emails_b = Self::emails(b);
        if !emails_a.is_disjoint(&emails_b) {
            return PairDecision::Merge(MergeReason::SharedAttendee);
        }
        if location == LocationRelation::Same {
            return PairDecision::Merge(MergeReason::SameLocation);
        }
        if !emails_a.is_empty() && !emails_b.is_empty() {
            return PairDecision::Separate(SeparateReason::ConflictingAttendees);
        }
        // The same title inside the time gate is one event copied onto two calendars (the ends often differ);
        // it comes after every veto so two same-named meetings with different rooms or people stay apart.
        let title = Self::normalize(a.title());
        if !title.is_empty() && title == Self::normalize(b.title()) {
            return PairDecision::Merge(MergeReason::SameTitle);
        }
        PairDecision::Ambiguous
    }

    /// Lowercased, accent-folded, punctuation collapsed to single spaces.
    pub fn normalize(text: &str) -> String {
        let folded = fold(text);
        // Swift maps `Character`s: a cluster stays when its first scalar is a letter or a number.
        let mapped: String = graphemes(&folded)
            .into_iter()
            .map(|cluster| match cluster.chars().next() {
                Some(first) if is_letter_or_number(first) => cluster,
                _ => " ",
            })
            .collect();
        mapped
            .split(' ')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// How many kinds of detail the event has (location, conference, attendees, notes); a higher score is a fuller copy.
    pub fn detail_score(event: &TimeTugCalendarEvent) -> i64 {
        Self::detail_parts(event).len() as i64
    }

    /// "bare" or a "+"-joined list of location, conference, attendees, notes.
    pub fn detail_summary(event: &TimeTugCalendarEvent) -> String {
        let parts = Self::detail_parts(event);
        if parts.is_empty() {
            "bare".to_string()
        } else {
            parts.join("+")
        }
    }

    fn detail_parts(event: &TimeTugCalendarEvent) -> Vec<&'static str> {
        let mut parts = Vec::new();
        if Self::normalized_location(event.location()).is_some() {
            parts.push("location");
        }
        if !Self::conference_identities(event).is_empty() {
            parts.push("conference");
        }
        if event.other_attendee_count > 0 || !event.attendees().is_empty() {
            parts.push("attendees");
        }
        if event.notes().is_some_and(|notes| !notes.trim().is_empty()) {
            parts.push("notes");
        }
        parts
    }

    /// The identity of each conference link the event has (see `ConferenceInfo.identity`). The event's own
    /// generic `url` (origin `.eventURL`) never identifies a meeting; a provider-supplied link counts even for a
    /// provider not on the allowlist (Chime, say).
    pub(crate) fn conference_identities(event: &TimeTugCalendarEvent) -> BTreeSet<String> {
        event
            .conferences
            .iter()
            .filter(|c| c.origin != ConferenceOrigin::EventUrl)
            .map(|c| c.identity())
            .collect()
    }

    pub(crate) fn emails(event: &TimeTugCalendarEvent) -> BTreeSet<String> {
        event
            .attendees()
            .into_iter()
            .filter_map(|a| a.email())
            .chain(event.organizer_email())
            .map(str::to_string)
            .collect()
    }

    fn location_relation(a: Option<&str>, b: Option<&str>) -> LocationRelation {
        let (Some(a), Some(b)) = (Self::normalized_location(a), Self::normalized_location(b))
        else {
            return LocationRelation::Unknown;
        };
        if a == b {
            return LocationRelation::Same;
        }
        let (short, long) = if a.chars().count() <= b.chars().count() {
            (&a, &b)
        } else {
            (&b, &a)
        };
        if short.chars().count() >= Self::MIN_CONTAINED_LOCATION_LENGTH
            && format!(" {long} ").contains(&format!(" {short} "))
        {
            return LocationRelation::Same;
        }
        LocationRelation::Conflict
    }

    /// `None` for empty text and for URLs (a link in the location field is a conference, not a place).
    pub(crate) fn normalized_location(raw: Option<&str>) -> Option<String> {
        let raw = raw.filter(|raw| !raw.contains("://"))?;
        let normalized = Self::normalize(raw);
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    }
}

impl MergedMember {
    /// Swift's `MergedMember.init(_ event:)`.
    pub fn from_event(event: &TimeTugCalendarEvent) -> MergedMember {
        MergedMember::new(
            event.title(),
            event.calendar_key(),
            event.content_key(),
            DuplicateRules::detail_summary(event),
            event.start(),
            event.end(),
        )
    }
}

impl TimeTugCalendarEvent {
    /// The (title, calendar) copies this event stands for: itself when it was never merged.
    pub fn participants(&self) -> Vec<MergedMember> {
        if self.merged_members.is_empty() {
            vec![MergedMember::from_event(self)]
        } else {
            self.merged_members.clone()
        }
    }
}
