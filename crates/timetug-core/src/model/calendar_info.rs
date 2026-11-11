use calendar_core::model::{CalendarDescriptor, CalendarKind, CalendarProvider, CalendarService};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CalendarInfo {
    pub source_id: String,
    pub calendar_id: String,
    pub title: String,
    /// Owning account (iCloud, Google, ...), for grouping. Not part of `key`.
    pub account_name: Option<String>,
    /// Calendar color as "#RRGGBB" (uppercase, sRGB); `None` when unknown. Not part of `key`. Set through
    /// [`CalendarInfo::with_color_hex`], which normalizes it (Swift's `init` does).
    color_hex: Option<String>,
    /// Birthdays and subscribed feeds are system-style calendars. Descriptive only; not part of `key`.
    pub kind: CalendarKind,
    /// The connector the calendar is read through, and who hosts it (`None` when the source cannot tell). Descriptive
    /// only; not part of `key`. Lets Core see which calendars are the same account across sources.
    pub service: Option<CalendarService>,
    pub provider: Option<CalendarProvider>,
}

impl CalendarInfo {
    /// Swift's `init` with every optional argument at its default; assign the public fields for the rest and set the
    /// color through [`CalendarInfo::with_color_hex`].
    pub fn new(
        source_id: impl Into<String>,
        calendar_id: impl Into<String>,
        title: impl Into<String>,
    ) -> CalendarInfo {
        CalendarInfo {
            source_id: source_id.into(),
            calendar_id: calendar_id.into(),
            title: title.into(),
            account_name: None,
            color_hex: None,
            kind: CalendarKind::Standard,
            service: None,
            provider: None,
        }
    }

    /// Sets the color as `init(colorHex:)` does: normalized with [`CalendarInfo::normalized_hex`].
    pub fn with_color_hex(mut self, raw: Option<&str>) -> CalendarInfo {
        self.color_hex = CalendarInfo::normalized_hex(raw);
        self
    }

    pub fn color_hex(&self) -> Option<&str> {
        self.color_hex.as_deref()
    }

    /// Normalizes "#RGB", "#RRGGBB" or "RRGGBB" (any case, surrounding whitespace ok)
    /// to "#RRGGBB" uppercase; `None` for anything invalid.
    pub fn normalized_hex(raw: Option<&str>) -> Option<String> {
        CalendarDescriptor::normalized_hex(raw)
    }

    pub fn key(&self) -> String {
        CalendarInfo::key_for(&self.source_id, &self.calendar_id)
    }

    pub fn id(&self) -> String {
        self.key()
    }

    /// Swift's static `key(sourceID:calendarID:)`.
    pub fn key_for(source_id: &str, calendar_id: &str) -> String {
        format!("{source_id}/{calendar_id}")
    }
}
