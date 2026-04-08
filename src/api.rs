//! HomeCore API client — thin wrappers over gloo-net HTTP requests.
//!
//! All request helpers detect HTTP 401 responses and trigger a session
//! expiry logout via `handle_session_expiry()`.

use crate::auth::API_BASE;
use crate::models::{
    Area, CriteriaModeConfig, DeviceState, ModeConfig, ModeDefinition, ModeKind, ModeRecord, Scene,
};
use gloo_net::http::Request;
use serde_json::Value;

// ── Generic request helpers ───────────────────────────────────────────────────

/// Clear the expired token so the auth guard redirects to login.
/// Called when any API request returns 401.
fn handle_session_expiry() {
    // Try to get the auth context (available when called from a reactive scope).
    if let Some(auth) = leptos::prelude::use_context::<crate::auth::AuthState>() {
        auth.logout();
    } else {
        // Fallback: clear localStorage directly (auth signal won't update,
        // but a page reload will redirect to login).
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = storage.remove_item("hc-leptos:token");
        }
    }
}

/// Extract a meaningful error message from a non-ok response.
/// On 401, triggers session expiry logout automatically.
async fn api_error(resp: &gloo_net::http::Response) -> String {
    let status = resp.status();

    // 401 = expired or invalid JWT → clear session
    if status == 401 {
        handle_session_expiry();
        return "Session expired — please log in again".to_string();
    }

    if let Ok(body) = resp.text().await {
        if let Ok(json) = serde_json::from_str::<Value>(&body) {
            if let Some(msg) = json["error"].as_str() {
                return format!("{status}: {msg}");
            }
        }
        if !body.is_empty() && body.len() < 200 {
            return format!("{status}: {body}");
        }
    }
    format!("{status} {}", resp.status_text())
}

async fn get_json<T: serde::de::DeserializeOwned>(path: &str, token: &str) -> Result<T, String> {
    let resp = Request::get(&format!("{API_BASE}{path}"))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(api_error(&resp).await);
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
        return Err(api_error(&resp).await);
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
        return Err(api_error(&resp).await);
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
        return Err(api_error(&resp).await);
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
        return Err(api_error(&resp).await);
    }

    Ok(())
}

async fn patch_json_with_response<T: serde::de::DeserializeOwned>(
    path: &str,
    token: &str,
    body: &Value,
) -> Result<T, String> {
    let resp = Request::patch(&format!("{API_BASE}{path}"))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(api_error(&resp).await);
    }

    resp.json::<T>().await.map_err(|e| e.to_string())
}

async fn post_json_no_response(path: &str, token: &str, body: &Value) -> Result<(), String> {
    let resp = Request::post(&format!("{API_BASE}{path}"))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(api_error(&resp).await);
    }

    Ok(())
}

async fn put_json_no_response(path: &str, token: &str, body: &Value) -> Result<(), String> {
    let resp = Request::put(&format!("{API_BASE}{path}"))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(api_error(&resp).await);
    }

    Ok(())
}

async fn post_binary(path: &str, token: &str) -> Result<Vec<u8>, String> {
    let resp = Request::post(&format!("{API_BASE}{path}"))
        .header("Authorization", &format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(api_error(&resp).await);
    }

    resp.binary().await.map_err(|e| e.to_string())
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
        return Err(api_error(&resp).await);
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
        return Err(api_error(&resp).await);
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
        return Err(api_error(&resp).await);
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
        return Err(api_error(&resp).await);
    }

    resp.json::<DeleteDeviceResponse>()
        .await
        .map_err(|e| e.to_string())
}

use crate::models::HistoryEntry;
use serde::Deserialize;

// ── Rules API ─────────────────────────────────────────────────────────────────
// UI terminology: "rule". Wire path: /api/v1/automations.

use crate::models::Rule;

pub async fn fetch_rules(token: &str) -> Result<Vec<Rule>, String> {
    get_json("/automations", token).await
}

pub async fn fetch_rule(token: &str, id: &str) -> Result<Rule, String> {
    get_json(&format!("/automations/{id}"), token).await
}

pub async fn create_rule(token: &str, rule: &Rule) -> Result<Rule, String> {
    let body = serde_json::to_value(rule).map_err(|e| e.to_string())?;
    post_json("/automations", token, &body).await
}

pub async fn update_rule(token: &str, id: &str, rule: &Rule) -> Result<Rule, String> {
    let body = serde_json::to_value(rule).map_err(|e| e.to_string())?;
    put_json(&format!("/automations/{id}"), token, &body).await
}

pub async fn patch_rule(token: &str, id: &str, body: &Value) -> Result<Value, String> {
    let resp = Request::patch(&format!("{API_BASE}/automations/{id}"))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(api_error(&resp).await);
    }

    resp.json::<Value>().await.map_err(|e| e.to_string())
}

pub async fn delete_rule(token: &str, id: &str) -> Result<(), String> {
    delete_no_body(&format!("/automations/{id}"), token).await
}

pub async fn clone_rule(token: &str, id: &str) -> Result<Rule, String> {
    post_json(&format!("/automations/{id}/clone"), token, &Value::Null).await
}

pub async fn test_rule(token: &str, id: &str) -> Result<Value, String> {
    post_json(&format!("/automations/{id}/test"), token, &Value::Null).await
}

pub async fn rule_fire_history(token: &str, id: &str) -> Result<Value, String> {
    get_json(&format!("/automations/{id}/history"), token).await
}

pub async fn rule_stale_refs(token: &str) -> Result<Value, String> {
    get_json("/automations/stale-refs", token).await
}

// ── Glue Devices API ─────────────────────────────────────────────────────────

pub async fn fetch_glue(token: &str) -> Result<Vec<Value>, String> {
    get_json("/glue", token).await
}

pub async fn create_glue(token: &str, body: &Value) -> Result<Value, String> {
    post_json("/glue", token, body).await
}

pub async fn delete_glue(token: &str, id: &str) -> Result<(), String> {
    delete_no_body(&format!("/glue/{id}"), token).await
}

pub async fn fetch_glue_device(token: &str, id: &str) -> Result<Value, String> {
    get_json(&format!("/devices/{id}"), token).await
}

pub async fn send_glue_command(token: &str, id: &str, body: &Value) -> Result<(), String> {
    patch_json(&format!("/devices/{id}/state"), token, body).await
}

// ── Plugins API ─────────────────────────────────────────────────────────────

pub async fn fetch_plugins(token: &str) -> Result<Vec<crate::models::PluginInfo>, String> {
    get_json("/plugins", token).await
}

pub async fn fetch_plugin(token: &str, id: &str) -> Result<crate::models::PluginInfo, String> {
    get_json(&format!("/plugins/{id}"), token).await
}

pub async fn start_plugin(token: &str, id: &str) -> Result<(), String> {
    post_no_body(&format!("/plugins/{id}/start"), token).await
}

pub async fn stop_plugin(token: &str, id: &str) -> Result<(), String> {
    post_no_body(&format!("/plugins/{id}/stop"), token).await
}

pub async fn restart_plugin(token: &str, id: &str) -> Result<(), String> {
    post_no_body(&format!("/plugins/{id}/restart"), token).await
}

pub async fn patch_plugin(token: &str, id: &str, body: &serde_json::Value) -> Result<(), String> {
    patch_json(&format!("/plugins/{id}"), token, body).await
}

pub async fn fetch_plugin_config(token: &str, id: &str) -> Result<serde_json::Value, String> {
    get_json(&format!("/plugins/{id}/config"), token).await
}

pub async fn update_plugin_config(token: &str, id: &str, body: &serde_json::Value) -> Result<(), String> {
    put_json(&format!("/plugins/{id}/config"), token, body).await.map(|_: serde_json::Value| ())
}

// ── Events API ───────────────────────────────────────────────────────────────

pub async fn fetch_events(token: &str, limit: u32) -> Result<Vec<Value>, String> {
    get_json(&format!("/events?limit={limit}"), token).await
}

// ── Admin: Users API ────────────────────────────────────────────────────────

use crate::models::{SystemStatus, UserInfo};

pub async fn fetch_users(token: &str) -> Result<Vec<UserInfo>, String> {
    get_json("/auth/users", token).await
}

pub async fn create_user(
    token: &str,
    username: &str,
    password: &str,
    role: &str,
) -> Result<UserInfo, String> {
    post_json(
        "/auth/users",
        token,
        &serde_json::json!({ "username": username, "password": password, "role": role }),
    )
    .await
}

pub async fn delete_user(token: &str, id: &str) -> Result<(), String> {
    delete_no_body(&format!("/auth/users/{id}"), token).await
}

pub async fn set_user_role(token: &str, id: &str, role: &str) -> Result<UserInfo, String> {
    patch_json_with_response(
        &format!("/auth/users/{id}/role"),
        token,
        &serde_json::json!({ "role": role }),
    )
    .await
}

pub async fn change_password(
    token: &str,
    current: &str,
    new_pass: &str,
) -> Result<(), String> {
    post_json_no_response(
        "/auth/change-password",
        token,
        &serde_json::json!({ "current_password": current, "new_password": new_pass }),
    )
    .await
}

// ── Admin: System API ───────────────────────────────────────────────────────

pub async fn fetch_system_status(token: &str) -> Result<SystemStatus, String> {
    get_json("/system/status", token).await
}

pub async fn get_log_level(token: &str) -> Result<Value, String> {
    get_json("/system/log-level", token).await
}

pub async fn set_log_level(token: &str, level: &str) -> Result<(), String> {
    put_json_no_response(
        "/system/log-level",
        token,
        &serde_json::json!({ "level": level }),
    )
    .await
}

pub async fn trigger_backup(token: &str) -> Result<Vec<u8>, String> {
    post_binary("/system/backup", token).await
}

pub async fn fetch_me(token: &str) -> Result<Value, String> {
    get_json("/auth/me", token).await
}

pub async fn fetch_stale_refs(token: &str) -> Result<Vec<Value>, String> {
    get_json("/automations/stale-refs", token).await
}

pub async fn bulk_delete_devices(token: &str, ids: &[String]) -> Result<Value, String> {
    let body = serde_json::json!({ "ids": ids });
    let url = format!("{}/devices", API_BASE);
    let resp = Request::delete(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&body).unwrap())
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json().await.map_err(|e| e.to_string())
}
