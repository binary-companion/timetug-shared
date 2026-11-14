use std::collections::BTreeSet;
use std::fmt;

use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

/// Shared, platform-neutral settings. Core defines the shape; each front end stores and edits it.
///
/// Persisted as JSON (family A of the conventions); the file is the user's settings file, so every key is optional on
/// read and the retired keys are read but never written.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeoverSettings {
    /// Seconds before start that the takeover fires. 0 means "starting now".
    #[serde(serialize_with = "calendar_core::json::f64_swift")]
    pub lead_time: f64,
    /// `CalendarInfo::key` values allowed to trigger takeovers (opt-in).
    pub takeover_calendar_keys: BTreeSet<String>,
    /// `CalendarInfo::key` values hidden from the day list (default: none hidden).
    pub hidden_calendar_keys: BTreeSet<String>,
    /// When true, only events with a conference link take over.
    pub require_conference_link: bool,
    /// When true, events with no attendees besides the owner never take over. Declined events never do either; that is not a setting.
    pub require_other_attendees: bool,
    /// Hides all-day events from the day list only. All-day events never trigger a takeover, regardless of this setting.
    pub skip_all_day_events: bool,
    /// Master switch: while false no takeover or pre-meeting popup fires. The agenda is unaffected.
    pub enabled: bool,
}

impl Default for TakeoverSettings {
    fn default() -> TakeoverSettings {
        TakeoverSettings {
            lead_time: 60.0,
            takeover_calendar_keys: BTreeSet::new(),
            hidden_calendar_keys: BTreeSet::new(),
            require_conference_link: false,
            require_other_attendees: false,
            skip_all_day_events: true,
            enabled: true,
        }
    }
}

/// A legacy key's value, read leniently: Swift reads a legacy key only when the current key is missing, so a legacy key
/// of the wrong type is an error only then.
#[derive(Deserialize)]
#[serde(untagged)]
enum LegacyFlag {
    Value(bool),
    WrongType(IgnoredAny),
}

impl LegacyFlag {
    /// `legacy.decodeIfPresent(Bool.self, forKey:)`.
    fn read<E: de::Error>(flag: Option<LegacyFlag>, key: &str) -> Result<Option<bool>, E> {
        match flag {
            None => Ok(None),
            Some(LegacyFlag::Value(value)) => Ok(Some(value)),
            Some(LegacyFlag::WrongType(_)) => Err(E::custom(format!("{key}: expected a boolean"))),
        }
    }
}

// Persisted as JSON: every field must decode with decodeIfPresent and its default, so saved settings survive newly added fields.
// Keys written by earlier versions (`disabled`, `skipSoloEvents`) are read only to carry the user's choice forward, never written.
// Hand-written key by key, as in Swift: a JSON `null` is a missing key, an unknown key is ignored, and only an object is accepted.
impl<'de> Deserialize<'de> for TakeoverSettings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<TakeoverSettings, D::Error> {
        struct SettingsVisitor;

        impl<'de> Visitor<'de> for SettingsVisitor {
            type Value = TakeoverSettings;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a settings object")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<TakeoverSettings, A::Error> {
                let mut lead_time: Option<f64> = None;
                let mut takeover_calendar_keys: Option<BTreeSet<String>> = None;
                let mut hidden_calendar_keys: Option<BTreeSet<String>> = None;
                let mut require_conference_link: Option<bool> = None;
                let mut require_other_attendees: Option<bool> = None;
                let mut skip_all_day_events: Option<bool> = None;
                let mut enabled: Option<bool> = None;
                let mut legacy_disabled: Option<LegacyFlag> = None;
                let mut legacy_skip_solo_events: Option<LegacyFlag> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "leadTime" => lead_time = map.next_value()?,
                        "takeoverCalendarKeys" => takeover_calendar_keys = map.next_value()?,
                        "hiddenCalendarKeys" => hidden_calendar_keys = map.next_value()?,
                        "requireConferenceLink" => require_conference_link = map.next_value()?,
                        "requireOtherAttendees" => require_other_attendees = map.next_value()?,
                        "skipAllDayEvents" => skip_all_day_events = map.next_value()?,
                        "enabled" => enabled = map.next_value()?,
                        "disabled" => legacy_disabled = map.next_value()?,
                        "skipSoloEvents" => legacy_skip_solo_events = map.next_value()?,
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                let d = TakeoverSettings::default();
                let require_other_attendees = match require_other_attendees {
                    Some(value) => value,
                    None => LegacyFlag::read(legacy_skip_solo_events, "skipSoloEvents")?
                        .unwrap_or(d.require_other_attendees),
                };
                let enabled = match enabled {
                    Some(value) => value,
                    None => LegacyFlag::read(legacy_disabled, "disabled")?
                        .map_or(d.enabled, |disabled| !disabled),
                };
                let hidden_calendar_keys = hidden_calendar_keys.unwrap_or(d.hidden_calendar_keys);
                // A calendar that is not shown never tugs, so saved data that both hides and opts in a calendar keeps it hidden.
                let takeover_calendar_keys = takeover_calendar_keys
                    .unwrap_or(d.takeover_calendar_keys)
                    .difference(&hidden_calendar_keys)
                    .cloned()
                    .collect();
                Ok(TakeoverSettings {
                    lead_time: lead_time.unwrap_or(d.lead_time),
                    takeover_calendar_keys,
                    hidden_calendar_keys,
                    require_conference_link: require_conference_link
                        .unwrap_or(d.require_conference_link),
                    require_other_attendees,
                    skip_all_day_events: skip_all_day_events.unwrap_or(d.skip_all_day_events),
                    enabled,
                })
            }
        }

        deserializer.deserialize_map(SettingsVisitor)
    }
}

impl TakeoverSettings {
    /// True when the calendar `key` is not hidden from the day list.
    pub fn is_shown_in_list(&self, key: &str) -> bool {
        !self.hidden_calendar_keys.contains(key)
    }

    /// Enabling takeover also shows the calendar in the list; disabling leaves visibility alone.
    pub fn set_takeover(&mut self, enabled: bool, key: &str) {
        if enabled {
            self.takeover_calendar_keys.insert(key.to_string());
            self.hidden_calendar_keys.remove(key);
        } else {
            self.takeover_calendar_keys.remove(key);
        }
    }

    /// Swift's `setTakeover(_:forCalendars:)`.
    pub fn set_takeover_for_calendars(&mut self, enabled: bool, keys: &[&str]) {
        for key in keys {
            self.set_takeover(enabled, key);
        }
    }

    /// Hiding a calendar also turns takeover off for it; showing leaves takeover alone.
    pub fn set_shown_in_list(&mut self, shown: bool, key: &str) {
        if shown {
            self.hidden_calendar_keys.remove(key);
        } else {
            self.hidden_calendar_keys.insert(key.to_string());
            self.takeover_calendar_keys.remove(key);
        }
    }

    /// Removes every stored calendar key whose source id (the text before the first "/") satisfies `is_removed`.
    pub fn remove_calendars_where_source_id(&mut self, is_removed: impl Fn(&str) -> bool) {
        fn source_id_of(key: &str) -> &str {
            key.split_once('/').map_or(key, |(source_id, _)| source_id)
        }
        self.takeover_calendar_keys
            .retain(|key| !is_removed(source_id_of(key)));
        self.hidden_calendar_keys
            .retain(|key| !is_removed(source_id_of(key)));
    }

    /// Forgets the Tug and visibility choices of one source (a removed account).
    pub fn remove_calendars_for_source_id(&mut self, source_id: &str) {
        self.remove_calendars_where_source_id(|id| id == source_id);
    }
}
