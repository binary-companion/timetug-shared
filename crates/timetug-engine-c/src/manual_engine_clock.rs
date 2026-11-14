//! Port of `ManualEngineClock.swift`: the clock of a `testing: true` engine. The implementation moved to
//! `timetug_engine::manual_clock` so the bindings' test clock is the same code; this module keeps the old path.

pub use timetug_engine::manual_clock::ManualEngineClock;
