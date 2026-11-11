use calendar_core::model::{CalendarDescriptor, CalendarEvent, EventStatus};
use timetug_core::{CalendarInfo, TimeTugCalendarEvent};

/// Maps the connector library's model to TimeTug's. The library event is carried as is (times, all-day form and
/// attendees are used natively); only the source id and Core's own state are added.
#[derive(Clone, Copy, Debug, Default)]
pub struct EventMapper;

impl EventMapper {
    pub fn new() -> EventMapper {
        EventMapper
    }

    pub fn calendar_info(&self, d: &CalendarDescriptor, source_id: &str) -> CalendarInfo {
        let mut info = CalendarInfo::new(source_id, d.id.clone(), d.title.clone())
            .with_color_hex(d.color_hex());
        info.account_name = d.account_name.clone();
        info.kind = d.kind;
        info.service = Some(d.service.clone());
        info.provider = d.provider.clone();
        info
    }

    /// `None` for cancelled events. Titles are never rewritten: each connector chooses its own placeholder.
    pub fn event(&self, e: CalendarEvent, source_id: &str) -> Option<TimeTugCalendarEvent> {
        if e.status == EventStatus::Cancelled {
            None
        } else {
            Some(TimeTugCalendarEvent::new(e, source_id))
        }
    }
}
