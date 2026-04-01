//! HomeCore API client — thin wrappers over gloo-net HTTP requests.

use crate::auth::API_BASE;
use crate::models::DeviceState;
use gloo_net::http::Request;
use serde_json::Value;

// ── Generic request helpers ───────────────────────────────────────────────────

async fn get_json<T: serde::de::DeserializeOwned>(
    path: &str,
    token: &str,
) -> Result<T, String> {
    let resp = Request::get(&format!("{API_BASE}{path}"))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!(
            "{} {}",
            resp.status(),
            resp.status_text()
        ));
    }

    resp.json::<T>().await.map_err(|e| e.to_string())
}

async fn patch_json(path: &str, token: &str, body: &Value) -> Result<(), String> {
    let resp = Request::patch(&format!("{API_BASE}{path}"))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("{} {}", resp.status(), resp.status_text()));
    }
    Ok(())
}

// ── Device API ────────────────────────────────────────────────────────────────

pub async fn fetch_devices(token: &str) -> Result<Vec<DeviceState>, String> {
    get_json("/devices", token).await
}

pub async fn fetch_device(token: &str, id: &str) -> Result<DeviceState, String> {
    get_json(&format!("/devices/{id}"), token).await
}

pub async fn set_device_state(
    token: &str,
    device_id: &str,
    body: &Value,
) -> Result<(), String> {
    patch_json(&format!("/devices/{device_id}/state"), token, body).await
}

pub async fn update_device_meta(
    token: &str,
    id: &str,
    body: &Value,
) -> Result<(), String> {
    patch_json(&format!("/devices/{id}"), token, body).await
}

pub async fn fetch_device_history(
    token: &str,
    id: &str,
    limit: u32,
) -> Result<Vec<HistoryEntry>, String> {
    get_json(&format!("/devices/{id}/history?limit={limit}"), token).await
}

// ── WebSocket event types ─────────────────────────────────────────────────────
//
// Only the subset needed for live-updating the devices page.
// The full `hc_types::Event` enum is tagged with `"type"` using
// snake_case variant names.

use crate::models::HistoryEntry;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    DeviceStateChanged {
        device_id: String,
        current:   HashMap<String, Value>,
        #[serde(default)]
        #[allow(dead_code)]
        changed:   Vec<String>,
    },
    DeviceAvailabilityChanged {
        device_id: String,
        available: bool,
    },
    #[serde(other)]
    Other,
}
