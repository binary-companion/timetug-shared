//! Port of `TimeTugEngineTests/Inference/TextGeneratorAdjudicatorTests.swift`.
//!
//! Swift's `Boom` error becomes `Failure::Host("Boom ...")`: Rust has no error type names, so the engine logs the
//! variant name (`HostError`) and the test checks that the host's text never reaches the diagnostic.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use calendar_core::failure::{Failure, Outcome};
use calendar_core::model::CalendarEvent;
use calendar_core::time::Zone;
use timetug_core::TimeTugCalendarEvent;
use timetug_core::dedup::adjudication::{
    AdjudicationEvent, AdjudicationRequest, AdjudicationVerdict, AdjudicatorAvailability,
    DuplicateAdjudicator, EngineInfo, VerdictAnswer,
};

use super::support::{LockedBox, ManualClock, poll, ts};
use crate::clock::EngineClock;
use crate::diagnostic::EngineDiagnostic;
use crate::inference::text_generator::{TextGenerator, TextGeneratorAvailability};
use crate::inference::text_generator_adjudicator::TextGeneratorAdjudicator;

#[derive(Clone)]
enum Step {
    Answer(&'static str),
    Fail,
    /// Never returns (Swift sleeps for 60 seconds, far longer than any test waits).
    Hang,
    Cancelled,
}

struct FakeTextGenerator {
    availability_value: LockedBox<TextGeneratorAvailability>,
    steps: Mutex<VecDeque<Step>>,
    calls: LockedBox<i64>,
    on_call: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl FakeTextGenerator {
    fn new(steps: Vec<Step>) -> Arc<FakeTextGenerator> {
        Arc::new(FakeTextGenerator {
            availability_value: LockedBox::new(TextGeneratorAvailability::Available(
                EngineInfo::new("fake", "Fake", true),
            )),
            steps: Mutex::new(steps.into()),
            calls: LockedBox::new(0),
            on_call: Mutex::new(None),
        })
    }

    fn set_on_call(&self, body: impl Fn() + Send + Sync + 'static) {
        *self.on_call.lock().unwrap() = Some(Arc::new(body));
    }
}

#[async_trait]
impl TextGenerator for FakeTextGenerator {
    async fn availability(&self) -> TextGeneratorAvailability {
        self.availability_value.get()
    }

    async fn generate(&self, _instructions: &str, _prompt: &str) -> Outcome<String> {
        self.calls.mutate(|n| *n += 1);
        let on_call = self.on_call.lock().unwrap().clone();
        if let Some(on_call) = on_call {
            on_call();
        }
        let step = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Step::Answer("unsure"));
        match step {
            Step::Answer(text) => Ok(text.to_string()),
            Step::Fail => Err(Failure::Host("Boom with event text A".to_string())),
            Step::Hang => std::future::pending().await,
            Step::Cancelled => Err(Failure::Cancelled),
        }
    }
}

fn request(id: &str) -> AdjudicationRequest {
    let start = ts(1_800_000_000);
    let ev = |title: &str| {
        AdjudicationEvent::new(
            &TimeTugCalendarEvent::new(
                CalendarEvent::new(title, "c", title, start, ts(1_800_003_600)),
                "s",
            ),
            None,
        )
    };
    AdjudicationRequest::new(id, ev("A"), ev("B"), vec![])
}

fn make(
    generator: &Arc<FakeTextGenerator>,
    clock: Option<Arc<ManualClock>>,
    log: Option<Arc<LockedBox<Vec<EngineDiagnostic>>>>,
) -> TextGeneratorAdjudicator {
    let clock: Arc<dyn EngineClock> =
        clock.unwrap_or_else(|| Arc::new(ManualClock::new(ts(1_800_000_000))));
    TextGeneratorAdjudicator::new(
        generator.clone(),
        clock,
        Arc::new(|| Zone::named("UTC").unwrap()),
        Arc::new(move |e| {
            if let Some(log) = &log {
                log.mutate(|entries| entries.push(e));
            }
        }),
    )
}

fn verdict(id: &str, answer: VerdictAnswer) -> AdjudicationVerdict {
    AdjudicationVerdict::new(id, answer)
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn parses_answers_trimming_and_lowercasing() {
    let generator = FakeTextGenerator::new(vec![
        Step::Answer("same"),
        Step::Answer("  Different\n"),
        Step::Answer("UNSURE "),
    ]);
    let verdicts = make(&generator, None, None)
        .judge(&[request("1"), request("2"), request("3")])
        .await;
    assert_eq!(
        verdicts,
        vec![
            verdict("1", VerdictAnswer::Same),
            verdict("2", VerdictAnswer::Different),
            verdict("3", VerdictAnswer::Unsure),
        ]
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn unparseable_answer_skips_only_that_request() {
    let generator = FakeTextGenerator::new(vec![Step::Answer("maybe"), Step::Answer("same")]);
    let verdicts = make(&generator, None, None)
        .judge(&[request("1"), request("2")])
        .await;
    assert_eq!(verdicts, vec![verdict("2", VerdictAnswer::Same)]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn throwing_generator_yields_no_verdict_and_logs_only_the_type_name() {
    let log = Arc::new(LockedBox::new(Vec::new()));
    let generator = FakeTextGenerator::new(vec![Step::Fail, Step::Answer("same")]);
    let verdicts = make(&generator, None, Some(log.clone()))
        .judge(&[request("1"), request("2")])
        .await;
    assert_eq!(verdicts, vec![verdict("2", VerdictAnswer::Same)]);
    let events = log.get();
    assert_eq!(events.len(), 1);
    // Swift: `message.contains("Boom")`, the error's type name. The Rust name for a host failure is `HostError`.
    assert_eq!(events[0].message, "Judgment failed: HostError");
    assert_eq!(events[0].category, "inference");
    assert!(!events[0].message.contains("Boom"));
    assert!(!events[0].message.contains("\"A\""));
    assert!(!events[0].message.contains("event text"));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn temporarily_unavailable_returns_nothing_without_calling_the_generator() {
    let generator = FakeTextGenerator::new(vec![Step::Answer("same")]);
    generator
        .availability_value
        .set(TextGeneratorAvailability::TemporarilyUnavailable {
            reason: "energy saver".to_string(),
        });
    let adjudicator = make(&generator, None, None);
    assert!(adjudicator.judge(&[request("1")]).await.is_empty());
    assert_eq!(generator.calls.get(), 0);
    assert_eq!(
        adjudicator.availability(),
        AdjudicatorAvailability::Unavailable {
            reason: "temporary: energy saver".to_string()
        }
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn permanent_unavailability_keeps_its_reason_and_available_is_cached() {
    let generator = FakeTextGenerator::new(vec![]);
    let adjudicator = make(&generator, None, None);
    assert_eq!(
        adjudicator.availability(),
        AdjudicatorAvailability::Unavailable {
            reason: "not checked yet".to_string()
        }
    );
    generator
        .availability_value
        .set(TextGeneratorAvailability::Unavailable {
            reason: "model not ready".to_string(),
        });
    adjudicator.refresh_availability().await;
    assert_eq!(
        adjudicator.availability(),
        AdjudicatorAvailability::Unavailable {
            reason: "model not ready".to_string()
        }
    );
    let info = EngineInfo::new("fake", "Fake", true);
    generator
        .availability_value
        .set(TextGeneratorAvailability::Available(info.clone()));
    adjudicator.refresh_availability().await;
    assert_eq!(
        adjudicator.availability(),
        AdjudicatorAvailability::Available(info)
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn at_most_twenty_requests_are_judged() {
    let generator = FakeTextGenerator::new(vec![]);
    let requests: Vec<AdjudicationRequest> = (0..25).map(|i| request(&format!("r{i}"))).collect();
    let verdicts = make(&generator, None, None).judge(&requests).await;
    assert_eq!(verdicts.len(), 20);
    assert_eq!(generator.calls.get(), 20);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn deadline_uses_the_injected_clock() {
    let clock = Arc::new(ManualClock::new(ts(1_800_000_000)));
    let generator = FakeTextGenerator::new(vec![]);
    // The first call "takes" 31 seconds on the manual clock; the loop must stop before the second.
    let ticking = clock.clone();
    generator.set_on_call(move || ticking.advance_now(31.0));
    let verdicts = make(&generator, Some(clock), None)
        .judge(&[request("1"), request("2"), request("3")])
        .await;
    assert_eq!(verdicts.len(), 1);
    assert_eq!(generator.calls.get(), 1);
}

/// Swift cancels the task, `generate` throws `CancellationError` and the loop breaks. A Rust task that is aborted just
/// stops at its await point (conventions section 10, item 11): no later request is started and no verdict returns.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn cancellation_stops_the_loop() {
    let generator = FakeTextGenerator::new(vec![Step::Hang, Step::Answer("same")]);
    let adjudicator = Arc::new(make(&generator, None, None));
    let task = {
        let adjudicator = adjudicator.clone();
        tokio::spawn(async move { adjudicator.judge(&[request("1"), request("2")]).await })
    };
    poll(
        || "the first generate call".to_string(),
        30.0,
        || async { generator.calls.get() > 0 },
    )
    .await;
    task.abort();
    let result = task.await;
    assert!(result.unwrap_err().is_cancelled());
    assert_eq!(generator.calls.get(), 1);
}

/// Rust-only: a generator that reports `Failure::Cancelled` ends the loop quietly (Swift's `CancellationError` branch):
/// no verdict, no diagnostic, no further call.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_cancelled_generate_call_ends_the_loop_without_a_diagnostic() {
    let log = Arc::new(LockedBox::new(Vec::new()));
    let generator = FakeTextGenerator::new(vec![
        Step::Answer("same"),
        Step::Cancelled,
        Step::Answer("same"),
    ]);
    let verdicts = make(&generator, None, Some(log.clone()))
        .judge(&[request("1"), request("2"), request("3")])
        .await;
    assert_eq!(verdicts, vec![verdict("1", VerdictAnswer::Same)]);
    assert_eq!(generator.calls.get(), 2);
    assert!(log.get().is_empty());
}

/// Rust-only: an unparseable answer logs a fixed message that carries none of the model's text.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn an_unexpected_answer_logs_a_fixed_message() {
    let log = Arc::new(LockedBox::new(Vec::new()));
    let generator = FakeTextGenerator::new(vec![Step::Answer("secret model words")]);
    let verdicts = make(&generator, None, Some(log.clone()))
        .judge(&[request("1")])
        .await;
    assert!(verdicts.is_empty());
    let events = log.get();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].message, "Unexpected answer from the text model");
}
