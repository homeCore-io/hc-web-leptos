//! Device Cards page — `/cards`
//!
//! Each `DeviceCard` owns a `Memo<Option<DeviceState>>` keyed to its device_id.
//! Because `DeviceState: PartialEq`, Leptos stops propagation at the Memo when
//! the device's data hasn't actually changed — so updating device B doesn't
//! cause device A's card view to re-run at all.
//!
//! Timer cards own an isolated `tick: RwSignal<u64>` inside `CardTimerCountdown`
//! so the per-second countdown only re-renders the countdown text + progress bar,
//! never the rest of the page.
//!
//! The canvas `<div class="cards-canvas">` is the hook for future drag/drop.
//! Each card sits in a `.card-slot` wrapper that will carry `draggable` + event
//! handlers when that feature is added.

use crate::api::{fetch_devices, set_device_state};
use crate::auth::use_auth;
use crate::models::*;
use serde_json::{json, Value};
use crate::pages::shared::{
    card_size_canvas_class, common_card_prefs_map, json_str_set, load_common_card_prefs,
    load_pref_json, ls_set, set_to_json_array, CardSize, CardSizeSelect, CommonCardPrefs,
    LiveStatusBanner, MultiSelectDropdown, ResetFiltersButton, SearchField,
    SortDir, SortDirToggle, SortSelect,
};
use crate::ws::use_ws;
use leptos::prelude::*;
use leptos::task::spawn_local;
use std::collections::HashSet;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

// ── Prefs ─────────────────────────────────────────────────────────────────────

const CARDS_PREFS_KEY: &str = "hc-leptos:cards:prefs";
const COLLAPSED_AREAS_KEY: &str = "hc-leptos:cards:collapsed-areas";

/// Returns the set of area labels the user has chosen to collapse on
/// the device-cards page. Defaults to empty (all areas expanded).
fn load_collapsed_areas() -> HashSet<String> {
    crate::pages::shared::ls_get(COLLAPSED_AREAS_KEY)
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

fn save_collapsed_areas(set: &HashSet<String>) {
    let arr: Vec<&String> = set.iter().collect();
    if let Ok(json) = serde_json::to_string(&arr) {
        ls_set(COLLAPSED_AREAS_KEY, &json);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Name,
    Area,
    Status,
    Type,
    LastSeen,
}

fn sort_key_from_str(value: Option<&str>) -> SortKey {
    match value {
        Some("area") => SortKey::Area,
        Some("status") => SortKey::Status,
        Some("type") => SortKey::Type,
        Some("last_seen") => SortKey::LastSeen,
        _ => SortKey::Name,
    }
}

fn sort_key_to_str(value: SortKey) -> &'static str {
    match value {
        SortKey::Name => "name",
        SortKey::Area => "area",
        SortKey::Status => "status",
        SortKey::Type => "type",
        SortKey::LastSeen => "last_seen",
    }
}

struct CardsPrefs {
    card_size: CardSize,
    search: String,
    avail_filter: HashSet<String>,
    area_filter: HashSet<String>,
    type_filter: HashSet<String>,
    plugin_filter: HashSet<String>,
    sort_by: SortKey,
    sort_dir: SortDir,
}

impl Default for CardsPrefs {
    fn default() -> Self {
        CardsPrefs {
            card_size: CardSize::Medium,
            search: String::new(),
            avail_filter: HashSet::new(),
            area_filter: HashSet::new(),
            type_filter: HashSet::new(),
            plugin_filter: HashSet::new(),
            sort_by: SortKey::Name,
            sort_dir: SortDir::Asc,
        }
    }
}

fn load_prefs() -> CardsPrefs {
    let Some(v) = load_pref_json(CARDS_PREFS_KEY) else {
        return CardsPrefs::default();
    };
    let common = load_common_card_prefs(&v, sort_key_from_str);

    CardsPrefs {
        card_size: common.card_size,
        search: common.search,
        avail_filter: json_str_set(&v, "avail_filter"),
        area_filter: json_str_set(&v, "area_filter"),
        type_filter: json_str_set(&v, "type_filter"),
        plugin_filter: json_str_set(&v, "plugin_filter"),
        sort_by: common.sort_by,
        sort_dir: common.sort_dir,
    }
}

fn save_prefs(
    card_size: CardSize,
    search: &str,
    avail_filter: &HashSet<String>,
    area_filter: &HashSet<String>,
    type_filter: &HashSet<String>,
    plugin_filter: &HashSet<String>,
    sort_by: SortKey,
    sort_dir: SortDir,
) {
    let common = CommonCardPrefs {
        card_size,
        search: search.to_string(),
        sort_by,
        sort_dir,
    };
    let mut value = common_card_prefs_map(&common, sort_key_to_str);
    value.insert("avail_filter".to_string(), set_to_json_array(avail_filter));
    value.insert("area_filter".to_string(), set_to_json_array(area_filter));
    value.insert("type_filter".to_string(), set_to_json_array(type_filter));
    value.insert(
        "plugin_filter".to_string(),
        set_to_json_array(plugin_filter),
    );
    ls_set(
        CARDS_PREFS_KEY,
        &serde_json::Value::Object(value).to_string(),
    );
}

// ── CardTimerCountdown ────────────────────────────────────────────────────────
//
// Owns its own tick signal.  Only the countdown text and progress bar
// re-render every second — the rest of the card is unaffected.

#[component]
fn CardTimerCountdown(
    /// The card's device Memo — CardTimerCountdown subscribes independently.
    device: Memo<Option<DeviceState>>,
) -> impl IntoView {
    let tick = RwSignal::new(0u64);

    // One interval per card; cleaned up when the card unmounts.
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

    // Recomputes every tick AND on WS events.  timer_remaining_secs uses
    // started_at + duration - now() so it stays accurate without server pushes.
    let remaining: Memo<Option<u64>> = Memo::new(move |_| {
        let _ = tick.get();
        device.get().as_ref().and_then(|d| timer_remaining_secs(d))
    });

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

    let pct: Signal<u32> = Signal::derive(move || {
        let d = dur.get();
        let r = remaining.get().unwrap_or(0);
        if d == 0 {
            0u32
        } else {
            ((d.saturating_sub(r)) as f64 / d as f64 * 100.0).min(100.0) as u32
        }
    });

    // show/hide reacts only to device state changes, not to tick
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
            <div class="card-timer-display">
                // Only this text node re-renders every second
                <span class="card-timer-remaining">
                    {move || {
                        remaining
                            .get()
                            .map(|r| format_duration_secs(r))
                            .unwrap_or_default()
                    }}
                </span>
                <div class="card-timer-bar">
                    // Only this style attribute updates every second
                    <div
                        class="card-timer-fill"
                        style=move || format!("width:{}%", pct.get())
                    ></div>
                </div>
            </div>
        })}
    }
}

// ── DeviceCard ────────────────────────────────────────────────────────────────

#[component]
pub fn DeviceCard(device_id: String) -> impl IntoView {
    let ws = use_ws();
    let auth = use_auth();

    // ── The isolation key ─────────────────────────────────────────────────────
    //
    // Memo<Option<DeviceState>>:
    //   - re-runs whenever ws.devices changes (any WS event)
    //   - BUT: DeviceState: PartialEq — Leptos stops propagation here if the
    //     value is the same as last time.
    //   - Result: only THIS card's view closure re-runs when its device changes.
    //     Other cards' views are never invoked.
    let did = device_id.clone();
    let device: Memo<Option<DeviceState>> = Memo::new(move |_| ws.devices.get().get(&did).cloned());

    let busy = RwSignal::new(false);
    // Brief command-sent pulse: set true on click, auto-clears 600ms
    // later. CSS animates `.device-card--pulse` for that window.
    let pulse = RwSignal::new(false);

    // Convenience: send a state command to this device
    let did_cmd = device_id.clone();
    let send = move |body: serde_json::Value| {
        let token = auth.token_str().unwrap_or_default();
        let id = did_cmd.clone();
        busy.set(true);
        // Trigger the pulse animation and schedule its clear. Use a
        // restart pattern: re-set false→true via toggling so a rapid
        // double-click re-runs the animation.
        pulse.set(false);
        let p = pulse;
        gloo_timers::callback::Timeout::new(0, move || p.set(true)).forget();
        gloo_timers::callback::Timeout::new(600, move || p.set(false)).forget();
        spawn_local(async move {
            let _ = set_device_state(&token, &id, &body).await;
            busy.set(false);
        });
    };

    let detail_href = format!("/devices/{}", device_id);

    view! {
        // card-slot is the drag/drop unit for future layout features
        <div class="card-slot" data-device-id=device_id>
            {move || {
                let Some(d) = device.get() else {
                    return view! { <div class="device-card device-card--ghost"></div> }.into_any();
                };

                let name    = display_name(&d).to_string();
                let area    = d.area.as_deref().map(display_area_name);
                let type_label = presentation_device_type_label(&d).to_string();
                let avail   = d.available;
                let tone    = status_tone(&d);
                let _icon   = status_icon_name(&d);
                let mdi     = device_mdi_icon(&d);
                let last    = last_change_time(&d).copied();
                // Card visual props: type rim, color reflection, brightness halo
                let type_class = card_type_class(&d);
                let color_css = device_color_css(&d);

                let is_timer  = is_timer_device(&d);
                let is_media  = is_media_player(&d);
                let is_scene  = is_scene_like(&d);
                let is_thermostat = is_thermostat_device(&d);
                let can_toggle = supports_inline_toggle(&d);
                let can_lock = supports_inline_lock(&d);
                let has_brightness = d.attributes.get("brightness_pct").and_then(|v| v.as_f64()).is_some();
                let brightness_pct = d.attributes.get("brightness_pct")
                    .and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;

                let cur_on  = bool_attr(d.attributes.get("on")).unwrap_or(false);
                let cur_locked = bool_attr(d.attributes.get("locked")).unwrap_or(false);
                let pb      = playback_state(&d);
                let is_playing = pb == "playing";
                let timer_st = str_attr(d.attributes.get("state"))
                    .unwrap_or("idle").to_string();
                let state_text = status_text(&d);

                if is_media {
                    // ── Fancy Media Card ──────────────────────────────────────
                    let title_str  = media_title(&d).map(str::to_string);
                    let artist_str = media_artist(&d).map(str::to_string);
                    let album_str  = media_album(&d).map(str::to_string);
                    let img_str    = media_image_url(&d).map(str::to_string);
                    let vol        = d.attributes.get("volume")
                        .and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
                    let sup_prev   = supports_action(&d, "previous");
                    let sup_next   = supports_action(&d, "next");

                    let send_play  = send.clone();
                    let send_pause = send.clone();
                    let send_prev  = send.clone();
                    let send_next  = send.clone();
                    let send_vol   = send.clone();

                    view! {
                        <div
                            class="device-card device-card--media"
                            class:device-card--offline=!avail
                        >
                            // ── Art + now-playing overlay ─────────────────────
                            <div class="card-media-art-wrap">
                                {if let Some(url) = img_str {
                                    view! {
                                        <img src=url class="card-media-art" alt="album art" />
                                    }.into_any()
                                } else {
                                    view! {
                                        <div class="card-media-art card-media-art--placeholder">
                                            <i class="ph ph-music-note"></i>
                                        </div>
                                    }.into_any()
                                }}
                                <div class="card-media-overlay">
                                    <p class="card-media-overlay-title">
                                        {title_str.unwrap_or_else(|| pb.clone())}
                                    </p>
                                    {artist_str.map(|a| view! {
                                        <p class="card-media-overlay-artist">{a}</p>
                                    })}
                                    {album_str.map(|a| view! {
                                        <p class="card-media-overlay-album">{a}</p>
                                    })}
                                </div>
                            </div>

                            // ── Device name row ───────────────────────────────
                            <div class="card-media-name-row">
                                <span class="card-name" title=name.clone()>{name.clone()}</span>
                                <span
                                    class="card-avail-dot"
                                    class:card-avail-dot--on=avail
                                    class:card-avail-dot--off=!avail
                                    title=if avail { "Online" } else { "Offline" }
                                ></span>
                            </div>

                            // ── Transport controls ────────────────────────────
                            <div class="card-media-transport">
                                {sup_prev.then(move || view! {
                                    <button
                                        class="card-media-ctrl"
                                        disabled={move || busy.get() || !avail}
                                        on:click=move |_| {
                                            send_prev(serde_json::json!({"action":"previous"}));
                                        }
                                    >
                                        <i class="ph ph-skip-back"></i>
                                    </button>
                                })}
                                <button
                                    class="card-media-ctrl card-media-ctrl--primary"
                                    disabled={move || busy.get() || !avail}
                                    on:click=move |_| {
                                        if is_playing {
                                            send_pause(serde_json::json!({"action":"pause"}));
                                        } else {
                                            send_play(serde_json::json!({"action":"play"}));
                                        }
                                    }
                                >
                                    <i class=move || if is_playing { "ph ph-pause" } else { "ph ph-play" }></i>
                                </button>
                                {sup_next.then(move || view! {
                                    <button
                                        class="card-media-ctrl"
                                        disabled={move || busy.get() || !avail}
                                        on:click=move |_| {
                                            send_next(serde_json::json!({"action":"next"}));
                                        }
                                    >
                                        <i class="ph ph-skip-forward"></i>
                                    </button>
                                })}
                            </div>

                            // ── Volume row ────────────────────────────────────
                            <div class="card-media-vol-row">
                                <i class="ph ph-speaker-low card-media-vol-icon"></i>
                                <input
                                    type="range"
                                    class="card-media-vol-slider"
                                    min="0" max="100"
                                    value=vol.to_string()
                                    on:change=move |ev| {
                                        let el = ev.target()
                                            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok());
                                        if let Some(el) = el {
                                            let v: u64 = el.value().parse().unwrap_or(0);
                                            send_vol(serde_json::json!({"action":"set_volume","volume": v}));
                                        }
                                    }
                                />
                                <span class="card-media-vol-pct">{format!("{}%", vol)}</span>
                            </div>

                            // ── Footer ────────────────────────────────────────
                            <div class="card-footer">
                                <span class="card-meta">
                                    {area.clone().map(|a| view! {
                                        <span>{a}</span>
                                        <span class="card-meta-sep">" · "</span>
                                    })}
                                    <span>{type_label}</span>
                                </span>
                                <a href=detail_href.clone() class="card-detail-link">
                                    <i class="ph ph-arrow-square-out" style="font-size:15px"></i>
                                </a>
                            </div>
                        </div>
                    }.into_any()

                } else if is_thermostat {
                    // ── Thermostat Card ───────────────────────────────────────
                    let sp = d.attributes.get("setpoint").and_then(|v| v.as_f64()).unwrap_or(70.0);
                    let temp = d.attributes.get("current_temperature").and_then(|v| v.as_f64());
                    let mode = d.attributes.get("mode").and_then(|v| v.as_str()).unwrap_or("off").to_string();
                    let call = d.attributes.get("call_for").and_then(|v| v.as_str()).unwrap_or("idle").to_string();
                    let pill_class = match call.as_str() {
                        "heat" => "pill-heat",
                        "cool" => "pill-cool",
                        "stale" => "pill-stale",
                        _ => "pill-idle",
                    };
                    let temp_unit = thermostat_temperature_unit(&d, &ws.devices.get());
                    let fmt_temp = |t: f64| -> String {
                        match temp_unit {
                            Some(unit) => format!("{t:.1} {unit}"),
                            None => format!("{t:.1}°"),
                        }
                    };
                    let temp_str = temp.map(fmt_temp).unwrap_or_else(|| "—".to_string());
                    let sp_str = fmt_temp(sp);

                    let send_sp_minus = send.clone();
                    let send_sp_plus = send.clone();
                    let send_heat = send.clone();
                    let send_cool = send.clone();
                    let send_off = send.clone();
                    let mode_heat = mode == "heat";
                    let mode_cool = mode == "cool";
                    let mode_off = mode == "off";

                    view! {
                        <div class="device-card device-card--thermostat"
                            class:device-card--offline=!avail
                        >
                            <div class="card-header">
                                <span class=format!("card-status-icon status-badge-sm {}", tone.css_class())>
                                    <i class="ph ph-thermometer-simple card-mdi-icon"></i>
                                </span>
                                <div class="card-header-text">
                                    <p class="card-name" title=name.clone()>{name.clone()}</p>
                                    <p class="card-meta">
                                        {area.clone().map(|a| view! {
                                            <span>{a}</span>
                                            <span class="card-meta-sep">" · "</span>
                                        })}
                                        <span>"Thermostat"</span>
                                    </p>
                                </div>
                                <a href=detail_href.clone() class="card-detail-link"
                                    title="Open detail">
                                    <i class="ph ph-arrow-square-out" style="font-size:15px"></i>
                                </a>
                            </div>

                            <div class="card-body">
                                <div class="thermostat-card-compact">
                                    <div class="thermostat-temp" style="font-size:1.7rem">{temp_str}</div>
                                    <div class="thermostat-status">
                                        <span class=format!("pill {pill_class}")>{call}</span>
                                        <span class="glue-meta">{format!(" → {sp_str}")}</span>
                                    </div>

                                    <div class="glue-ctrl-row" style="margin-top:0.4rem">
                                        <div class="glue-ctrl-btns">
                                            <button class="hc-btn hc-btn--sm"
                                                on:click=move |_| send_sp_minus(json!({"command":"set_setpoint","value": sp - 0.5}))
                                            >"−"</button>
                                            <span class="glue-ctrl-value" style="font-size:1rem; min-width:3rem">{sp_str.clone()}</span>
                                            <button class="hc-btn hc-btn--sm"
                                                on:click=move |_| send_sp_plus(json!({"command":"set_setpoint","value": sp + 0.5}))
                                            >"+"</button>
                                        </div>
                                    </div>

                                    <div class="glue-ctrl-row" style="margin-top:0.4rem">
                                        <div class="toggle-group">
                                            <button class:active=mode_heat
                                                on:click=move |_| send_heat(json!({"command":"set_mode","value":"heat"}))
                                            >"Heat"</button>
                                            <button class:active=mode_cool
                                                on:click=move |_| send_cool(json!({"command":"set_mode","value":"cool"}))
                                            >"Cool"</button>
                                            <button class:active=mode_off
                                                on:click=move |_| send_off(json!({"command":"set_mode","value":"off"}))
                                            >"Off"</button>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </div>
                    }.into_any()

                } else {
                    // ── Standard Card ─────────────────────────────────────────
                    let send_on  = send.clone();
                    let send_off = send.clone();
                    let send_bri = send.clone();
                    let send_lock = send.clone();

                    // Compose dynamic style: --card-color set when the
                    // device exposes color_xy; --card-brightness scales
                    // the success-glow opacity (0.0 → no halo, 1.0 →
                    // full halo). Defaults to 1.0 for non-dimmers.
                    let style_str = {
                        let mut s = String::new();
                        if has_brightness {
                            s.push_str(&format!("--card-brightness:{:.2};", (brightness_pct as f64) / 100.0));
                        }
                        if let Some(c) = color_css.as_ref() {
                            s.push_str(&format!("--card-color:{};", c));
                        }
                        s
                    };
                    // Precompute static facts so the move-closures below
                    // don't have to borrow `d`.
                    let is_lighting = matches!(
                        presentation_device_type_key(&d),
                        "light" | "dimmer" | "switch" | "virtual_switch"
                    );
                    let show_on_glow = cur_on && is_lighting;

                    view! {
                        <div
                            class={format!("device-card {}", type_class)}
                            class:device-card--offline=!avail
                            class:device-card--scene=is_scene
                            class:device-card--on=show_on_glow
                            class:device-card--pulse=move || pulse.get()
                            style=style_str
                        >
                            // ── Header ────────────────────────────────────────
                            <div class="card-header">
                                <span class=format!("card-status-icon status-badge-sm {}", tone.css_class())>
                                    <i class=format!("{} card-mdi-icon", mdi)></i>
                                </span>
                                <div class="card-header-text">
                                    <p class="card-name" title=name.clone()>{name.clone()}</p>
                                    <p class="card-meta">
                                        {area.clone().map(|a| view! {
                                            <span>{a}</span>
                                            <span class="card-meta-sep">" · "</span>
                                        })}
                                        <span>{type_label}</span>
                                    </p>
                                </div>
                                <span
                                    class="card-avail-dot"
                                    class:card-avail-dot--on=avail
                                    class:card-avail-dot--off=!avail
                                    title=if avail { "Online" } else { "Offline" }
                                ></span>
                            </div>

                            // ── Body ──────────────────────────────────────────
                            <div class="card-body">
                                // Timer: live countdown + progress bar
                                {is_timer.then(|| view! {
                                    <CardTimerCountdown device=device />
                                    <div class="card-timer-state">
                                        <span class=format!("card-timer-badge timer-badge--{}", timer_st)>
                                            {timer_st.clone()}
                                        </span>
                                    </div>
                                })}

                                // Sensor / switch / generic state badge
                                // Hidden for devices with a toggle or lock button
                                // (state is shown on the button itself).
                                {(!is_timer && !is_scene && !can_toggle && !can_lock).then(|| view! {
                                    <div class="card-state-row">
                                        <span class=format!(
                                            "card-state-badge card-state-badge--tone-{}",
                                            tone.css_class()
                                        )>
                                            {state_text.clone()}
                                        </span>
                                    </div>
                                })}

                                // Controls
                                <div class="card-controls">
                                    // (timer devices: no card controls — use device detail page)

                                    // Switch/light toggle
                                    {can_toggle.then(move || view! {
                                        <button
                                            class="card-ctrl-btn"
                                            class:card-ctrl-btn--on=cur_on
                                            class:card-ctrl-btn--off=!cur_on
                                            disabled={move || busy.get() || !avail}
                                            on:click=move |_| {
                                                if cur_on {
                                                    send_off(serde_json::json!({"on": false}));
                                                } else {
                                                    send_on(serde_json::json!({"on": true}));
                                                }
                                            }
                                        >
                                            <i class="ph ph-power" style="font-size:18px"></i>
                                            {if cur_on { " Turn off" } else { " Turn on" }}
                                        </button>
                                    })}

                                    // Lock/unlock toggle
                                    {can_lock.then(move || view! {
                                        <button
                                            class="card-ctrl-btn"
                                            class:card-ctrl-btn--on=cur_locked
                                            class:card-ctrl-btn--off=!cur_locked
                                            disabled={move || busy.get() || !avail}
                                            on:click=move |_| {
                                                send_lock(serde_json::json!({"locked": !cur_locked}));
                                            }
                                        >
                                            <i class=move || if cur_locked { "ph ph-lock" } else { "ph ph-lock-open" } style="font-size:18px"></i>
                                            {if cur_locked { " Unlock" } else { " Lock" }}
                                        </button>
                                    })}
                                </div>

                                // Brightness slider for dimmer devices
                                {has_brightness.then(move || view! {
                                    <div class="card-brightness-row">
                                        <i class="ph ph-sun card-brightness-icon"></i>
                                        <input
                                            type="range"
                                            class="card-brightness-slider"
                                            min="0" max="100"
                                            value=brightness_pct.to_string()
                                            disabled=move || busy.get() || !avail
                                            on:change=move |ev| {
                                                let el = ev.target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok());
                                                if let Some(el) = el {
                                                    let v: u64 = el.value().parse().unwrap_or(0);
                                                    send_bri(serde_json::json!({"brightness_pct": v}));
                                                }
                                            }
                                        />
                                        <span class="card-brightness-pct">{format!("{}%", brightness_pct)}</span>
                                    </div>
                                })}
                            </div>

                            // ── Footer ────────────────────────────────────────
                            <div class="card-footer">
                                <span class="card-last-changed">
                                    {format_abs(last.as_ref())}
                                </span>
                                <a href=detail_href.clone() class="card-detail-link">
                                    <i class="ph ph-arrow-square-out" style="font-size:15px"></i>
                                </a>
                            </div>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

// ── DeviceCardsPage ───────────────────────────────────────────────────────────

#[component]
pub fn DeviceCardsPage() -> impl IntoView {
    let ws = use_ws();
    let auth = use_auth();

    // Seed the shared device map from REST on first load.
    Effect::new(move |_| {
        let token = auth.token_str().unwrap_or_default();
        spawn_local(async move {
            if let Ok(list) = fetch_devices(&token).await {
                ws.devices.update(|m| {
                    for d in list {
                        m.insert(d.device_id.clone(), d);
                    }
                });
            }
        });
    });

    let prefs = load_prefs();
    let card_size = RwSignal::new(prefs.card_size);
    // Whether the .filter-body (multi-select dropdowns) is expanded.
    // Below 768px, CSS hides the body unless this is true; above
    // 768px, the body is always shown and this toggle is hidden.
    let filters_expanded = RwSignal::new(false);
    let search = RwSignal::new(prefs.search);
    let avail_filter = RwSignal::new(prefs.avail_filter);
    let area_filter = RwSignal::new(prefs.area_filter);
    let type_filter = RwSignal::new(prefs.type_filter);
    let plugin_filter = RwSignal::new(prefs.plugin_filter);
    let sort_by = RwSignal::new(prefs.sort_by);
    let sort_dir = RwSignal::new(prefs.sort_dir);


    // Persist preferences
    Effect::new(move |_| {
        save_prefs(
            card_size.get(),
            &search.get(),
            &avail_filter.get(),
            &area_filter.get(),
            &type_filter.get(),
            &plugin_filter.get(),
            sort_by.get(),
            sort_dir.get(),
        );
    });

    // ── Filter option lists ───────────────────────────────────────────────────

    // Filter option lists — each returns (value, display_label) pairs

    let avail_options: Signal<Vec<(String, String)>> = Signal::derive(|| {
        vec![
            ("online".to_string(), "Online".to_string()),
            ("offline".to_string(), "Offline".to_string()),
        ]
    });

    let sort_options: Signal<Vec<(String, String)>> = Signal::derive(|| {
        vec![
            ("name".to_string(), "Sort: Name".to_string()),
            ("area".to_string(), "Sort: Area".to_string()),
            ("status".to_string(), "Sort: Status".to_string()),
            ("type".to_string(), "Sort: Type".to_string()),
            ("last_seen".to_string(), "Sort: Last Changed".to_string()),
        ]
    });

    let area_options: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let mut areas: Vec<String> = ws
            .devices
            .get()
            .values()
            .filter(|d| !is_scene_like(d))
            .filter_map(|d| d.area.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        areas.sort_by_key(|a| display_area_name(a));
        areas
            .into_iter()
            .map(|a| {
                let lbl = display_area_name(&a);
                (a, lbl)
            })
            .collect()
    });

    let type_options: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let mut types: Vec<String> = ws
            .devices
            .get()
            .values()
            .filter(|d| !is_scene_like(d))
            .map(|d| presentation_device_type_label(d).to_string())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        types.sort();
        types.into_iter().map(|t| (t.clone(), t)).collect()
    });

    let plugin_options: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let mut plugins: Vec<String> = ws
            .devices
            .get()
            .values()
            .filter(|d| !is_scene_like(d))
            .map(|d| d.plugin_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        plugins.sort();
        plugins.into_iter().map(|p| (p.clone(), p)).collect()
    });

    // ── Focus filter (URL-driven) ────────────────────────────────────────────
    //
    // The hero's Security tile navigates here with `?focus=security` to
    // pre-filter to security-tagged devices currently in alert state. Read
    // once via the location hash; reactive on URL change.
    let location = leptos_router::hooks::use_location();
    let focus: Memo<Option<String>> = Memo::new(move |_| {
        let raw = location.search.get();
        // Parse "?focus=security&x=y" — first match wins.
        raw.trim_start_matches('?')
            .split('&')
            .find_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                (k == "focus").then(|| v.to_string())
            })
    });

    // Battery threshold from `?below=N`; defaults to 20 when not specified.
    // Used by the `focus=battery` filter so its set matches the hero tile's
    // count exactly.
    let battery_below: Memo<f64> = Memo::new(move |_| {
        let raw = location.search.get();
        raw.trim_start_matches('?')
            .split('&')
            .find_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                (k == "below").then(|| v.parse::<f64>().ok()).flatten()
            })
            .unwrap_or(20.0)
    });

    // ── Sorted + filtered device ID list ─────────────────────────────────────
    //
    // Produces Vec<String> (ordered IDs) — not Vec<DeviceState>.
    // <For> keys on these IDs; each DeviceCard reads its own state via Memo.
    let card_ids: Memo<Vec<String>> = Memo::new(move |_| {
        let all = ws.devices.get();
        let q = search.get().trim().to_lowercase();
        let avail_f = avail_filter.get();
        let area_f = area_filter.get();
        let type_f = type_filter.get();
        let plugin_f = plugin_filter.get();
        let sb = sort_by.get();
        let sd = sort_dir.get();
        let focus_value = focus.get();
        let battery_threshold = battery_below.get();

        // Pre-compute once: which devices are in the security set?
        let security_tags = load_security_tags();
        let in_security_set = |d: &DeviceState| -> bool {
            if !security_tags.is_empty() {
                security_tags.contains(&d.device_id)
            } else {
                matches!(
                    d.device_type.as_deref(),
                    Some("lock") | Some("contact_sensor")
                )
            }
        };
        let device_in_alert = |d: &DeviceState| -> bool {
            match d.device_type.as_deref() {
                Some("lock") => !d
                    .attributes
                    .get("locked")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                Some("contact_sensor") => d
                    .attributes
                    .get("open")
                    .and_then(Value::as_bool)
                    .or_else(|| d.attributes.get("contact").and_then(Value::as_bool))
                    .unwrap_or(false),
                _ => false,
            }
        };

        let mut result: Vec<&DeviceState> = all
            .values()
            .filter(|d| !is_scene_like(d))
            .filter(|d| {
                // Focus filter trumps the manual filters. Each focus
                // mode narrows the list to "what the user clicked
                // through to see":
                //   security → security-relevant + in alert
                //   lighting → lights/dimmers/switches currently on
                //   climate  → all thermostats
                //   media    → all media players
                //   energy   → all power monitors
                match focus_value.as_deref() {
                    Some("security") => in_security_set(d) && device_in_alert(d),
                    Some("lighting") => {
                        // Match the hero's count exactly: lights + dimmers
                        // only, currently on. Switches are excluded — they
                        // often control non-light loads (fans, outlets,
                        // appliances) and including them would inflate
                        // "lighting on" beyond what the user means.
                        let is_lighting = matches!(
                            d.device_type.as_deref(),
                            Some("light") | Some("dimmer")
                        );
                        let is_on = d
                            .attributes
                            .get("on")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        is_lighting && is_on
                    }
                    Some("climate") => d.device_type.as_deref() == Some("thermostat"),
                    Some("media") => d.device_type.as_deref() == Some("media_player"),
                    Some("energy") => d.device_type.as_deref() == Some("power_monitor"),
                    Some("battery") => is_battery_low(d, battery_threshold).unwrap_or(false),
                    _ => true,
                }
            })
            .filter(|d| {
                avail_f.is_empty() || {
                    let key = if d.available { "online" } else { "offline" };
                    avail_f.contains(key)
                }
            })
            .filter(|d| {
                area_f.is_empty() || {
                    let a = d.area.as_deref().unwrap_or("Unassigned");
                    area_f.contains(a)
                }
            })
            .filter(|d| type_f.is_empty() || type_f.contains(presentation_device_type_label(d)))
            .filter(|d| plugin_f.is_empty() || plugin_f.contains(&d.plugin_id))
            .filter(|d| {
                if q.is_empty() {
                    return true;
                }
                let hay = format!(
                    "{} {} {} {} {} {}",
                    display_name(d),
                    d.device_id,
                    d.area.as_deref().unwrap_or(""),
                    presentation_device_type_label(d),
                    d.plugin_id,
                    status_text(d),
                )
                .to_lowercase();
                hay.contains(&q)
            })
            .collect();

        result.sort_by(|a, b| {
            let cmp = match sb {
                SortKey::Name => sort_key_str(display_name(a)).cmp(&sort_key_str(display_name(b))),
                SortKey::Area => sort_key_str(&display_area_value(a.area.as_deref()))
                    .cmp(&sort_key_str(&display_area_value(b.area.as_deref()))),
                SortKey::Status => cmp_status(a, b),
                SortKey::Type => sort_key_str(presentation_device_type_label(a))
                    .cmp(&sort_key_str(presentation_device_type_label(b))),
                SortKey::LastSeen => last_change_time(a).cmp(&last_change_time(b)),
            };
            if sd == SortDir::Desc {
                cmp.reverse()
            } else {
                cmp
            }
        });

        result.iter().map(|d| d.device_id.clone()).collect()
    });

    let total = Signal::derive(move || card_ids.get().len());
    let online_count = Signal::derive(move || {
        ws.devices
            .get()
            .values()
            .filter(|d| d.available && !is_scene_like(d))
            .count()
    });

    let canvas_class = move || card_size_canvas_class(card_size.get());

    // ── Chapters: bucket card_ids by area ─────────────────────────────────
    //
    // Each chapter is (area_label, device_ids_in_that_area), in chapter
    // display order: alphabetical by area name, with "Unassigned"
    // pinned last. Within a chapter, device order follows the sorted
    // card_ids (so the existing sort_by preference still applies inside
    // each chapter).
    let chapters: Memo<Vec<(String, Vec<String>)>> = Memo::new(move |_| {
        let ids = card_ids.get();
        let devmap = ws.devices.get();
        let mut groups: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for id in &ids {
            let label = devmap
                .get(id)
                .and_then(|d| d.area.as_deref())
                .map(display_area_name)
                .unwrap_or_else(|| "Unassigned".to_string());
            groups.entry(label).or_default().push(id.clone());
        }
        let mut chapters: Vec<(String, Vec<String>)> = groups.into_iter().collect();
        // Pin "Unassigned" to the bottom — areas-with-names first.
        chapters.sort_by(|a, b| match (a.0 == "Unassigned", b.0 == "Unassigned") {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => a.0.cmp(&b.0),
        });
        chapters
    });

    // Per-area collapse state — persisted to localStorage so reload
    // preserves which rooms the user has collapsed.
    let collapsed_areas: RwSignal<HashSet<String>> = RwSignal::new(load_collapsed_areas());
    Effect::new(move |_| {
        save_collapsed_areas(&collapsed_areas.get());
    });

    // Per-chapter "all off" pending flag, keyed by area label. Used to
    // disable the button + show a sending state while POSTs are in
    // flight. Not persisted — purely transient UI state.
    let chapter_sending: RwSignal<HashSet<String>> = RwSignal::new(HashSet::new());

    view! {
        <div class="page">
            <div class="heading">
                <div>
                    <h1>"Device Cards"</h1>
                    <p>
                        {move || format!("{} devices", total.get())}
                        " · "
                        {move || format!("{} online", online_count.get())}
                    </p>
                </div>
            </div>

            <LiveStatusBanner status=Signal::derive(move || ws.status.get()) />

            // Focus banner — appears when the URL carries ?focus={system}
            // (e.g. user clicked a hero tile). Reminds them they're
            // seeing a filtered subset and provides a way out.
            <Show when=move || focus.get().is_some()>
                {move || {
                    let f = focus.get().unwrap_or_default();
                    let bat_below = battery_below.get();
                    let (icon, headline, hint): (&'static str, String, &'static str) = match f.as_str() {
                        "security" => (
                            "ph ph-shield-check",
                            "Security-relevant devices currently needing attention.".into(),
                            if load_security_tags().is_empty() {
                                "No devices are tagged yet — falling back to all locks and contact sensors. \
                                 Mark specific devices on their detail page."
                            } else {
                                ""
                            },
                        ),
                        "lighting" => (
                            "ph ph-lightbulb",
                            "Lights and switches currently on.".into(),
                            "",
                        ),
                        "climate" => (
                            "ph ph-thermometer-simple",
                            "Climate devices in your home.".into(),
                            "",
                        ),
                        "media" => (
                            "ph ph-speaker-hifi",
                            "Media players in your home.".into(),
                            "",
                        ),
                        "energy" => (
                            "ph ph-lightning",
                            "Power monitors in your home.".into(),
                            "",
                        ),
                        "battery" => (
                            "ph ph-battery-low",
                            format!(
                                "Battery-powered devices flagged low — at or below {:.0}% \
                                 for percentage-reporting sensors, or marked low by their \
                                 plugin (e.g. Ecowitt voltage / level sensors).",
                                bat_below
                            ),
                            "",
                        ),
                        _ => (
                            "ph ph-funnel",
                            "Filtered view.".into(),
                            "",
                        ),
                    };
                    view! {
                        <div class="hc-focus-banner">
                            <i class={icon}></i>
                            <span class="hc-focus-banner__text">
                                {headline}
                                {(!hint.is_empty()).then(|| view! { " " {hint} })}
                            </span>
                            <a class="hc-focus-banner__clear" href="/devices">"Clear filter"</a>
                        </div>
                    }
                }}
            </Show>

            // ── Filter/sort toolbar ───────────────────────────────────────────
            <div
                class="filter-panel panel"
                class:filter-panel--expanded=move || filters_expanded.get()
            >
                <div class="filter-bar">
                    <SearchField search placeholder="Search name, area, type, plugin…" />

                    // Card size
                    <CardSizeSelect card_size />

                    // Sort
                    <SortSelect
                        current_value=Signal::derive(move || sort_key_to_str(sort_by.get()).to_string())
                        options=sort_options
                        on_change=Callback::new(move |value: String| {
                            sort_by.set(sort_key_from_str(Some(&value)));
                        })
                    />

                    <SortDirToggle sort_dir />

                    // Mobile-only toggle for the filter body. Hidden on
                    // desktop via CSS; tap to reveal/hide the multi-
                    // select dropdowns below. Active filter count
                    // surfaces so users can see at a glance whether
                    // any filtering is in effect.
                    <button
                        class="btn btn-outline filter-bar__toggle"
                        type="button"
                        on:click=move |_| filters_expanded.update(|v| *v = !*v)
                    >
                        <i class="ph ph-funnel" style="font-size:14px"></i>
                        {move || if filters_expanded.get() { "Hide filters" } else { "Filters" }}
                        {move || {
                            let n = avail_filter.get().len()
                                + area_filter.get().len()
                                + type_filter.get().len()
                                + plugin_filter.get().len();
                            (n > 0).then(|| view! { <span class="filter-bar__count">{n}</span> })
                        }}
                    </button>

                </div>

                <div class="filter-body">
                    <div class="filter-multisel-row">
                        <MultiSelectDropdown
                            label="statuses"
                            placeholder="All statuses"
                            options=avail_options
                            selected=avail_filter
                        />
                        <MultiSelectDropdown
                            label="areas"
                            placeholder="All areas"
                            options=Signal::derive(move || area_options.get())
                            selected=area_filter
                        />
                        <MultiSelectDropdown
                            label="types"
                            placeholder="All types"
                            options=Signal::derive(move || type_options.get())
                            selected=type_filter
                        />
                        <MultiSelectDropdown
                            label="plugins"
                            placeholder="All plugins"
                            options=Signal::derive(move || plugin_options.get())
                            selected=plugin_filter
                        />
                        <ResetFiltersButton on_reset=Callback::new(move |_| {
                            search.set(String::new());
                            avail_filter.set(HashSet::new());
                            area_filter.set(HashSet::new());
                            type_filter.set(HashSet::new());
                            plugin_filter.set(HashSet::new());
                            sort_by.set(SortKey::Name);
                            sort_dir.set(SortDir::Asc);
                        }) />
                    </div>
                </div>
            </div>

            // ── Dashboard canvas ──────────────────────────────────────────────
            //
            // data-canvas="device-cards" marks this as the drag/drop surface.
            // When drag/drop is added:
            //   1. Add on:dragover + on:drop handlers here
            //   2. Add draggable="true" to .card-slot
            //   3. Maintain canvas_layout: RwSignal<Vec<String>> for ordering
            <div data-canvas="device-cards">
                {move || {
                    if card_ids.get().is_empty() {
                        if ws.devices.get().is_empty() {
                            view! {
                                <div class="cards-empty hc-skeleton-grid">
                                    <div class="hc-skeleton hc-skeleton--card"></div>
                                    <div class="hc-skeleton hc-skeleton--card"></div>
                                    <div class="hc-skeleton hc-skeleton--card"></div>
                                    <div class="hc-skeleton hc-skeleton--card"></div>
                                    <div class="hc-skeleton hc-skeleton--card"></div>
                                    <div class="hc-skeleton hc-skeleton--card"></div>
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <div class="cards-empty">
                                    <div class="hc-empty">
                                        <i class="ph ph-funnel hc-empty__icon"></i>
                                        <div class="hc-empty__title">"No devices match"</div>
                                        <p class="hc-empty__body">
                                            "Try clearing filters or widening the search to see all \
                                             registered devices."
                                        </p>
                                    </div>
                                </div>
                            }.into_any()
                        }
                    } else {
                        let canvas_cls = canvas_class();
                        view! {
                            <div>
                                {chapters.get().into_iter().map(|(area, ids)| {
                                    // Wrap area + id list in StoredValue so child
                                    // closures can read them without moving —
                                    // StoredValue is Copy and Fn-safe.
                                    let area_sv: StoredValue<String, leptos::reactive::owner::LocalStorage> =
                                        StoredValue::new_local(area.clone());
                                    let ids_sv: StoredValue<Vec<String>, leptos::reactive::owner::LocalStorage> =
                                        StoredValue::new_local(ids.clone());
                                    let count = ids.len();

                                    view! {
                                        <section
                                            class="hc-chapter"
                                            class:hc-chapter--collapsed=move || area_sv.with_value(|a| collapsed_areas.get().contains(a))
                                        >
                                            <header class="hc-chapter__head">
                                                <span class="hc-chapter__name">
                                                    {area.to_lowercase()}
                                                </span>
                                                <span class="hc-chapter__count">
                                                    {format!("{count} {}", if count == 1 { "device" } else { "devices" })}
                                                </span>
                                                <span class="hc-chapter__sep"></span>
                                                <Show when=move || {
                                                    let devmap = ws.devices.get();
                                                    ids_sv.with_value(|ids| ids.iter().any(|id| {
                                                        devmap.get(id)
                                                            .and_then(|d| d.attributes.get("on"))
                                                            .and_then(|v| v.as_bool())
                                                            .unwrap_or(false)
                                                    }))
                                                }>
                                                    <button
                                                        class="hc-chapter__action"
                                                        title="Turn off every light/switch in this room that's currently on"
                                                        disabled=move || area_sv.with_value(|a| chapter_sending.get().contains(a))
                                                        on:click=move |_| {
                                                            let token = auth.token_str().unwrap_or_default();
                                                            let key = area_sv.with_value(|a| a.clone());
                                                            let devmap = ws.devices.get();
                                                            let targets: Vec<String> = ids_sv.with_value(|ids| {
                                                                ids.iter()
                                                                    .filter(|id| {
                                                                        devmap
                                                                            .get(*id)
                                                                            .and_then(|d| d.attributes.get("on"))
                                                                            .and_then(|v| v.as_bool())
                                                                            .unwrap_or(false)
                                                                    })
                                                                    .cloned()
                                                                    .collect()
                                                            });
                                                            if targets.is_empty() { return; }
                                                            chapter_sending.update(|s| { s.insert(key.clone()); });
                                                            spawn_local(async move {
                                                                for id in targets {
                                                                    let _ = set_device_state(
                                                                        &token,
                                                                        &id,
                                                                        &serde_json::json!({"on": false}),
                                                                    ).await;
                                                                }
                                                                chapter_sending.update(|s| { s.remove(&key); });
                                                            });
                                                        }
                                                    >
                                                        {move || if area_sv.with_value(|a| chapter_sending.get().contains(a)) {
                                                            "sending…"
                                                        } else {
                                                            "all off"
                                                        }}
                                                    </button>
                                                </Show>
                                                <button
                                                    class="hc-chapter__collapse"
                                                    title="Collapse / expand"
                                                    on:click=move |_| {
                                                        let key = area_sv.with_value(|a| a.clone());
                                                        collapsed_areas.update(|s| {
                                                            if !s.remove(&key) { s.insert(key); }
                                                        });
                                                    }
                                                >
                                                    <i class=move || {
                                                        if area_sv.with_value(|a| collapsed_areas.get().contains(a)) {
                                                            "ph ph-caret-right"
                                                        } else {
                                                            "ph ph-caret-down"
                                                        }
                                                    }></i>
                                                </button>
                                            </header>
                                            <div class={format!("hc-chapter__body {canvas_cls}")}>
                                                <For
                                                    each=move || ids_sv.with_value(|v| v.clone())
                                                    key=|id| id.clone()
                                                    children=move |device_id| view! {
                                                        <DeviceCard device_id=device_id />
                                                    }
                                                />
                                            </div>
                                        </section>
                                    }
                                }).collect_view()}
                            </div>
                        }.into_any()
                    }
                }}
            </div>
        </div>
    }
}

// ── Status sort helper ────────────────────────────────────────────────────────

fn cmp_status(a: &DeviceState, b: &DeviceState) -> std::cmp::Ordering {
    // Online first, then by name
    match (a.available, b.available) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => sort_key_str(display_name(a)).cmp(&sort_key_str(display_name(b))),
    }
}
