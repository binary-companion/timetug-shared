//! The model of `TimeTugCore/Model`.

/// `CalendarInfo`, the calendar descriptor Core keeps per calendar.
pub mod calendar_info;
/// `TimeTugCalendarEvent`, the event type Core works with.
pub mod event;
/// All-day date helpers for `TimeTugCalendarEvent`.
pub mod event_all_day;
/// Merge provenance types: why events were merged and which calendars they came from.
pub mod merge_types;
/// `TakeoverSettings`, the user's takeover and display preferences.
pub mod takeover_settings;
