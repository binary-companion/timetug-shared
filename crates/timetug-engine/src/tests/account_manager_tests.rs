//! Port of `Accounts/AccountManagerTests.swift` (32 tests), plus the Rust-only tests at the end.
//!
//! Every async test runs on a current-thread runtime with a paused clock, so `wait_until`'s 20 ms sleeps cost no
//! wall-clock time. Swift's `Issue.record("condition not reached")` is a panic.

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use calendar_core::connection::{
    AuthorizationMethod, Connection, ConnectorRegistry, CredentialField, CredentialStore,
    SyncStateStore,
};
use calendar_core::failure::Failure;
use calendar_core::file_connection_store::FileConnectionStore;
use calendar_core::source_types::SourceError;

use crate::accounts::account_manager::{AccountManager, Status, WaitingStyle};
use crate::settings::{EngineSettings, SettingsStorage};
use crate::state::{AccountKindInfo, Authorization};

use super::support::account_fakes::{
    AccountHarness, FakeConnectorKind, FakeOutcome, Gate, HarnessOptions,
};
use super::support::{LockedBox, TempDir};

async fn wait_until<F, Fut>(condition: F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    for _ in 0..250 {
        if condition().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condition not reached");
}

fn caldav_fields() -> Vec<CredentialField> {
    vec![
        CredentialField::new("username", "User name"),
        CredentialField::new("password", "Password").secret(),
    ]
}

fn ids(connections: &[Connection]) -> Vec<String> {
    connections
        .iter()
        .map(|c| c.connection_id.clone())
        .collect()
}

fn names(connections: &[Connection]) -> Vec<String> {
    connections.iter().map(|c| c.display_name.clone()).collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

fn caldav_kind(outcome: FakeOutcome) -> Arc<FakeConnectorKind> {
    FakeConnectorKind::with(
        "caldav",
        "Other CalDAV",
        AuthorizationMethod::Password {
            fields: caldav_fields(),
        },
        outcome,
    )
}

fn auth_expired() -> Failure {
    Failure::Source(SourceError::AuthExpired)
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn add_account_stores_the_connection_and_applies_sources() {
    let h = AccountHarness::new(vec![FakeConnectorKind::new()]);
    h.manager.add_account("google").await;
    let status = h.manager.status();
    assert_eq!(ids(&status.accounts), strings(&["1"]));
    assert_eq!(status.error_message, None);
    assert!(!status.is_working);
    let stored = FileConnectionStore::new(h.accounts_file())
        .connections()
        .await;
    assert_eq!(ids(&stored), strings(&["1"]));
    assert_eq!(
        h.applied.get().last(),
        Some(&strings(&["eventkit", "google-1"]))
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_duplicate_display_name_is_rejected_and_its_fresh_secrets_removed() {
    let kind = FakeConnectorKind::new();
    let h = AccountHarness::new(vec![kind.clone()]);
    h.manager.add_account("google").await;
    kind.set_next_connection_id("2");
    h.manager.add_account("google").await;
    let status = h.manager.status();
    assert_eq!(ids(&status.accounts), strings(&["1"]));
    assert_eq!(
        status.error_message.as_deref(),
        Some("a@x.test is already added.")
    );
    assert!(h.credentials.secrets("2").await.unwrap().is_none());
    assert!(h.credentials.secrets("1").await.unwrap().is_some());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_duplicate_that_reuses_the_stored_connection_id_keeps_the_stored_secrets() {
    let h = AccountHarness::new(vec![FakeConnectorKind::new()]);
    h.manager.add_account("google").await;
    h.manager.add_account("google").await;
    assert_eq!(ids(&h.manager.status().accounts), strings(&["1"]));
    assert!(h.credentials.secrets("1").await.unwrap().is_some());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn cancelled_sign_in_discards_unowned_secrets() {
    let kind = FakeConnectorKind::with_outcome(FakeOutcome::SucceedThenCancel {
        connection_id: "c1".to_string(),
    });
    let h = AccountHarness::new(vec![kind]);
    h.manager.add_account("google").await;
    assert!(h.credentials.secrets("c1").await.unwrap().is_none());
    let status = h.manager.status();
    assert!(status.accounts.is_empty());
    assert_eq!(status.error_message, None);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_failed_account_save_discards_the_new_sign_in() {
    let dir = TempDir::make();
    std::fs::write(dir.path().join("blocker"), b"x").unwrap();
    // The accounts file would have to live under a regular file, so every save fails.
    let h = AccountHarness::with(
        vec![FakeConnectorKind::new()],
        HarnessOptions {
            directory: Some(dir.path().join("blocker")),
            ..HarnessOptions::default()
        },
    );
    h.manager.add_account("google").await;
    let status = h.manager.status();
    assert!(status.error_message.is_some());
    assert!(status.accounts.is_empty());
    assert!(h.credentials.secrets("1").await.unwrap().is_none());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn removal_runs_in_the_documented_order() {
    let h = AccountHarness::new(vec![FakeConnectorKind::new()]);
    h.manager.add_account("google").await;
    let mut settings = EngineSettings::default();
    settings.takeover.takeover_calendar_keys = set(&["google-1/c", "eventkit/x"]);
    settings.takeover.hidden_calendar_keys = set(&["google-1/d"]);
    h.settings.save(&settings);
    h.sync_state.set_token(Some("t"), "1", "s").await;
    h.log.set(Vec::new());

    h.manager.remove_account("1").await;

    // Stored connection first (the reconcile already sees it gone), then source, selections, sync state, secrets.
    assert_eq!(
        h.log.get(),
        strings(&["reconcile", "selections", "sync", "secrets"])
    );
    assert_eq!(h.reconciler.stored_at_reconcile().last(), Some(&false));
    assert!(
        FileConnectionStore::new(h.accounts_file())
            .connections()
            .await
            .is_empty()
    );
    assert_eq!(
        h.settings.load().takeover.takeover_calendar_keys,
        set(&["eventkit/x"])
    );
    assert!(h.settings.load().takeover.hidden_calendar_keys.is_empty());
    assert!(h.sync_state.token("1", "s").await.is_none());
    assert!(h.credentials.secrets("1").await.unwrap().is_none());
    assert_eq!(h.applied.get().last(), Some(&strings(&["eventkit"])));
    assert_eq!(h.manager.status().error_message, None);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn removal_is_idempotent() {
    let h = AccountHarness::new(vec![FakeConnectorKind::new()]);
    h.manager.add_account("google").await;
    h.manager.remove_account("1").await;
    assert_eq!(h.manager.status().error_message, None);
    let applied = h.applied.get().len();
    // The second removal changes nothing; it names an account that no longer exists (review ruling).
    h.manager.remove_account("1").await;
    let status = h.manager.status();
    assert!(status.accounts.is_empty());
    assert_eq!(h.applied.get().len(), applied);
    assert_eq!(
        status.error_message.as_deref(),
        Some("That account no longer exists.")
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_secrets_failure_during_removal_reports_it_but_the_account_is_gone() {
    let h = AccountHarness::with(
        vec![FakeConnectorKind::new()],
        HarnessOptions {
            failing_removal: true,
            ..HarnessOptions::default()
        },
    );
    h.manager.add_account("google").await;
    h.manager.remove_account("1").await;
    let status = h.manager.status();
    assert!(status.accounts.is_empty());
    assert!(
        FileConnectionStore::new(h.accounts_file())
            .connections()
            .await
            .is_empty()
    );
    assert!(
        status
            .error_message
            .as_deref()
            .unwrap()
            .starts_with("The account was removed, but its saved sign-in could not be deleted")
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reauthorize_replaces_the_account_and_rebuilds_its_source() {
    let kind = FakeConnectorKind::new();
    let h = AccountHarness::new(vec![kind.clone()]);
    h.manager.add_account("google").await;
    let builds_before = h.reconciler.builds();
    h.manager.reauthorize("1").await;
    assert_eq!(kind.reauthorized(), 1);
    assert_eq!(h.reconciler.builds(), builds_before + 1);
    assert_eq!(
        h.applied.get().last(),
        Some(&strings(&["eventkit", "google-1"]))
    );
    assert_eq!(h.manager.status().error_message, None);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reauthorizing_as_a_different_account_surfaces_the_error_and_keeps_the_old_account() {
    let kind = FakeConnectorKind::new();
    let h = AccountHarness::new(vec![kind.clone()]);
    h.manager.add_account("google").await;
    let before = h.manager.status().accounts;
    kind.set_reauthorize_error(Failure::Source(SourceError::InvalidResponse(
        "signed in as someone else".to_string(),
    )));
    h.manager.reauthorize("1").await;
    let status = h.manager.status();
    assert_eq!(status.accounts, before);
    assert_eq!(
        status.error_message.as_deref(),
        Some("Sign-in failed: signed in as someone else.")
    );
    assert_eq!(
        FileConnectionStore::new(h.accounts_file())
            .connections()
            .await,
        before
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn cancelling_during_sign_in_again_changes_nothing() {
    let kind = FakeConnectorKind::new();
    let h = AccountHarness::new(vec![kind.clone()]);
    h.manager.add_account("google").await;
    let before = h.manager.status().accounts;
    let builds_before = h.reconciler.builds();
    kind.set_reauthorize_suspends(true);
    h.manager.begin_reauthorize("1");
    wait_until(|| async { h.manager.status().is_working }).await;
    h.manager.cancel_authorization();
    wait_until(|| async { !h.manager.status().is_working }).await;
    let status = h.manager.status();
    assert_eq!(status.accounts, before);
    assert_eq!(status.error_message, None);
    assert_eq!(h.reconciler.builds(), builds_before);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn an_unreadable_accounts_file_loads_none_shows_the_message_and_is_never_written() {
    let dir = TempDir::make();
    let bytes = b"not json".to_vec();
    std::fs::write(dir.path().join("accounts.json"), &bytes).unwrap();
    let mut initial = EngineSettings::default();
    initial.takeover.takeover_calendar_keys = set(&["google-1/c", "google-9/c", "eventkit/x"]);
    let h = AccountHarness::with(
        vec![FakeConnectorKind::new()],
        HarnessOptions {
            initial,
            directory: Some(dir.path().to_path_buf()),
            ..HarnessOptions::default()
        },
    );
    h.manager.start().await;
    let status = h.manager.status();
    assert!(status.accounts.is_empty());
    assert_eq!(
        status.error_message.as_deref(),
        Some("Your saved accounts could not be read, so none were loaded. Nothing was deleted.")
    );
    assert_eq!(std::fs::read(h.accounts_file()).unwrap(), bytes);
    assert_eq!(
        h.settings.load().takeover.takeover_calendar_keys,
        set(&["google-1/c", "google-9/c", "eventkit/x"])
    );
    // A later add must not overwrite the unreadable file either.
    h.manager.add_account("google").await;
    assert_eq!(std::fs::read(h.accounts_file()).unwrap(), bytes);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn start_sweeps_orphaned_selections_for_known_kind_prefixes_only() {
    let dir = TempDir::make();
    FileConnectionStore::new(dir.path().join("accounts.json"))
        .save(&[Connection::new("google", "1", "a@x.test")])
        .await
        .unwrap();
    let mut initial = EngineSettings::default();
    initial.takeover.takeover_calendar_keys =
        set(&["google-1/c", "google-9/c", "eventkit/x", "mystery-3/c"]);
    let h = AccountHarness::with(
        vec![FakeConnectorKind::new()],
        HarnessOptions {
            initial,
            directory: Some(dir.path().to_path_buf()),
            ..HarnessOptions::default()
        },
    );
    h.manager.start().await;
    assert_eq!(
        h.settings.load().takeover.takeover_calendar_keys,
        set(&["google-1/c", "eventkit/x", "mystery-3/c"])
    );
    assert_eq!(ids(&h.manager.status().accounts), strings(&["1"]));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn system_calendar_toggle_off_keeps_stored_selections() {
    let mut initial = EngineSettings::default();
    initial.takeover.takeover_calendar_keys = set(&["eventkit/x"]);
    let h = AccountHarness::with(
        vec![FakeConnectorKind::new()],
        HarnessOptions {
            initial,
            ..HarnessOptions::default()
        },
    );
    h.manager.start().await;
    assert_eq!(h.applied.get().last(), Some(&strings(&["eventkit"])));

    h.manager.set_system_calendar_enabled(false).await;
    assert_eq!(h.applied.get().last(), Some(&Vec::<String>::new()));
    assert!(!h.settings.load().system_calendar_enabled);
    assert!(
        h.settings
            .load()
            .takeover
            .takeover_calendar_keys
            .contains("eventkit/x")
    );

    h.manager.set_system_calendar_enabled(true).await;
    assert_eq!(h.applied.get().last(), Some(&strings(&["eventkit"])));
    assert!(
        h.settings
            .load()
            .takeover
            .takeover_calendar_keys
            .contains("eventkit/x")
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_rejected_password_reopens_the_form_with_the_error_and_what_was_typed() {
    let kind = caldav_kind(FakeOutcome::Password {
        good: "right".to_string(),
    });
    let h = AccountHarness::with(
        vec![kind],
        HarnessOptions {
            passwords: strings(&["wrong", "right"]),
            ..HarnessOptions::default()
        },
    );
    h.manager.add_account("caldav").await;
    let armed = h.sign_in.prepared();
    assert_eq!(armed.len(), 2);
    assert_eq!(armed[0].title, "Other CalDAV account");
    assert_eq!(armed[0].error, None);
    assert_eq!(
        armed[1].error.as_deref(),
        Some("User name or password was not accepted.")
    );
    assert_eq!(
        armed[1].values,
        [("username".to_string(), "me".to_string())].into()
    );
    let status = h.manager.status();
    assert_eq!(names(&status.accounts), strings(&["me"]));
    assert_eq!(status.error_message, None);
    assert_eq!(status.waiting_style, WaitingStyle::InApp);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn cancelling_the_password_retry_loop_adds_nothing_and_shows_no_error() {
    let kind = caldav_kind(FakeOutcome::Password {
        good: "right".to_string(),
    });
    // One wrong attempt, then the form is cancelled (the fake form throws a cancellation when it has no answer).
    let h = AccountHarness::with(
        vec![kind],
        HarnessOptions {
            passwords: strings(&["wrong"]),
            ..HarnessOptions::default()
        },
    );
    h.manager.add_account("caldav").await;
    let status = h.manager.status();
    assert!(status.accounts.is_empty());
    assert_eq!(status.error_message, None);
    assert_eq!(h.sign_in.prepared().len(), 2);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn browser_sign_ins_wait_for_the_browser() {
    let h = AccountHarness::new(vec![FakeConnectorKind::new()]);
    h.manager.add_account("google").await;
    assert_eq!(h.manager.status().waiting_style, WaitingStyle::Browser);
}

#[test]
fn describe_sign_in_names_the_kinds_own_fields() {
    let icloud = vec![
        CredentialField::new("username", "Apple ID"),
        CredentialField::new("password", "App-specific password").secret(),
    ];
    assert_eq!(
        AccountManager::describe_sign_in(&auth_expired(), &icloud),
        "Apple ID or app-specific password was not accepted."
    );
    assert_eq!(
        AccountManager::describe_sign_in(
            &Failure::Source(SourceError::Server { status: 500 }),
            &icloud
        ),
        "Sign-in failed (server(status: 500))."
    );
}

#[test]
fn a_rejected_link_is_described_for_a_kind_with_only_a_secret_field() {
    let link = vec![CredentialField::new("link", "iCal link").secret()];
    assert_eq!(
        AccountManager::describe_sign_in(&auth_expired(), &link),
        "iCal link was not accepted."
    );
    assert_eq!(
        AccountManager::describe_sign_in(
            &Failure::Source(SourceError::InvalidResponse(
                "that link did not return a calendar".to_string()
            )),
            &link
        ),
        "Sign-in failed: that link did not return a calendar."
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn other_cal_dav_is_offered_last() {
    let caldav = FakeConnectorKind::with(
        "caldav",
        "Other CalDAV",
        AuthorizationMethod::Password {
            fields: caldav_fields(),
        },
        FakeOutcome::Succeed,
    );
    let apple = FakeConnectorKind::with(
        "apple",
        "Apple Calendar",
        AuthorizationMethod::System,
        FakeOutcome::Succeed,
    );
    let h = AccountHarness::new(vec![caldav, FakeConnectorKind::new(), apple]);
    assert_eq!(
        h.manager.available_kind_ids(),
        strings(&["google", "caldav"])
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn an_add_during_removal_cannot_be_wiped_by_the_removal_tail() {
    let gate = Arc::new(Gate::new());
    let h = AccountHarness::with(
        vec![FakeConnectorKind::new()],
        HarnessOptions {
            sync_gate: Some(gate.clone()),
            ..HarnessOptions::default()
        },
    );
    h.manager.add_account("google").await;
    let manager = h.manager.clone();
    let removal = tokio::spawn(async move { manager.remove_account("1").await });
    wait_until(|| gate.is_waiting()).await;
    // Removal is suspended mid-way; the same connection id signs in again.
    h.manager.add_account("google").await;
    assert!(h.manager.status().accounts.is_empty());
    assert_eq!(
        h.manager.status().error_message.as_deref(),
        Some("That account is being removed.")
    );
    gate.open().await;
    removal.await.unwrap();
    assert!(h.manager.status().accounts.is_empty());
    assert!(
        FileConnectionStore::new(h.accounts_file())
            .connections()
            .await
            .is_empty()
    );
    assert!(h.credentials.secrets("1").await.unwrap().is_none());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reauthorizing_an_account_being_removed_is_refused() {
    let gate = Arc::new(Gate::new());
    let kind = FakeConnectorKind::new();
    let h = AccountHarness::with(
        vec![kind.clone()],
        HarnessOptions {
            sync_gate: Some(gate.clone()),
            ..HarnessOptions::default()
        },
    );
    h.manager.add_account("google").await;
    let manager = h.manager.clone();
    let removal = tokio::spawn(async move { manager.remove_account("1").await });
    wait_until(|| gate.is_waiting()).await;
    h.manager.reauthorize("1").await;
    assert_eq!(kind.reauthorized(), 0);
    gate.open().await;
    removal.await.unwrap();
    assert!(h.manager.status().accounts.is_empty());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_failed_save_during_reauthorize_leaves_memory_on_the_old_account() {
    let kind = FakeConnectorKind::new();
    let h = AccountHarness::new(vec![kind.clone()]);
    h.manager.add_account("google").await;
    let before = h.manager.status().accounts;
    // A directory where the file belongs makes the file unreadable, so the store refuses to write.
    std::fs::remove_file(h.accounts_file()).unwrap();
    std::fs::create_dir_all(h.accounts_file()).unwrap();
    kind.set_reauthorize_result(Connection::new("google", "1", "renamed"));
    h.manager.reauthorize("1").await;
    let status = h.manager.status();
    assert_eq!(status.accounts, before);
    assert!(status.error_message.is_some());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_different_connection_id_from_reauthorize_leaves_accounts_unchanged() {
    let kind = FakeConnectorKind::new();
    let h = AccountHarness::new(vec![kind.clone()]);
    h.manager.add_account("google").await;
    let before = h.manager.status().accounts;
    kind.set_reauthorize_result(Connection::new("google", "other", "z@x.test"));
    h.manager.reauthorize("1").await;
    let status = h.manager.status();
    assert_eq!(status.accounts, before);
    assert_eq!(
        status.error_message.as_deref(),
        Some("Sign-in failed: signed in as a different account.")
    );
    assert_eq!(
        FileConnectionStore::new(h.accounts_file())
            .connections()
            .await,
        before
    );
    assert!(h.credentials.secrets("1").await.unwrap().is_some());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_reauthorization_cancelled_as_it_completes_persists_nothing() {
    let kind = FakeConnectorKind::new();
    let h = AccountHarness::new(vec![kind.clone()]);
    h.manager.add_account("google").await;
    let builds_before = h.reconciler.builds();
    kind.set_reauthorize_cancels_and_renames("renamed");
    h.manager.reauthorize("1").await;
    let status = h.manager.status();
    assert_eq!(names(&status.accounts), strings(&["a@x.test"]));
    assert_eq!(
        names(
            &FileConnectionStore::new(h.accounts_file())
                .connections()
                .await
        ),
        strings(&["a@x.test"])
    );
    assert_eq!(h.reconciler.builds(), builds_before);
}

// MARK: - Identity families (one mailbox through two Microsoft kinds is one account)

/// A browser kind with no family (its own kind id) and a brokered kind whose family is "microsoft".
fn microsoft_kinds() -> (Arc<FakeConnectorKind>, Arc<FakeConnectorKind>) {
    let browser = FakeConnectorKind::family("microsoft", "Microsoft", "microsoft");
    let brokered = FakeConnectorKind::family("microsoft-brokered", "Microsoft", "microsoft");
    (browser, brokered)
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn brokered_microsoft_is_refused_when_browser_microsoft_has_the_same_email() {
    let (browser, brokered) = microsoft_kinds();
    let h = AccountHarness::new(vec![browser, brokered.clone()]);
    h.manager.add_account("microsoft").await;
    brokered.set_next_connection_id("2");
    h.manager.add_account("microsoft-brokered").await;
    let status = h.manager.status();
    assert_eq!(ids(&status.accounts), strings(&["1"]));
    assert_eq!(
        status.error_message.as_deref(),
        Some("a@x.test is already added.")
    );
    assert_eq!(
        h.applied.get().last(),
        Some(&strings(&["eventkit", "microsoft-1"]))
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn browser_microsoft_is_refused_when_brokered_has_the_same_email() {
    let (browser, brokered) = microsoft_kinds();
    let h = AccountHarness::new(vec![browser.clone(), brokered]);
    h.manager.add_account("microsoft-brokered").await;
    browser.set_next_connection_id("2");
    h.manager.add_account("microsoft").await;
    let status = h.manager.status();
    let kinds: Vec<String> = status.accounts.iter().map(|c| c.kind_id.clone()).collect();
    assert_eq!(kinds, strings(&["microsoft-brokered"]));
    assert_eq!(
        status.error_message.as_deref(),
        Some("a@x.test is already added.")
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn same_email_in_different_case_is_refused() {
    let (browser, brokered) = microsoft_kinds();
    let h = AccountHarness::new(vec![browser.clone(), brokered.clone()]);
    browser.set_next_email("Me@Example.com");
    h.manager.add_account("microsoft").await;
    brokered.set_next_email("me@example.com");
    brokered.set_next_connection_id("2");
    h.manager.add_account("microsoft-brokered").await;
    let status = h.manager.status();
    assert_eq!(ids(&status.accounts), strings(&["1"]));
    assert_eq!(
        status.error_message.as_deref(),
        Some("me@example.com is already added.")
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn same_email_on_different_families_is_allowed() {
    let google = FakeConnectorKind::new();
    let (_, brokered) = microsoft_kinds();
    let h = AccountHarness::new(vec![google, brokered.clone()]);
    h.manager.add_account("google").await;
    brokered.set_next_connection_id("2");
    h.manager.add_account("microsoft-brokered").await;
    let status = h.manager.status();
    assert_eq!(ids(&status.accounts), strings(&["1", "2"]));
    assert_eq!(status.error_message, None);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn unknown_kind_of_an_existing_account_falls_back_to_its_kind_id() {
    // A stored account whose kind this build no longer registers is its own family: it does not match a
    // registered kind's account even with the same name, and a second account of the same unknown kind would.
    let dir = TempDir::make();
    FileConnectionStore::new(dir.path().join("accounts.json"))
        .save(&[Connection::new("retired", "1", "a@x.test")])
        .await
        .unwrap();
    let (browser, brokered) = microsoft_kinds();
    brokered.set_next_connection_id("2");
    let h = AccountHarness::with(
        vec![browser, brokered],
        HarnessOptions {
            directory: Some(dir.path().to_path_buf()),
            ..HarnessOptions::default()
        },
    );
    h.manager.start().await;
    h.manager.add_account("microsoft-brokered").await;
    let status = h.manager.status();
    assert_eq!(ids(&status.accounts), strings(&["1", "2"]));
    assert_eq!(status.error_message, None);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn refused_brokered_add_leaves_stored_accounts_and_credentials_untouched() {
    let (browser, brokered) = microsoft_kinds();
    let h = AccountHarness::new(vec![browser, brokered.clone()]);
    h.manager.add_account("microsoft").await;
    let before = std::fs::read(h.accounts_file()).unwrap();
    brokered.set_next_connection_id("2");
    h.manager.add_account("microsoft-brokered").await;
    assert_eq!(std::fs::read(h.accounts_file()).unwrap(), before);
    assert!(h.credentials.secrets("1").await.unwrap().is_some());
    assert!(
        h.credentials.secrets("2").await.unwrap().is_none(),
        "the refused sign-in's fresh secrets are discarded"
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn same_kind_duplicate_rule_unchanged() {
    // A kind without the family protocol is its own family, so the old rule (same kind, same name) holds.
    let kind = FakeConnectorKind::new();
    let h = AccountHarness::new(vec![kind.clone()]);
    h.manager.add_account("google").await;
    kind.set_next_connection_id("2");
    h.manager.add_account("google").await;
    assert_eq!(
        h.manager.status().error_message.as_deref(),
        Some("a@x.test is already added.")
    );
    kind.set_next_email("b@x.test");
    h.manager.add_account("google").await;
    assert_eq!(
        names(&h.manager.status().accounts),
        strings(&["a@x.test", "b@x.test"])
    );
}

// MARK: - Rust-only tests

/// Conventions section 10, item 11: cancelling an authorization ends a pending `authorize` future by dropping it, so
/// the guards it holds (the OAuth redirect session) run.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn cancelling_an_authorization_drops_the_pending_authorize_future() {
    let dropped = Arc::new(AtomicBool::new(false));
    let kind = FakeConnectorKind::with_outcome(FakeOutcome::HangHoldingGuard {
        dropped: dropped.clone(),
    });
    let h = AccountHarness::new(vec![kind]);
    h.manager.begin_add_account("google");
    wait_until(|| async { h.manager.status().is_working }).await;
    assert!(!dropped.load(Ordering::SeqCst));
    h.manager.cancel_authorization();
    wait_until(|| async { !h.manager.status().is_working }).await;
    assert!(dropped.load(Ordering::SeqCst));
    let status = h.manager.status();
    assert!(status.accounts.is_empty());
    assert_eq!(status.error_message, None);
}

/// Swift's `describeSignIn` lowercases `secret.label.prefix(1)`, a Character; Rust takes the first `char`. Both give the
/// same text for a combining sequence and for an emoji sequence, because nothing after the first scalar changes case.
#[test]
fn describe_sign_in_lowercases_only_the_first_character_of_the_secret_label() {
    for (label, expected) in [
        ("E\u{0301}tiquette", "e\u{0301}tiquette"),
        (
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} key",
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} key",
        ),
        ("PIN Code", "pIN Code"),
    ] {
        let fields = vec![
            CredentialField::new("username", "User"),
            CredentialField::new("password", label).secret(),
        ];
        assert_eq!(
            AccountManager::describe_sign_in(&auth_expired(), &fields),
            format!("User or {expected} was not accepted."),
            "label {label:?}"
        );
    }
}

/// The text Swift's `"\(error)"` gives for each `SourceError` case (the sentences the user sees in "Sign-in failed (...)").
#[test]
fn sign_in_failures_use_swifts_case_descriptions() {
    let link = vec![CredentialField::new("link", "iCal link").secret()];
    for (error, expected) in [
        (
            SourceError::Network("offline".to_string()),
            "network(\"offline\")",
        ),
        (
            SourceError::RateLimited { retry_after: None },
            "rateLimited(retryAfter: nil)",
        ),
        (
            SourceError::RateLimited {
                retry_after: Some(3.0),
            },
            "rateLimited(retryAfter: Optional(3.0))",
        ),
        (SourceError::Server { status: 503 }, "server(status: 503)"),
        (SourceError::NeedsPermission, "needsPermission"),
        (SourceError::NotFound, "notFound"),
    ] {
        assert_eq!(
            AccountManager::describe_sign_in(&Failure::Source(error), &link),
            format!("Sign-in failed ({expected})."),
            "{expected}"
        );
    }
    assert_eq!(
        AccountManager::describe_sign_in(&auth_expired(), &[]),
        "Sign-in was not completed."
    );
}

/// The fixed sentences that stand in for Foundation's `localizedDescription` (listed in the pull request).
#[test]
fn errors_without_a_swift_description_use_fixed_sentences() {
    let link = vec![CredentialField::new("link", "iCal link").secret()];
    assert_eq!(
        AccountManager::describe_sign_in(&Failure::Host("keychain locked".to_string()), &link),
        "keychain locked"
    );
    assert_eq!(
        AccountManager::describe_sign_in(&Failure::Cancelled, &link),
        "The operation couldn\u{2019}t be completed."
    );
}

#[test]
fn kind_infos_lists_unconfigured_kinds_sorted_with_caldav_last() {
    let google = FakeConnectorKind::new();
    let caldav = caldav_kind(FakeOutcome::Succeed);
    let apple = FakeConnectorKind::with(
        "apple",
        "Apple Calendar",
        AuthorizationMethod::System,
        FakeOutcome::Succeed,
    );
    let mut registry = ConnectorRegistry::new();
    registry.register(google);
    registry.register(caldav);
    registry.register(apple);
    let infos = AccountManager::kind_infos(
        &registry,
        &strings(&["google", "zoom", "apple", "microsoft"]),
    );
    assert_eq!(
        infos,
        vec![
            AccountKindInfo::new("google", "Google", Authorization::Oauth, false),
            AccountKindInfo::new("microsoft", "Microsoft", Authorization::Oauth, false),
            AccountKindInfo::new("zoom", "Zoom", Authorization::Oauth, false),
            AccountKindInfo::new("caldav", "Other CalDAV", Authorization::Password, true),
        ]
    );
}

/// Swift's `current { didSet }`: every change of the status is reported once, in order, and an assignment that changes
/// nothing (`errorMessage = nil` while it is nil) is not reported.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn every_status_change_is_reported_once_in_order() {
    let seen: Arc<LockedBox<Vec<Status>>> = Arc::new(LockedBox::new(Vec::new()));
    let observed = seen.clone();
    let h = AccountHarness::with(
        vec![FakeConnectorKind::new()],
        HarnessOptions {
            on_status_change: Some(Arc::new(move |status| {
                observed.mutate(|all| all.push(status));
            })),
            ..HarnessOptions::default()
        },
    );
    h.manager.add_account("google").await;
    let seen = seen.get();
    let shape: Vec<(bool, usize)> = seen
        .iter()
        .map(|s| (s.is_working, s.accounts.len()))
        .collect();
    assert_eq!(shape, vec![(true, 0), (true, 1), (false, 1)]);
    assert_eq!(seen.last(), Some(&h.manager.status()));
}
