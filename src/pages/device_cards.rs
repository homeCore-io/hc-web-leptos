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
use crate::ws::{WsStatus, use_ws};
use leptos::prelude::*;
use leptos::task::spawn_local;
use std::collections::HashSet;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

// ── MultiSelectDropdown ───────────────────────────────────────────────────────
//
// Generic multi-select dropdown.  `options` is (value, display_label).
// Empty `selected` set means "no filter / show all".

#[component]
fn MultiSelectDropdown(
    /// Short category label shown in summary when items are selected, e.g. "Areas"
    label: &'static str,
    /// Text shown when nothing is selected
    placeholder: &'static str,
    #[prop(into)] options: Signal<Vec<(String, String)>>,
    selected: RwSignal<HashSet<String>>,
) -> impl IntoView {
    let open = RwSignal::new(false);

    let summary = move || {
        let sel = selected.get();
        if sel.is_empty() {
            placeholder.to_string()
        } else if sel.len() == 1 {
            sel.iter().next().unwrap().clone()
        } else {
            format!("{} {} selected", sel.len(), label)
        }
    };

    view! {
        <div class="multisel">
            <button
                class="multisel-trigger"
                class:multisel-trigger--active=move || !selected.get().is_empty()
                on:click=move |ev| {
                    ev.stop_propagation();
                    open.update(|v| *v = !*v);
                }
            >
                <span class="multisel-summary">{summary}</span>
                <span class="material-icons" style="font-size:14px">
                    {move || if open.get() { "expand_less" } else { "expand_more" }}
                </span>
            </button>
            {move || open.get().then(|| {
                let opts = options.get();
                view! {
                    // Full-screen backdrop — clicking outside the dropdown closes it
                    <div
                        class="multisel-backdrop"
                        on:mousedown=move |_| open.set(false)
                    ></div>
                    <div class="multisel-dropdown">
                        {opts.into_iter().map(|(val, lbl)| {
                            let v_check = val.clone();
                            let v_toggle = val.clone();
                            view! {
                                <label class="multisel-option">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || selected.get().contains(&v_check)
                                        on:change=move |_| {
                                            let v = v_toggle.clone();
                                            selected.update(|s| {
                                                if s.contains(&v) { s.remove(&v); } else { s.insert(v); }
                                            });
                                        }
                                    />
                                    {lbl}
                                </label>
                            }
                        }).collect_view()}
                        {move || (!selected.get().is_empty()).then(|| view! {
                            <button
                                class="multisel-clear"
                                on:click=move |_| selected.set(HashSet::new())
                            >"Clear"</button>
                        })}
                    </div>
                }
            })}
        </div>
    }
}

// ── Card size ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardSize {
    Small,
    Medium,
    Large,
}

// ── Prefs ─────────────────────────────────────────────────────────────────────

const CARDS_PREFS_KEY: &str = "hc-leptos:cards:prefs";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Name,
    Area,
    Status,
    Type,
    LastSeen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortDir {
    Asc,
    Desc,
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

fn json_str_set(v: &serde_json::Value, key: &str) -> HashSet<String> {
    v[key]
        .as_array()
        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn load_prefs() -> CardsPrefs {
    let raw = match ls_get(CARDS_PREFS_KEY) {
        Some(s) => s,
        None => return CardsPrefs::default(),
    };
    let v: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return CardsPrefs::default(),
    };
    CardsPrefs {
        card_size: match v["card_size"].as_str() {
            Some("small") => CardSize::Small,
            Some("large") => CardSize::Large,
            _ => CardSize::Medium,
        },
        search: v["search"].as_str().unwrap_or("").to_string(),
        avail_filter:  json_str_set(&v, "avail_filter"),
        area_filter:   json_str_set(&v, "area_filter"),
        type_filter:   json_str_set(&v, "type_filter"),
        plugin_filter: json_str_set(&v, "plugin_filter"),
        sort_by: match v["sort_by"].as_str() {
            Some("area") => SortKey::Area,
            Some("status") => SortKey::Status,
            Some("type") => SortKey::Type,
            Some("last_seen") => SortKey::LastSeen,
            _ => SortKey::Name,
        },
        sort_dir: if v["sort_dir"] == "desc" { SortDir::Desc } else { SortDir::Asc },
    }
}

fn set_to_json_array(s: &HashSet<String>) -> serde_json::Value {
    serde_json::Value::Array(s.iter().map(|v| serde_json::Value::String(v.clone())).collect())
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
    let value = serde_json::json!({
        "card_size": match card_size {
            CardSize::Small => "small",
            CardSize::Medium => "medium",
            CardSize::Large => "large",
        },
        "search": search,
        "avail_filter":  set_to_json_array(avail_filter),
        "area_filter":   set_to_json_array(area_filter),
        "type_filter":   set_to_json_array(type_filter),
        "plugin_filter": set_to_json_array(plugin_filter),
        "sort_by": match sort_by {
            SortKey::Name => "name",
            SortKey::Area => "area",
            SortKey::Status => "status",
            SortKey::Type => "type",
            SortKey::LastSeen => "last_seen",
        },
        "sort_dir": if sort_dir == SortDir::Desc { "desc" } else { "asc" },
    });
    ls_set(CARDS_PREFS_KEY, &value.to_string());
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
    let device: Memo<Option<DeviceState>> = Memo::new(move |_| {
        ws.devices.get().get(&did).cloned()
    });

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
                let icon    = status_icon_name(&d);
                let last    = last_change_time(&d).copied();

                let is_timer  = is_timer_device(&d);
                let is_media  = is_media_player(&d);
                let is_scene  = is_scene_like(&d);
                let can_toggle = supports_inline_toggle(&d);

                let cur_on  = bool_attr(d.attributes.get("on")).unwrap_or(false);
                let pb      = playback_state(&d);
                let is_playing = pb == "playing";
                let timer_st = str_attr(d.attributes.get("state"))
                    .unwrap_or("idle").to_string();
                let state_text = status_text(&d);
                let media_title_str = media_title(&d).map(str::to_string);
                let media_artist_str = media_artist(&d).map(str::to_string);

                // ID clones for closures
                let send_on    = send.clone();
                let send_off   = send.clone();
                let send_play  = send.clone();
                let send_pause = send.clone();

                view! {
                    <div
                        class="device-card"
                        class:device-card--offline=!avail
                        class:device-card--scene=is_scene
                    >
                        // ── Header ────────────────────────────────────────────
                        <div class="card-header">
                            <span class=format!("card-status-icon status-badge-sm {}", tone.css_class())>
                                <span class="material-icons" style="font-size:18px">{icon}</span>
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

                        // ── Body ──────────────────────────────────────────────
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

                            // Media: now playing info
                            {is_media.then(|| {
                                let title = media_title_str.clone();
                                let artist = media_artist_str.clone();
                                view! {
                                    <div class="card-media-info">
                                        {match title {
                                            Some(t) => view! {
                                                <p class="card-media-title">{t}</p>
                                                {artist.map(|a| view! {
                                                    <p class="card-media-artist">{a}</p>
                                                })}
                                            }.into_any(),
                                            None => view! {
                                                <p class="card-media-stopped">{pb.clone()}</p>
                                            }.into_any(),
                                        }}
                                    </div>
                                }
                            })}

                            // Sensor / switch / generic state badge
                            {(!is_timer && !is_media && !is_scene).then(|| view! {
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

                                // Media controls
                                {is_media.then(move || view! {
                                    <button
                                        class="card-ctrl-btn card-ctrl-btn--icon"
                                        disabled={move || busy.get() || !avail}
                                        on:click=move |_| {
                                            if is_playing {
                                                send_pause(serde_json::json!({"action":"pause"}));
                                            } else {
                                                send_play(serde_json::json!({"action":"play"}));
                                            }
                                        }
                                    >
                                        <span class="material-icons" style="font-size:20px">
                                            {if is_playing { "pause" } else { "play_arrow" }}
                                        </span>
                                    </button>
                                })}

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
                        </div>

                        // ── Footer ────────────────────────────────────────────
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
    let card_size     = RwSignal::new(prefs.card_size);
    let search        = RwSignal::new(prefs.search);
    let avail_filter  = RwSignal::new(prefs.avail_filter);
    let area_filter   = RwSignal::new(prefs.area_filter);
    let type_filter   = RwSignal::new(prefs.type_filter);
    let plugin_filter = RwSignal::new(prefs.plugin_filter);
    let sort_by       = RwSignal::new(prefs.sort_by);
    let sort_dir      = RwSignal::new(prefs.sort_dir);
    let filter_open   = RwSignal::new(false);

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

    let avail_options: Signal<Vec<(String, String)>> = Signal::derive(|| vec![
        ("online".to_string(),  "Online".to_string()),
        ("offline".to_string(), "Offline".to_string()),
    ]);

    let area_options: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let mut areas: Vec<String> = ws
            .devices.get().values()
            .filter(|d| !is_scene_like(d))
            .filter_map(|d| d.area.clone())
            .collect::<HashSet<_>>().into_iter().collect();
        areas.sort_by_key(|a| display_area_name(a));
        areas.into_iter()
            .map(|a| { let lbl = display_area_name(&a); (a, lbl) })
            .collect()
    });

    let type_options: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let mut types: Vec<String> = ws
            .devices.get().values()
            .filter(|d| !is_scene_like(d))
            .map(|d| presentation_device_type_label(d).to_string())
            .collect::<HashSet<_>>().into_iter().collect();
        types.sort();
        types.into_iter().map(|t| (t.clone(), t)).collect()
    });

    let plugin_options: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let mut plugins: Vec<String> = ws
            .devices.get().values()
            .filter(|d| !is_scene_like(d))
            .map(|d| d.plugin_id.clone())
            .collect::<HashSet<_>>().into_iter().collect();
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
        let avail_f  = avail_filter.get();
        let area_f   = area_filter.get();
        let type_f   = type_filter.get();
        let plugin_f = plugin_filter.get();
        let sb = sort_by.get();
        let sd = sort_dir.get();

        let mut result: Vec<&DeviceState> = all
            .values()
            .filter(|d| !is_scene_like(d))
            .filter(|d| avail_f.is_empty() || {
                let key = if d.available { "online" } else { "offline" };
                avail_f.contains(key)
            })
            .filter(|d| area_f.is_empty() || {
                let a = d.area.as_deref().unwrap_or("Unassigned");
                area_f.contains(a)
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
                SortKey::Area => {
                    sort_key_str(&display_area_value(a.area.as_deref()))
                        .cmp(&sort_key_str(&display_area_value(b.area.as_deref())))
                }
                SortKey::Status => cmp_status(a, b),
                SortKey::Type => sort_key_str(presentation_device_type_label(a))
                    .cmp(&sort_key_str(presentation_device_type_label(b))),
                SortKey::LastSeen => last_change_time(a).cmp(&last_change_time(b)),
            };
            if sd == SortDir::Desc { cmp.reverse() } else { cmp }
        });

        result.iter().map(|d| d.device_id.clone()).collect()
    });

    let total = Signal::derive(move || card_ids.get().len());
    let online_count = Signal::derive(move || {
        ws.devices.get().values()
            .filter(|d| d.available && !is_scene_like(d))
            .count()
    });

    let canvas_class = move || match card_size.get() {
        CardSize::Small  => "cards-canvas cards-canvas--sm",
        CardSize::Medium => "cards-canvas cards-canvas--md",
        CardSize::Large  => "cards-canvas cards-canvas--lg",
    };

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

            // WS status banner
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

            // ── Filter/sort toolbar ───────────────────────────────────────────
            <div class="filter-panel panel">
                <div class="filter-bar">
                    <input
                        type="search"
                        class="search-input"
                        placeholder="Search name, area, type, plugin…"
                        prop:value=move || search.get()
                        on:input=move |ev| search.set(event_target_value(&ev))
                    />

                    // Card size
                    <select
                        on:change=move |ev| {
                            card_size.set(match event_target_value(&ev).as_str() {
                                "small" => CardSize::Small,
                                "large" => CardSize::Large,
                                _ => CardSize::Medium,
                            });
                        }
                    >
                        <option value="small" selected={move || card_size.get() == CardSize::Small}>"Small"</option>
                        <option value="medium" selected={move || card_size.get() == CardSize::Medium}>"Medium"</option>
                        <option value="large" selected={move || card_size.get() == CardSize::Large}>"Large"</option>
                    </select>

                    // Sort
                    <select
                        on:change=move |ev| {
                            sort_by.set(match event_target_value(&ev).as_str() {
                                "area"      => SortKey::Area,
                                "status"    => SortKey::Status,
                                "type"      => SortKey::Type,
                                "last_seen" => SortKey::LastSeen,
                                _           => SortKey::Name,
                            });
                        }
                    >
                        <option value="name"     selected={move || sort_by.get() == SortKey::Name}>"Sort: Name"</option>
                        <option value="area"     selected={move || sort_by.get() == SortKey::Area}>"Sort: Area"</option>
                        <option value="status"   selected={move || sort_by.get() == SortKey::Status}>"Sort: Status"</option>
                        <option value="type"     selected={move || sort_by.get() == SortKey::Type}>"Sort: Type"</option>
                        <option value="last_seen" selected={move || sort_by.get() == SortKey::LastSeen}>"Sort: Last Changed"</option>
                    </select>

                    <button
                        class="filter-toggle"
                        class:filter-toggle--active=move || sort_dir.get() == SortDir::Desc
                        on:click=move |_| {
                            sort_dir.update(|d| *d = if *d == SortDir::Asc { SortDir::Desc } else { SortDir::Asc });
                        }
                    >
                        {move || if sort_dir.get() == SortDir::Asc {
                            view! { <span class="material-icons" style="font-size:16px">"arrow_upward"</span> }
                        } else {
                            view! { <span class="material-icons" style="font-size:16px">"arrow_downward"</span> }
                        }}
                    </button>

                    <button
                        class="filter-toggle"
                        on:click=move |_| filter_open.update(|v| *v = !*v)
                    >
                        <span class="material-icons" style="font-size:16px;vertical-align:middle">"tune"</span>
                        {move || if filter_open.get() { " Less" } else { " Filters" }}
                    </button>
                </div>

                // Expanded filters
                {move || filter_open.get().then(|| view! {
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
                            <button
                                class="btn-outline"
                                on:click=move |_| {
                                    search.set(String::new());
                                    avail_filter.set(HashSet::new());
                                    area_filter.set(HashSet::new());
                                    type_filter.set(HashSet::new());
                                    plugin_filter.set(HashSet::new());
                                    sort_by.set(SortKey::Name);
                                    sort_dir.set(SortDir::Asc);
                                }
                            >"Reset"</button>
                        </div>
                    </div>
                })}
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
