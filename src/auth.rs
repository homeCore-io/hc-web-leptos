//! Auth state — JWT token in localStorage + login/logout helpers.
//!
//! `AuthState` is provided via Leptos context at the root and consumed with
//! `use_auth()` anywhere in the tree.  It is `Clone + Copy` (RwSignal is
//! Copy) so it can be captured freely in closures.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use wasm_bindgen::JsCast;

pub const API_BASE: &str = "/api/v1";
const TOKEN_KEY: &str = "hc-leptos:token";
/// Per-tab fingerprint, persisted in `sessionStorage`. Survives reloads,
/// dies with the tab. Surfaces in core's WS connect/disconnect logs as
/// `client_id=<value>` so reconnect storms can be correlated to a tab.
const CLIENT_ID_KEY: &str = "hc-leptos:client_id";

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
    pub user: RwSignal<Option<HcUser>>,
    #[allow(dead_code)]
    pub ready: RwSignal<bool>,
}

impl AuthState {
    pub fn new() -> Self {
        // Drop any stored token that's already expired so the AuthGuard
        // redirects to /login silently instead of showing a "session expired"
        // toast after the first 401 from an API call.
        let stored = ls_get(TOKEN_KEY).filter(|t| {
            if jwt_is_expired(t) {
                ls_remove(TOKEN_KEY);
                false
            } else {
                true
            }
        });
        let token = RwSignal::new(stored);
        let user = RwSignal::new(None::<HcUser>);
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

    /// True when a token is stored and its `exp` claim is in the past.
    /// False when there is no token (nothing to expire) or it's still valid.
    pub fn is_session_expired(&self) -> bool {
        self.token
            .get_untracked()
            .map(|t| jwt_is_expired(&t))
            .unwrap_or(false)
    }
}

/// Retrieve the `AuthState` from Leptos context.
pub fn use_auth() -> AuthState {
    use_context::<AuthState>().expect("AuthState not in context — wrap with <AuthProvider>")
}

// ── Out-of-context AuthState handle ───────────────────────────────────────────
//
// `api.rs` reaches `handle_session_expiry` from inside `spawn_local` blocks,
// which detach the reactive owner — `use_context::<AuthState>()` returns
// `None` there. To reliably trigger a logout on 401, we stash the AuthState
// in a thread-local cell at App init and read it back from any thread of
// control. AuthState is `Copy`, so this is just a couple of pointers.

thread_local! {
    static AUTH_HANDLE: Cell<Option<AuthState>> = const { Cell::new(None) };
}

/// Install the global AuthState handle. Call once from `App` after creating
/// the AuthState. Subsequent calls overwrite (only meaningful in tests).
pub fn install_auth_handle(auth: AuthState) {
    AUTH_HANDLE.with(|c| c.set(Some(auth)));
}

/// Read the global AuthState handle. Returns `None` only if `App` did not
/// install it — code paths that depend on this should fall back gracefully.
pub fn try_auth_handle() -> Option<AuthState> {
    AUTH_HANDLE.with(|c| c.get())
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

// ── JWT expiry check ──────────────────────────────────────────────────────────

/// Return `true` if the JWT's `exp` claim is in the past, or if the token
/// can't be decoded (safer to treat as expired than to trust it).
///
/// Doesn't verify the signature — that's the server's job.  We only look at
/// `exp` to decide whether to bother sending the token at all.
fn jwt_is_expired(token: &str) -> bool {
    let Some(payload_b64) = token.split('.').nth(1) else {
        return true;
    };
    let Some(json) = b64url_decode_to_string(payload_b64) else {
        return true;
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&json) else {
        return true;
    };
    let Some(exp) = val.get("exp").and_then(|v| v.as_f64()) else {
        // No exp claim — treat as non-expiring.
        return false;
    };
    let now_secs = js_sys::Date::now() / 1000.0;
    now_secs >= exp
}

/// Decode a base64url string (no padding, `-_` alphabet) using the browser's
/// built-in `atob`.  Returns `None` on any decode/UTF-8 error.
fn b64url_decode_to_string(input: &str) -> Option<String> {
    // Convert base64url → base64 standard and re-pad.
    let mut s = input.replace('-', "+").replace('_', "/");
    while s.len() % 4 != 0 {
        s.push('=');
    }
    let decoded = js_sys::global()
        .dyn_into::<web_sys::Window>()
        .ok()?
        .atob(&s)
        .ok()?;
    // `atob` returns a binary string (each char is a byte 0..=255).  JWT
    // payloads are UTF-8 JSON and typically ASCII, so this is sufficient.
    Some(decoded)
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

// ── sessionStorage helpers (per-tab, dies with the tab) ───────────────────────

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

/// Stable per-tab fingerprint. Generated lazily on first call and stashed
/// in `sessionStorage`; reloads of the same tab return the same value.
pub fn client_id() -> String {
    if let Some(existing) = ss_get(CLIENT_ID_KEY) {
        return existing;
    }
    let id = uuid::Uuid::new_v4().to_string();
    ss_set(CLIENT_ID_KEY, &id);
    id
}

// ── WebSocket URL helper ──────────────────────────────────────────────────────

/// Build the WebSocket URL for the HomeCore events stream.
/// Appends ?token=<jwt> so the server can authenticate the upgrade,
/// and &client_id=<uuid> so server logs can correlate reconnect storms.
pub fn events_ws_url(token: &str) -> String {
    let location = web_sys::window()
        .and_then(|w| w.location().href().ok())
        .unwrap_or_default();

    let protocol = if location.starts_with("https") {
        "wss"
    } else {
        "ws"
    };

    // When running through Trunk proxy, the host is localhost
    let host = web_sys::window()
        .and_then(|w| w.location().host().ok())
        .unwrap_or_else(|| "localhost:8080".to_string());

    let cid = client_id();
    format!("{protocol}://{host}/api/v1/events/stream?token={token}&client_id={cid}")
}

/// Build the WebSocket URL for the HomeCore log stream.
pub fn logs_ws_url(token: &str, history: u32) -> String {
    let location = web_sys::window()
        .and_then(|w| w.location().href().ok())
        .unwrap_or_default();
    let protocol = if location.starts_with("https") { "wss" } else { "ws" };
    let host = web_sys::window()
        .and_then(|w| w.location().host().ok())
        .unwrap_or_else(|| "localhost:8080".to_string());

    let cid = client_id();
    format!("{protocol}://{host}/api/v1/logs/stream?token={token}&history={history}&level=info&client_id={cid}")
}

/// Build the SSE URL for a plugin streaming-action request. `EventSource`
/// can't set `Authorization`, so the token rides in `?token=`.
pub fn plugin_stream_sse_url(plugin_id: &str, request_id: &str, token: &str) -> String {
    let host = web_sys::window()
        .and_then(|w| w.location().host().ok())
        .unwrap_or_else(|| "localhost:8080".to_string());
    let scheme = web_sys::window()
        .and_then(|w| w.location().protocol().ok())
        .unwrap_or_else(|| "http:".to_string());
    let enc_pid = js_sys::encode_uri_component(plugin_id);
    let enc_rid = js_sys::encode_uri_component(request_id);
    let enc_tok = js_sys::encode_uri_component(token);
    let enc_cid = js_sys::encode_uri_component(&client_id());
    format!(
        "{scheme}//{host}/api/v1/plugins/{enc_pid}/command/{enc_rid}/stream?token={enc_tok}&client_id={enc_cid}"
    )
}
