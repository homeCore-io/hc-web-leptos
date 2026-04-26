//! Device domain types and pure helper functions.
//!
//! Rule types are re-exported from `hc_types::rule` (shared with the core
//! server).  Device types are still defined locally because `DeviceState`
//! has minor field differences (last_seen is Optional here).
//!
//! All helpers are pure functions — they read a &DeviceState and return
//! display values.  No signals live here; signals belong in pages/components.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Re-exports from hc-types (shared with core) ────────────────────────────

pub use hc_types::rule::{Rule, RunMode, Trigger};
pub use hc_types::dashboard::{
    DashboardDefinition, DashboardRefreshPolicy, DashboardResponse, DashboardVisibility,
    DashboardWidget, DashboardWidgetType,
};

// ── Admin types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub role: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemStatus {
    pub version: String,
    pub uptime_seconds: i64,
    pub started_at: String,
    pub rules_total: u64,
    pub rules_enabled: u64,
    pub devices_total: u64,
    pub plugins_active: u64,
    pub state_db_bytes: u64,
    pub history_db_bytes: u64,
}

// ── Rule Groups ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuleGroup {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub rule_ids: Vec<String>,
}

// ── Core type ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Area {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub device_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scene {
    pub id: String,
    pub name: String,
    pub states: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModeKind {
    Solar,
    Manual,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CriteriaOffBehavior {
    #[default]
    Inverse,
    Explicit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModeConfig {
    pub id: String,
    pub name: String,
    pub kind: ModeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off_event: Option<String>,
    #[serde(default)]
    pub on_offset_minutes: i32,
    #[serde(default)]
    pub off_offset_minutes: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModeRecord {
    pub config: ModeConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<DeviceState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<ModeDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CriteriaModeConfig {
    pub on_condition: serde_json::Value,
    #[serde(default)]
    pub off_behavior: CriteriaOffBehavior,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off_condition: Option<serde_json::Value>,
    #[serde(default)]
    pub reevaluate_every_n_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModeDefinition {
    pub mode_id: String,
    pub criteria: CriteriaModeConfig,
    #[serde(default)]
    pub generated_rule_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceChangeKind {
    Homecore,
    Physical,
    External,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceChange {
    pub changed_at: DateTime<Utc>,
    pub kind: DeviceChangeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

/// Canonical state snapshot for a single device.
/// Field layout is identical to `hc_types::DeviceState`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceState {
    pub device_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_icon: Option<String>,
    pub name: String,
    pub plugin_id: String,
    pub area: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_type: Option<String>,
    /// User-set UI presentation hint (e.g. "light", "door", "window", "garage").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_hint: Option<String>,
    pub available: bool,
    pub attributes: HashMap<String, serde_json::Value>,
    pub last_seen: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_change: Option<DeviceChange>,
}

// ── Attribute helpers ─────────────────────────────────────────────────────────

pub fn bool_attr(v: Option<&serde_json::Value>) -> Option<bool> {
    v.and_then(|v| {
        v.as_bool().or_else(|| {
            v.as_str()
                .and_then(|s| match s.trim().to_ascii_lowercase().as_str() {
                    "true" | "on" | "open" | "active" | "occupied" | "detected" => Some(true),
                    "false" | "off" | "closed" | "inactive" | "clear" | "unoccupied" => Some(false),
                    _ => None,
                })
        })
    })
}

pub fn str_attr<'a>(v: Option<&'a serde_json::Value>) -> Option<&'a str> {
    v.and_then(|v| v.as_str())
}

pub fn num_attr(v: Option<&serde_json::Value>) -> Option<f64> {
    v.and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_i64().map(|n| n as f64))
            .or_else(|| v.as_u64().map(|n| n as f64))
    })
}

pub fn battery_pct(d: &DeviceState) -> Option<f64> {
    num_attr(
        d.attributes
            .get("battery_pct")
            .or_else(|| d.attributes.get("battery"))
            .or_else(|| d.attributes.get("battery_level")),
    )
}

pub fn temperature_unit(d: &DeviceState) -> Option<&'static str> {
    let explicit = str_attr(
        d.attributes
            .get("temperature_unit")
            .or_else(|| d.attributes.get("tempUnit")),
    )
    .map(str::trim)
    .filter(|value| !value.is_empty());

    match explicit {
        Some("F" | "f" | "°F" | "℉") => Some("F"),
        Some("C" | "c" | "°C" | "℃") => Some("C"),
        Some(_) => None,
        None if d.attributes.contains_key("temperature_f") => Some("F"),
        None if d.attributes.contains_key("temperature_c") => Some("C"),
        None => None,
    }
}

/// Resolve the temperature unit the UI should use for a thermostat, by first
/// checking the thermostat's own `temperature_unit` attribute and falling back
/// to the first configured sensor's unit. Returns `None` if no unit is
/// discoverable — callers typically fall back to a bare `°` symbol.
pub fn thermostat_temperature_unit(
    d: &DeviceState,
    devices: &std::collections::HashMap<String, DeviceState>,
) -> Option<&'static str> {
    if let Some(unit) = temperature_unit(d) {
        return Some(unit);
    }
    let sensor_ids = d.attributes.get("sensor_ids").and_then(|v| v.as_array())?;
    for sid_v in sensor_ids {
        let Some(sid) = sid_v.as_str() else { continue };
        if let Some(sensor) = devices.get(sid) {
            if let Some(unit) = temperature_unit(sensor) {
                return Some(unit);
            }
        }
    }
    None
}

pub fn illuminance_value(d: &DeviceState) -> Option<f64> {
    num_attr(
        d.attributes
            .get("illuminance")
            .or_else(|| d.attributes.get("illuminance_lux"))
            .or_else(|| d.attributes.get("illuminance_raw")),
    )
}

pub fn illuminance_unit(d: &DeviceState) -> Option<&'static str> {
    let explicit = str_attr(d.attributes.get("illuminance_unit"))
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match explicit {
        Some("lux" | "lx" | "Lux" | "LUX") => Some("lux"),
        Some("raw" | "Raw" | "RAW") => Some("raw"),
        Some(_) => None,
        None if d.attributes.contains_key("illuminance_lux") => Some("lux"),
        None if d.attributes.contains_key("illuminance_raw") => Some("raw"),
        None => None,
    }
}

// ── Classification ────────────────────────────────────────────────────────────

pub fn is_media_player(d: &DeviceState) -> bool {
    d.device_type.as_deref() == Some("media_player")
        || str_attr(d.attributes.get("kind")) == Some("media_player")
}

pub fn is_scene_like(d: &DeviceState) -> bool {
    let dt = d.device_type.as_deref().unwrap_or("").to_lowercase();
    let kind = str_attr(d.attributes.get("kind"))
        .unwrap_or("")
        .to_lowercase();
    dt == "scene" || kind == "scene"
}

pub fn is_timer_device(d: &DeviceState) -> bool {
    d.device_id.starts_with("timer_")
        || d.plugin_id.starts_with("core.timer")
        || d.device_type.as_deref() == Some("timer")
        || str_attr(d.attributes.get("kind")) == Some("timer")
}

pub fn is_thermostat_device(d: &DeviceState) -> bool {
    d.device_id.starts_with("thermostat_") || d.device_type.as_deref() == Some("thermostat")
}

pub fn is_plugin_scene_active(d: &DeviceState) -> bool {
    bool_attr(d.attributes.get("on"))
        .or_else(|| bool_attr(d.attributes.get("active")))
        .or_else(|| bool_attr(d.attributes.get("activate")))
        .or_else(|| bool_attr(d.attributes.get("state")))
        .unwrap_or(false)
}

pub fn scene_matches_live_state(scene: &Scene, devices: &HashMap<String, DeviceState>) -> bool {
    if scene.states.is_empty() {
        return false;
    }

    scene.states.iter().all(|(device_id, desired)| {
        let Some(device) = devices.get(device_id) else {
            return false;
        };
        let Some(expected_attrs) = desired.as_object() else {
            return false;
        };

        expected_attrs
            .iter()
            .all(|(key, expected)| device.attributes.get(key) == Some(expected))
    })
}

pub fn mode_is_on(d: &DeviceState) -> bool {
    bool_attr(d.attributes.get("on")).unwrap_or(false)
}

pub fn mode_kind_label(kind: ModeKind) -> &'static str {
    match kind {
        ModeKind::Solar => "Solar",
        ModeKind::Manual => "Manual",
    }
}

pub fn criteria_off_behavior_label(value: CriteriaOffBehavior) -> &'static str {
    match value {
        CriteriaOffBehavior::Inverse => "Inverse",
        CriteriaOffBehavior::Explicit => "Explicit",
    }
}

pub fn solar_event_label(value: Option<&str>) -> String {
    value
        .map(|raw| {
            raw.split('_')
                .map(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        Some(first) => {
                            first.to_uppercase().collect::<String>()
                                + &chars.as_str().to_lowercase()
                        }
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|| "Unknown".to_string())
}

fn timer_duration_secs(d: &DeviceState) -> Option<u64> {
    d.attributes
        .get("duration_secs")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            d.attributes
                .get("duration_ms")
                .and_then(|v| v.as_u64())
                .map(|ms| ms / 1000)
        })
}

fn timer_reported_remaining_secs(d: &DeviceState) -> Option<u64> {
    d.attributes
        .get("remaining_secs")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            d.attributes
                .get("remaining_ms")
                .and_then(|v| v.as_u64())
                .map(|ms| ms / 1000)
        })
}

fn timer_started_at(d: &DeviceState) -> Option<DateTime<Utc>> {
    str_attr(d.attributes.get("started_at"))
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

pub fn timer_remaining_secs(d: &DeviceState) -> Option<u64> {
    if !is_timer_device(d) {
        return None;
    }

    let timer_state = str_attr(d.attributes.get("state"))
        .unwrap_or("idle")
        .trim()
        .to_lowercase();
    let reported = timer_reported_remaining_secs(d);

    match timer_state.as_str() {
        "paused" => reported,
        "running" => {
            if let (Some(started_at), Some(duration_secs)) =
                (timer_started_at(d), timer_duration_secs(d))
            {
                let elapsed = (Utc::now() - started_at).num_seconds().max(0) as u64;
                Some(duration_secs.saturating_sub(elapsed))
            } else {
                reported
            }
        }
        _ => reported,
    }
}

// ── Display helpers ───────────────────────────────────────────────────────────

pub fn display_name(d: &DeviceState) -> &str {
    &d.name
}

pub fn playback_state(d: &DeviceState) -> String {
    str_attr(d.attributes.get("state"))
        .unwrap_or("unknown")
        .to_lowercase()
}

pub fn media_title(d: &DeviceState) -> Option<&str> {
    str_attr(d.attributes.get("title")).or_else(|| str_attr(d.attributes.get("media_title")))
}

pub fn media_artist(d: &DeviceState) -> Option<&str> {
    str_attr(d.attributes.get("artist")).or_else(|| str_attr(d.attributes.get("media_artist")))
}

pub fn media_album(d: &DeviceState) -> Option<&str> {
    str_attr(d.attributes.get("album")).or_else(|| str_attr(d.attributes.get("media_album")))
}

pub fn media_source(d: &DeviceState) -> Option<&str> {
    str_attr(d.attributes.get("source"))
}

pub fn media_image_url(d: &DeviceState) -> Option<&str> {
    str_attr(d.attributes.get("media_image_url"))
        .or_else(|| str_attr(d.attributes.get("image_url")))
        .or_else(|| str_attr(d.attributes.get("album_art_url")))
        .or_else(|| str_attr(d.attributes.get("albumArtUri")))
}

pub fn media_summary(d: &DeviceState) -> Option<String> {
    let title = media_title(d);
    let artist = media_artist(d);
    match (artist, title) {
        (Some(a), Some(t)) => Some(format!("{a} – {t}")),
        (None, Some(t)) => Some(t.to_string()),
        (Some(a), None) => Some(a.to_string()),
        (None, None) => media_album(d)
            .or_else(|| media_source(d))
            .map(str::to_string),
    }
}

pub fn string_list_attr(v: Option<&serde_json::Value>) -> Vec<String> {
    v.and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn media_ui_enrichments(d: &DeviceState) -> Vec<String> {
    string_list_attr(d.attributes.get("ui_enrichments"))
}

pub fn media_available_favorites(d: &DeviceState) -> Vec<String> {
    let favorites = string_list_attr(d.attributes.get("available_favorites"));
    if favorites.is_empty() {
        string_list_attr(
            d.attributes
                .get("sonos")
                .and_then(|value| value.get("favorites")),
        )
    } else {
        favorites
    }
}

pub fn media_available_playlists(d: &DeviceState) -> Vec<String> {
    let playlists = string_list_attr(d.attributes.get("available_playlists"));
    if playlists.is_empty() {
        string_list_attr(
            d.attributes
                .get("sonos")
                .and_then(|value| value.get("playlists")),
        )
    } else {
        playlists
    }
}

pub fn supported_actions(d: &DeviceState) -> Vec<&str> {
    d.attributes
        .get("supported_actions")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default()
}

pub fn supports_action(d: &DeviceState, action: &str) -> bool {
    let actions = supported_actions(d);
    if !actions.is_empty() {
        return actions.contains(&action);
    }
    if !is_media_player(d) {
        return false;
    }
    matches!(action, "play" | "pause")
}

pub fn supports_inline_toggle(d: &DeviceState) -> bool {
    bool_attr(d.attributes.get("on")).is_some()
}

pub fn supports_inline_lock(d: &DeviceState) -> bool {
    bool_attr(d.attributes.get("locked")).is_some()
}

pub fn presentation_device_type_key(d: &DeviceState) -> &'static str {
    // User-set ui_hint takes highest priority.
    if let Some(ref hint) = d.ui_hint {
        match hint.trim().to_lowercase().as_str() {
            "light" => return "light",
            "dimmer" | "dimmer_light" => return "dimmer",
            "switch" => return "switch",
            "lock" => return "lock",
            "shade" | "blind" | "cover" => return "shade",
            "door" | "contact_sensor" | "window" | "garage" | "gate" => return "contact_sensor",
            "motion" | "motion_sensor" => return "motion_sensor",
            "occupancy" | "occupancy_sensor" => return "occupancy_sensor",
            "leak" | "leak_sensor" | "water" => return "leak_sensor",
            "vibration" | "vibration_sensor" => return "vibration_sensor",
            "temperature" | "temperature_sensor" => return "temperature_sensor",
            "humidity" | "humidity_sensor" => return "humidity_sensor",
            "environment" | "environment_sensor" => return "environment_sensor",
            "media_player" => return "media_player",
            "keypad" => return "keypad",
            "remote" => return "remote",
            "timer" => return "timer",
            "sensor" => return "sensor",
            _ => {} // unrecognized hint — fall through to auto-detection
        }
    }

    let raw = d.device_type.as_deref().unwrap_or("").trim().to_lowercase();

    if is_media_player(d) {
        return "media_player";
    }
    if bool_attr(d.attributes.get("locked")).is_some() || raw == "lock" {
        return "lock";
    }
    if d.attributes
        .get("position")
        .and_then(serde_json::Value::as_f64)
        .is_some()
        || raw == "shade"
    {
        return "shade";
    }
    if d.attributes
        .get("brightness_pct")
        .and_then(serde_json::Value::as_f64)
        .is_some()
    {
        return "dimmer";
    }
    if bool_attr(d.attributes.get("motion")).is_some() || raw == "motion" || raw == "motion_sensor"
    {
        return "motion_sensor";
    }
    if bool_attr(d.attributes.get("occupied")).is_some()
        || bool_attr(d.attributes.get("occupancy")).is_some()
        || raw == "occupancy_group"
        || raw == "occupancy_sensor"
    {
        return "occupancy_sensor";
    }
    if bool_attr(d.attributes.get("contact")).is_some()
        || d.attributes.contains_key("contact_state")
        || raw == "contact_sensor"
    {
        return "contact_sensor";
    }
    if bool_attr(d.attributes.get("leak")).is_some()
        || bool_attr(d.attributes.get("water")).is_some()
        || raw == "leak_sensor"
    {
        return "leak_sensor";
    }
    if bool_attr(d.attributes.get("vibration")).is_some() || raw == "vibration_sensor" {
        return "vibration_sensor";
    }
    if d.attributes
        .get("temperature")
        .and_then(serde_json::Value::as_f64)
        .is_some()
        || d.attributes
            .get("temperature_c")
            .and_then(serde_json::Value::as_f64)
            .is_some()
    {
        if d.attributes
            .get("humidity")
            .and_then(serde_json::Value::as_f64)
            .is_some()
        {
            return "environment_sensor";
        }
        if raw == "temperature_sensor" {
            return "temperature_sensor";
        }
    }
    if d.attributes
        .get("humidity")
        .and_then(serde_json::Value::as_f64)
        .is_some()
        || raw == "humidity_sensor"
    {
        return "humidity_sensor";
    }
    match raw.as_str() {
        "light" => "light",
        "dimmer_light" | "dimmer" => "dimmer",
        "switch" => "switch",
        "vswitch" => "virtual_switch",
        "timer" => "timer",
        "keypad" => "keypad",
        "pico_remote" => "remote",
        "temperature_sensor" => "temperature_sensor",
        "binary_sensor" => "sensor",
        "sensor" => "sensor",
        _ => {
            if bool_attr(d.attributes.get("on")).is_some() {
                "switch"
            } else {
                "device"
            }
        }
    }
}

pub fn presentation_device_type_label(d: &DeviceState) -> &'static str {
    match presentation_device_type_key(d) {
        "media_player" => "Media Player",
        "lock" => "Lock",
        "shade" => "Shade",
        "light" => "Light",
        "dimmer" => "Dimmer",
        "switch" => "Switch",
        "virtual_switch" => "Virtual Switch",
        "motion_sensor" => "Motion Sensor",
        "occupancy_sensor" => "Occupancy Sensor",
        "contact_sensor" => "Contact Sensor",
        "leak_sensor" => "Leak Sensor",
        "vibration_sensor" => "Vibration Sensor",
        "temperature_sensor" => "Temperature Sensor",
        "humidity_sensor" => "Humidity Sensor",
        "environment_sensor" => "Temp / Humidity Sensor",
        "keypad" => "Keypad",
        "remote" => "Remote",
        "timer" => "Timer",
        "sensor" => "Sensor",
        _ => "Device",
    }
}

/// Return the Phosphor icon class for a device based on its type and current
/// state. Returns the full class string (`"ph ph-NAME"`) ready to slot
/// directly into an `<i class=...>` tag. Phosphor regular weight; loaded via
/// `@phosphor-icons/web` in index.html.
pub fn device_mdi_icon(d: &DeviceState) -> &'static str {
    let key = presentation_device_type_key(d);
    let on = bool_attr(d.attributes.get("on")).unwrap_or(false);
    let open = bool_attr(d.attributes.get("open")).unwrap_or(false);
    let locked = bool_attr(d.attributes.get("locked")).unwrap_or(true);
    let occupied = bool_attr(d.attributes.get("occupied"))
        .or_else(|| bool_attr(d.attributes.get("occupancy")))
        .unwrap_or(false);
    let motion = bool_attr(d.attributes.get("motion")).unwrap_or(false);

    match key {
        "light" | "dimmer" => if on { "ph ph-lightbulb-filament" } else { "ph ph-lightbulb" },
        "switch" => if on { "ph ph-toggle-right" } else { "ph ph-toggle-left" },
        "virtual_switch" => if on { "ph ph-toggle-right" } else { "ph ph-toggle-left" },
        "lock" => if locked { "ph ph-lock" } else { "ph ph-lock-open" },
        "shade" => if open { "ph ph-blinds" } else { "ph ph-blinds" },
        "media_player" => "ph ph-speaker-hifi",
        "motion_sensor" => if motion { "ph ph-person-simple-walk" } else { "ph ph-person-simple" },
        "occupancy_sensor" => if occupied { "ph ph-user-circle" } else { "ph ph-house" },
        "contact_sensor" => {
            // Check ui_hint first, then fall back to name-based detection
            let hint = d.ui_hint.as_deref().unwrap_or("").to_lowercase();
            let name = d.name.to_lowercase();
            if hint == "garage" || hint == "gate" || name.contains("garage") || name.contains("oh1") || name.contains("oh2") || name.contains("overhead") || name.contains("gate") {
                if open { "ph ph-garage" } else { "ph ph-garage" }
            } else if hint == "window" || name.contains("window") {
                if open { "ph ph-frame-corners" } else { "ph ph-square" }
            } else {
                // door is the default for contact sensors
                if open { "ph ph-door-open" } else { "ph ph-door" }
            }
        }
        "leak_sensor" => "ph ph-drop",
        "vibration_sensor" => "ph ph-vibrate",
        "temperature_sensor" => "ph ph-thermometer",
        "humidity_sensor" => "ph ph-drop-half",
        "environment_sensor" => "ph ph-thermometer-simple",
        "keypad" => "ph ph-grid-nine",
        "remote" => "ph ph-device-mobile",
        "timer" => "ph ph-timer",
        "power_monitor" => "ph ph-lightning",
        "sensor" => "ph ph-eye",
        "vcrx" => "ph ph-garage",
        _ => "ph ph-devices",
    }
}

pub fn raw_device_type_label(d: &DeviceState) -> String {
    d.device_type
        .as_deref()
        .unwrap_or("unknown")
        .replace('_', " ")
}

/// Returns a `device-card--type-X` class for the device-card type rim.
/// `""` if the device's type doesn't have a category we'd visually
/// differentiate (e.g. unknown/raw devices).
pub fn card_type_class(d: &DeviceState) -> &'static str {
    match presentation_device_type_key(d) {
        "light" | "dimmer" => "device-card--type-light",
        "switch" | "virtual_switch" => "device-card--type-switch",
        "lock" => "device-card--type-lock",
        "shade" => "device-card--type-shade",
        "contact_sensor" => "device-card--type-contact",
        "motion_sensor" => "device-card--type-motion",
        "occupancy_sensor" => "device-card--type-occupancy",
        "leak_sensor" => "device-card--type-leak",
        "vibration_sensor" => "device-card--type-vibration",
        "temperature_sensor" => "device-card--type-temp",
        "humidity_sensor" | "environment_sensor" => "device-card--type-humidity",
        "media_player" => "device-card--type-media",
        "thermostat" => "device-card--type-thermo",
        "keypad" => "device-card--type-keypad",
        "remote" => "device-card--type-remote",
        "timer" => "device-card--type-timer",
        "power_monitor" => "device-card--type-power",
        "sensor" => "device-card--type-sensor",
        _ => "",
    }
}

/// CIE xyY → sRGB conversion for color-capable lights (Hue, etc.).
/// Y is fixed at 1.0 since we want the *hue*, not the brightness — the
/// card body just needs to know what color the light is set to.
/// Returns an `rgb(R,G,B)` string ready for CSS use.
pub fn xy_to_rgb_string(x: f64, y: f64) -> String {
    if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) || y <= 0.0001 {
        return "rgb(255,255,255)".into();
    }
    let z = 1.0 - x - y;
    let big_y = 1.0_f64;
    let big_x = (big_y / y) * x;
    let big_z = (big_y / y) * z;

    // Linear RGB via the Hue/sRGB matrix.
    let r_lin = big_x * 1.656_492 - big_y * 0.354_851 - big_z * 0.255_038;
    let g_lin = -big_x * 0.707_196 + big_y * 1.655_397 + big_z * 0.036_152;
    let b_lin = big_x * 0.051_713 - big_y * 0.121_364 + big_z * 1.011_530;

    // Normalize so the brightest channel sits at 1.0 — keeps highly
    // saturated colors readable.
    let max = r_lin.max(g_lin).max(b_lin).max(1.0);
    let r_lin = (r_lin / max).max(0.0);
    let g_lin = (g_lin / max).max(0.0);
    let b_lin = (b_lin / max).max(0.0);

    // Gamma to sRGB.
    let to_srgb = |v: f64| -> u8 {
        let v = if v <= 0.003_130_8 {
            12.92 * v
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (v * 255.0).clamp(0.0, 255.0) as u8
    };
    format!(
        "rgb({},{},{})",
        to_srgb(r_lin),
        to_srgb(g_lin),
        to_srgb(b_lin)
    )
}

/// If the device exposes `color_xy: {x, y}`, return the corresponding
/// sRGB string for use in `--card-color`. Otherwise None.
pub fn device_color_css(d: &DeviceState) -> Option<String> {
    let xy = d.attributes.get("color_xy")?.as_object()?;
    let x = xy.get("x")?.as_f64()?;
    let y = xy.get("y")?.as_f64()?;
    Some(xy_to_rgb_string(x, y))
}

// ── Security tags ────────────────────────────────────────────────────────────
//
// Client-side per-device flag marking which locks / contact sensors count
// toward the "Security" hero tile and the ?focus=security filter on the
// Devices page. Stored in localStorage under SECURITY_TAGS_KEY as a JSON
// array of device_ids. Lightweight, no backend dependency. If/when this
// needs to roam between users or devices, promote to a server-side tags
// field.

const SECURITY_TAGS_KEY: &str = "hc-leptos:security-tags";

pub fn load_security_tags() -> std::collections::HashSet<String> {
    crate::pages::shared::ls_get(SECURITY_TAGS_KEY)
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

pub fn save_security_tags(tags: &std::collections::HashSet<String>) {
    let arr: Vec<&String> = tags.iter().collect();
    if let Ok(json) = serde_json::to_string(&arr) {
        crate::pages::shared::ls_set(SECURITY_TAGS_KEY, &json);
    }
}

pub fn is_security_tagged(device_id: &str) -> bool {
    load_security_tags().contains(device_id)
}

pub fn toggle_security_tag(device_id: &str) {
    let mut tags = load_security_tags();
    if !tags.remove(device_id) {
        tags.insert(device_id.to_string());
    }
    save_security_tags(&tags);
}

fn humanize_identifier(value: &str) -> String {
    value.replace(['_', '.'], " ")
}

fn title_case_words(value: &str) -> String {
    value
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn display_area_name(value: &str) -> String {
    title_case_words(&humanize_identifier(value))
}

pub fn display_area_value(value: Option<&str>) -> String {
    value
        .map(display_area_name)
        .unwrap_or_else(|| "Unassigned".to_string())
}

pub fn last_change_time(d: &DeviceState) -> Option<&DateTime<Utc>> {
    d.last_change
        .as_ref()
        .map(|change| &change.changed_at)
        .or(d.last_seen.as_ref())
}

pub fn change_kind_label(kind: DeviceChangeKind) -> &'static str {
    match kind {
        DeviceChangeKind::Homecore => "HomeCore",
        DeviceChangeKind::Physical => "Physical",
        DeviceChangeKind::External => "Plugin",
        DeviceChangeKind::Unknown => "Unknown",
    }
}

pub fn change_summary(d: &DeviceState) -> String {
    let Some(change) = d.last_change.as_ref() else {
        return "Unknown source".to_string();
    };

    let mut parts = vec![change_kind_label(change.kind).to_string()];

    if let Some(source) = change
        .source
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(humanize_identifier(source));
    }

    if let Some(actor) = change
        .actor_name
        .as_deref()
        .or(change.actor_id.as_deref())
        .filter(|value| !value.trim().is_empty())
    {
        let actor = actor.to_string();
        if !parts.iter().any(|part| part.eq_ignore_ascii_case(&actor)) {
            parts.push(actor);
        }
    }

    parts.join(" · ")
}

pub fn change_correlation_id(d: &DeviceState) -> Option<&str> {
    d.last_change
        .as_ref()
        .and_then(|change| change.correlation_id.as_deref())
}

// ── Status ────────────────────────────────────────────────────────────────────

pub fn status_text(d: &DeviceState) -> String {
    if !d.available {
        return "Offline".to_string();
    }
    if is_timer_device(d) {
        let timer_state = str_attr(d.attributes.get("state"))
            .unwrap_or("idle")
            .trim()
            .to_lowercase();
        let remaining_secs = timer_remaining_secs(d).unwrap_or(0);

        return match timer_state.as_str() {
            "running" | "paused" if remaining_secs > 0 => {
                format!("{} remaining", format_duration_secs(remaining_secs))
            }
            "finished" => "Finished".to_string(),
            "idle" => "Idle".to_string(),
            other => other.replace('_', " "),
        };
    }
    if is_media_player(d) {
        let s = playback_state(d);
        return match s.as_str() {
            "playing" => "Playing".to_string(),
            "paused" => "Paused".to_string(),
            "stopped" => "Stopped".to_string(),
            other => other.replace('_', " "),
        };
    }
    if let Some(on) = bool_attr(d.attributes.get("on")) {
        return if on { "On" } else { "Off" }.to_string();
    }
    if let Some(open) = bool_attr(d.attributes.get("open")) {
        return if open { "Open" } else { "Closed" }.to_string();
    }
    if let Some(motion) = bool_attr(d.attributes.get("motion")) {
        return if motion { "Motion detected" } else { "Clear" }.to_string();
    }
    if let Some(occupied) = bool_attr(
        d.attributes
            .get("occupied")
            .or_else(|| d.attributes.get("occupancy")),
    ) {
        return if occupied { "Occupied" } else { "Clear" }.to_string();
    }
    if let Some(contact) = bool_attr(d.attributes.get("contact")) {
        return if contact { "Open" } else { "Closed" }.to_string();
    }
    if let Some(locked) = bool_attr(d.attributes.get("locked")) {
        return if locked { "Locked" } else { "Unlocked" }.to_string();
    }
    let temperature = num_attr(
        d.attributes
            .get("temperature")
            .or_else(|| d.attributes.get("temperature_f"))
            .or_else(|| d.attributes.get("temperature_c")),
    );
    let humidity = num_attr(
        d.attributes
            .get("humidity_pct")
            .or_else(|| d.attributes.get("humidity")),
    );
    let temp_unit = temperature_unit(d);
    if let (Some(temp), Some(humidity)) = (temperature, humidity) {
        return match temp_unit {
            Some(unit) => format!("Temp {temp:.1} {unit}, RH {humidity:.0}%"),
            None => format!("Temp {temp:.1}°, RH {humidity:.0}%"),
        };
    }
    if let Some(temp) = temperature {
        return match temp_unit {
            Some(unit) => format!("Temp {temp:.1} {unit}"),
            None => format!("Temp {temp:.1}°"),
        };
    }
    if let Some(humidity) = humidity {
        return format!("RH {humidity:.0}%");
    }
    if let Some(illuminance) = illuminance_value(d) {
        return match illuminance_unit(d) {
            Some("raw") => format!("Light {illuminance:.0} raw"),
            Some("lux") => format!("Light {illuminance:.1} lux"),
            Some(unit) => format!("Light {illuminance:.1} {unit}"),
            None => format!("Light {illuminance:.1}"),
        };
    }
    let battery_pct = battery_pct(d);
    let battery_state = str_attr(d.attributes.get("battery_state"))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(level) = battery_pct {
        if matches!(battery_state, Some("low" | "critical")) {
            return format!("Battery {level:.0}% ({})", battery_state.unwrap());
        }
        return format!("Battery {level:.0}%");
    }
    if let Some(state) = battery_state {
        return format!("Battery {}", state.replace('_', " "));
    }
    if let Some(s) = str_attr(d.attributes.get("state")) {
        if !s.trim().is_empty() {
            return s.replace('_', " ");
        }
    }
    d.device_type
        .as_deref()
        .unwrap_or("Ready")
        .replace('_', " ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Good,
    Warn,
    Idle,
    Media,
    Offline,
}

impl StatusTone {
    pub fn css_class(self) -> &'static str {
        match self {
            Self::Good => "tone-good",
            Self::Warn => "tone-warn",
            Self::Idle => "tone-idle",
            Self::Media => "tone-media",
            Self::Offline => "tone-offline",
        }
    }
}

pub fn status_tone(d: &DeviceState) -> StatusTone {
    if !d.available {
        return StatusTone::Offline;
    }
    if is_media_player(d) {
        return StatusTone::Media;
    }
    if let Some(on) = bool_attr(d.attributes.get("on")) {
        return if on {
            StatusTone::Good
        } else {
            StatusTone::Idle
        };
    }
    if let Some(motion) = bool_attr(d.attributes.get("motion")) {
        return if motion {
            StatusTone::Warn
        } else {
            StatusTone::Idle
        };
    }
    if let Some(occupied) = bool_attr(
        d.attributes
            .get("occupied")
            .or_else(|| d.attributes.get("occupancy")),
    ) {
        return if occupied {
            StatusTone::Warn
        } else {
            StatusTone::Idle
        };
    }
    if let Some(open) = bool_attr(
        d.attributes
            .get("open")
            .or_else(|| d.attributes.get("contact")),
    ) {
        return if open {
            StatusTone::Warn
        } else {
            StatusTone::Idle
        };
    }
    StatusTone::Idle
}

// Maps logical icon names → Phosphor identifiers (slot into "ph ph-{name}").
// Override order: explicit status_icon field → derived from device state.
pub fn status_icon_name(d: &DeviceState) -> &'static str {
    // Explicit user override
    if let Some(icon) = d.status_icon.as_deref() {
        if let Some(m) = map_icon_name(icon) {
            return m;
        }
    }
    if !d.available {
        return "wifi-slash";
    }
    if is_media_player(d) {
        return match playback_state(d).as_str() {
            "playing" => "play",
            "paused" => "pause",
            "stopped" => "stop",
            _ => "speaker-hifi",
        };
    }
    if let Some(on) = bool_attr(d.attributes.get("on")) {
        return if on { "power" } else { "plugs" };
    }
    if let Some(locked) = bool_attr(d.attributes.get("locked")) {
        return if locked { "lock" } else { "lock-open" };
    }
    if let Some(motion) = bool_attr(d.attributes.get("motion")) {
        return if motion {
            "person-simple-walk"
        } else {
            "person-simple"
        };
    }
    if let Some(occupied) = bool_attr(
        d.attributes
            .get("occupied")
            .or_else(|| d.attributes.get("occupancy")),
    ) {
        return if occupied { "user" } else { "user-minus" };
    }
    if bool_attr(
        d.attributes
            .get("open")
            .or_else(|| d.attributes.get("contact")),
    )
    .is_some()
    {
        return "door";
    }
    "devices"
}

fn map_icon_name(s: &str) -> Option<&'static str> {
    // Returns Phosphor names (slot into "ph ph-{name}" by the view).
    Some(match s {
        "power" => "power",
        "power_off" => "plugs",
        "lock" => "lock",
        "lock_open" => "lock-open",
        "motion" => "person-simple-walk",
        "occupied" => "user",
        "unoccupied" => "user-minus",
        "open" => "door-open",
        "closed" => "door",
        "play" => "play",
        "pause" => "pause",
        "stop" => "stop",
        "media" => "speaker-hifi",
        "devices" => "devices",
        "offline" => "wifi-slash",
        "warning" => "warning",
        "check" => "check-circle",
        _ => return None,
    })
}

// ── History ───────────────────────────────────────────────────────────────────

/// One state-change record from `GET /devices/{id}/history`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistoryEntry {
    pub attribute: String,
    pub value: serde_json::Value,
    pub recorded_at: DateTime<Utc>,
}

impl HistoryEntry {
    /// Format `value` as a short display string.
    pub fn value_display(&self) -> String {
        match &self.value {
            serde_json::Value::Bool(b) => {
                if *b {
                    "true".into()
                } else {
                    "false".into()
                }
            }
            serde_json::Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    if f.fract() == 0.0 {
                        format!("{}", f as i64)
                    } else {
                        format!("{f:.2}")
                    }
                } else {
                    n.to_string()
                }
            }
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Null => "null".into(),
            other => other.to_string(),
        }
    }
}

// ── Sorting helpers ───────────────────────────────────────────────────────────

pub fn sort_key_str(s: &str) -> String {
    s.trim().to_lowercase()
}

// ── Time formatting ───────────────────────────────────────────────────────────

pub fn format_relative(ts: Option<&DateTime<Utc>>) -> String {
    let ts = match ts {
        Some(t) => t,
        None => return "Unknown".to_string(),
    };
    let now = Utc::now();
    let diff = (now - ts).num_seconds().max(0) as u64;
    if diff < 60 {
        format!("{diff}s ago")
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

pub fn format_duration_secs(total: u64) -> String {
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub fn format_abs(ts: Option<&DateTime<Utc>>) -> String {
    ts.map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

// ── Plugin types ────────────────────────────────────────────────────────────

/// Plugin record matching the enriched `PluginRecord` from core API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginInfo {
    pub plugin_id: String,
    pub registered_at: DateTime<Utc>,
    pub status: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub managed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_restart: Option<DateTime<Utc>>,
    #[serde(default)]
    pub restart_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_started: Option<DateTime<Utc>>,
    #[serde(default)]
    pub device_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub supports_management: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<hc_types::Capabilities>,
}

impl PluginInfo {
    /// Display name: strip "plugin." prefix and capitalize.
    pub fn display_name(&self) -> String {
        let name = self.plugin_id.strip_prefix("plugin.").unwrap_or(&self.plugin_id);
        let mut chars = name.chars();
        match chars.next() {
            Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
            None => name.to_string(),
        }
    }

    /// Uptime as a human-readable string.
    pub fn uptime_str(&self) -> String {
        let Some(started) = self.uptime_started else { return "—".into() };
        let secs = (Utc::now() - started).num_seconds().max(0) as u64;
        let d = secs / 86400;
        let h = (secs % 86400) / 3600;
        let m = (secs % 3600) / 60;
        if d > 0 { format!("{d}d {h}h {m}m") }
        else if h > 0 { format!("{h}h {m}m") }
        else { format!("{m}m") }
    }
}
