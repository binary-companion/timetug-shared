use calendar_core::model::{CalendarDescriptor, CalendarEvent, EventStatus};
use timetug_core::{CalendarInfo, TimeTugCalendarEvent};

/// Maps the connector library's model to TimeTug's. The library event is carried as is (times, all-day form and
/// attendees are used natively); only the source id and Core's own state are added.
#[derive(Clone, Copy, Debug, Default)]
pub struct EventMapper;

impl EventMapper {
    /// Creates the mapper (it has no state).
    pub fn new() -> EventMapper {
        EventMapper
    }

    /// Maps a library calendar descriptor to Core's `CalendarInfo` for the source `source_id`, carrying over the
    /// title, colour, account name, kind, service and provider.
    pub fn calendar_info(&self, d: &CalendarDescriptor, source_id: &str) -> CalendarInfo {
        let mut info = CalendarInfo::new(source_id, d.id.clone(), d.title.clone())
            .with_color_hex(d.color_hex());
        info.account_name = d.account_name.clone();
        info.kind = d.kind;
        info.service = Some(d.service.clone());
        info.provider = d.provider.clone();
        info
    }

    /// Wraps a library event as a `TimeTugCalendarEvent` for the source `source_id`.
    /// `None` for cancelled events. Titles are never rewritten: each connector chooses its own placeholder.
    pub fn event(&self, e: CalendarEvent, source_id: &str) -> Option<TimeTugCalendarEvent> {
        if e.status == EventStatus::Cancelled {
            None
        } else {
            Some(TimeTugCalendarEvent::new(e, source_id))
        }
    }
}
