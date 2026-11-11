//! Replays `vectors/agenda/day-agenda.json`: `DayAgenda.make` for 320 scenarios made by the Swift code at the
//! baseline, including all-day events viewed from zones across the date line and days with a clock change.

use serde_json::Value;

use crate::agenda::day_agenda::DayAgenda;
use crate::model::event::TimeTugCalendarEvent;

use super::support::zone;
use super::vector_support::{assert_no_failures, event_from_spec, ms, settings_from, state_name};

const DAY_AGENDA: &str = include_str!("../../../../vectors/agenda/day-agenda.json");

fn index_of(events: &[TimeTugCalendarEvent], wanted: &TimeTugCalendarEvent) -> usize {
    // Swift: `events.firstIndex(of:)`, by value equality.
    events.iter().position(|e| e == wanted).unwrap()
}

#[test]
fn day_agenda_vector() {
    let file: Value = serde_json::from_str(DAY_AGENDA).unwrap();
    assert_eq!(file["area"], "agenda/day-agenda");
    let cases = file["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 320);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let input = &case["input"];
        let events: Vec<TimeTugCalendarEvent> = input["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(event_from_spec)
            .collect();
        let agenda = DayAgenda::make(
            &events,
            &settings_from(&input["settings"]),
            ms(&input["now"]),
            &zone(input["zone"].as_str().unwrap()),
        );
        let items: Vec<Value> = agenda
            .items
            .iter()
            .map(|item| {
                serde_json::json!({
                    "index": index_of(&events, &item.event), "id": item.id(), "state": state_name(item.state)
                })
            })
            .collect();
        let next = agenda.next.as_ref().map_or(
            Value::Null,
            |e| serde_json::json!({ "index": index_of(&events, e), "id": e.id() }),
        );
        let expected = &case["expected"];
        if Value::Array(items.clone()) != expected["items"] || next != expected["next"] {
            failures.push(format!(
                "{name}: expected items {} next {}, got items {} next {}",
                expected["items"],
                expected["next"],
                Value::Array(items),
                next
            ));
        }
    }
    assert_no_failures("agenda/day-agenda", cases.len(), failures);
}
