//! Replays `vectors/widget/snapshot.json`: `WidgetSnapshot.make`, `isStale` and the `WidgetTimeline` functions for
//! 370 cases made by the Swift code at the baseline (all-day events across the date line, clock-change days).

use serde_json::{Value, json};

use crate::model::calendar_info::CalendarInfo;
use crate::widget::snapshot::WidgetSnapshot;
use crate::widget::timeline::{TodayRow, WidgetTimeline};

use super::support::zone;
use super::vector_support::{
    assert_no_failures, event_from_spec, ms, settings_from, state_from, state_name, to_ms,
    widget_event, widget_event_json,
};

const SNAPSHOT: &str = include_str!("../../../../vectors/widget/snapshot.json");

fn snapshot_from(j: &Value) -> WidgetSnapshot {
    WidgetSnapshot::new(
        ms(&j["generatedAt"]),
        j["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(widget_event)
            .collect(),
    )
}

fn snapshot_json(s: &WidgetSnapshot) -> Value {
    json!({ "generatedAt": to_ms(s.generated_at), "events": s.events.iter().map(widget_event_json).collect::<Vec<_>>() })
}

fn row_json(r: &TodayRow) -> Value {
    json!({ "event": widget_event_json(&r.event), "state": state_name(r.state) })
}

fn row_from(j: &Value) -> TodayRow {
    TodayRow::new(
        widget_event(&j["event"]),
        state_from(j["state"].as_str().unwrap()),
    )
}

fn run(input: &Value) -> Value {
    match input["fn"].as_str().unwrap() {
        "make" => {
            let calendars: Vec<CalendarInfo> = input["calendars"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| {
                    CalendarInfo::new(
                        c["sourceID"].as_str().unwrap(),
                        c["calendarID"].as_str().unwrap(),
                        c["title"].as_str().unwrap(),
                    )
                    .with_color_hex(c["colorHex"].as_str())
                })
                .collect();
            let events: Vec<_> = input["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(event_from_spec)
                .collect();
            let made = WidgetSnapshot::make(
                &events,
                &calendars,
                &settings_from(&input["settings"]),
                ms(&input["now"]),
                &zone(input["zone"].as_str().unwrap()),
            );
            json!({ "snapshot": snapshot_json(&made) })
        }
        "isStale" => {
            json!({ "stale": snapshot_from(&input["snapshot"]).is_stale(ms(&input["now"])) })
        }
        "changeDates" => {
            let dates = WidgetTimeline::change_dates(
                &snapshot_from(&input["snapshot"]),
                ms(&input["now"]),
                &zone(input["zone"].as_str().unwrap()),
                input["limit"].as_i64().unwrap(),
            );
            json!({ "dates": dates.into_iter().map(to_ms).collect::<Vec<_>>() })
        }
        "nextUp" => {
            let next = WidgetTimeline::next_up(
                &snapshot_from(&input["snapshot"]),
                ms(&input["now"]),
                input["upcomingLimit"].as_i64().unwrap(),
            );
            json!({
                "current": next.current.as_ref().map_or(Value::Null, widget_event_json),
                "upcoming": next.upcoming.iter().map(widget_event_json).collect::<Vec<_>>()
            })
        }
        "today" => {
            let rows = WidgetTimeline::today(
                &snapshot_from(&input["snapshot"]),
                ms(&input["now"]),
                &zone(input["zone"].as_str().unwrap()),
            );
            json!({ "rows": rows.iter().map(row_json).collect::<Vec<_>>() })
        }
        "visibleToday" => {
            let rows: Vec<TodayRow> = input["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(row_from)
                .collect();
            let visible = WidgetTimeline::visible_today(&rows, input["capacity"].as_i64().unwrap());
            json!({ "rows": visible.iter().map(row_json).collect::<Vec<_>>() })
        }
        other => panic!("unknown fn {other}"),
    }
}

#[test]
fn widget_snapshot_vector() {
    let file: Value = serde_json::from_str(SNAPSHOT).unwrap();
    assert_eq!(file["area"], "widget/snapshot");
    let cases = file["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 370);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let actual = run(&case["input"]);
        if actual != case["expected"] {
            failures.push(format!(
                "{name}: expected {}, got {actual}",
                case["expected"]
            ));
        }
    }
    assert_no_failures("widget/snapshot", cases.len(), failures);
}
