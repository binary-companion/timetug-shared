use jiff::Timestamp;

/// One original copy folded into a merged event.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MergedMember {
    /// The copy's title as it appeared on its calendar.
    pub title: String,
    /// Key of the calendar the copy came from (`CalendarInfo::key`).
    pub calendar_key: String,
    /// The copy's content key (`TimeTugCalendarEvent::content_key`).
    pub content_key: String,
    /// Which details the original copy had, e.g. "bare" or "location+notes" (see `DuplicateRules.detailSummary`).
    pub details: String,
    /// The original copy's own time range (a merged event shows only one of them).
    pub start: Timestamp,
    /// The original copy's own end.
    pub end: Timestamp,
}

impl MergedMember {
    /// Creates a member from its parts.
    pub fn new(
        title: impl Into<String>,
        calendar_key: impl Into<String>,
        content_key: impl Into<String>,
        details: impl Into<String>,
        start: Timestamp,
        end: Timestamp,
    ) -> MergedMember {
        MergedMember {
            title: title.into(),
            calendar_key: calendar_key.into(),
            content_key: content_key.into(),
            details: details.into(),
            start,
            end,
        }
    }
}

/// How a merged event came to be merged. Core carries data only; the app words the badge.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MergeProvenance {
    /// Merged by the duplicate rules.
    Rule,
    /// Merged on the verdict of an inference engine.
    Inference {
        /// Id of the engine that gave the verdict.
        engine_id: String,
        /// Display name of that engine.
        engine_name: String,
    },
    /// Merged by the user.
    UserConfirmed,
}
