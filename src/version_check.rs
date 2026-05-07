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

    /// True when the server is on a strictly newer version than this
    /// tab. Returns false when versions match, when the server is older
    /// (mid-deploy or rolled-back state), or when either side's
    /// version string can't be parsed as `X.Y.Z`.
    ///
    /// The asymmetry is deliberate. The banner exists to nudge the
    /// operator to reload after a server upgrade — that's a "click
    /// Reload to pick up new code" affordance. When the tab is ahead
    /// of the server (this can happen briefly during a multi-step
    /// release ceremony, or if the operator manually downgrades the
    /// server), Reload would just refetch the same WASM — the banner
    /// can't actually resolve the mismatch, and showing it just
    /// confuses the operator. CLIENT-VER-1-BUG-1 was the operator
    /// report that surfaced this gap.
    pub fn server_is_newer(&self) -> bool {
        let Some(sv) = self.server_version.get() else {
            return false;
        };
        let Some(server) = parse_simple_version(&sv) else {
            return false;
        };
        let Some(client) = parse_simple_version(CLIENT_VERSION) else {
            return false;
        };
        server > client
    }

    /// True when the banner should currently render.
    pub fn show_banner(&self) -> bool {
        self.server_is_newer() && !self.mismatch_dismissed.get()
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

// ── Version parsing ──────────────────────────────────────────────────────────

/// Parse `X.Y.Z` into a comparable tuple. Returns `None` for any input
/// that doesn't have exactly three dot-separated unsigned integer
/// components — that includes pre-release suffixes (`0.1.4-rc.1`),
/// build-metadata suffixes (`0.1.4+sha`), and dev-tags
/// (`dev-abc123`). The conservative behaviour for unparseable input
/// is "don't fire the banner" — better silence than a misleading
/// nudge. If the homeCore project ever ships pre-release versions,
/// this is the function to upgrade to a real semver crate.
fn parse_simple_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
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
                    "homeCore was upgraded — server is on "
                    <code>{move || format!("v{}", state.server_version.get().unwrap_or_default())}</code>
                    ", this tab is still on "
                    <code>{format!("v{}", CLIENT_VERSION)}</code>
                    ". Reload to pick up the new client."
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

#[cfg(test)]
mod tests {
    use super::parse_simple_version;

    #[test]
    fn parses_valid_xyz() {
        assert_eq!(parse_simple_version("0.1.4"), Some((0, 1, 4)));
        assert_eq!(parse_simple_version("1.0.0"), Some((1, 0, 0)));
        assert_eq!(parse_simple_version("12.34.56"), Some((12, 34, 56)));
    }

    #[test]
    fn rejects_pre_release_and_build_metadata() {
        // CLIENT-VER-1-BUG-1: parser returns None for any non-X.Y.Z,
        // so the banner doesn't fire on parseable-but-unusual inputs.
        assert_eq!(parse_simple_version("0.1.4-rc.1"), None);
        assert_eq!(parse_simple_version("0.1.4+sha.abc"), None);
        assert_eq!(parse_simple_version("dev-abc123"), None);
        assert_eq!(parse_simple_version("0.1"), None);
        assert_eq!(parse_simple_version("0.1.4.5"), None);
        assert_eq!(parse_simple_version(""), None);
        assert_eq!(parse_simple_version("0.1.x"), None);
    }

    #[test]
    fn comparison_orders_by_major_minor_patch() {
        // Tuple comparison gives the right ordering for our X.Y.Z.
        let v013 = parse_simple_version("0.1.3").unwrap();
        let v014 = parse_simple_version("0.1.4").unwrap();
        let v020 = parse_simple_version("0.2.0").unwrap();
        let v100 = parse_simple_version("1.0.0").unwrap();
        assert!(v013 < v014);
        assert!(v014 < v020);
        assert!(v020 < v100);
        assert!(v013 < v100);
        // Same version isn't "newer".
        assert!(!(v014 > v014));
    }
}
