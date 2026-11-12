//! `Engine.swift` lines 295-355 (Accounts), 625-656 (Settings) and 706-729 (Stop).

use crate::accounts::account_manager::{SettingsChange, Status};
use crate::diagnostic::{EngineDiagnostic, Level};
use crate::settings::EngineSettings;

use super::{Engine, EngineInner, SignIn, TaskSlot};

impl Engine {
    // MARK: - Accounts

    /// Runs one sign-in in an engine-owned task so `cancelAuthorization` can cancel it; returns when it ends.
    pub(super) async fn run_authorization(&self, sign_in: SignIn) {
        let task = {
            let mut inner = self.lock();
            if inner.auth_task.is_some() {
                self.diagnose(
                    &mut inner,
                    Level::Warning,
                    "accounts",
                    "a sign-in is already running; ignored",
                );
                return;
            }
            let manager = self.account_manager.clone();
            let slot = TaskSlot::spawn(async move {
                match sign_in {
                    SignIn::Add(kind_id) => manager.add_account(&kind_id).await,
                    SignIn::Reauthorize(connection_id) => manager.reauthorize(&connection_id).await,
                }
            });
            let finished = slot.finished();
            inner.auth_task = Some(slot);
            finished
        };
        // await point (Engine.swift:306)
        task.wait().await;
        self.lock().auth_task = None;
        // await point (Engine.swift:308)
        self.accounts_changed().await;
    }

    /// An account manager diagnostic: to the host sink and, as `.diagnostic`, to `events`.
    pub(super) fn forward(&self, diagnostic: &EngineDiagnostic) {
        (self.host.diagnostics)(diagnostic.clone());
        let mut inner = self.lock();
        self.emit(
            &mut inner,
            crate::event::EngineEvent::Diagnostic(diagnostic.clone()),
        );
    }

    /// Sign-in progress and every other account status change: a new revision over the same snapshot.
    pub(super) fn account_status_changed(&self, status: Status) {
        let mut inner = self.lock();
        if status == inner.account_status {
            return;
        }
        inner.account_status = status;
        self.publish(&mut inner, false);
    }

    /// Re-reads the account manager's status and publishes it.
    pub(super) async fn accounts_changed(&self) {
        // await point (Engine.swift:333): `AccountManager::status` is synchronous in Rust.
        let status = self.account_manager.status();
        let mut inner = self.lock();
        inner.account_status = status;
        self.publish(&mut inner, true);
    }

    /// The settings the account manager sees: the system calendar switch reads as off when the host has no system source.
    pub(super) fn effective_account_settings(&self) -> EngineSettings {
        let mut effective = self.lock().settings.clone();
        if self.host.build_system_source.is_none() {
            effective.system_calendar_enabled = false;
        }
        effective
    }

    /// Changes, persists and publishes the behaviour settings (used by `applySettings` and the account manager).
    pub(super) fn update_settings(&self, change: SettingsChange) {
        let mut inner = self.lock();
        self.update_settings_locked(&mut inner, change);
    }

    fn update_settings_locked(&self, inner: &mut EngineInner, change: SettingsChange) {
        let mut updated = inner.settings.clone();
        change(&mut updated);
        if updated == inner.settings {
            return;
        }
        inner.settings = updated.clone();
        self.host.settings.save(&updated);
        self.rearm(inner);
        self.publish(inner, true);
        self.emit_widget_snapshot(inner);
    }

    // MARK: - Settings

    /// Persists the change (through `update_settings`), then turns inference and the system calendar on or off.
    pub(super) async fn apply_settings(&self, change: SettingsChange) {
        let old = {
            let mut inner = self.lock();
            let old = inner.settings.clone();
            self.update_settings_locked(&mut inner, change);
            old
        };
        // await point (Engine.swift:631)
        self.settings_did_change(old).await;
    }

    /// The host's storage changed behind the engine (another process, a widget): re-read it, never write it back.
    pub(super) async fn settings_changed_externally(&self) {
        let old = {
            let mut inner = self.lock();
            let loaded = self.host.settings.load();
            if loaded == inner.settings {
                return;
            }
            let old = inner.settings.clone();
            inner.settings = loaded;
            self.rearm(&mut inner);
            self.publish(&mut inner, true);
            self.emit_widget_snapshot(&mut inner);
            old
        };
        // await point (Engine.swift:643)
        self.settings_did_change(old).await;
    }

    async fn settings_did_change(&self, old: EngineSettings) {
        let inference = {
            let inner = self.lock();
            (inner.settings.inference_enabled != old.inference_enabled)
                .then_some(inner.settings.inference_enabled)
        };
        if let Some(enabled) = inference {
            // await point (Engine.swift:647)
            self.set_inference(enabled).await;
        }
        // Without a host system source the account manager always sees the switch off; nothing to rebuild.
        // The setting is read again here, after the await above, as Swift reads `settings` again.
        let system_changed =
            self.lock().settings.system_calendar_enabled != old.system_calendar_enabled;
        if system_changed && self.host.build_system_source.is_some() {
            // The value is already stored; the account manager re-reads the engine's settings when it rebuilds, so an
            // external change that lands meanwhile wins and nothing stale is written back.
            // await point (Engine.swift:652)
            self.account_manager.system_calendar_setting_changed().await;
            // await point (Engine.swift:653)
            self.accounts_changed().await;
        }
    }

    // MARK: - Stop

    /// Cancels background work, stops the sources and ends every stream.
    pub(super) async fn stop(&self) {
        {
            let mut inner = self.lock();
            inner.stopped = true;
            self.activity.mark_stopped();
            for task in [
                inner.periodic_task.take(),
                inner.fire_task.take(),
                inner.resolve_task.take(),
                inner.tick_task.take(),
            ]
            .into_iter()
            .flatten()
            {
                task.cancel();
            }
            if let Some(task) = &inner.auth_task {
                task.cancel();
            }
            // `statusTask.cancel(); statusContinuation.finish()`: later posts end their own count.
            self.status_poster.finish();
            self.status_task.abort();
        }
        // await point (Engine.swift:723)
        self.account_manager.stop().await;
        let mut inner = self.lock();
        // Dropping the senders ends every `states()` and `events()` receiver (after it has read what is queued).
        inner.state_tx = None;
        inner.event_senders.clear();
    }
}
