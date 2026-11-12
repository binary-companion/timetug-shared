//! Replays `vectors/inference/prompts.json` (150 cases, made by the Swift code at the baseline): 100 prompt cases
//! (the whole prompt text, the cache key and, on the first case, the instructions) and 50 answer cases (the answer
//! parsing driven through `judge` with a stub generator). The input shapes are documented in
//! `Sources/VectorExporter/Inference.swift`.

use std::sync::Arc;

use async_trait::async_trait;
use calendar_core::failure::{Failure, Outcome};
use calendar_core::time::Zone;
use jiff::Timestamp;
use serde_json::Value;
use timetug_core::dedup::adjudication::{
    AdjudicationEvent, AdjudicationRequest, DuplicateAdjudicator, EngineInfo,
};
use timetug_core::dedup::lesson_book::{Lesson, LessonDecision};

use super::support::ts;
use crate::clock::EngineClock;
use crate::diagnostic::ignore_diagnostics;
use crate::inference::prompt_builder::PromptBuilder;
use crate::inference::text_generator::{TextGenerator, TextGeneratorAvailability};
use crate::inference::text_generator_adjudicator::TextGeneratorAdjudicator;

const PROMPTS: &str = include_str!("../../../../vectors/inference/prompts.json");

fn ms(value: &Value) -> Timestamp {
    Timestamp::from_millisecond(value.as_i64().unwrap()).unwrap()
}

fn text(j: &Value, key: &str) -> Option<String> {
    j.get(key).and_then(Value::as_str).map(str::to_string)
}

fn adjudication_event(j: &Value) -> AdjudicationEvent {
    AdjudicationEvent {
        title: text(j, "title").unwrap(),
        start: ms(&j["start"]),
        end: ms(&j["end"]),
        location: text(j, "location"),
        notes: text(j, "notes"),
        attendee_names: j
            .get("attendeeNames")
            .and_then(Value::as_array)
            .map(|names| {
                names
                    .iter()
                    .map(|n| n.as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default(),
        calendar_title: text(j, "calendarTitle"),
    }
}

fn lesson(j: &Value) -> Lesson {
    Lesson {
        title_a: text(j, "titleA").unwrap(),
        title_b: text(j, "titleB").unwrap(),
        calendar_key_a: text(j, "calendarKeyA").unwrap(),
        calendar_key_b: text(j, "calendarKeyB").unwrap(),
        signals_a: text(j, "signalsA").unwrap(),
        signals_b: text(j, "signalsB").unwrap(),
        decision: match j["decision"].as_str().unwrap() {
            "same" => LessonDecision::Same,
            other => {
                assert_eq!(other, "different");
                LessonDecision::Different
            }
        },
        last_used: ms(&j["lastUsed"]),
        content_key_a: text(j, "contentKeyA"),
        content_key_b: text(j, "contentKeyB"),
    }
}

fn request(j: &Value) -> AdjudicationRequest {
    let mut request = AdjudicationRequest::new(
        text(j, "id").unwrap(),
        adjudication_event(&j["first"]),
        adjudication_event(&j["second"]),
        j["lessons"]
            .as_array()
            .unwrap()
            .iter()
            .map(lesson)
            .collect(),
    );
    request.start_offset_minutes = j["startOffsetMinutes"].as_i64().unwrap();
    request.end_offset_minutes = j["endOffsetMinutes"].as_i64().unwrap();
    request.overlap_minutes = j["overlapMinutes"].as_i64().unwrap();
    request.first_details = text(j, "firstDetails").unwrap();
    request.second_details = text(j, "secondDetails").unwrap();
    request.has_conflicting_details = j["hasConflictingDetails"].as_bool().unwrap();
    request
}

struct StubGenerator {
    mode: String,
    raw: String,
}

#[async_trait]
impl TextGenerator for StubGenerator {
    async fn availability(&self) -> TextGeneratorAvailability {
        match self.mode.as_str() {
            "unavailable" => TextGeneratorAvailability::Unavailable {
                reason: "stub".to_string(),
            },
            "temporarilyUnavailable" => TextGeneratorAvailability::TemporarilyUnavailable {
                reason: "stub".to_string(),
            },
            _ => TextGeneratorAvailability::Available(EngineInfo::new("stub", "Stub", true)),
        }
    }

    async fn generate(&self, _instructions: &str, _prompt: &str) -> Outcome<String> {
        if self.mode == "throws" {
            return Err(Failure::Host("failed".to_string()));
        }
        Ok(self.raw.clone())
    }
}

struct FixedClock;

#[async_trait]
impl EngineClock for FixedClock {
    fn now(&self) -> Timestamp {
        ts(1_800_000_000)
    }

    async fn sleep_until(&self, _deadline: Timestamp) -> Outcome<()> {
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn prompts_json_replays_every_case() {
    let file: Value = serde_json::from_str(PROMPTS).unwrap();
    assert_eq!(file["schema"], 1);
    let cases = file["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 150);
    let mut failures: Vec<String> = Vec::new();
    let (mut prompts, mut answers, mut instructions_checked) = (0, 0, 0);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let input = &case["input"];
        let expected = &case["expected"];
        match input["fn"].as_str().unwrap() {
            "prompt" => {
                prompts += 1;
                let request = request(&input["request"]);
                let zone = Zone::named(input["zone"].as_str().unwrap()).unwrap();
                let prompt = PromptBuilder::prompt_for(&request, &zone);
                if Some(prompt.as_str()) != expected["prompt"].as_str() {
                    failures.push(format!(
                        "{name}: prompt differs\n--- got\n{prompt}\n--- expected\n{}",
                        expected["prompt"].as_str().unwrap_or("<missing>")
                    ));
                }
                let key = request
                    .input()
                    .cache_key(input["engineID"].as_str().unwrap());
                if Some(key.as_str()) != expected["cacheKey"].as_str() {
                    failures.push(format!(
                        "{name}: cache key {key} != {}",
                        expected["cacheKey"]
                    ));
                }
                if let Some(instructions) = expected.get("instructions") {
                    instructions_checked += 1;
                    if Some(PromptBuilder::INSTRUCTIONS) != instructions.as_str() {
                        failures.push(format!("{name}: instructions differ"));
                    }
                }
            }
            "answer" => {
                answers += 1;
                let adjudicator = TextGeneratorAdjudicator::new(
                    Arc::new(StubGenerator {
                        mode: input["mode"].as_str().unwrap().to_string(),
                        raw: input["raw"].as_str().unwrap().to_string(),
                    }),
                    Arc::new(FixedClock),
                    Arc::new(|| Zone::named("UTC").unwrap()),
                    ignore_diagnostics(),
                );
                let event =
                    adjudication_event(&serde_json::json!({"title": "A", "start": 0, "end": 1000}));
                let verdicts = adjudicator
                    .judge(&[AdjudicationRequest::new("r1", event.clone(), event, vec![])])
                    .await;
                let got: Vec<(String, &str)> = verdicts
                    .iter()
                    .map(|v| {
                        (
                            v.request_id.clone(),
                            match v.answer {
                                timetug_core::dedup::adjudication::VerdictAnswer::Same => "same",
                                timetug_core::dedup::adjudication::VerdictAnswer::Different => {
                                    "different"
                                }
                                timetug_core::dedup::adjudication::VerdictAnswer::Unsure => {
                                    "unsure"
                                }
                            },
                        )
                    })
                    .collect();
                let want: Vec<(String, &str)> = expected["verdicts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| {
                        (
                            v["requestID"].as_str().unwrap().to_string(),
                            v["answer"].as_str().unwrap(),
                        )
                    })
                    .collect();
                if got != want {
                    failures.push(format!(
                        "{name}: verdicts {got:?} != {want:?} for raw {:?}",
                        input["raw"]
                    ));
                }
            }
            other => failures.push(format!("{name}: unknown fn {other}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} failing case(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!((prompts, answers, instructions_checked), (100, 50, 1));
}
