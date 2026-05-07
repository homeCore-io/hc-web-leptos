//! Device detail page — `/devices/:id`

use crate::api::{
    delete_device as delete_device_request, fetch_areas, fetch_device, fetch_device_history,
    fetch_device_schema, set_device_state, update_device_meta,
};
use crate::auth::use_auth;
use crate::models::*;
use crate::pages::shared::ErrorBanner;
use crate::ws::{use_ws, WsStatus};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

// ── TimerDisplay component ────────────────────────────────────────────────────
//
// Owns its own 1-second tick signal so only the countdown text node and
// progress bar re-render every second.  The rest of the page (edit form,
// controls, attributes table) is completely unaffected.
#[component]
fn TimerDisplay(
    /// Shared device signal — same one the rest of the page reads.
    device: RwSignal<Option<DeviceState>>,
) -> impl IntoView {
    // Private tick — never read outside this component.
    let tick = RwSignal::new(0u64);

    // Set up the 1-second interval once on mount; clean up on unmount.
    Effect::new(move |_| {
        let callback = Closure::<dyn FnMut()>::new(move || {
            tick.update(|t| *t += 1);
        });
        let handle = web_sys::window().and_then(|w| {
            w.set_interval_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                1000,
            )
            .ok()
        });
        callback.forget();
        on_cleanup(move || {
            if let (Some(w), Some(h)) = (web_sys::window(), handle) {
                w.clear_interval_with_handle(h);
            }
        });
    });

    // Remaining seconds — recomputes on every tick AND on every WS update.
    // timer_remaining_secs() derives the value from started_at + duration - now(),
    // so it's always accurate without needing a separate server push.
    let remaining: Memo<Option<u64>> = Memo::new(move |_| {
        let _ = tick.get(); // subscribe to tick
        device.get().as_ref().and_then(|d| timer_remaining_secs(d))
    });

    // Duration (static unless config changes — no need to tick).
    let dur: Memo<u64> = Memo::new(move |_| {
        device
            .get()
            .as_ref()
            .and_then(|d| {
                d.attributes
                    .get("duration_secs")
                    .and_then(|v| v.as_u64())
                    .or_else(|| {
                        d.attributes
                            .get("duration_ms")
                            .and_then(|v| v.as_u64())
                            .map(|ms| ms / 1000)
                    })
            })
            .unwrap_or(0)
    });

    // Progress percentage — updates every tick while running.
    let pct: Signal<u32> = Signal::derive(move || {
        let d = dur.get();
        let r = remaining.get().unwrap_or(0);
        if d == 0 {
            0u32
        } else {
            ((d.saturating_sub(r)) as f64 / d as f64 * 100.0).min(100.0) as u32
        }
    });

    // Show the countdown only when the timer is running or paused.
    // This Memo does NOT subscribe to tick — it only reacts to device state changes.
    let is_active: Memo<bool> = Memo::new(move |_| {
        device
            .get()
            .as_ref()
            .and_then(|d| str_attr(d.attributes.get("state")))
            .map(|s| s == "running" || s == "paused")
            .unwrap_or(false)
    });

    view! {
        {move || is_active.get().then(|| view! {
            // Only this span re-renders every second — nothing else on the page.
            <span class="timer-remaining">
                {move || {
                    remaining
                        .get()
                        .map(|r| format!("{} remaining", format_duration_secs(r)))
                        .unwrap_or_default()
                }}
            </span>
            // Progress bar width also updates every second via pct.
            <div class="timer-progress-track">
                <div
                    class="timer-progress-fill"
                    style=move || format!("width:{}%", pct.get())
                ></div>
            </div>
        })}
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistorySortKey {
    Time,
    Attribute,
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistorySortDir {
    Asc,
    Desc,
}

fn sync_edit_fields(
    device: &DeviceState,
    edit_name: RwSignal<String>,
    edit_area: RwSignal<String>,
    edit_canonical: RwSignal<String>,
    edit_icon: RwSignal<String>,
    edit_ui_hint: RwSignal<String>,
) {
    edit_name.set(device.name.clone());
    edit_area.set(device.area.clone().unwrap_or_default());
    edit_canonical.set(device.canonical_name.clone().unwrap_or_default());
    edit_icon.set(device.status_icon.clone().unwrap_or_default());
    edit_ui_hint.set(device.ui_hint.clone().unwrap_or_default());
}

/// Local checkbox bound to the security membership for one device.
/// Reads the combined include/exclude state via
/// `should_include_in_security` so the checkbox accurately reflects
/// whether the device is currently in the Security tile, including
/// the default-set fallback for locks + contact_sensors.
/// OVERVIEW-SECURITY-OPT-IN-1: previously this read only the explicit
/// tag set, so a lock or contact_sensor showed unchecked yet still
/// appeared in the tile via the default fallback — operators couldn't
/// uncheck them out.
#[component]
fn SecurityToggleField(device: DeviceState) -> impl IntoView {
    let is_security = RwSignal::new(should_include_in_security(&device));
    let device_for_toggle = device.clone();
    view! {
        <label style="display:flex; align-items:center; gap:0.5rem; font-weight:400;">
            <input
                type="checkbox"
                prop:checked=move || is_security.get()
                on:change=move |_| {
                    toggle_security_membership(&device_for_toggle);
                    is_security.update(|v| *v = !*v);
                }
            />
            <span>{move || if is_security.get() { "Counted in Security tile" } else { "Not counted" }}</span>
        </label>
    }
}

#[component]
pub fn DeviceDetailPage() -> impl IntoView {
    let auth = use_auth();
    let params = use_params_map();
    let device_id: String =
        params.with_untracked(|p| p.get("id").map(|s| s.clone()).unwrap_or_default());

    // ── Signals (all RwSignal = Copy) ─────────────────────────────────────────
    let device: RwSignal<Option<DeviceState>> = RwSignal::new(None);
    let history: RwSignal<Vec<HistoryEntry>> = RwSignal::new(vec![]);
    let areas: RwSignal<Vec<Area>> = RwSignal::new(vec![]);
    let loading = RwSignal::new(true);
    let hist_loading = RwSignal::new(true);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);
    let busy = RwSignal::new(false);
    let show_edit = RwSignal::new(false);
    let edit_name = RwSignal::new(String::new());
    let edit_area = RwSignal::new(String::new());
    let edit_canonical = RwSignal::new(String::new());
    let edit_icon = RwSignal::new(String::new());
    let edit_ui_hint = RwSignal::new(String::new());
    let delete_confirm = RwSignal::new(String::new());
    let timer_secs = RwSignal::new("60".to_string());
    let selected_favorite = RwSignal::new(String::new());
    let selected_playlist = RwSignal::new(String::new());
    let history_sort_by = RwSignal::new(HistorySortKey::Time);
    let history_sort_dir = RwSignal::new(HistorySortDir::Desc);

    // Trigger signals — increment to re-run the matching effect.
    // All are RwSignal (Copy), safe to capture in any closure.
    let refresh_trigger = RwSignal::new(0u32);
    let hist_trigger = RwSignal::new(0u32);
    let save_trigger = RwSignal::new(0u32);

    let auth_token = auth.token; // RwSignal<Option<String>> — Copy
    let ws = use_ws();

    Effect::new(move |_| {
        let Some(d) = device.get() else { return };

        let favorites = media_available_favorites(&d);
        selected_favorite.update(|current| {
            if favorites.is_empty() {
                current.clear();
            } else if current.is_empty() || !favorites.iter().any(|item| item == current) {
                *current = favorites[0].clone();
            }
        });

        let playlists = media_available_playlists(&d);
        selected_playlist.update(|current| {
            if playlists.is_empty() {
                current.clear();
            } else if current.is_empty() || !playlists.iter().any(|item| item == current) {
                *current = playlists[0].clone();
            }
        });
    });

    // ── Areas fetch ───────────────────────────────────────────────────────────
    Effect::new(move |_| {
        let token = auth_token.get().unwrap_or_default();
        spawn_local(async move {
            if let Ok(mut list) = fetch_areas(&token).await {
                list.sort_by(|a, b| sort_key_str(&a.name).cmp(&sort_key_str(&b.name)));
                areas.set(list);
            }
        });
    });

    // ── Device fetch ──────────────────────────────────────────────────────────
    // Also seeds the shared WsContext map so subsequent WS events update this
    // device even if the devices list page was never visited.
    let did1 = device_id.clone();
    Effect::new(move |_| {
        let _ = refresh_trigger.get();
        let token = auth_token.get().unwrap_or_default();
        let id = did1.clone();
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match fetch_device(&token, &id).await {
                Ok(d) => {
                    ws.devices.update(|m| {
                        m.insert(d.device_id.clone(), d.clone());
                    });
                    if device.get_untracked().is_none() || !show_edit.get_untracked() {
                        sync_edit_fields(
                            &d,
                            edit_name,
                            edit_area,
                            edit_canonical,
                            edit_icon,
                            edit_ui_hint,
                        );
                    }
                    device.set(Some(d));
                }
                Err(e) => error.set(Some(format!("Failed to load device: {e}"))),
            }
            loading.set(false);
        });
    });

    // ── Live updates from shared WsContext ────────────────────────────────────
    // Replaces the per-page WebSocket.  Reacts whenever WsContext.devices
    // changes for this device_id.  Does not touch edit fields while editing.
    let did_live = device_id.clone();
    Effect::new(move |_| {
        let Some(d) = ws.devices.get().get(&did_live).cloned() else {
            return;
        };
        device.update(|existing| {
            if let Some(existing) = existing {
                existing.attributes = d.attributes.clone();
                existing.available = d.available;
                existing.last_seen = d.last_seen;
                existing.last_change = d.last_change.clone();
            }
        });
    });

    // ── History fetch ─────────────────────────────────────────────────────────
    let did2 = device_id.clone();
    Effect::new(move |_| {
        let _ = hist_trigger.get();
        let token = auth_token.get().unwrap_or_default();
        let id = did2.clone();
        hist_loading.set(true);
        spawn_local(async move {
            match fetch_device_history(&token, &id, 25).await {
                Ok(h) => history.set(h),
                Err(_) => {}
            }
            hist_loading.set(false);
        });
    });

    // ── Save metadata ─────────────────────────────────────────────────────────
    let did3 = device_id.clone();
    Effect::new(move |_| {
        let n = save_trigger.get();
        if n == 0 {
            return;
        }
        let token = auth_token.get().unwrap_or_default();
        let id = did3.clone();
        let name_val = edit_name.get();
        let area_val = edit_area.get();
        let canonical_val = edit_canonical.get();
        let icon_val = edit_icon.get();
        let hint_val = edit_ui_hint.get();
        let body = serde_json::json!({
            "name": name_val.trim(),
            "area": if area_val.trim().is_empty() { serde_json::Value::Null } else { area_val.trim().into() },
            "canonical_name": if canonical_val.trim().is_empty() { serde_json::Value::Null } else { canonical_val.trim().into() },
            "status_icon": if icon_val.trim().is_empty() { serde_json::Value::Null } else { icon_val.trim().into() },
            "ui_hint": if hint_val.trim().is_empty() { serde_json::Value::Null } else { hint_val.trim().into() },
        });
        busy.set(true);
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match update_device_meta(&token, &id, &body).await {
                Ok(updated) => {
                    sync_edit_fields(
                        &updated,
                        edit_name,
                        edit_area,
                        edit_canonical,
                        edit_icon,
                        edit_ui_hint,
                    );
                    device.set(Some(updated));
                    notice.set(Some("Device updated.".into()));
                    show_edit.set(false);
                }
                Err(e) => error.set(Some(format!("Save failed: {e}"))),
            }
            busy.set(false);
        });
    });

    let sorted_history: Memo<Vec<HistoryEntry>> = Memo::new(move |_| {
        let mut entries = history.get();
        let sort_key = history_sort_by.get();
        let sort_dir = history_sort_dir.get();

        entries.sort_by(|a, b| {
            let cmp = match sort_key {
                HistorySortKey::Time => a.recorded_at.cmp(&b.recorded_at),
                HistorySortKey::Attribute => {
                    sort_key_str(&a.attribute).cmp(&sort_key_str(&b.attribute))
                }
                HistorySortKey::Value => {
                    sort_key_str(&a.value_display()).cmp(&sort_key_str(&b.value_display()))
                }
            };

            let cmp = if cmp == std::cmp::Ordering::Equal {
                a.recorded_at.cmp(&b.recorded_at)
            } else {
                cmp
            };

            if sort_dir == HistorySortDir::Desc {
                cmp.reverse()
            } else {
                cmp
            }
        });

        entries
    });

    // ── View ──────────────────────────────────────────────────────────────────
    view! {
        <div class="page device-detail-page">

            // Back link
            <div>
                <a href="/devices" class="back-link">
                    <i class="ph ph-arrow-left" style="font-size:18px;vertical-align:middle"></i>
                    " Devices"
                </a>
            </div>

            // Heading — reactive on device
            {move || device.get().map(|d| {
                let tone  = status_tone(&d);
                let icon  = status_icon_name(&d);
                let stext = status_text(&d);
                let avail = d.available;
                let name  = d.name.clone();
                let area  = d.area.as_deref().map(display_area_name);
                view! {
                    <div class="detail-title-row">
                        <span class=format!("status-badge-lg {}", tone.css_class())>
                            <i class={format!("ph ph-{}", icon)} style="font-size:26px"></i>
                        </span>
                        <div class="detail-name-block">
                            <h1>{name}</h1>
                            <div class="detail-meta-chips">
                                <span class:chip-online=avail class:chip-offline=!avail>
                                    {if avail { "Online" } else { "Offline" }}
                                </span>
                                <span class="chip-neutral">{stext}</span>
                                {area.map(|a| view! { <span class="chip-neutral">{a}</span> })}
                            </div>
                        </div>
                        <div class="detail-heading-actions">
                            <button
                                class="btn btn-outline"
                                on:click=move |_| show_edit.update(|v| *v = !*v)
                            >
                                <i class="ph ph-pencil-simple" style="font-size:16px;vertical-align:middle"></i>
                                {move || if show_edit.get() { " Close editor" } else { " Edit" }}
                            </button>
                        </div>
                    </div>
                }
            })}
            {move || (loading.get() && device.get().is_none()).then(move || view! {
                <p style="color:var(--hc-text-muted)">"Loading device…"</p>
            })}

            // Feedback
            {move || {
                let status = ws.status.get();
                (status != WsStatus::Live).then(|| {
                    let msg = match status {
                        WsStatus::Connecting => "Connecting to live updates…",
                        WsStatus::Disconnected => "Live updates lost — reconnecting…",
                        WsStatus::Live => unreachable!(),
                    };
                    view! { <p class="msg-warning">{msg}</p> }
                })
            }}
            <ErrorBanner error=error />
            {move || notice.get().map(|n| view! { <p class="msg-notice">{n}</p> })}

            // Main content
            {move || device.get().map(|d| {
                let id = d.device_id.clone();
                let is_timer  = d.plugin_id.starts_with("core.timer");
                let is_switch = d.plugin_id.starts_with("core.switch");
                let is_media  = is_media_player(&d);
                let is_thermostat = is_thermostat_device(&d);
                let has_on    = bool_attr(d.attributes.get("on")).is_some();
                let has_bri   = d.attributes.get("brightness_pct").and_then(|v| v.as_f64()).is_some();
                let has_ct    = d.attributes.get("color_temp").and_then(|v| v.as_f64()).is_some();
                let has_vol   = d.attributes.get("volume").and_then(|v| v.as_f64()).is_some();
                let has_lock  = bool_attr(d.attributes.get("locked")).is_some();
                let media_muted = bool_attr(d.attributes.get("muted"));
                let media_shuffle = bool_attr(d.attributes.get("shuffle"));
                let media_loudness = bool_attr(d.attributes.get("loudness"));
                let media_repeat = str_attr(d.attributes.get("repeat"))
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                let media_bass = d.attributes.get("bass").and_then(|v| v.as_i64());
                let media_treble = d.attributes.get("treble").and_then(|v| v.as_i64());
                let cur_on    = bool_attr(d.attributes.get("on")).unwrap_or(false);
                let cur_bri   = d.attributes.get("brightness_pct").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let cur_ct    = d.attributes.get("color_temp").and_then(|v| v.as_f64()).unwrap_or(2700.0);
                let cur_vol   = d.attributes.get("volume").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let cur_lock  = bool_attr(d.attributes.get("locked")).unwrap_or(false);
                let timer_state  = str_attr(d.attributes.get("state")).unwrap_or("idle").to_string();
                let pb_state     = playback_state(&d);
                let media_title_text = media_title(&d).map(str::to_string);
                let media_artist_text = media_artist(&d).map(str::to_string);
                let media_album_text = media_album(&d).map(str::to_string);
                let media_source_text = media_source(&d).map(str::to_string);
                let media_sum    = media_summary(&d);
                let media_img    = media_image_url(&d).map(str::to_string);
                let media_enrichments = media_ui_enrichments(&d);
                let media_favorites = media_available_favorites(&d);
                let media_playlists = media_available_playlists(&d);
                let last_changed = last_change_time(&d);
                let change_text  = change_summary(&d);
                let correlation  = change_correlation_id(&d).map(str::to_string);
                let area_options = areas.get();

                let mut attr_pairs: Vec<(String, String)> = d.attributes.iter().map(|(k, v)| {
                    let disp = match v {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Number(n) => {
                            if let Some(f) = n.as_f64() {
                                if f.fract() == 0.0 { format!("{}", f as i64) } else { format!("{f:.2}") }
                            } else { n.to_string() }
                        }
                        other => other.to_string(),
                    };
                    (k.clone(), disp)
                }).collect();
                attr_pairs.sort_by(|a, b| a.0.cmp(&b.0));

                let no_controls = !has_on && !is_switch && !has_bri && !has_ct
                    && !has_lock && !is_media && !is_timer;

                // ── id clones — one per on:click closure ─────────────────────
                let id_on    = id.clone();
                let id_off   = id.clone();
                let id_bri   = id.clone();
                let id_ct    = id.clone();
                let id_lock  = id.clone();
                let id_unlck = id.clone();
                let id_play  = id.clone();
                let id_play_favorite = id.clone();
                let id_play_playlist = id.clone();
                let id_pause = id.clone();
                let id_stop  = id.clone();
                let id_prev  = id.clone();
                let id_next  = id.clone();
                let id_vol   = id.clone();
                let id_mute  = id.clone();
                let id_shuffle = id.clone();
                let id_repeat = id.clone();
                let id_bass  = id.clone();
                let id_treble = id.clone();
                let id_loudness = id.clone();
                let id_tpaus = id.clone();
                let id_tcanc = id.clone();
                let id_tresu = id.clone();
                let id_start = id.clone();
                let ts_label = timer_state.clone();
                let delete_confirm_label = d.device_id.clone();
                let delete_confirm_target = d.device_id.clone();
                let delete_request_target = d.device_id.clone();
                let security_device = d.clone();

                view! {
                    // ── Edit form ─────────────────────────────────────────────
                    {move || show_edit.get().then({
                        let area_options = area_options.clone();
                        let delete_confirm_label = delete_confirm_label.clone();
                        let delete_confirm_target = delete_confirm_target.clone();
                        let delete_request_target = delete_request_target.clone();
                        let security_device = security_device.clone();
                        move || view! {
                        <div class="detail-card edit-card">
                            <h2 class="card-title">"Edit Device"</h2>
                            <div class="edit-grid">
                                <div class="edit-field">
                                    <label>"Display Name"</label>
                                    <input
                                        class="input"
                                        type="text"
                                        prop:value=move || edit_name.get()
                                        on:input=move |ev| edit_name.set(event_target_value(&ev))
                                        placeholder="Display name"
                                    />
                                </div>
                                <div class="edit-field">
                                    <label>"Area"</label>
                                    <select
                                        on:change=move |ev| edit_area.set(event_target_value(&ev))
                                    >
                                        <option value="" selected=move || edit_area.get().is_empty()>
                                            "Unassigned"
                                        </option>
                                        <For
                                            each=move || area_options.clone()
                                            key=|area| area.id.clone()
                                            children=move |area| {
                                                let selected_name = area.name.clone();
                                                let label = display_area_name(&area.name);
                                                view! {
                                                    <option
                                                        value=selected_name.clone()
                                                        selected=move || edit_area.get() == selected_name
                                                    >
                                                        {label}
                                                    </option>
                                                }
                                            }
                                        />
                                    </select>
                                    <span class="cell-subtle">
                                        "Areas come from HomeCore’s defined areas list."
                                    </span>
                                </div>
                                <div class="edit-field">
                                    <label>"Canonical Name"</label>
                                    <input
                                        class="input"
                                        type="text"
                                        prop:value=move || edit_canonical.get()
                                        on:input=move |ev| edit_canonical.set(event_target_value(&ev))
                                        placeholder="e.g. living_room.floor_lamp (blank to clear)"
                                    />
                                </div>
                                <div class="edit-field">
                                    <label>"Status Icon"</label>
                                    <input
                                        class="input"
                                        type="text"
                                        prop:value=move || edit_icon.get()
                                        on:input=move |ev| edit_icon.set(event_target_value(&ev))
                                        placeholder="e.g. power, lock (blank to clear)"
                                    />
                                </div>
                                <div class="edit-field">
                                    <label>"UI Hint"</label>
                                    <select
                                        prop:value=move || edit_ui_hint.get()
                                        on:change=move |ev| edit_ui_hint.set(event_target_value(&ev))
                                    >
                                        <option value="" selected=move || edit_ui_hint.get().is_empty()>"Auto-detect"</option>
                                        <option value="light">"Light"</option>
                                        <option value="dimmer">"Dimmer"</option>
                                        <option value="switch">"Switch"</option>
                                        <option value="lock">"Lock"</option>
                                        <option value="shade">"Shade / Blind"</option>
                                        <option value="door">"Door (contact sensor)"</option>
                                        <option value="window">"Window (contact sensor)"</option>
                                        <option value="garage">"Garage door"</option>
                                        <option value="gate">"Gate"</option>
                                        <option value="motion">"Motion sensor"</option>
                                        <option value="occupancy">"Occupancy sensor"</option>
                                        <option value="leak">"Leak sensor"</option>
                                        <option value="temperature">"Temperature sensor"</option>
                                        <option value="humidity">"Humidity sensor"</option>
                                        <option value="environment">"Environment (temp+humidity)"</option>
                                        <option value="media_player">"Media player"</option>
                                        <option value="keypad">"Keypad"</option>
                                        <option value="remote">"Remote"</option>
                                        <option value="sensor">"Generic sensor"</option>
                                    </select>
                                    <span class="cell-subtle">
                                        "Overrides auto-detection for icons and dashboard counters."
                                    </span>
                                </div>
                                <div class="edit-field">
                                    <label>"Security relevant"</label>
                                    <SecurityToggleField device=security_device.clone() />
                                    <span class="cell-subtle">
                                        "Locks and contact sensors are counted by default. Uncheck to exclude \
                                         interior doors / windows from the Overview Security tile. Check this \
                                         box on devices that aren't locks or contact sensors but you still \
                                         want counted (e.g. a motion sensor at the back gate)."
                                    </span>
                                </div>
                            </div>
                            <div class="edit-actions">
                                <button
                                    class="btn btn-primary"
                                    on:click=move |_| save_trigger.update(|n| *n += 1)
                                    disabled=move || busy.get() || edit_name.get().trim().is_empty()
                                >
                                    {move || if busy.get() { "Saving…" } else { "Save" }}
                                </button>
                                <button
                                    class="btn btn-outline"
                                    on:click=move |_| {
                                        if let Some(current) = device.get() {
                                            sync_edit_fields(
                                                &current,
                                                edit_name,
                                                edit_area,
                                                edit_canonical,
                                                edit_icon,
                                                edit_ui_hint,
                                            );
                                        }
                                        delete_confirm.set(String::new());
                                    }
                                >
                                    "Reset fields"
                                </button>
                                <button
                                    class="btn btn-outline"
                                    on:click=move |_| {
                                        if let Some(current) = device.get() {
                                            sync_edit_fields(
                                                &current,
                                                edit_name,
                                                edit_area,
                                                edit_canonical,
                                                edit_icon,
                                                edit_ui_hint,
                                            );
                                        }
                                        delete_confirm.set(String::new());
                                        show_edit.set(false);
                                    }
                                >
                                    "Cancel"
                                </button>
                            </div>

                            <div class="danger-zone">
                                <div class="danger-zone-copy">
                                    <h3>"Delete Device"</h3>
                                    <p>
                                        "This removes the device from HomeCore. Rule references are rewritten to deleted placeholders on the backend."
                                    </p>
                                </div>
                                <div class="danger-zone-controls">
                                    <div class="edit-field">
                                        <label>{format!("Type {} to confirm", delete_confirm_label)}</label>
                                        <input
                                            class="input"
                                            type="text"
                                            prop:value=move || delete_confirm.get()
                                            on:input=move |ev| delete_confirm.set(event_target_value(&ev))
                                            placeholder="Device ID confirmation"
                                        />
                                    </div>
                                    <button
                                        class="danger"
                                        disabled=move || busy.get() || delete_confirm.get().trim() != delete_confirm_target
                                        on:click=move |_| {
                                            let token = auth_token.get().unwrap_or_default();
                                            let did = delete_request_target.clone();
                                            busy.set(true);
                                            error.set(None);
                                            notice.set(None);
                                            spawn_local(async move {
                                                match delete_device_request(&token, &did).await {
                                                    Ok(resp) if resp.deleted => {
                                                        let rule_note = if resp.affected_rules.is_empty() {
                                                            String::new()
                                                        } else {
                                                            format!(" {} rules updated.", resp.affected_rules.len())
                                                        };
                                                        notice.set(Some(format!("Device deleted.{rule_note}")));
                                                        if let Some(win) = web_sys::window() {
                                                            let _ = win.location().set_href("/devices");
                                                        }
                                                    }
                                                    Ok(_) => {
                                                        error.set(Some("Delete did not complete.".into()));
                                                        busy.set(false);
                                                    }
                                                    Err(e) => {
                                                        error.set(Some(format!("Delete failed: {e}")));
                                                        busy.set(false);
                                                    }
                                                }
                                            });
                                        }
                                    >
                                        {move || if busy.get() { "Deleting…" } else { "Delete device" }}
                                    </button>
                                </div>
                            </div>
                        </div>
                    }})}

                    // ── Two-column info grid ──────────────────────────────────
                    <div class="detail-grid">

                        // Overview card
                        <div class="detail-card">
                            <h2 class="card-title">"Overview"</h2>
                            <table class="info-table">
                                <tbody>
                                <tr><td class="info-label">"Device ID"</td>
                                    <td><code class="mono">{d.device_id.clone()}</code></td></tr>
                                <tr><td class="info-label">"Plugin"</td>
                                    <td>{d.plugin_id.clone()}</td></tr>
                                <tr><td class="info-label">"Area"</td>
                                    <td>{display_area_value(d.area.as_deref())}</td></tr>
                                <tr><td class="info-label">"Canonical"</td>
                                    <td><code class="mono">{d.canonical_name.as_deref().unwrap_or("—").to_string()}</code></td></tr>
                                <tr><td class="info-label">"Type"</td>
                                    <td>
                                        <div class="cell-primary">
                                            <span>{presentation_device_type_label(&d).to_string()}</span>
                                            <span class="cell-subtle">
                                                {"Raw type: "}{raw_device_type_label(&d)}
                                            </span>
                                        </div>
                                    </td></tr>
                                <tr><td class="info-label">"Availability"</td>
                                    <td>
                                        <span class:chip-online=d.available class:chip-offline=!d.available>
                                            {if d.available { "Online" } else { "Offline" }}
                                        </span>
                                    </td></tr>
                                <tr><td class="info-label">"Last seen"</td>
                                    <td>
                                        <div class="cell-primary">
                                            <span>{format_relative(d.last_seen.as_ref())}</span>
                                            <span class="cell-subtle">{format_abs(d.last_seen.as_ref())}</span>
                                        </div>
                                    </td></tr>
                                <tr><td class="info-label">"Last changed"</td>
                                    <td>
                                        <div class="cell-primary">
                                            <span>{format_relative(last_changed)}</span>
                                            <span class="cell-subtle">{format_abs(last_changed)}</span>
                                        </div>
                                    </td></tr>
                                <tr><td class="info-label">"Change source"</td>
                                    <td>
                                        <div class="cell-primary">
                                            <span>{change_text}</span>
                                            {correlation.map(|id| view! {
                                                <span class="cell-subtle">
                                                    {"Correlation: "}<code class="mono">{id}</code>
                                                </span>
                                            })}
                                        </div>
                                    </td></tr>
                                </tbody>
                            </table>
                        </div>

                        // Thermostat-specific card (includes controls + diagnostics
                        // + config + chart), rendered in place of the generic Controls
                        // card when the device is a thermostat.
                        {is_thermostat.then(|| view! {
                            <crate::pages::thermostat_card::ThermostatCard device=d.clone() />
                        })}

                        // Generic Controls card (suppressed for thermostats)
                        {(!is_thermostat).then(|| view! {
                        <div class="detail-card">
                            <h2 class="card-title">"Controls"</h2>

                            // On/Off toggle
                            {(has_on || is_switch).then(|| view! {
                                <div class="control-row">
                                    <span class="control-label">"Power"</span>
                                    <div class="toggle-group">
                                        <button class:active=cur_on disabled=move || busy.get()
                                            on:click=move |_| {
                                                let token = auth_token.get().unwrap_or_default();
                                                let did = id_on.clone();
                                                busy.set(true); error.set(None);
                                                spawn_local(async move {
                                                    let _ = set_device_state(&token, &did, &serde_json::json!({"on":true})).await;
                                                    busy.set(false);
                                                });
                                            }>"On"</button>
                                        <button class:active=!cur_on disabled=move || busy.get()
                                            on:click=move |_| {
                                                let token = auth_token.get().unwrap_or_default();
                                                let did = id_off.clone();
                                                busy.set(true); error.set(None);
                                                spawn_local(async move {
                                                    let _ = set_device_state(&token, &did, &serde_json::json!({"on":false})).await;
                                                    busy.set(false);
                                                });
                                            }>"Off"</button>
                                    </div>
                                </div>
                            })}

                            // Brightness slider
                            {has_bri.then(|| view! {
                                <div class="control-row">
                                    <span class="control-label">"Brightness"</span>
                                    <div class="slider-row">
                                        <input type="range" min="0" max="100" step="1"
                                            prop:value=cur_bri as i64
                                            on:change=move |ev| {
                                                if let Some(el) = ev.target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                {
                                                    let val: f64 = el.value().parse().unwrap_or(0.0);
                                                    let token = auth_token.get().unwrap_or_default();
                                                    let did = id_bri.clone();
                                                    busy.set(true); error.set(None);
                                                    spawn_local(async move {
                                                        let _ = set_device_state(&token, &did,
                                                            &serde_json::json!({"brightness_pct": val})).await;
                                                        busy.set(false);
                                                    });
                                                }
                                            }
                                        />
                                        <span class="slider-value">{format!("{:.0}%", cur_bri)}</span>
                                    </div>
                                </div>
                            })}

                            // Color temperature slider
                            {has_ct.then(|| view! {
                                <div class="control-row">
                                    <span class="control-label">"Color Temp"</span>
                                    <div class="slider-row">
                                        <input type="range" min="2700" max="6500" step="50"
                                            prop:value=cur_ct as i64
                                            style="accent-color:#1565c0"
                                            on:change=move |ev| {
                                                if let Some(el) = ev.target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                {
                                                    let val: f64 = el.value().parse().unwrap_or(2700.0);
                                                    let token = auth_token.get().unwrap_or_default();
                                                    let did = id_ct.clone();
                                                    busy.set(true); error.set(None);
                                                    spawn_local(async move {
                                                        let _ = set_device_state(&token, &did,
                                                            &serde_json::json!({"color_temp": val})).await;
                                                        busy.set(false);
                                                    });
                                                }
                                            }
                                        />
                                        <span class="slider-value">{format!("{:.0}K", cur_ct)}</span>
                                    </div>
                                </div>
                            })}

                            // Lock / Unlock
                            {has_lock.then(|| view! {
                                <div class="control-row">
                                    <span class="control-label">"Lock"</span>
                                    <div class="btn-group">
                                        <button class:active=cur_lock disabled=move || busy.get()
                                            on:click=move |_| {
                                                let token = auth_token.get().unwrap_or_default();
                                                let did = id_lock.clone();
                                                busy.set(true); error.set(None);
                                                spawn_local(async move {
                                                    let _ = set_device_state(&token, &did, &serde_json::json!({"locked":true})).await;
                                                    busy.set(false);
                                                });
                                            }>
                                            <i class="ph ph-lock" style="font-size:16px;vertical-align:middle"></i>" Lock"
                                        </button>
                                        <button class:active=!cur_lock disabled=move || busy.get()
                                            on:click=move |_| {
                                                let token = auth_token.get().unwrap_or_default();
                                                let did = id_unlck.clone();
                                                busy.set(true); error.set(None);
                                                spawn_local(async move {
                                                    let _ = set_device_state(&token, &did, &serde_json::json!({"locked":false})).await;
                                                    busy.set(false);
                                                });
                                            }>
                                            <i class="ph ph-lock-open" style="font-size:16px;vertical-align:middle"></i>" Unlock"
                                        </button>
                                    </div>
                                </div>
                            })}

                            // Media player controls
                            {is_media.then(|| {
                                let pb  = pb_state.clone();
                                let pb_label = pb_state.replace('_', " ");
                                let img = media_img.clone();
                                let sum = media_sum.clone();
                                let title = media_title_text.clone();
                                let artist = media_artist_text.clone();
                                let album = media_album_text.clone();
                                let source = media_source_text.clone();
                                let show_play  = pb != "playing";
                                let show_pause = pb == "playing";
                                let sup_prev = supports_action(&d, "previous");
                                let sup_stop = supports_action(&d, "stop");
                                let sup_next = supports_action(&d, "next");
                                let sup_play_media = supports_action(&d, "play_media");
                                let sup_set_mute = supports_action(&d, "set_mute");
                                let sup_set_shuffle = supports_action(&d, "set_shuffle");
                                let sup_set_repeat = supports_action(&d, "set_repeat");
                                let sup_set_bass = supports_action(&d, "set_bass");
                                let sup_set_treble = supports_action(&d, "set_treble");
                                let sup_set_loudness = supports_action(&d, "set_loudness");
                                let show_favorites = media_enrichments.iter().any(|item| item == "favorites")
                                    && !media_favorites.is_empty();
                                let show_playlists = media_enrichments.iter().any(|item| item == "playlists")
                                    && !media_playlists.is_empty();
                                let show_media_adv = sup_set_mute || sup_set_shuffle || sup_set_repeat
                                    || sup_set_bass || sup_set_treble || sup_set_loudness;
                                let show_media_header = img.is_some() || sum.is_some() || title.is_some()
                                    || artist.is_some() || album.is_some() || source.is_some();
                                view! {
                                    <div class="control-section">
                                        {show_media_header.then(|| view! {
                                            <div class="media-now-playing">
                                                {move || {
                                                    if let Some(url) = img.clone() {
                                                        view! { <img src=url alt="Album art" class="media-thumb" /> }.into_any()
                                                    } else {
                                                        view! {
                                                            <div class="media-thumb media-thumb-placeholder">
                                                                <i class="ph ph-record"></i>
                                                            </div>
                                                        }.into_any()
                                                    }
                                                }}
                                                <div class="media-now-playing-body">
                                                    <div class="media-now-playing-header">
                                                        <span class="media-now-playing-label">"Now Playing"</span>
                                                        <span class="cell-subtle">{pb_label.clone()}</span>
                                                    </div>
                                                    {sum.clone().map(|s| view! {
                                                        <span class="media-summary">{s}</span>
                                                    })}
                                                    {title.clone().map(|value| view! {
                                                        <div class="media-meta-row">
                                                            <span class="media-meta-key">"Title"</span>
                                                            <span class="media-meta-value">{value}</span>
                                                        </div>
                                                    })}
                                                    {artist.clone().map(|value| view! {
                                                        <div class="media-meta-row">
                                                            <span class="media-meta-key">"Artist"</span>
                                                            <span class="media-meta-value">{value}</span>
                                                        </div>
                                                    })}
                                                    {album.clone().map(|value| view! {
                                                        <div class="media-meta-row">
                                                            <span class="media-meta-key">"Album"</span>
                                                            <span class="media-meta-value">{value}</span>
                                                        </div>
                                                    })}
                                                    {source.clone().filter(|value| {
                                                        title.as_ref().map(|title| title != value).unwrap_or(true)
                                                    }).map(|value| view! {
                                                        <div class="media-meta-row">
                                                            <span class="media-meta-key">"Source"</span>
                                                            <span class="media-meta-value">{value}</span>
                                                        </div>
                                                    })}
                                                </div>
                                            </div>
                                        })}
                                        {show_favorites.then(|| {
                                            let favorites = media_favorites.clone();
                                            view! {
                                                <div class="control-row">
                                                    <span class="control-label">"Favorites"</span>
                                                    <div class="timer-start-row">
                                                        <select
                                                            on:change=move |ev| selected_favorite.set(event_target_value(&ev))
                                                        >
                                                            {favorites.into_iter().map(|favorite| {
                                                                let selected_name = favorite.clone();
                                                                view! {
                                                                    <option
                                                                        value=selected_name.clone()
                                                                        selected=move || selected_favorite.get() == selected_name
                                                                    >
                                                                        {favorite}
                                                                    </option>
                                                                }
                                                            }).collect_view()}
                                                        </select>
                                                        <button disabled=move || busy.get() || selected_favorite.get().is_empty()
                                                            on:click=move |_| {
                                                                let token = auth_token.get().unwrap_or_default();
                                                                let did = id_play_favorite.clone();
                                                                let favorite = selected_favorite.get();
                                                                if favorite.is_empty() {
                                                                    return;
                                                                }
                                                                let body = if sup_play_media {
                                                                    serde_json::json!({"action":"play_media","media_type":"favorite","name": favorite})
                                                                } else {
                                                                    serde_json::json!({"action":"play_favorite","favorite": favorite})
                                                                };
                                                                busy.set(true); error.set(None);
                                                                spawn_local(async move {
                                                                    let _ = set_device_state(&token, &did, &body).await;
                                                                    busy.set(false);
                                                                });
                                                            }>
                                                            <i class="ph ph-play"></i>" Play"
                                                        </button>
                                                    </div>
                                                </div>
                                            }
                                        })}
                                        {show_playlists.then(|| {
                                            let playlists = media_playlists.clone();
                                            view! {
                                                <div class="control-row">
                                                    <span class="control-label">"Playlists"</span>
                                                    <div class="timer-start-row">
                                                        <select
                                                            on:change=move |ev| selected_playlist.set(event_target_value(&ev))
                                                        >
                                                            {playlists.into_iter().map(|playlist| {
                                                                let selected_name = playlist.clone();
                                                                view! {
                                                                    <option
                                                                        value=selected_name.clone()
                                                                        selected=move || selected_playlist.get() == selected_name
                                                                    >
                                                                        {playlist}
                                                                    </option>
                                                                }
                                                            }).collect_view()}
                                                        </select>
                                                        <button disabled=move || busy.get() || selected_playlist.get().is_empty()
                                                            on:click=move |_| {
                                                                let token = auth_token.get().unwrap_or_default();
                                                                let did = id_play_playlist.clone();
                                                                let playlist = selected_playlist.get();
                                                                if playlist.is_empty() {
                                                                    return;
                                                                }
                                                                let body = if sup_play_media {
                                                                    serde_json::json!({"action":"play_media","media_type":"playlist","name": playlist})
                                                                } else {
                                                                    serde_json::json!({"action":"play_playlist","playlist": playlist})
                                                                };
                                                                busy.set(true); error.set(None);
                                                                spawn_local(async move {
                                                                    let _ = set_device_state(&token, &did, &body).await;
                                                                    busy.set(false);
                                                                });
                                                            }>
                                                            <i class="ph ph-play"></i>" Play"
                                                        </button>
                                                    </div>
                                                </div>
                                            }
                                        })}
                                        <div class="control-row">
                                            <span class="control-label">"Playback"</span>
                                            <div class="btn-group">
                                                {sup_prev.then(|| view! {
                                                    <button disabled=move || busy.get()
                                                        on:click=move |_| {
                                                            let token = auth_token.get().unwrap_or_default();
                                                            let did = id_prev.clone();
                                                            spawn_local(async move { let _ = set_device_state(&token, &did, &serde_json::json!({"action":"previous"})).await; });
                                                        }>
                                                        <i class="ph ph-skip-back"></i>
                                                    </button>
                                                })}
                                                {show_play.then(|| view! {
                                                    <button disabled=move || busy.get()
                                                        on:click=move |_| {
                                                            let token = auth_token.get().unwrap_or_default();
                                                            let did = id_play.clone();
                                                            busy.set(true); error.set(None);
                                                            spawn_local(async move {
                                                                let _ = set_device_state(&token, &did, &serde_json::json!({"action":"play"})).await;
                                                                busy.set(false);
                                                            });
                                                        }>
                                                        <i class="ph ph-play"></i>
                                                    </button>
                                                })}
                                                {show_pause.then(|| view! {
                                                    <button disabled=move || busy.get()
                                                        on:click=move |_| {
                                                            let token = auth_token.get().unwrap_or_default();
                                                            let did = id_pause.clone();
                                                            busy.set(true); error.set(None);
                                                            spawn_local(async move {
                                                                let _ = set_device_state(&token, &did, &serde_json::json!({"action":"pause"})).await;
                                                                busy.set(false);
                                                            });
                                                        }>
                                                        <i class="ph ph-pause"></i>
                                                    </button>
                                                })}
                                                {sup_stop.then(|| view! {
                                                    <button disabled=move || busy.get()
                                                        on:click=move |_| {
                                                            let token = auth_token.get().unwrap_or_default();
                                                            let did = id_stop.clone();
                                                            spawn_local(async move { let _ = set_device_state(&token, &did, &serde_json::json!({"action":"stop"})).await; });
                                                        }>
                                                        <i class="ph ph-stop"></i>
                                                    </button>
                                                })}
                                                {sup_next.then(|| view! {
                                                    <button disabled=move || busy.get()
                                                        on:click=move |_| {
                                                            let token = auth_token.get().unwrap_or_default();
                                                            let did = id_next.clone();
                                                            spawn_local(async move { let _ = set_device_state(&token, &did, &serde_json::json!({"action":"next"})).await; });
                                                        }>
                                                        <i class="ph ph-skip-forward"></i>
                                                    </button>
                                                })}
                                            </div>
                                        </div>
                                        {has_vol.then(|| view! {
                                            <div class="control-row">
                                                <span class="control-label">"Volume"</span>
                                                <div class="slider-row">
                                                    <i class="ph ph-speaker-low" style="font-size:18px;color:var(--hc-text-muted)"></i>
                                                    <input type="range" min="0" max="100" step="1"
                                                        prop:value=cur_vol as i64
                                                        on:change=move |ev| {
                                                            if let Some(el) = ev.target()
                                                                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                            {
                                                                let val: u64 = el.value().parse().unwrap_or(0);
                                                                let token = auth_token.get().unwrap_or_default();
                                                                let did = id_vol.clone();
                                                                spawn_local(async move {
                                                                    let _ = set_device_state(&token, &did,
                                                                        &serde_json::json!({"action":"set_volume","volume": val})).await;
                                                                });
                                                            }
                                                        }
                                                    />
                                                    <i class="ph ph-speaker-high" style="font-size:18px;color:var(--hc-text-muted)"></i>
                                                    <span class="slider-value">{format!("{:.0}%", cur_vol)}</span>
                                                </div>
                                            </div>
                                        })}
                                        {show_media_adv.then(|| view! {
                                            <div class="control-section">
                                                {(sup_set_mute && media_muted.is_some()).then(|| {
                                                    let muted = media_muted.unwrap_or(false);
                                                    view! {
                                                        <div class="control-row">
                                                            <span class="control-label">"Mute"</span>
                                                            <div class="btn-group">
                                                                <button class:active=muted disabled=move || busy.get()
                                                                    on:click=move |_| {
                                                                        let token = auth_token.get().unwrap_or_default();
                                                                        let did = id_mute.clone();
                                                                        busy.set(true); error.set(None);
                                                                        spawn_local(async move {
                                                                            let _ = set_device_state(&token, &did, &serde_json::json!({"action":"set_mute","muted": !muted})).await;
                                                                            busy.set(false);
                                                                        });
                                                                    }>
                                                                    <i class=move || if muted { "ph ph-speaker-x" } else { "ph ph-speaker-high" }></i>
                                                                    {if muted { " Unmute" } else { " Mute" }}
                                                                </button>
                                                            </div>
                                                        </div>
                                                    }
                                                })}
                                                {(sup_set_shuffle && media_shuffle.is_some()).then(|| {
                                                    let shuffle = media_shuffle.unwrap_or(false);
                                                    view! {
                                                        <div class="control-row">
                                                            <span class="control-label">"Shuffle"</span>
                                                            <div class="btn-group">
                                                                <button class:active=shuffle disabled=move || busy.get()
                                                                    on:click=move |_| {
                                                                        let token = auth_token.get().unwrap_or_default();
                                                                        let did = id_shuffle.clone();
                                                                        busy.set(true); error.set(None);
                                                                        spawn_local(async move {
                                                                            let _ = set_device_state(&token, &did, &serde_json::json!({"action":"set_shuffle","shuffle": !shuffle})).await;
                                                                            busy.set(false);
                                                                        });
                                                                    }>
                                                                    <i class="ph ph-shuffle"></i>
                                                                    {if shuffle { " On" } else { " Off" }}
                                                                </button>
                                                            </div>
                                                        </div>
                                                    }
                                                })}
                                                {(sup_set_repeat && media_repeat.is_some()).then(|| {
                                                    let repeat_value = media_repeat.clone().unwrap_or_else(|| "none".to_string());
                                                    view! {
                                                        <div class="control-row">
                                                            <span class="control-label">"Repeat"</span>
                                                            <div class="timer-start-row">
                                                                <select
                                                                    on:change=move |ev| {
                                                                        let value = event_target_value(&ev);
                                                                        let token = auth_token.get().unwrap_or_default();
                                                                        let did = id_repeat.clone();
                                                                        busy.set(true); error.set(None);
                                                                        spawn_local(async move {
                                                                            let _ = set_device_state(&token, &did, &serde_json::json!({"action":"set_repeat","repeat": value})).await;
                                                                            busy.set(false);
                                                                        });
                                                                    }
                                                                >
                                                                    <option value="none" selected=repeat_value == "none">"Off"</option>
                                                                    <option value="all" selected=repeat_value == "all">"All"</option>
                                                                    <option value="one" selected=repeat_value == "one">"One"</option>
                                                                </select>
                                                            </div>
                                                        </div>
                                                    }
                                                })}
                                                {(sup_set_bass && media_bass.is_some()).then(|| {
                                                    let bass = media_bass.unwrap_or(0);
                                                    view! {
                                                        <div class="control-row">
                                                            <span class="control-label">"Bass"</span>
                                                            <div class="slider-row">
                                                                <input type="range" min="-10" max="10" step="1"
                                                                    prop:value=bass
                                                                    on:change=move |ev| {
                                                                        if let Some(el) = ev.target()
                                                                            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                                        {
                                                                            let val: i64 = el.value().parse().unwrap_or(0);
                                                                            let token = auth_token.get().unwrap_or_default();
                                                                            let did = id_bass.clone();
                                                                            busy.set(true); error.set(None);
                                                                            spawn_local(async move {
                                                                                let _ = set_device_state(&token, &did, &serde_json::json!({"action":"set_bass","bass": val})).await;
                                                                                busy.set(false);
                                                                            });
                                                                        }
                                                                    }
                                                                />
                                                                <span class="slider-value">{bass}</span>
                                                            </div>
                                                        </div>
                                                    }
                                                })}
                                                {(sup_set_treble && media_treble.is_some()).then(|| {
                                                    let treble = media_treble.unwrap_or(0);
                                                    view! {
                                                        <div class="control-row">
                                                            <span class="control-label">"Treble"</span>
                                                            <div class="slider-row">
                                                                <input type="range" min="-10" max="10" step="1"
                                                                    prop:value=treble
                                                                    on:change=move |ev| {
                                                                        if let Some(el) = ev.target()
                                                                            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                                        {
                                                                            let val: i64 = el.value().parse().unwrap_or(0);
                                                                            let token = auth_token.get().unwrap_or_default();
                                                                            let did = id_treble.clone();
                                                                            busy.set(true); error.set(None);
                                                                            spawn_local(async move {
                                                                                let _ = set_device_state(&token, &did, &serde_json::json!({"action":"set_treble","treble": val})).await;
                                                                                busy.set(false);
                                                                            });
                                                                        }
                                                                    }
                                                                />
                                                                <span class="slider-value">{treble}</span>
                                                            </div>
                                                        </div>
                                                    }
                                                })}
                                                {(sup_set_loudness && media_loudness.is_some()).then(|| {
                                                    let loudness = media_loudness.unwrap_or(false);
                                                    view! {
                                                        <div class="control-row">
                                                            <span class="control-label">"Loudness"</span>
                                                            <div class="btn-group">
                                                                <button class:active=loudness disabled=move || busy.get()
                                                                    on:click=move |_| {
                                                                        let token = auth_token.get().unwrap_or_default();
                                                                        let did = id_loudness.clone();
                                                                        busy.set(true); error.set(None);
                                                                        spawn_local(async move {
                                                                            let _ = set_device_state(&token, &did, &serde_json::json!({"action":"set_loudness","loudness": !loudness})).await;
                                                                            busy.set(false);
                                                                        });
                                                                    }>
                                                                    <i class="ph ph-equalizer"></i>
                                                                    {if loudness { " On" } else { " Off" }}
                                                                </button>
                                                            </div>
                                                        </div>
                                                    }
                                                })}
                                            </div>
                                        })}
                                    </div>
                                }
                            })}

                            // Timer controls
                            {is_timer.then(|| view! {
                                <div class="control-section">
                                    // Status + live countdown (reactive — TimerDisplay owns its tick)
                                    <div class="timer-status">
                                        <span class="timer-state-label">{ts_label.clone()}</span>
                                        <TimerDisplay device=device />
                                    </div>

                                    // Pause / Resume — reactive closure so it updates when WS changes state
                                    <div class="btn-group">
                                        {move || {
                                            let state = str_attr(device.get().as_ref()
                                                .and_then(|d| d.attributes.get("state")))
                                                .unwrap_or("idle")
                                                .to_string();
                                            if state == "running" {
                                                // Clone per-run so the reactive closure stays FnMut
                                                let did_pause = id_tpaus.clone();
                                                view! {
                                                    <button disabled=move || busy.get()
                                                        on:click=move |_| {
                                                            let token = auth_token.get().unwrap_or_default();
                                                            let did = did_pause.clone();
                                                            spawn_local(async move { let _ = set_device_state(&token, &did, &serde_json::json!({"command":"pause"})).await; });
                                                        }>
                                                        <i class="ph ph-pause"></i>" Pause"
                                                    </button>
                                                }.into_any()
                                            } else if state == "paused" {
                                                let did_resume = id_tresu.clone();
                                                view! {
                                                    <button disabled=move || busy.get()
                                                        on:click=move |_| {
                                                            let token = auth_token.get().unwrap_or_default();
                                                            let did = did_resume.clone();
                                                            spawn_local(async move { let _ = set_device_state(&token, &did, &serde_json::json!({"command":"resume"})).await; });
                                                        }>
                                                        <i class="ph ph-play"></i>" Resume"
                                                    </button>
                                                }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }
                                        }}

                                        // Cancel — always visible; valid from any timer state
                                        <button class="danger" disabled=move || busy.get()
                                            on:click=move |_| {
                                                let token = auth_token.get().unwrap_or_default();
                                                let did = id_tcanc.clone();
                                                spawn_local(async move { let _ = set_device_state(&token, &did, &serde_json::json!({"command":"cancel"})).await; });
                                            }>
                                            <i class="ph ph-x-circle"></i>" Cancel"
                                        </button>
                                    </div>

                                    // Duration input + Start — always visible
                                    <div class="control-row">
                                        <span class="control-label">"Duration (seconds)"</span>
                                        <div class="timer-start-row">
                                            <input
                                                class="input"
                                                type="number"
                                                prop:value=move || timer_secs.get()
                                                on:input=move |ev| timer_secs.set(event_target_value(&ev))
                                                placeholder="60"
                                            />
                                            <button disabled=move || busy.get()
                                                on:click=move |_| {
                                                    let secs: u64 = timer_secs.get().trim().parse().unwrap_or(60);
                                                    let token = auth_token.get().unwrap_or_default();
                                                    let did = id_start.clone();
                                                    busy.set(true); error.set(None);
                                                    spawn_local(async move {
                                                        let _ = set_device_state(&token, &did,
                                                            &serde_json::json!({"command":"start","duration_secs": secs})).await;
                                                        busy.set(false);
                                                    });
                                                }>
                                                <i class="ph ph-timer"></i>" Start"
                                            </button>
                                        </div>
                                    </div>
                                </div>
                            })}

                            // No controls fallback
                            {no_controls.then(|| view! {
                                <p class="no-controls-msg">
                                    <i class="ph ph-info" style="font-size:18px;vertical-align:middle"></i>
                                    " Read-only device — no interactive controls."
                                </p>
                            })}
                        </div>
                        })}
                    </div>

                    // ── Raw attributes ────────────────────────────────────────
                    <div class="detail-card">
                        <h2 class="card-title">"Attributes"
                            <span class="card-subtitle">" — live via WebSocket"</span>
                        </h2>
                        {if attr_pairs.is_empty() {
                            view! { <p class="no-controls-msg">"No attributes reported."</p> }.into_any()
                        } else {
                            view! {
                                <div class="attr-grid">
                                    {attr_pairs.into_iter().map(|(k, v)| view! {
                                        <div class="attr-row">
                                            <span class="attr-key">{k}</span>
                                            <span class="attr-val">{v}</span>
                                        </div>
                                    }).collect_view()}
                                </div>
                            }.into_any()
                        }}
                    </div>

                    // ── History ───────────────────────────────────────────────
                    <div class="detail-card">
                        <div class="card-title-row">
                            <h2 class="card-title">"State History"
                                <span class="card-subtitle">" — last 25 changes"</span>
                            </h2>
                            <button
                                class="btn btn-outline"
                                on:click=move |_| hist_trigger.update(|n| *n += 1)
                                disabled=move || hist_loading.get()
                            >
                                {move || if hist_loading.get() { "Reloading…" } else { "Reload" }}
                            </button>
                        </div>
                        {move || {
                            let h = sorted_history.get();
                            if hist_loading.get() && h.is_empty() {
                                view! { <p style="padding:0.5rem 0;color:var(--hc-text-muted)">"Loading history…"</p> }.into_any()
                            } else if h.is_empty() {
                                view! { <p style="padding:0.5rem 0;color:var(--hc-text-muted)">"No history recorded yet."</p> }.into_any()
                            } else {
                                view! {
                                    <div class="history-toolbar">
                                        <div class="history-toolbar-meta">
                                            <strong>{move || sorted_history.get().len()}</strong>
                                            <span>" rows"</span>
                                        </div>
                                        <div class="history-sort-group">
                                            <button
                                                class="hist-sort-btn"
                                                class:active=move || history_sort_by.get() == HistorySortKey::Time
                                                on:click=move |_| {
                                                    if history_sort_by.get() == HistorySortKey::Time {
                                                        history_sort_dir.update(|dir| {
                                                            *dir = if *dir == HistorySortDir::Desc {
                                                                HistorySortDir::Asc
                                                            } else {
                                                                HistorySortDir::Desc
                                                            }
                                                        });
                                                    } else {
                                                        history_sort_by.set(HistorySortKey::Time);
                                                        history_sort_dir.set(HistorySortDir::Desc);
                                                    }
                                                }
                                            >
                                                "Time"
                                            </button>
                                            <button
                                                class="hist-sort-btn"
                                                class:active=move || history_sort_by.get() == HistorySortKey::Attribute
                                                on:click=move |_| {
                                                    if history_sort_by.get() == HistorySortKey::Attribute {
                                                        history_sort_dir.update(|dir| {
                                                            *dir = if *dir == HistorySortDir::Desc {
                                                                HistorySortDir::Asc
                                                            } else {
                                                                HistorySortDir::Desc
                                                            }
                                                        });
                                                    } else {
                                                        history_sort_by.set(HistorySortKey::Attribute);
                                                        history_sort_dir.set(HistorySortDir::Asc);
                                                    }
                                                }
                                            >
                                                "Attribute"
                                            </button>
                                            <button
                                                class="hist-sort-btn"
                                                class:active=move || history_sort_by.get() == HistorySortKey::Value
                                                on:click=move |_| {
                                                    if history_sort_by.get() == HistorySortKey::Value {
                                                        history_sort_dir.update(|dir| {
                                                            *dir = if *dir == HistorySortDir::Desc {
                                                                HistorySortDir::Asc
                                                            } else {
                                                                HistorySortDir::Desc
                                                            }
                                                        });
                                                    } else {
                                                        history_sort_by.set(HistorySortKey::Value);
                                                        history_sort_dir.set(HistorySortDir::Asc);
                                                    }
                                                }
                                            >
                                                "Value"
                                            </button>
                                            <button
                                                class="hist-sort-btn hist-sort-dir"
                                                on:click=move |_| {
                                                    history_sort_dir.update(|dir| {
                                                        *dir = if *dir == HistorySortDir::Desc {
                                                            HistorySortDir::Asc
                                                        } else {
                                                            HistorySortDir::Desc
                                                        }
                                                    });
                                                }
                                            >
                                                {move || if history_sort_dir.get() == HistorySortDir::Desc {
                                                    "Descending"
                                                } else {
                                                    "Ascending"
                                                }}
                                            </button>
                                        </div>
                                    </div>

                                    <div class="hist-wrap">
                                        <table class="hist-table">
                                            <thead><tr>
                                                <th>"Time"</th><th>"Attribute"</th><th>"Value"</th>
                                            </tr></thead>
                                            <tbody>
                                                <For
                                                    each=move || sorted_history.get()
                                                    key=|e| format!("{}{}{}", e.recorded_at, e.attribute, e.value)
                                                    children=|entry| {
                                                        let rel  = format_relative(Some(&entry.recorded_at));
                                                        let abs  = format_abs(Some(&entry.recorded_at));
                                                        let val  = entry.value_display();
                                                        let attr = entry.attribute.clone();
                                                        view! {
                                                            <tr>
                                                                <td><div class="cell-primary">
                                                                    <span>{rel}</span>
                                                                    <span class="cell-subtle">{abs}</span>
                                                                </div></td>
                                                                <td><code class="mono">{attr}</code></td>
                                                                <td class="hist-val">{val}</td>
                                                            </tr>
                                                        }
                                                    }
                                                />
                                            </tbody>
                                        </table>
                                    </div>
                                }.into_any()
                            }
                        }}
                    </div>

                    // ── Schema ────────────────────────────────────────────────
                    <DeviceSchemaSection device_id=device_id.clone() />

                }
            })}

        </div>
    }
}

// ── Device Schema Sub-Component ─────────────────────────────────────────────

#[component]
fn DeviceSchemaSection(device_id: String) -> impl IntoView {
    let auth = use_auth();
    let schema = RwSignal::new(Option::<serde_json::Value>::None);
    let loading = RwSignal::new(false);
    let expanded = RwSignal::new(false);

    let id = device_id.clone();
    let load_schema = move || {
        let token = auth.token_str().unwrap_or_default();
        let id = id.clone();
        loading.set(true);
        spawn_local(async move {
            match fetch_device_schema(&token, &id).await {
                Ok(s) => schema.set(Some(s)),
                Err(_) => schema.set(None),
            }
            loading.set(false);
        });
    };

    // Load on first expand
    Effect::new(move |_| {
        if expanded.get() && schema.get().is_none() && !loading.get() {
            load_schema();
        }
    });

    view! {
        <div class="detail-card">
            <div class="card-title-row">
                <h2 class="card-title">"Device Schema"</h2>
                <button
                    class="btn btn-outline"
                    on:click=move |_| expanded.update(|v| *v = !*v)
                >
                    {move || if expanded.get() { "Hide" } else { "Show" }}
                </button>
            </div>
            {move || {
                if !expanded.get() {
                    view! { <p class="cell-subtle">"Click Show to load the device capability schema."</p> }.into_any()
                } else if loading.get() {
                    view! { <p class="no-controls-msg">"Loading schema..."</p> }.into_any()
                } else if let Some(s) = schema.get() {
                    // Render schema attributes as a table
                    if let Some(attrs) = s["attributes"].as_object() {
                        let mut rows: Vec<(String, String, String)> = attrs
                            .iter()
                            .map(|(name, def)| {
                                let typ = def["type"].as_str().unwrap_or("unknown").to_string();
                                let mut details = Vec::new();
                                if let Some(min) = def.get("min") {
                                    details.push(format!("min: {min}"));
                                }
                                if let Some(max) = def.get("max") {
                                    details.push(format!("max: {max}"));
                                }
                                if let Some(unit) = def["unit"].as_str() {
                                    details.push(format!("unit: {unit}"));
                                }
                                if let Some(vals) = def["values"].as_array() {
                                    let vs: Vec<String> = vals.iter()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect();
                                    if !vs.is_empty() {
                                        details.push(format!("values: [{}]", vs.join(", ")));
                                    }
                                }
                                if def["read_only"].as_bool().unwrap_or(false) {
                                    details.push("read-only".to_string());
                                }
                                (name.clone(), typ, details.join(", "))
                            })
                            .collect();
                        rows.sort_by(|a, b| a.0.cmp(&b.0));
                        view! {
                            <table class="admin-table">
                                <thead><tr>
                                    <th>"Attribute"</th>
                                    <th>"Type"</th>
                                    <th>"Details"</th>
                                </tr></thead>
                                <tbody>
                                    {rows.into_iter().map(|(name, typ, details)| view! {
                                        <tr>
                                            <td><code class="mono">{name}</code></td>
                                            <td>{typ}</td>
                                            <td class="cell-subtle">{details}</td>
                                        </tr>
                                    }).collect_view()}
                                </tbody>
                            </table>
                        }.into_any()
                    } else {
                        // Fallback: render raw JSON
                        let raw = serde_json::to_string_pretty(&s).unwrap_or_default();
                        view! { <pre class="schema-raw">{raw}</pre> }.into_any()
                    }
                } else {
                    view! { <p class="cell-subtle">"No schema available for this device."</p> }.into_any()
                }
            }}
        </div>
    }
}
