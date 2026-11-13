//! The takeover rules of `TimeTugCore/Takeover`: who qualifies, what already fired, the last check before showing,
//! and which takeover to arm next. Pure: every function that needs the time takes `now`.

/// `TakeoverGuard`, the last check before a takeover is shown.
pub mod guard;
/// `TakeoverLedger`, which remembers which events already fired or are snoozed.
pub mod ledger;
/// `TakeoverPolicy`, which decides which events qualify for a takeover.
pub mod policy;
/// `TakeoverRequest`, the data a front end needs to render a takeover.
pub mod request;
/// `Scheduler`, which picks the next takeover to arm.
pub mod scheduler;
