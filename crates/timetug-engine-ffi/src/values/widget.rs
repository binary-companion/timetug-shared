//! Mirrors of the widget snapshot (the contract between the app and its widgets).
#![allow(missing_docs)]

use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use timetug_core::widget::snapshot as snap;

use super::mirror_record;

mirror_record!(WidgetEvent <=> snap::WidgetEvent {
    id: String,
    title: String,
    start: Timestamp,
    end: Timestamp,
    is_all_day: bool,
    color_hex: Option<String>,
    join_url: Option<UrlString>,
});

mirror_record!(WidgetSnapshot <=> snap::WidgetSnapshot {
    generated_at: Timestamp,
    events: Vec<WidgetEvent>,
});
