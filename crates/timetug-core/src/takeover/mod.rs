//! The takeover rules of `TimeTugCore/Takeover`: who qualifies, what already fired, the last check before showing,
//! and which takeover to arm next. Pure: every function that needs the time takes `now`.

pub mod guard;
pub mod ledger;
pub mod policy;
pub mod request;
pub mod scheduler;
