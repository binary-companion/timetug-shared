//! The widget data of `TimeTugCore/Widget`.

/// `WidgetSnapshot` and `WidgetEvent`, the agenda a front end hands to widgets.
pub mod snapshot;
/// `WidgetTimeline`, the pure functions a widget uses to turn a snapshot into what it shows.
pub mod timeline;
