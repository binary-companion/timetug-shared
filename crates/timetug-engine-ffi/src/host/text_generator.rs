//! `TextGenerator`: the host's text model, used to judge ambiguous duplicate events.
use std::sync::Arc;

use async_trait::async_trait;
use calendar_core::failure::Outcome;
use timetug_engine::inference::text_generator::{
    TextGenerator as EngineTextGenerator, TextGeneratorAvailability as EngineAvailability,
};

use crate::errors::{HostError, HostService, failure_from_host_error};
use crate::values::ToEngine;
use crate::values::host::TextGeneratorAvailability;

/// A text model on the host.
#[uniffi::export(foreign)]
#[async_trait]
pub trait TextGenerator: Send + Sync {
    /// Whether the model can be used now.
    async fn availability(&self) -> TextGeneratorAvailability;
    /// Answers `prompt` under `instructions`.
    async fn generate(&self, instructions: String, prompt: String) -> Result<String, HostError>;
}

/// Presents the host's model to the engine.
pub struct TextGeneratorAdapter {
    host: Arc<dyn TextGenerator>,
}

impl TextGeneratorAdapter {
    /// Wraps the host's model.
    pub fn new(host: Arc<dyn TextGenerator>) -> TextGeneratorAdapter {
        TextGeneratorAdapter { host }
    }
}

#[async_trait]
impl EngineTextGenerator for TextGeneratorAdapter {
    async fn availability(&self) -> EngineAvailability {
        self.host.availability().await.to_engine()
    }

    async fn generate(&self, instructions: &str, prompt: &str) -> Outcome<String> {
        self.host
            .generate(instructions.to_string(), prompt.to_string())
            .await
            .map_err(|error| failure_from_host_error(error, HostService::General))
    }
}
