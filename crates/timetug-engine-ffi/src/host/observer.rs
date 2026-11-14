//! `EngineObserver`: how state and events reach the host.
use crate::values::events::EngineEvent;
use crate::values::state::EngineState;

/// The single observer a host passes when it creates the engine. The engine calls it from its delivery thread, one call at
/// a time and in order; states are coalesced (a slow observer sees the newest state, not every one), events are never
/// dropped; nothing is delivered after `shutdown()` returned. Calls must return quickly.
#[uniffi::export(foreign)]
pub trait EngineObserver: Send + Sync {
    /// A new state. Sent only to hand over the newest one.
    fn state_changed(&self, state: EngineState);
    /// A one-shot event (a takeover is due, a widget snapshot, a join request, a diagnostic).
    fn event(&self, event: EngineEvent);
}
