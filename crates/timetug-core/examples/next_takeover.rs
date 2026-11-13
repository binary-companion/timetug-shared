//! Builds an agenda and picks the next takeover from a list of events. `timetug-core` is pure: it never reads the clock,
//! so the example passes `now` itself, and it needs no runtime and no network.
//!
//! Run it with `cargo run -p timetug-core --example next_takeover`.

#![allow(clippy::print_stdout, clippy::expect_used)]

use calendar_core::model::CalendarEvent;
use calendar_core::time::Zone;
use jiff::Timestamp;
use timetug_core::TimeTugCalendarEvent;
use timetug_core::agenda::day_agenda::{DayAgenda, State};
use timetug_core::model::takeover_settings::TakeoverSettings;
use timetug_core::takeover::ledger::TakeoverLedger;
use timetug_core::takeover::request::TakeoverRequest;
use timetug_core::takeover::scheduler::Scheduler;

fn at(text: &str) -> Timestamp {
    text.parse().expect("a valid instant")
}

/// A meeting with two attendees, as a connector would report it, wrapped for Core with the id of its source.
fn meeting(id: &str, title: &str, start: &str, end: &str) -> TimeTugCalendarEvent {
    let mut event = CalendarEvent::new(id, "work", title, at(start), at(end));
    event.url = calendar_core::url_string::UrlString::parse("https://meet.example.test/room");
    TimeTugCalendarEvent::new(event, "demo-account")
}

/// Runs the example (public so that the crate's test can call it).
pub fn main() {
    let zone = Zone::named("Europe/Berlin").expect("a zone every platform knows");
    let events = vec![
        meeting(
            "a",
            "Standup",
            "2026-09-18T07:00:00Z",
            "2026-09-18T07:15:00Z",
        ),
        meeting(
            "b",
            "Design review",
            "2026-09-18T12:00:00Z",
            "2026-09-18T13:00:00Z",
        ),
    ];
    // The defaults: takeover on, one minute of lead time, all-day events skipped. No calendar takes over until the
    // user turns it on; the key of a calendar is "<source id>/<calendar id>".
    let mut settings = TakeoverSettings::default();
    settings.set_takeover(true, "demo-account/work");
    let now = at("2026-09-18T07:05:00Z");

    // Today's agenda: each event is Past, Current or Upcoming at `now`.
    let agenda = DayAgenda::make(&events, &settings, now, &zone);
    assert_eq!(agenda.items.len(), 2);
    assert_eq!(agenda.items[0].state, State::Current);
    assert_eq!(agenda.items[1].state, State::Upcoming);

    // The next takeover to arm. The first event is already running, so it fires at once; after it has fired the
    // ledger skips it and the review is next, one lead time before it starts.
    let mut ledger = TakeoverLedger::new();
    let first = Scheduler::next(&events, &settings, &ledger, now).expect("something to arm");
    assert_eq!(first.event.title(), "Standup");
    assert_eq!(first.fire_at, now);
    ledger.mark_fired(&first.event, now);

    let second = Scheduler::next(&events, &settings, &ledger, now).expect("something to arm");
    assert_eq!(second.event.title(), "Design review");
    assert_eq!(second.fire_at, at("2026-09-18T11:59:00Z"));

    // What a front end needs to draw the takeover: plain data, no display strings.
    let request = TakeoverRequest::make(&second.event, second.fire_at);
    assert!(!request.has_started);
    println!("{} at {}", request.event.title(), second.fire_at);
}
