//! Rust port of the Swift product `TimeTugEngine`: the portable engine.
//!
//! This crate is the only one that may touch `tokio::time`, and only in [`clock::SystemClock`]; everything else sleeps
//! through [`clock::EngineClock`] (`scripts/ci/check-no-clock.py`). It owns no runtime and no thread: the caller (the C
//! interface, the bindings) provides the `tokio` runtime the engine's tasks run on.
#![forbid(unsafe_code)]

pub mod accounts;
pub mod activity;
pub mod clock;
pub mod command;
pub mod connector_setup;
pub mod diagnostic;
pub mod engine;
pub mod event;
pub mod host;
pub mod inference;
pub mod persistence;
pub mod scheduling;
pub mod settings;
pub mod state;

pub use engine::Engine;

#[cfg(test)]
mod tests;
