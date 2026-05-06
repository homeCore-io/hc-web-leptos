//! Periodic version-skew check between this WASM bundle and the homeCore
//! server it talks to.
//!
//! When operators deploy a new homeCore, existing browser tabs keep running
//! the WASM they loaded earlier — `Ctrl+Shift+R` only reloads the active
//! tab. Stale tabs miss new features and may exhibit subtle bugs (this is
//! how the WS-3 reconnect storm went undiagnosed for so long). This module
//! surfaces the mismatch via a top-of-viewport banner with a "Reload"
//! button.
//!
//! Probe sources:
//! - `/api/v1/health` — unauthenticated, so the check works on the login
//!   page and after a token has expired.
//!
//! When we re-check:
//! - Once at App boot.
//! - Every time the shared WS transitions to `Live`. WS reconnects often
//!   coincide with server restarts (i.e. deploys), so this catches the
//!   common case quickly. We don't bother with a setInterval because
//!   real-life deploys always either bounce the server (→ WS reconnect)
//!   or aren't relevant to long-idle tabs.
//!
//! Dismissal scope is per-server-version + per-tab (sessionStorage).
//! When the server moves to a new version the dismissal is naturally
//! invalidated and the banner re-appears.
//!
//! Comparison is currently strict equality of `core` from `/health` against
//! `env!("CARGO_PKG_VERSION")`. Per the component-versioning shift planned
//! for 0.1.4+, hc-web-leptos and core may diverge intentionally; when that
//! happens this check needs a compatibility-window predicate instead of
//! equality. For 0.1.3's lockstep-cohort releases, equality is right.

use crate::ws::{use_ws, WsStatus};
use leptos::prelude::*;
use serde::Deserialize;

/// Compiled-in version of this crate. Updates automatically when
/// `Cargo.toml`'s `[package].version` is bumped.
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// sessionStorage key prefix used to remember per-version dismissals.
const DISMISSED_KEY_PREFIX: &str = "hc-leptos:dismissed_version_alert:";

// ── State ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct VersionState {
    /// Server version reported by `/api/v1/health`. None until first probe
    /// resolves.
    pub server_version: RwSignal<Option<String>>,
    /// True when the user has clicked the dismiss button on the banner for
    /// the current `server_version`.
    pub mismatch_dismissed: RwSignal<bool>,
}

impl VersionState {
    pub fn new() -> Self {
        Self {
            server_version: RwSignal::new(None),
            mismatch_dismissed: RwSignal::new(false),
        }
    }

    /// True when we have a server version and it differs from the client.
    pub fn has_mismatch(&self) -> bool {
        self.server_version
            .get()
            .map(|sv| sv != CLIENT_VERSION)
            .unwrap_or(false)
    }

    /// True when the banner should currently render.
    pub fn show_banner(&self) -> bool {
        self.has_mismatch() && !self.mismatch_dismissed.get()
    }

    /// Persist a dismissal for the current server version and hide the
    /// banner. A future server-version change will surface the banner
    /// again because the sessionStorage key includes the version.
    pub fn dismiss(&self) {
        if let Some(sv) = self.server_version.get_untracked() {
            ss_set(&format!("{DISMISSED_KEY_PREFIX}{sv}"), "1");
        }
        self.mismatch_dismissed.set(true);
    }

    /// Check sessionStorage to decide whether the banner is dismissed for
    /// the version we just observed.
    fn restore_dismissal(&self, server_version: &str) {
        let dismissed = ss_get(&format!("{DISMISSED_KEY_PREFIX}{server_version}")).is_some();
        self.mismatch_dismissed.set(dismissed);
    }
}

// ── Probe ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct HealthResponse {
    version: String,
}

async fn fetch_server_version() -> Option<String> {
    use gloo_net::http::Request;
    let resp = Request::get("/api/v1/health").send().await.ok()?;
    if !resp.ok() {
        return None;
    }
    resp.json::<HealthResponse>().await.ok().map(|h| h.version)
}

async fn refresh(state: VersionState) {
    if let Some(v) = fetch_server_version().await {
        state.restore_dismissal(&v);
        state.server_version.set(Some(v));
    }
}

/// Mount the version-check Effects. Call once from `App` after the
/// `WsContext` has been provided.
pub fn mount_version_check(state: VersionState) {
    let ws = use_ws();

    // Initial probe at app boot — fires regardless of auth state so the
    // banner can warn an operator on /login that the tab is stale.
    Effect::new(move |_| {
        leptos::task::spawn_local(async move { refresh(state).await });
    });

    // Re-probe each time the WS transitions to Live. WS reconnects after a
    // server restart give us a free signal that "the deployment may have
    // changed" without polling.
    Effect::new(move |_| {
        if ws.status.get() != WsStatus::Live {
            return;
        }
        leptos::task::spawn_local(async move { refresh(state).await });
    });
}

// ── Banner ───────────────────────────────────────────────────────────────────

#[component]
pub fn VersionBanner() -> impl IntoView {
    let state: VersionState = expect_context();

    view! {
        <Show when=move || state.show_banner()>
            <div class="version-banner" role="alert">
                <i class="ph ph-arrows-clockwise version-banner__icon"></i>
                <span class="version-banner__text">
                    "New homeCore version available "
                    <code>{move || format!("v{}", state.server_version.get().unwrap_or_default())}</code>
                    " — this tab is on "
                    <code>{format!("v{}", CLIENT_VERSION)}</code>
                    ". Reload to update."
                </span>
                <button
                    class="version-banner__action"
                    on:click=move |_| {
                        if let Some(w) = web_sys::window() {
                            let _ = w.location().reload();
                        }
                    }
                >
                    "Reload"
                </button>
                <button
                    class="version-banner__dismiss"
                    title="Dismiss for this session"
                    on:click=move |_| state.dismiss()
                >
                    "×"
                </button>
            </div>
        </Show>
    }
}

// ── sessionStorage helpers ───────────────────────────────────────────────────

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
