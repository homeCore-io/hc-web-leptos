//! Device domain types and pure helper functions.
//!
//! `DeviceState` mirrors `hc_types::device::DeviceState` field-for-field.
//! In a workspace-integrated build this entire module can be replaced with:
//!   pub use hc_types::device::DeviceState;
//!
//! All helpers are pure functions — they read a &DeviceState and return
//! display values.  No signals live here; signals belong in pages/components.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Core type ────────────────────────────────────────────────────────────────

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
    pub available: bool,
    pub attributes: HashMap<String, serde_json::Value>,
    pub last_seen: Option<DateTime<Utc>>,
}

// ── Attribute helpers ─────────────────────────────────────────────────────────

pub fn bool_attr(v: Option<&serde_json::Value>) -> Option<bool> {
    v.and_then(|v| v.as_bool())
}

pub fn str_attr<'a>(v: Option<&'a serde_json::Value>) -> Option<&'a str> {
    v.and_then(|v| v.as_str())
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
    str_attr(d.attributes.get("title"))
        .or_else(|| str_attr(d.attributes.get("media_title")))
}

pub fn media_artist(d: &DeviceState) -> Option<&str> {
    str_attr(d.attributes.get("artist"))
        .or_else(|| str_attr(d.attributes.get("media_artist")))
}

pub fn media_album(d: &DeviceState) -> Option<&str> {
    str_attr(d.attributes.get("album"))
        .or_else(|| str_attr(d.attributes.get("media_album")))
}

pub fn media_source(d: &DeviceState) -> Option<&str> {
    str_attr(d.attributes.get("source"))
}

pub fn media_image_url(d: &DeviceState) -> Option<&str> {
    str_attr(d.attributes.get("media_image_url"))
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

// ── Status ────────────────────────────────────────────────────────────────────

pub fn status_text(d: &DeviceState) -> String {
    if !d.available {
        return "Offline".to_string();
    }
    if is_media_player(d) {
        let s = playback_state(d);
        return match s.as_str() {
            "playing" => "Playing".to_string(),
            "paused"  => "Paused".to_string(),
            "stopped" => "Stopped".to_string(),
            other     => other.replace('_', " "),
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
    if let Some(contact) = bool_attr(d.attributes.get("contact")) {
        return if contact { "Open" } else { "Closed" }.to_string();
    }
    if let Some(locked) = bool_attr(d.attributes.get("locked")) {
        return if locked { "Locked" } else { "Unlocked" }.to_string();
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
            Self::Good    => "tone-good",
            Self::Warn    => "tone-warn",
            Self::Idle    => "tone-idle",
            Self::Media   => "tone-media",
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
        return if on { StatusTone::Good } else { StatusTone::Idle };
    }
    if let Some(motion) = bool_attr(d.attributes.get("motion")) {
        return if motion { StatusTone::Warn } else { StatusTone::Idle };
    }
    if let Some(open) = bool_attr(
        d.attributes.get("open").or_else(|| d.attributes.get("contact")),
    ) {
        return if open { StatusTone::Warn } else { StatusTone::Idle };
    }
    StatusTone::Idle
}

// Maps logical icon names → Material Icons ligatures.
// Override order: explicit status_icon field → derived from device state.
pub fn status_icon_name(d: &DeviceState) -> &'static str {
    // Explicit user override
    if let Some(icon) = d.status_icon.as_deref() {
        if let Some(m) = map_icon_name(icon) {
            return m;
        }
    }
    if !d.available {
        return "wifi_off";
    }
    if is_media_player(d) {
        return match playback_state(d).as_str() {
            "playing" => "play_arrow",
            "paused"  => "pause",
            "stopped" => "stop",
            _         => "speaker",
        };
    }
    if let Some(on) = bool_attr(d.attributes.get("on")) {
        return if on { "power" } else { "power_off" };
    }
    if let Some(locked) = bool_attr(d.attributes.get("locked")) {
        return if locked { "lock" } else { "lock_open_right" };
    }
    if bool_attr(d.attributes.get("motion")).is_some() {
        return "motion_sensor_active";
    }
    if bool_attr(
        d.attributes.get("open").or_else(|| d.attributes.get("contact")),
    ).is_some()
    {
        return "door_front";
    }
    "devices"
}

fn map_icon_name(s: &str) -> Option<&'static str> {
    Some(match s {
        "power"     => "power",
        "power_off" => "power_off",
        "lock"      => "lock",
        "lock_open" => "lock_open_right",
        "motion"    => "motion_sensor_active",
        "open"      => "door_open",
        "closed"    => "door_front",
        "play"      => "play_arrow",
        "pause"     => "pause",
        "stop"      => "stop",
        "media"     => "speaker",
        "devices"   => "devices",
        "offline"   => "wifi_off",
        "warning"   => "warning",
        "check"     => "check_circle",
        _           => return None,
    })
}

// ── History ───────────────────────────────────────────────────────────────────

/// One state-change record from `GET /devices/{id}/history`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub attribute: String,
    pub value: serde_json::Value,
    pub recorded_at: DateTime<Utc>,
}

impl HistoryEntry {
    /// Format `value` as a short display string.
    pub fn value_display(&self) -> String {
        match &self.value {
            serde_json::Value::Bool(b)   => if *b { "true".into() } else { "false".into() },
            serde_json::Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    if f.fract() == 0.0 { format!("{}", f as i64) } else { format!("{f:.2}") }
                } else { n.to_string() }
            }
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Null      => "null".into(),
            other                        => other.to_string(),
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
        None    => return "Unknown".to_string(),
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

/// Format a duration in milliseconds as M:SS or H:MM:SS.
pub fn format_duration_ms(ms: u64) -> String {
    let total = ms / 1000;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

pub fn format_abs(ts: Option<&DateTime<Utc>>) -> String {
    ts.map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}
