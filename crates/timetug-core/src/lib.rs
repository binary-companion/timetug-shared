//! Rust port of the Swift product `TimeTugCore`: the platform-neutral model, rules and stores of TimeTug.
//!
//! This crate is pure: it never reads the clock, the environment or the file system, and it has no runtime of its own.
//! Every function that needs the time takes `now: Timestamp` (enforced by `scripts/ci/check-no-clock.py`).
#![forbid(unsafe_code)]

pub mod model;
pub mod sources;
pub(crate) mod text;

pub use model::calendar_info::CalendarInfo;
pub use model::event::TimeTugCalendarEvent;
pub use model::merge_types::{MergeProvenance, MergedMember};
pub use model::takeover_settings::TakeoverSettings;
pub use sources::{CalendarSource, SourceChange, SourceError, SourceStatus};

#[cfg(test)]
mod tests;
