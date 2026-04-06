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
fn DeviceCard(device_id: String) -> impl IntoView {
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

    // Convenience: send a state command to this device
    let did_cmd = device_id.clone();
    let send = move |body: serde_json::Value| {
        let token = auth.token_str().unwrap_or_default();
        let id = did_cmd.clone();
        busy.set(true);
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

                let is_timer  = is_timer_device(&d);
                let is_media  = is_media_player(&d);
                let is_scene  = is_scene_like(&d);
                let can_toggle = supports_inline_toggle(&d);
                let has_brightness = d.attributes.get("brightness_pct").and_then(|v| v.as_f64()).is_some();
                let brightness_pct = d.attributes.get("brightness_pct")
                    .and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;

                let cur_on  = bool_attr(d.attributes.get("on")).unwrap_or(false);
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
                                            <span class="material-icons">"music_note"</span>
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
                                        <span class="material-icons">"skip_previous"</span>
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
                                    <span class="material-icons">
                                        {if is_playing { "pause" } else { "play_arrow" }}
                                    </span>
                                </button>
                                {sup_next.then(move || view! {
                                    <button
                                        class="card-media-ctrl"
                                        disabled={move || busy.get() || !avail}
                                        on:click=move |_| {
                                            send_next(serde_json::json!({"action":"next"}));
                                        }
                                    >
                                        <span class="material-icons">"skip_next"</span>
                                    </button>
                                })}
                            </div>

                            // ── Volume row ────────────────────────────────────
                            <div class="card-media-vol-row">
                                <span class="material-icons card-media-vol-icon">"volume_down"</span>
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
                                    <span class="material-icons" style="font-size:15px">"open_in_new"</span>
                                </a>
                            </div>
                        </div>
                    }.into_any()

                } else {
                    // ── Standard Card ─────────────────────────────────────────
                    let send_on  = send.clone();
                    let send_off = send.clone();
                    let send_bri = send.clone();

                    view! {
                        <div
                            class="device-card"
                            class:device-card--offline=!avail
                            class:device-card--scene=is_scene
                        >
                            // ── Header ────────────────────────────────────────
                            <div class="card-header">
                                <span class=format!("card-status-icon status-badge-sm {}", tone.css_class())>
                                    <i class=format!("mdi {} card-mdi-icon", mdi)></i>
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
                                // Hidden for devices with a toggle button (state is shown on the button)
                                {(!is_timer && !is_scene && !can_toggle).then(|| view! {
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
                                            <span class="material-icons" style="font-size:18px">"power_settings_new"</span>
                                            {if cur_on { " Turn off" } else { " Turn on" }}
                                        </button>
                                    })}
                                </div>

                                // Brightness slider for dimmer devices
                                {has_brightness.then(move || view! {
                                    <div class="card-brightness-row">
                                        <span class="material-icons card-brightness-icon">"light_mode"</span>
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
                                    <span class="material-icons" style="font-size:15px">"open_in_new"</span>
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

        let mut result: Vec<&DeviceState> = all
            .values()
            .filter(|d| !is_scene_like(d))
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

            // ── Filter/sort toolbar ───────────────────────────────────────────
            <div class="filter-panel panel">
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
            <div
                class=canvas_class
                data-canvas="device-cards"
            >
                {move || {
                    if card_ids.get().is_empty() {
                        view! {
                            <p class="cards-empty">
                                {if ws.devices.get().is_empty() {
                                    "Loading devices…"
                                } else {
                                    "No devices match the current filters."
                                }}
                            </p>
                        }.into_any()
                    } else {
                        view! {
                            <For
                                each=move || card_ids.get()
                                key=|id| id.clone()
                                children=move |device_id| view! {
                                    <DeviceCard device_id=device_id />
                                }
                            />
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
