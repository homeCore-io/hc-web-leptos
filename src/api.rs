//! HomeCore API client — thin wrappers over gloo-net HTTP requests.

use crate::auth::API_BASE;
use crate::models::{
    Area, CriteriaModeConfig, DeviceState, ModeConfig, ModeDefinition, ModeKind, ModeRecord, Scene,
};
use gloo_net::http::Request;
use serde_json::Value;

// ── Generic request helpers ───────────────────────────────────────────────────

async fn get_json<T: serde::de::DeserializeOwned>(path: &str, token: &str) -> Result<T, String> {
    let resp = Request::get(&format!("{API_BASE}{path}"))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("{} {}", resp.status(), resp.status_text()));
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

async fn post_json<T: serde::de::DeserializeOwned>(
    path: &str,
    token: &str,
    body: &Value,
) -> Result<T, String> {
    let resp = Request::post(&format!("{API_BASE}{path}"))
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

    resp.json::<T>().await.map_err(|e| e.to_string())
}

async fn post_no_body(path: &str, token: &str) -> Result<(), String> {
    let resp = Request::post(&format!("{API_BASE}{path}"))
        .header("Authorization", &format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("{} {}", resp.status(), resp.status_text()));
    }

    Ok(())
}

async fn delete_no_body(path: &str, token: &str) -> Result<(), String> {
    let resp = Request::delete(&format!("{API_BASE}{path}"))
        .header("Authorization", &format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("{} {}", resp.status(), resp.status_text()));
    }

    Ok(())
}

async fn put_json<T: serde::de::DeserializeOwned>(
    path: &str,
    token: &str,
    body: &Value,
) -> Result<T, String> {
    let resp = Request::put(&format!("{API_BASE}{path}"))
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

    resp.json::<T>().await.map_err(|e| e.to_string())
}

// ── Device API ────────────────────────────────────────────────────────────────

pub async fn fetch_devices(token: &str) -> Result<Vec<DeviceState>, String> {
    get_json("/devices", token).await
}

pub async fn fetch_areas(token: &str) -> Result<Vec<Area>, String> {
    get_json("/areas", token).await
}

pub async fn fetch_scenes(token: &str) -> Result<Vec<Scene>, String> {
    get_json("/scenes", token).await
}

pub async fn fetch_modes(token: &str) -> Result<Vec<ModeRecord>, String> {
    get_json("/modes", token).await
}

pub async fn create_mode(
    token: &str,
    id: &str,
    name: &str,
    kind: ModeKind,
    criteria_definition: Option<&CriteriaModeConfig>,
) -> Result<ModeConfig, String> {
    post_json(
        "/modes",
        token,
        &serde_json::json!({
            "id": id,
            "name": name,
            "kind": kind,
            "criteria_definition": criteria_definition,
        }),
    )
    .await
}

pub async fn delete_mode(token: &str, id: &str) -> Result<(), String> {
    delete_no_body(&format!("/modes/{id}"), token).await
}

pub async fn put_mode_definition(
    token: &str,
    id: &str,
    criteria: &CriteriaModeConfig,
) -> Result<ModeDefinition, String> {
    put_json(
        &format!("/modes/{id}/definition"),
        token,
        &serde_json::json!(criteria),
    )
    .await
}

pub async fn delete_mode_definition(token: &str, id: &str) -> Result<(), String> {
    delete_no_body(&format!("/modes/{id}/definition"), token).await
}

pub async fn fetch_scene(token: &str, id: &str) -> Result<Scene, String> {
    get_json(&format!("/scenes/{id}"), token).await
}

pub async fn create_scene(
    token: &str,
    name: &str,
    states: &serde_json::Map<String, Value>,
) -> Result<Scene, String> {
    post_json(
        "/scenes",
        token,
        &serde_json::json!({
            "name": name,
            "states": states,
        }),
    )
    .await
}

pub async fn update_scene(
    token: &str,
    id: &str,
    name: &str,
    states: &serde_json::Map<String, Value>,
) -> Result<Scene, String> {
    put_json(
        &format!("/scenes/{id}"),
        token,
        &serde_json::json!({
            "name": name,
            "states": states,
        }),
    )
    .await
}

pub async fn delete_scene(token: &str, id: &str) -> Result<(), String> {
    delete_no_body(&format!("/scenes/{id}"), token).await
}

pub async fn activate_scene(token: &str, id: &str) -> Result<(), String> {
    post_no_body(&format!("/scenes/{id}/activate"), token).await
}

pub async fn fetch_device(token: &str, id: &str) -> Result<DeviceState, String> {
    get_json(&format!("/devices/{id}"), token).await
}

pub async fn create_area(token: &str, name: &str) -> Result<Area, String> {
    post_json("/areas", token, &serde_json::json!({ "name": name })).await
}

pub async fn update_area(token: &str, id: &str, name: &str) -> Result<Area, String> {
    let resp = Request::patch(&format!("{API_BASE}/areas/{id}"))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(serde_json::json!({ "name": name }).to_string())
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("{} {}", resp.status(), resp.status_text()));
    }

    resp.json::<Area>().await.map_err(|e| e.to_string())
}

pub async fn delete_area(token: &str, id: &str) -> Result<(), String> {
    delete_no_body(&format!("/areas/{id}"), token).await
}

pub async fn set_area_devices(
    token: &str,
    id: &str,
    device_ids: &[String],
) -> Result<Area, String> {
    put_json(
        &format!("/areas/{id}/devices"),
        token,
        &serde_json::json!(device_ids),
    )
    .await
}

pub async fn set_device_state(token: &str, device_id: &str, body: &Value) -> Result<(), String> {
    patch_json(&format!("/devices/{device_id}/state"), token, body).await
}

pub async fn update_device_meta(
    token: &str,
    id: &str,
    body: &Value,
) -> Result<DeviceState, String> {
    let resp = Request::patch(&format!("{API_BASE}/devices/{id}"))
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

    resp.json::<DeviceState>().await.map_err(|e| e.to_string())
}

pub async fn fetch_device_history(
    token: &str,
    id: &str,
    limit: u32,
) -> Result<Vec<HistoryEntry>, String> {
    get_json(&format!("/devices/{id}/history?limit={limit}"), token).await
}

#[derive(Debug, Deserialize)]
pub struct DeleteDeviceResponse {
    pub deleted: bool,
    #[serde(default)]
    pub affected_rules: Vec<String>,
}

pub async fn delete_device(token: &str, id: &str) -> Result<DeleteDeviceResponse, String> {
    let resp = Request::delete(&format!("{API_BASE}/devices/{id}"))
        .header("Authorization", &format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("{} {}", resp.status(), resp.status_text()));
    }

    resp.json::<DeleteDeviceResponse>()
        .await
        .map_err(|e| e.to_string())
}

use crate::models::HistoryEntry;
use serde::Deserialize;
