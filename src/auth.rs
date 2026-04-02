//! Auth state — JWT token in localStorage + login/logout helpers.
//!
//! `AuthState` is provided via Leptos context at the root and consumed with
//! `use_auth()` anywhere in the tree.  It is `Clone + Copy` (RwSignal is
//! Copy) so it can be captured freely in closures.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

pub const API_BASE: &str = "/api/v1";
const TOKEN_KEY: &str = "hc-leptos:token";
const CLIENT_ID_KEY: &str = "hc-leptos:tab-id";

// ── User type ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HcUser {
    pub username: String,
    #[serde(default)]
    pub role: String,
}

// ── Auth state ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct AuthState {
    pub token: RwSignal<Option<String>>,
    pub user:  RwSignal<Option<HcUser>>,
    #[allow(dead_code)]
    pub ready: RwSignal<bool>,
}

impl AuthState {
    pub fn new() -> Self {
        let token = RwSignal::new(ls_get(TOKEN_KEY));
        let user  = RwSignal::new(None::<HcUser>);
        let ready = RwSignal::new(true); // token read synchronously; user loaded lazily
        Self { token, user, ready }
    }

    pub fn token_str(&self) -> Option<String> {
        self.token.get()
    }

    pub fn is_authenticated(&self) -> bool {
        self.token.get().is_some()
    }

    pub fn set_token(&self, tok: String) {
        ls_set(TOKEN_KEY, &tok);
        self.token.set(Some(tok));
    }

    pub fn logout(&self) {
        ls_remove(TOKEN_KEY);
        self.token.set(None);
        self.user.set(None);
    }
}

/// Retrieve the `AuthState` from Leptos context.
pub fn use_auth() -> AuthState {
    use_context::<AuthState>().expect("AuthState not in context — wrap with <AuthProvider>")
}

// ── Login API call ────────────────────────────────────────────────────────────

pub async fn api_login(username: &str, password: &str) -> Result<String, String> {
    use gloo_net::http::Request;

    let body = serde_json::json!({ "username": username, "password": password });

    let resp = Request::post(&format!("{API_BASE}/auth/login"))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| resp.status().to_string());
        return Err(format!("Login failed ({msg})"));
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    data["token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "No token in response".to_string())
}

// ── localStorage helpers ──────────────────────────────────────────────────────

fn ls_get(key: &str) -> Option<String> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(key).ok().flatten())
}

fn ls_set(key: &str, val: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(key, val);
    }
}

fn ls_remove(key: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.remove_item(key);
    }
}

fn ss_get(key: &str) -> Option<String> {
    web_sys::window()
        .and_then(|w| w.session_storage().ok().flatten())
        .and_then(|s| s.get_item(key).ok().flatten())
}

fn ss_set(key: &str, val: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.session_storage().ok().flatten()) {
        let _ = s.set_item(key, val);
    }
}

fn stable_client_id() -> String {
    if let Some(existing) = ss_get(CLIENT_ID_KEY).filter(|value| !value.trim().is_empty()) {
        return existing;
    }

    let seed = format!(
        "hc-web-leptos-tab-{}-{}",
        js_sys::Date::now() as u64,
        (js_sys::Math::random() * 1_000_000_000.0) as u64
    );
    ss_set(CLIENT_ID_KEY, &seed);
    seed
}

// ── WebSocket URL helper ──────────────────────────────────────────────────────

/// Build the WebSocket URL for the HomeCore events stream.
/// Appends ?token=<jwt> so the server can authenticate the upgrade.
pub fn events_ws_url(token: &str) -> String {
    let location = web_sys::window()
        .and_then(|w| w.location().href().ok())
        .unwrap_or_default();

    let protocol = if location.starts_with("https") { "wss" } else { "ws" };

    // When running through Trunk proxy, the host is localhost
    let host = web_sys::window()
        .and_then(|w| w.location().host().ok())
        .unwrap_or_else(|| "localhost:8080".to_string());
    let client_id = stable_client_id();

    format!("{protocol}://{host}/api/v1/events/stream?token={token}&client_id={client_id}")
}
