//! The UniFFI bindings crate for Swift and Kotlin hosts: the typed front door of the engine.
pub mod config;
pub mod custom_types;
pub mod engine;
pub mod errors;
pub mod functions;
pub mod host;
pub mod runtime;
pub mod values;

uniffi::setup_scaffolding!();

#[cfg(test)]
mod tests;
