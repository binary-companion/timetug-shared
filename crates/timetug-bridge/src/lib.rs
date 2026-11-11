//! Rust port of the Swift product `CalendarBridge`: adapts the connector library's sources and events to
//! `timetug-core`'s own model, so `CalendarStore` never sees library types.
//!
//! Like `timetug-core` this crate never reads the clock or the environment (`scripts/ci/check-no-clock.py`). The one
//! runtime facility it uses is `tokio::spawn`, for the task that forwards a source's changes; it needs no timer.
#![forbid(unsafe_code)]

pub mod connected_source;
pub mod event_mapper;

pub use connected_source::ConnectedSource;
pub use event_mapper::EventMapper;

#[cfg(test)]
mod tests;
