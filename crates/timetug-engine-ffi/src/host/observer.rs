//! `EngineObserver`: how state and events reach the host.
use crate::values::events::EngineEvent;
use crate::values::summary::EngineStateSummary;

/// The single observer a host passes when it creates the engine. The engine calls it from its delivery thread, one call at
/// a time and in order; states are coalesced (a slow observer sees the newest state, not every one), events are never
/// dropped; nothing is delivered after `shutdown()` returned. Calls must return quickly.
#[uniffi::export(foreign)]
pub trait EngineObserver: Send + Sync {
    /// A new state, as a summary: the parts that are large (events, agenda, candidates) are fetched from the engine when
    /// their revision in the summary changed.
    fn state_changed(&self, summary: EngineStateSummary);
    /// A one-shot event (a takeover is due, a widget snapshot, a join request, a diagnostic).
    fn event(&self, event: EngineEvent);
}
