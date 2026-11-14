//! Mirrors of the engine's settings. The Swift closure of `applySettings` becomes a `SettingsPatch`.
#![allow(missing_docs)]

use std::collections::HashSet;

use timetug_core::TakeoverSettings as EngineTakeoverSettings;
use timetug_engine::settings::EngineSettings as EngineEngineSettings;

use super::mirror_record;

mirror_record!(TakeoverSettings <=> EngineTakeoverSettings {
    lead_time: f64,
    takeover_calendar_keys: HashSet<String>,
    hidden_calendar_keys: HashSet<String>,
    require_conference_link: bool,
    require_other_attendees: bool,
    skip_all_day_events: bool,
    enabled: bool,
});

mirror_record!(EngineSettings <=> EngineEngineSettings {
    takeover: TakeoverSettings,
    inference_enabled: bool,
    system_calendar_enabled: bool,
});

/// What `applySettings` changes: a field left `None` is left alone. Replaces the Swift closure `(inout EngineSettings)`.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct SettingsPatch {
    #[uniffi(default = None)]
    pub lead_time: Option<f64>,
    #[uniffi(default = None)]
    pub takeover_calendar_keys: Option<HashSet<String>>,
    #[uniffi(default = None)]
    pub hidden_calendar_keys: Option<HashSet<String>>,
    #[uniffi(default = None)]
    pub require_conference_link: Option<bool>,
    #[uniffi(default = None)]
    pub require_other_attendees: Option<bool>,
    #[uniffi(default = None)]
    pub skip_all_day_events: Option<bool>,
    #[uniffi(default = None)]
    pub enabled: Option<bool>,
    #[uniffi(default = None)]
    pub inference_enabled: Option<bool>,
    #[uniffi(default = None)]
    pub system_calendar_enabled: Option<bool>,
}

impl SettingsPatch {
    /// Applies the patch to `settings`, as the Swift closure did.
    pub fn apply(self, settings: &mut EngineEngineSettings) {
        if let Some(value) = self.lead_time {
            settings.takeover.lead_time = value;
        }
        if let Some(value) = self.takeover_calendar_keys {
            settings.takeover.takeover_calendar_keys = value.into_iter().collect();
        }
        if let Some(value) = self.hidden_calendar_keys {
            settings.takeover.hidden_calendar_keys = value.into_iter().collect();
        }
        if let Some(value) = self.require_conference_link {
            settings.takeover.require_conference_link = value;
        }
        if let Some(value) = self.require_other_attendees {
            settings.takeover.require_other_attendees = value;
        }
        if let Some(value) = self.skip_all_day_events {
            settings.takeover.skip_all_day_events = value;
        }
        if let Some(value) = self.enabled {
            settings.takeover.enabled = value;
        }
        if let Some(value) = self.inference_enabled {
            settings.inference_enabled = value;
        }
        if let Some(value) = self.system_calendar_enabled {
            settings.system_calendar_enabled = value;
        }
    }
}
