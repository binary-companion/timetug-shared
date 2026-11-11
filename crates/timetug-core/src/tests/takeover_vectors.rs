//! Replays `vectors/takeover/scheduler.json` and `vectors/takeover/ledger.json`, made by the Swift code at the
//! baseline (exporter: `Sources/VectorExporter/Takeover.swift`). Every case runs; the failures are reported together.

use serde_json::Value;

use super::takeover_vector_support::{
    Runner, event_from_spec, json_close, returned_json, settings_from, ts_of,
};
use crate::takeover::policy::TakeoverPolicy;
use crate::takeover::scheduler::Scheduler;

const SCHEDULER: &str = include_str!("../../../../vectors/takeover/scheduler.json");
const LEDGER: &str = include_str!("../../../../vectors/takeover/ledger.json");

/// The tolerance for reference-date numbers (see `json_close`).
const DATE_TOLERANCE: f64 = 1e-6;

fn load(text: &str, area: &str) -> Vec<Value> {
    let file: Value = serde_json::from_str(text).unwrap();
    assert_eq!(file["area"], area);
    let cases = file["cases"].as_array().unwrap().clone();
    assert!(!cases.is_empty());
    cases
}

fn report(area: &str, total: usize, failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "{area}: {} of {total} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn scheduler_vector() {
    let cases = load(SCHEDULER, "takeover/scheduler");
    let mut failures = Vec::new();
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let input = &case["input"];
        let mut runner = Runner::new(&input["events"]);
        for op in input["ledger"].as_array().unwrap() {
            runner.apply(op);
        }
        let settings = settings_from(&input["settings"]);
        let now = ts_of(&input["now"]);
        let qualifies: Vec<Value> = runner
            .events
            .iter()
            .map(|e| Value::Bool(TakeoverPolicy::qualifies(e, &settings)))
            .collect();
        if Value::Array(qualifies.clone()) != case["expected"]["qualifies"] {
            failures.push(format!(
                "{name}: qualifies: expected {}, got {:?}",
                case["expected"]["qualifies"], qualifies
            ));
        }
        let next = Scheduler::next(&runner.events, &settings, &runner.ledger, now);
        let expected = &case["expected"]["next"];
        match (next, expected) {
            (None, Value::Null) => {}
            (Some(next), Value::Object(_)) => {
                // The exporter reports `events.firstIndex(of: next.event)`: the first equal event.
                let index = runner.events.iter().position(|e| *e == next.event).unwrap();
                let ok = expected["index"].as_u64() == Some(index as u64)
                    && expected["id"].as_str() == Some(next.event.id().as_str())
                    && expected["fireAt"].as_i64() == Some(next.fire_at.as_millisecond());
                if !ok {
                    failures.push(format!(
                        "{name}: next: expected {expected}, got index {index} id {} fireAt {}",
                        next.event.id(),
                        next.fire_at.as_millisecond()
                    ));
                }
            }
            (next, expected) => failures.push(format!(
                "{name}: next: expected {expected}, got {:?}",
                next.map(|n| (n.event.id(), n.fire_at.as_millisecond()))
            )),
        }
    }
    report("takeover/scheduler", cases.len(), failures);
}

#[test]
fn ledger_vector() {
    let cases = load(LEDGER, "takeover/ledger");
    let mut failures = Vec::new();
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let input = &case["input"];
        let mut runner = Runner::new(&input["events"]);
        let expected_steps = case["expected"]["steps"].as_array().unwrap();
        let ops = input["ops"].as_array().unwrap();
        if ops.len() != expected_steps.len() {
            failures.push(format!("{name}: step count"));
            continue;
        }
        for (i, (op, expected)) in ops.iter().zip(expected_steps).enumerate() {
            let returned = returned_json(runner.apply(op));
            let mut problems = Vec::new();
            if returned != expected["returned"] {
                problems.push(format!(
                    "returned {} (expected {})",
                    returned, expected["returned"]
                ));
            }
            if expected["keyCount"].as_u64() != Some(runner.ledger.key_count() as u64) {
                problems.push(format!(
                    "keyCount {} (expected {})",
                    runner.ledger.key_count(),
                    expected["keyCount"]
                ));
            }
            if let Some(expected_ledger) = expected.get("ledger") {
                let actual = serde_json::to_value(&runner.ledger).unwrap();
                if !json_close(expected_ledger, &actual, DATE_TOLERANCE) {
                    problems.push(format!("ledger {actual} (expected {expected_ledger})"));
                }
            }
            let flags = |f: &dyn Fn(&crate::model::event::TimeTugCalendarEvent) -> bool| {
                Value::Array(runner.events.iter().map(|e| Value::Bool(f(e))).collect())
            };
            if let Some(expected_fired) = expected.get("hasFired") {
                let actual = flags(&|e| runner.ledger.has_fired(e));
                if &actual != expected_fired {
                    problems.push(format!("hasFired {actual} (expected {expected_fired})"));
                }
            }
            if let Some(expected_snoozed) = expected.get("isSnoozed") {
                let actual = flags(&|e| runner.ledger.is_snoozed(e));
                if &actual != expected_snoozed {
                    problems.push(format!("isSnoozed {actual} (expected {expected_snoozed})"));
                }
            }
            if !problems.is_empty() {
                failures.push(format!("{name}: step {i}: {}", problems.join("; ")));
            }
        }
    }
    report("takeover/ledger", cases.len(), failures);
}

#[test]
fn every_spec_in_the_vectors_parses() {
    // A guard against a replay that silently skips events: both files' event pools are read in full.
    for (text, area) in [
        (SCHEDULER, "takeover/scheduler"),
        (LEDGER, "takeover/ledger"),
    ] {
        for case in load(text, area) {
            for spec in case["input"]["events"].as_array().unwrap() {
                let _ = event_from_spec(spec);
            }
        }
    }
}
