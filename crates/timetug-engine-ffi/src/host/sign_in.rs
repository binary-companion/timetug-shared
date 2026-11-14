//! `SignInPrompting`: the host's sign-in form for password kinds.
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use calendar_core::connection::CredentialHelp as EngineHelp;
use timetug_engine::host::SignInPrompting as EngineSignInPrompting;

use crate::values::accounts::CredentialHelp;
use crate::values::{ToEngine, ToMirror};

/// Arms the form for the next prompt and reports what was typed.
#[uniffi::export(foreign)]
#[async_trait]
pub trait SignInPrompting: Send + Sync {
    /// Arms the form with its title, optional help, prefilled values and the previous attempt's error.
    async fn prepare(
        &self,
        title: String,
        help: Option<CredentialHelp>,
        values: HashMap<String, String>,
        error: Option<String>,
    );
    /// The values typed in the last submitted form, without secrets; `None` when sign-in failed before a form was submitted.
    async fn last_non_secret_values(&self) -> Option<HashMap<String, String>>;
}

/// Presents the host's form to the engine.
pub struct SignInPromptingAdapter {
    host: Arc<dyn SignInPrompting>,
}

impl SignInPromptingAdapter {
    /// Wraps the host's form.
    pub fn new(host: Arc<dyn SignInPrompting>) -> SignInPromptingAdapter {
        SignInPromptingAdapter { host }
    }
}

#[async_trait]
impl EngineSignInPrompting for SignInPromptingAdapter {
    async fn prepare(
        &self,
        title: String,
        help: Option<EngineHelp>,
        values: BTreeMap<String, String>,
        error: Option<String>,
    ) {
        self.host
            .prepare(title, help.to_mirror(), values.to_mirror(), error)
            .await;
    }

    async fn last_non_secret_values(&self) -> Option<BTreeMap<String, String>> {
        self.host
            .last_non_secret_values()
            .await
            .map(|values| values.to_engine())
    }
}
