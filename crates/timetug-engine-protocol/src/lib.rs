//! Rust port of the Swift product `TimeTugEngineProtocol`: the JSON contract v1 between the engine and its host.
//!
//! * [`envelope`]: `{"v":1,"type":...,"id":...,"payload":...}` and the protocol version.
//! * [`message_catalog`]: every message type of the contract, and the `ready` payload.
//! * [`config`]: `WireConfig::parse`, the `tt_engine_create` configuration.
//! * [`wire_types`]: `WireDates` and the event, state and settings DTOs.
//! * [`command_mapping`]: host command envelopes to engine commands.
//! * [`state_mapping`]: engine and Core types to the DTOs, and the host-bound messages.
//! * [`service_messages`]: the `service.*` requests and replies.
//! * [`compat`]: the superset relation of the backward-compatibility rule.
//!
//! `JSONValue` of the Swift module is not ported: a JSON document is `serde_json::Value`.
#![forbid(unsafe_code)]

pub mod command_mapping;
pub mod compat;
pub mod config;
pub mod envelope;
pub mod message_catalog;
pub mod service_messages;
pub mod state_mapping;
pub mod wire_types;

#[cfg(test)]
mod tests;
