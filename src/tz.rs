//! Configured-timezone helpers for the Leptos client.
//!
//! Mirrors the server-side `hc-time` crate: storage-on-the-wire stays
//! UTC RFC-3339, the UI converts to the **server's configured** zone
//! when rendering. We use the server's zone (not the browser's) so
//! the operator sees the same wall-clock times the server schedules
//! against — important when the user is on a phone in a different
//! travel zone but cares about what's happening at home.
//!
//! Populated once at app boot from `GET /system/status`. Defaults to
//! UTC if `set_app_tz` has not been called, matching `hc-time`'s
//! conservative default.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use std::cell::Cell;

thread_local! {
    /// Per-tab app TZ. Cell because the WASM client is single-threaded
    /// and Leptos signals re-render synchronously off the main loop —
    /// a Mutex would be overhead for no benefit.
    static APP_TZ: Cell<Tz> = const { Cell::new(Tz::UTC) };
}

/// Set the app-wide configured TZ. Call once after `fetch_system_status`
/// returns the server's configured zone. Bad zone names are silently
/// dropped (UTC fallback) — the server already validated this on its
/// side; if the client somehow receives garbage, we don't want the UI
/// to crash.
pub fn set_app_tz(name: &str) {
    if let Ok(tz) = name.parse::<Tz>() {
        APP_TZ.with(|c| c.set(tz));
    }
}

/// Read the current configured TZ. Defaults to UTC.
pub fn app_tz() -> Tz {
    APP_TZ.with(|c| c.get())
}

/// Convert a UTC timestamp to the configured zone and format as
/// `YYYY-MM-DD HH:MM`. The shape that `format_abs` used to render
/// from raw UTC — but now actually localized.
pub fn fmt_abs(utc: &DateTime<Utc>) -> String {
    utc.with_timezone(&app_tz())
        .format("%Y-%m-%d %H:%M")
        .to_string()
}
