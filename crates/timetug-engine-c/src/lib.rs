//! Rust port of the Swift product `TimeTugEngineC`: the `tt_engine_*` C interface.
//! The library keeps the file names the C# wrapper loads (`TimeTugEngineC.dll`, `libTimeTugEngineC.dylib`,
//! `libTimeTugEngineC.so`), so the crate name is not snake case.
//!
//! `unsafe` appears in exactly two files: `c_interface.rs` (the four exports and their pointer handling) and
//! `delivery.rs` (the call of the host's callback). Everything else is safe Rust (`deny(unsafe_code)` below), and each
//! `unsafe` block says why it is sound in a `// SAFETY:` comment.
#![allow(non_snake_case)]
#![deny(unsafe_code)]

pub mod delivery;
pub mod handle_registry;
pub mod manual_engine_clock;
pub mod remote_host_services;
pub mod request_table;

#[cfg(test)]
mod tests;
