//! The UniFFI bindings crate for Swift and Kotlin hosts: the typed front door of the engine.
pub mod custom_types;
pub mod errors;

uniffi::setup_scaffolding!();

#[cfg(test)]
mod tests;
