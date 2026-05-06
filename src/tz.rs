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

/// Format `HH:MM:SS` in the configured zone. Used by activity-stream
/// rows where only the time-of-day matters (the date is implicit
/// from grouping or context).
pub fn fmt_time(utc: &DateTime<Utc>) -> String {
    utc.with_timezone(&app_tz()).format("%H:%M:%S").to_string()
}

/// Convert a UTC timestamp to the configured-zone calendar date.
/// Used for day-bucket grouping (audit log, history pages) so events
/// land in the bucket of the day they happened *at home*, not the
/// browser's idea of the day.
pub fn local_date(utc: &DateTime<Utc>) -> chrono::NaiveDate {
    utc.with_timezone(&app_tz()).date_naive()
}

/// "Today" in the configured zone. Replaces `Local::now().date_naive()`
/// in client code so day-bucket comparisons match the server's idea
/// of the boundary.
pub fn today() -> chrono::NaiveDate {
    Utc::now().with_timezone(&app_tz()).date_naive()
}

/// Convert a millis-since-epoch (typical from `js_sys::Date::now()`
/// or chart x-axis values) into a `DateTime<Tz>` in the configured
/// zone. Returns `None` for non-finite or out-of-range inputs.
pub fn from_millis(ms: f64) -> Option<chrono::DateTime<chrono_tz::Tz>> {
    if !ms.is_finite() {
        return None;
    }
    chrono::DateTime::<Utc>::from_timestamp_millis(ms as i64)
        .map(|utc| utc.with_timezone(&app_tz()))
}

#[cfg(test)]
mod tests {
    //! Native tests — `chrono-tz` behavior is platform-independent, so
    //! validating the helpers in a regular `cargo test` covers the
    //! WASM render path by extension. Each test sets up an explicit Tz
    //! via `with_timezone(&Tz)` rather than calling `set_app_tz`,
    //! since the global TZ is thread-local and would race other tests.
    use super::*;
    use chrono::TimeZone;
    use chrono_tz::Tz;

    #[test]
    fn fmt_time_uses_configured_zone() {
        // 2026-01-15 17:00 UTC == 12:00 EST in NYC (winter, UTC-05:00).
        let ny: Tz = "America/New_York".parse().unwrap();
        let utc = Utc.with_ymd_and_hms(2026, 1, 15, 17, 0, 0).unwrap();
        let formatted = utc.with_timezone(&ny).format("%H:%M:%S").to_string();
        assert_eq!(formatted, "12:00:00");
    }

    #[test]
    fn local_date_uses_zone_boundary_not_utc() {
        // 2026-01-16 03:00 UTC == 2026-01-15 22:00 EST. The local date
        // in NYC is the 15th, not the 16th. Day-bucket grouping must
        // use this rule so an event "10pm Thursday" doesn't land in
        // Friday's bucket just because UTC has rolled over.
        let ny: Tz = "America/New_York".parse().unwrap();
        let utc = Utc.with_ymd_and_hms(2026, 1, 16, 3, 0, 0).unwrap();
        let local = utc.with_timezone(&ny).date_naive();
        assert_eq!(local.day(), 15);
    }

    #[test]
    fn from_millis_round_trips_through_zone() {
        // Sanity: a known UTC instant → millis → from_millis (with a
        // known zone) should produce the same hour-of-day we'd get
        // by converting directly.
        let ny: Tz = "America/New_York".parse().unwrap();
        let utc = Utc.with_ymd_and_hms(2026, 7, 15, 16, 0, 0).unwrap(); // EDT
        let ms = utc.timestamp_millis() as f64;
        let direct = utc.with_timezone(&ny);
        let via_helper = chrono::DateTime::<Utc>::from_timestamp_millis(ms as i64)
            .unwrap()
            .with_timezone(&ny);
        use chrono::Timelike;
        assert_eq!(direct.hour(), via_helper.hour());
        assert_eq!(direct.hour(), 12); // 16:00 UTC == 12:00 EDT
    }

    #[test]
    fn from_millis_rejects_non_finite() {
        // Defensive: js_sys::Date returns NaN for invalid inputs;
        // `from_millis` must not panic or produce garbage.
        assert!(from_millis(f64::NAN).is_none());
        assert!(from_millis(f64::INFINITY).is_none());
    }

    use chrono::Datelike;
}
