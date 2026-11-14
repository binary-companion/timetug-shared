//! The host interfaces: what the Swift or Kotlin app implements, as foreign UniFFI traits, and the adapters that present
//! each one to the engine as the engine's own trait. Every adapter converts values with the mirrors and converts what a
//! host callback throws with `failure_from_host_error`, as `RemoteHostServices` does for the C interface.
//!
//! Threading: every method may be called from the engine thread, concurrently with the others. Implementors must be
//! thread-safe, must not block for long, and hop to their own actor or queue inside a method when they touch main-actor
//! or EventKit state. The synchronous methods (`SettingsStorage`, the observer, the diagnostic sink, the redirect URI)
//! run on the engine or delivery thread and must return quickly.

pub mod authorization;
pub mod credentials;
pub mod diagnostics;
pub mod external_tokens;
pub mod http;
pub mod observer;
pub mod settings_storage;
pub mod sign_in;
pub mod system_calendar;
pub mod text_generator;
