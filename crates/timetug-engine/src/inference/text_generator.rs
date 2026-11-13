//! Port of `Inference/TextGenerator.swift`.

use async_trait::async_trait;
use calendar_core::failure::Outcome;
use timetug_core::dedup::adjudication::EngineInfo;

#[derive(Clone, Debug, PartialEq, Eq)]
/// Whether a `TextGenerator` can answer now.
pub enum TextGeneratorAvailability {
    /// Ready; carries the model's name and version for cached verdicts.
    Available(EngineInfo),
    /// Permanent for now: not supported, not enabled, model not ready.
    Unavailable {
        /// Why the model is not available.
        reason: String,
    },
    /// For example energy saver; pairs stay pending.
    TemporarilyUnavailable {
        /// Why the model is not available for the moment.
        reason: String,
    },
}

/// A platform's text model. Hosts implement it; the engine owns the prompt and the answer parsing.
#[async_trait]
pub trait TextGenerator: Send + Sync {
    /// Swift's `var availability: TextGeneratorAvailability { get async }`.
    async fn availability(&self) -> TextGeneratorAvailability;
    /// Returns the model's one-word answer for `prompt` given `instructions`. Fails on failure or cancellation.
    async fn generate(&self, instructions: &str, prompt: &str) -> Outcome<String>;
}
