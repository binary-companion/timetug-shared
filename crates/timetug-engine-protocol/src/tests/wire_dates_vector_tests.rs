//! Replay of `vectors/wire/dates.json` (produced by the Swift `WireDates` at the baseline): `fn string` writes the
//! Date given as a Double of Unix seconds; `fn date` reads a text and answers its seconds (within 1e-6; Foundation's
//! `Date` keeps seconds since 2001, so far from 2001 it differs from the exact value by about 1e-7) and its
//! millisecond value (exact).

use serde_json::Value;

use crate::wire_types::WireDates;

const VECTOR: &str = include_str!("../../../../vectors/wire/dates.json");

#[test]
fn replays_every_case() {
    let file: Value = serde_json::from_str(VECTOR).unwrap();
    assert_eq!(file["area"], "wire/dates");
    let cases = file["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 413);
    let mut failures = Vec::new();
    let (mut strings, mut dates) = (0, 0);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let input = &case["input"];
        let expected = &case["expected"];
        match input["fn"].as_str().unwrap() {
            "string" => {
                strings += 1;
                let seconds = input["seconds"].as_f64().unwrap();
                let got = WireDates::string_from_unix_seconds(seconds);
                if Some(got.as_str()) != expected["text"].as_str() {
                    failures.push(format!(
                        "{name}: string({seconds}) = {got}, expected {}",
                        expected["text"]
                    ));
                }
            }
            "date" => {
                dates += 1;
                let text = input["text"].as_str().unwrap();
                let got = WireDates::unix_nanoseconds(text);
                match (got, expected["millis"].as_i64()) {
                    (None, None) => {
                        if !expected["seconds"].is_null() {
                            failures.push(format!("{name}: seconds without millis"));
                        }
                    }
                    (Some(nanos), Some(millis)) => {
                        let seconds = nanos as f64 / 1e9;
                        let want = expected["seconds"].as_f64().unwrap();
                        if (seconds - want).abs() >= 1e-6 {
                            failures.push(format!("{name}: seconds {seconds} vs {want}"));
                        }
                        // Round half away from zero, as `Double.rounded()` does.
                        let got_millis = (seconds * 1000.0).round() as i64;
                        if got_millis != millis {
                            failures.push(format!("{name}: millis {got_millis} vs {millis}"));
                        }
                    }
                    (got, want) => failures.push(format!(
                        "{name}: date({text:?}) = {got:?}, expected {want:?}"
                    )),
                }
            }
            other => failures.push(format!("{name}: unknown fn {other}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases failed:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
    assert_eq!((strings, dates), (201, 212));
}
