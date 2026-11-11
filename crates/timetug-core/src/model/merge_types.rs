use jiff::Timestamp;

/// One original copy folded into a merged event.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MergedMember {
    pub title: String,
    pub calendar_key: String,
    pub content_key: String,
    /// Which details the original copy had, e.g. "bare" or "location+notes" (see `DuplicateRules.detailSummary`).
    pub details: String,
    /// The original copy's own time range (a merged event shows only one of them).
    pub start: Timestamp,
    pub end: Timestamp,
}

impl MergedMember {
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
    Rule,
    Inference {
        engine_id: String,
        engine_name: String,
    },
    UserConfirmed,
}
