//! Port of `Inference/TextGeneratorAdjudicator.swift`.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use calendar_core::failure::Failure;
use calendar_core::time::{Zone, add_seconds};
use timetug_core::dedup::adjudication::{
    AdjudicationRequest, AdjudicationVerdict, AdjudicatorAvailability, DuplicateAdjudicator,
    VerdictAnswer,
};

use crate::clock::EngineClock;
use crate::diagnostic::{EngineDiagnostic, EngineDiagnosticSink, Level};
use crate::inference::prompt_builder::PromptBuilder;
use crate::inference::text_generator::{TextGenerator, TextGeneratorAvailability};

/// Judges ambiguous duplicate pairs with a host-supplied `TextGenerator`.
pub struct TextGeneratorAdjudicator {
    generator: Arc<dyn TextGenerator>,
    clock: Arc<dyn EngineClock>,
    time_zone: Arc<dyn Fn() -> Zone + Send + Sync>,
    diagnostics: EngineDiagnosticSink,
    cached: Mutex<AdjudicatorAvailability>,
}

impl TextGeneratorAdjudicator {
    /// Seconds one `judge` call may take in total; requests not started by then are left pending.
    pub const DEADLINE: f64 = 30.0;
    /// Most requests one `judge` call sends to the model.
    pub const MAX_REQUESTS: usize = 20;
    /// Prefix on the reason of an `Unavailable` result that is only temporary.
    pub const TEMPORARY_PREFIX: &'static str = "temporary: ";

    /// Swift's default `diagnostics` argument is `{ _ in }`: pass `ignore_diagnostics()`.
    pub fn new(
        generator: Arc<dyn TextGenerator>,
        clock: Arc<dyn EngineClock>,
        time_zone: Arc<dyn Fn() -> Zone + Send + Sync>,
        diagnostics: EngineDiagnosticSink,
    ) -> TextGeneratorAdjudicator {
        TextGeneratorAdjudicator {
            generator,
            clock,
            time_zone,
            diagnostics,
            cached: Mutex::new(AdjudicatorAvailability::Unavailable {
                reason: "not checked yet".to_string(),
            }),
        }
    }

    fn cache(&self) -> MutexGuard<'_, AdjudicatorAvailability> {
        self.cached.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Asks the generator and caches the answer for the synchronous `availability`.
    pub async fn refresh_availability(&self) -> AdjudicatorAvailability {
        let mapped = match self.generator.availability().await {
            TextGeneratorAvailability::Available(info) => AdjudicatorAvailability::Available(info),
            TextGeneratorAvailability::Unavailable { reason } => {
                AdjudicatorAvailability::Unavailable { reason }
            }
            TextGeneratorAvailability::TemporarilyUnavailable { reason } => {
                AdjudicatorAvailability::Unavailable {
                    reason: format!("{}{reason}", Self::TEMPORARY_PREFIX),
                }
            }
        };
        *self.cache() = mapped.clone();
        mapped
    }
}

/// Swift logs `String(describing: type(of: error))`. Rust has no error type names, so the variant of `Failure` stands in
/// for it; the text a host failure carries (it can embed event content) never reaches a diagnostic.
fn failure_name(failure: &Failure) -> &'static str {
    match failure {
        Failure::Source(_) => "SourceError",
        Failure::Write(_) => "WriteError",
        Failure::Cancelled => "CancellationError",
        Failure::Host(_) => "HostError",
    }
}

/// Foundation's `CharacterSet.whitespacesAndNewlines`, as CoreFoundation defines it: tab, space, U+00A0, U+1680,
/// U+2000 to U+200B (the zero width space is a member, which Rust's `char::is_whitespace` does not include), U+202F,
/// U+205F, U+3000, and the newlines U+000A to U+000D, U+0085, U+2028, U+2029. `str::trim` would leave a leading or
/// trailing U+200B in place (pinned by the `answer-26` case of `vectors/inference/prompts.json`).
fn is_whitespace_or_newline(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{D}'
            | '\u{20}'
            | '\u{85}'
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200B}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

#[async_trait]
impl DuplicateAdjudicator for TextGeneratorAdjudicator {
    /// The last value `refresh_availability()` saw; `Unavailable` until the first refresh.
    fn availability(&self) -> AdjudicatorAvailability {
        self.cache().clone()
    }

    async fn judge(&self, requests: &[AdjudicationRequest]) -> Vec<AdjudicationVerdict> {
        if !matches!(
            self.refresh_availability().await,
            AdjudicatorAvailability::Available(_)
        ) {
            return Vec::new();
        }
        let mut verdicts = Vec::new();
        let deadline = add_seconds(self.clock.now(), Self::DEADLINE);
        // `Task.isCancelled` has no counterpart: a cancelled Rust task is dropped at its next await.
        for request in requests.iter().take(Self::MAX_REQUESTS) {
            if self.clock.now() >= deadline {
                break;
            }
            let zone = (self.time_zone)();
            match self
                .generator
                .generate(
                    PromptBuilder::INSTRUCTIONS,
                    &PromptBuilder::prompt_for(request, &zone),
                )
                .await
            {
                Ok(raw) => {
                    let word = raw.trim_matches(is_whitespace_or_newline).to_lowercase();
                    let answer = match word.as_str() {
                        "same" => VerdictAnswer::Same,
                        "different" => VerdictAnswer::Different,
                        "unsure" => VerdictAnswer::Unsure,
                        _ => {
                            (self.diagnostics)(EngineDiagnostic::new(
                                Level::Error,
                                "inference",
                                "Unexpected answer from the text model",
                            ));
                            continue;
                        }
                    };
                    verdicts.push(AdjudicationVerdict::new(request.id.clone(), answer));
                }
                Err(Failure::Cancelled) => break,
                Err(failure) => {
                    // No verdict is cached, so the pair is retried later. Only the error kind is logged: model errors
                    // can embed event content.
                    (self.diagnostics)(EngineDiagnostic::new(
                        Level::Error,
                        "inference",
                        format!("Judgment failed: {}", failure_name(&failure)),
                    ));
                }
            }
        }
        verdicts
    }
}
