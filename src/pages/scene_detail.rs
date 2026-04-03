//! Scene detail pages — native scene editor and plugin scene detail.

use crate::api::{
    activate_scene, create_scene, delete_scene, fetch_device, fetch_devices, fetch_scene,
    set_device_state, update_scene,
};
use crate::auth::use_auth;
use crate::models::*;
use crate::ws::use_ws;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_params_map};
use serde_json::{Map, Value};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
struct SceneMemberDraft {
    device_id: String,
    payload_text: String,
}

fn device_display(devices: &[DeviceState], device_id: &str) -> String {
    devices
        .iter()
        .find(|device| device.device_id == device_id)
        .map(|device| format!("{} ({})", device.name, device.device_id))
        .unwrap_or_else(|| device_id.to_string())
}

fn scene_to_rows(scene: &Scene) -> Vec<SceneMemberDraft> {
    scene
        .states
        .iter()
        .map(|(device_id, value)| SceneMemberDraft {
            device_id: device_id.clone(),
            payload_text: serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
        })
        .collect()
}

fn rows_to_states(rows: &[SceneMemberDraft]) -> Result<Map<String, Value>, String> {
    let mut states = Map::new();
    for row in rows {
        let value: Value = serde_json::from_str(&row.payload_text)
            .map_err(|e| format!("Invalid JSON for {}: {}", row.device_id, e))?;
        states.insert(row.device_id.clone(), value);
    }
    Ok(states)
}

fn payload_error(payload_text: &str) -> Option<String> {
    serde_json::from_str::<Value>(payload_text)
        .err()
        .map(|e| e.to_string())
}

// ── Scene payload helpers ─────────────────────────────────────────────────────

fn payload_obj(text: &str) -> Map<String, Value> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| {
            if let Value::Object(m) = v {
                Some(m)
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn payload_get_bool(text: &str, key: &str) -> Option<bool> {
    payload_obj(text).get(key).and_then(|v| v.as_bool())
}

fn payload_get_f64(text: &str, key: &str) -> Option<f64> {
    payload_obj(text).get(key).and_then(|v| v.as_f64())
}

fn payload_set_key(
    rows: RwSignal<Vec<SceneMemberDraft>>,
    idx: usize,
    key: &'static str,
    val: Value,
) {
    rows.update(|items| {
        if let Some(item) = items.get_mut(idx) {
            let mut map = payload_obj(&item.payload_text);
            map.insert(key.to_string(), val);
            item.payload_text = serde_json::to_string_pretty(&Value::Object(map))
                .unwrap_or_else(|_| "{}".to_string());
        }
    });
}

fn payload_text_for(rows: RwSignal<Vec<SceneMemberDraft>>, idx: usize) -> String {
    rows.get()
        .get(idx)
        .map(|r| r.payload_text.clone())
        .unwrap_or_else(|| "{}".to_string())
}

// ── Media action list helpers ─────────────────────────────────────────────────
//
// A media scene member payload is encoded as:
//   • 0 actions  → {}
//   • 1 action   → {"action":"play_media", ...}   (plain object, backward-compat)
//   • 2+ actions → {"actions":[{...},{...},...]}
//
// The backend `activate_scene` expands the "actions" array and publishes each
// item to the device cmd topic in order.

fn decode_media_actions(text: &str) -> Vec<Value> {
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Object(Map::new()));
    if let Some(arr) = v.get("actions").and_then(|a| a.as_array()) {
        arr.clone()
    } else if v.is_object() && !v.as_object().map(|m| m.is_empty()).unwrap_or(true) {
        vec![v]
    } else {
        vec![]
    }
}

fn encode_media_actions(actions: &[Value]) -> String {
    let v = match actions {
        [] => Value::Object(Map::new()),
        [single] => single.clone(),
        multiple => serde_json::json!({ "actions": multiple }),
    };
    serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{}".to_string())
}

fn media_action_get_str(
    rows: RwSignal<Vec<SceneMemberDraft>>,
    row_idx: usize,
    aidx: usize,
    key: &str,
) -> Option<String> {
    let list = decode_media_actions(&payload_text_for(rows, row_idx));
    list.get(aidx)?.get(key)?.as_str().map(str::to_string)
}

fn media_action_get_f64(
    rows: RwSignal<Vec<SceneMemberDraft>>,
    row_idx: usize,
    aidx: usize,
    key: &str,
) -> Option<f64> {
    let list = decode_media_actions(&payload_text_for(rows, row_idx));
    list.get(aidx)?.get(key)?.as_f64()
}

fn media_action_get_bool(
    rows: RwSignal<Vec<SceneMemberDraft>>,
    row_idx: usize,
    aidx: usize,
    key: &str,
) -> Option<bool> {
    let list = decode_media_actions(&payload_text_for(rows, row_idx));
    list.get(aidx)?.get(key)?.as_bool()
}

fn media_action_set_key(
    rows: RwSignal<Vec<SceneMemberDraft>>,
    row_idx: usize,
    aidx: usize,
    key: &'static str,
    val: Value,
) {
    rows.update(|items| {
        if let Some(item) = items.get_mut(row_idx) {
            let mut list = decode_media_actions(&item.payload_text);
            if let Some(Value::Object(m)) = list.get_mut(aidx) {
                m.insert(key.to_string(), val);
            }
            item.payload_text = encode_media_actions(&list);
        }
    });
}

fn media_action_replace_one(
    rows: RwSignal<Vec<SceneMemberDraft>>,
    row_idx: usize,
    aidx: usize,
    new_action: Value,
) {
    rows.update(|items| {
        if let Some(item) = items.get_mut(row_idx) {
            let mut list = decode_media_actions(&item.payload_text);
            if let Some(entry) = list.get_mut(aidx) {
                *entry = new_action;
            }
            item.payload_text = encode_media_actions(&list);
        }
    });
}

fn media_actions_push(rows: RwSignal<Vec<SceneMemberDraft>>, row_idx: usize) {
    rows.update(|items| {
        if let Some(item) = items.get_mut(row_idx) {
            let mut list = decode_media_actions(&item.payload_text);
            list.push(Value::Object(Map::new()));
            item.payload_text = encode_media_actions(&list);
        }
    });
}

fn media_actions_remove(rows: RwSignal<Vec<SceneMemberDraft>>, row_idx: usize, aidx: usize) {
    rows.update(|items| {
        if let Some(item) = items.get_mut(row_idx) {
            let mut list = decode_media_actions(&item.payload_text);
            if aidx < list.len() {
                list.remove(aidx);
            }
            item.payload_text = encode_media_actions(&list);
        }
    });
}

// ── MediaActionRow component ──────────────────────────────────────────────────

#[component]
fn MediaActionRow(
    rows: RwSignal<Vec<SceneMemberDraft>>,
    row_idx: usize,
    aidx: usize,
    has_vol: bool,
    has_favs: bool,
    has_pls: bool,
    sup_stop: bool,
    has_mute: bool,
    has_shuffle: bool,
    has_bass: bool,
    has_treble: bool,
    favorites: Vec<String>,
    playlists: Vec<String>,
) -> impl IntoView {
    let cur_act =
        Memo::new(move |_| media_action_get_str(rows, row_idx, aidx, "action").unwrap_or_default());
    let cur_name =
        Memo::new(move |_| media_action_get_str(rows, row_idx, aidx, "name").unwrap_or_default());
    let cur_vol =
        Memo::new(move |_| media_action_get_f64(rows, row_idx, aidx, "volume").unwrap_or(0.0));
    let cur_muted =
        Memo::new(move |_| media_action_get_bool(rows, row_idx, aidx, "muted").unwrap_or(false));
    let cur_shuf =
        Memo::new(move |_| media_action_get_bool(rows, row_idx, aidx, "shuffle").unwrap_or(false));
    let cur_bass = Memo::new(move |_| {
        media_action_get_f64(rows, row_idx, aidx, "bass")
            .map(|v| v as i64)
            .unwrap_or(0)
    });
    let cur_treb = Memo::new(move |_| {
        media_action_get_f64(rows, row_idx, aidx, "treble")
            .map(|v| v as i64)
            .unwrap_or(0)
    });

    view! {
        <div class="media-action-row">
            <div class="media-action-row-header">
                <span class="control-label">"Action"</span>
                <select on:change=move |ev| {
                    let new_action = match event_target_value(&ev).as_str() {
                        "play"          => serde_json::json!({"action":"play"}),
                        "pause"         => serde_json::json!({"action":"pause"}),
                        "stop"          => serde_json::json!({"action":"stop"}),
                        "play_favorite" => serde_json::json!({"action":"play_media","media_type":"favorite","name":""}),
                        "play_playlist" => serde_json::json!({"action":"play_media","media_type":"playlist","name":""}),
                        "set_volume"    => serde_json::json!({"action":"set_volume","volume":0}),
                        "set_mute"      => serde_json::json!({"action":"set_mute","muted":false}),
                        "set_shuffle"   => serde_json::json!({"action":"set_shuffle","shuffle":false}),
                        "set_bass"      => serde_json::json!({"action":"set_bass","bass":0}),
                        "set_treble"    => serde_json::json!({"action":"set_treble","treble":0}),
                        _               => Value::Object(Map::new()),
                    };
                    media_action_replace_one(rows, row_idx, aidx, new_action);
                }>
                    <option value="" selected=move || cur_act.get().is_empty()>"— select —"</option>
                    <option value="play" selected=move || cur_act.get() == "play">"Play"</option>
                    <option value="pause" selected=move || cur_act.get() == "pause">"Pause"</option>
                    {sup_stop.then(|| view! {
                        <option value="stop" selected=move || cur_act.get() == "stop">"Stop"</option>
                    })}
                    {has_favs.then(|| view! {
                        <option value="play_favorite" selected=move || {
                            cur_act.get() == "play_media" &&
                            media_action_get_str(rows, row_idx, aidx, "media_type").as_deref() == Some("favorite")
                        }>"Play Favorite"</option>
                    })}
                    {has_pls.then(|| view! {
                        <option value="play_playlist" selected=move || {
                            cur_act.get() == "play_media" &&
                            media_action_get_str(rows, row_idx, aidx, "media_type").as_deref() == Some("playlist")
                        }>"Play Playlist"</option>
                    })}
                    {has_vol.then(|| view! {
                        <option value="set_volume" selected=move || cur_act.get() == "set_volume">"Set Volume"</option>
                    })}
                    {has_mute.then(|| view! {
                        <option value="set_mute" selected=move || cur_act.get() == "set_mute">"Set Mute"</option>
                    })}
                    {has_shuffle.then(|| view! {
                        <option value="set_shuffle" selected=move || cur_act.get() == "set_shuffle">"Set Shuffle"</option>
                    })}
                    {has_bass.then(|| view! {
                        <option value="set_bass" selected=move || cur_act.get() == "set_bass">"Set Bass"</option>
                    })}
                    {has_treble.then(|| view! {
                        <option value="set_treble" selected=move || cur_act.get() == "set_treble">"Set Treble"</option>
                    })}
                </select>
                <button class="media-action-remove"
                    on:click=move |_| media_actions_remove(rows, row_idx, aidx)>
                    <span class="material-icons" style="font-size:16px">"close"</span>
                </button>
            </div>

            {move || match cur_act.get().as_str() {
                "play_media" => {
                    let mt = media_action_get_str(rows, row_idx, aidx, "media_type").unwrap_or_default();
                    if mt == "favorite" {
                        let favs = favorites.clone();
                        view! {
                            <div class="control-row">
                                <span class="control-label">"Favorite"</span>
                                <select on:change=move |ev| {
                                    media_action_set_key(rows, row_idx, aidx, "name",
                                        Value::String(event_target_value(&ev)));
                                }>
                                    <option value="" selected=move || cur_name.get().is_empty()>"— select —"</option>
                                    {favs.into_iter().map(|fav| {
                                        let f = fav.clone();
                                        let label = fav.clone();
                                        view! {
                                            <option value=fav selected=move || cur_name.get() == f>{label}</option>
                                        }
                                    }).collect_view()}
                                </select>
                            </div>
                        }.into_any()
                    } else {
                        let pls = playlists.clone();
                        view! {
                            <div class="control-row">
                                <span class="control-label">"Playlist"</span>
                                <select on:change=move |ev| {
                                    media_action_set_key(rows, row_idx, aidx, "name",
                                        Value::String(event_target_value(&ev)));
                                }>
                                    <option value="" selected=move || cur_name.get().is_empty()>"— select —"</option>
                                    {pls.into_iter().map(|pl| {
                                        let p = pl.clone();
                                        let label = pl.clone();
                                        view! {
                                            <option value=pl selected=move || cur_name.get() == p>{label}</option>
                                        }
                                    }).collect_view()}
                                </select>
                            </div>
                        }.into_any()
                    }
                }
                "set_volume" => view! {
                    <div class="control-row">
                        <span class="control-label">"Volume"</span>
                        <div class="slider-row">
                            <span class="material-icons" style="font-size:18px;color:var(--hc-text-muted)">"volume_down"</span>
                            <input type="range" min="0" max="100" step="1"
                                prop:value=move || cur_vol.get() as i64
                                on:change=move |ev| {
                                    if let Ok(val) = event_target_value(&ev).parse::<f64>() {
                                        media_action_set_key(rows, row_idx, aidx, "volume", serde_json::json!(val));
                                    }
                                }
                            />
                            <span class="material-icons" style="font-size:18px;color:var(--hc-text-muted)">"volume_up"</span>
                            <span class="slider-value">{move || format!("{:.0}%", cur_vol.get())}</span>
                        </div>
                    </div>
                }.into_any(),
                "set_mute" => view! {
                    <div class="control-row">
                        <span class="control-label">"Mute"</span>
                        <div class="toggle-group">
                            <button class:active=move || cur_muted.get()
                                on:click=move |_| media_action_set_key(rows, row_idx, aidx, "muted", Value::Bool(true))>
                                <span class="material-icons" style="font-size:16px;vertical-align:middle">"volume_off"</span>
                                " Mute"
                            </button>
                            <button class:active=move || !cur_muted.get()
                                on:click=move |_| media_action_set_key(rows, row_idx, aidx, "muted", Value::Bool(false))>
                                <span class="material-icons" style="font-size:16px;vertical-align:middle">"volume_up"</span>
                                " Unmute"
                            </button>
                        </div>
                    </div>
                }.into_any(),
                "set_shuffle" => view! {
                    <div class="control-row">
                        <span class="control-label">"Shuffle"</span>
                        <div class="toggle-group">
                            <button class:active=move || cur_shuf.get()
                                on:click=move |_| media_action_set_key(rows, row_idx, aidx, "shuffle", Value::Bool(true))>
                                <span class="material-icons" style="font-size:16px;vertical-align:middle">"shuffle"</span>
                                " On"
                            </button>
                            <button class:active=move || !cur_shuf.get()
                                on:click=move |_| media_action_set_key(rows, row_idx, aidx, "shuffle", Value::Bool(false))>
                                " Off"
                            </button>
                        </div>
                    </div>
                }.into_any(),
                "set_bass" => view! {
                    <div class="control-row">
                        <span class="control-label">"Bass"</span>
                        <div class="slider-row">
                            <input type="range" min="-10" max="10" step="1"
                                prop:value=move || cur_bass.get()
                                on:change=move |ev| {
                                    if let Ok(val) = event_target_value(&ev).parse::<i64>() {
                                        media_action_set_key(rows, row_idx, aidx, "bass", serde_json::json!(val));
                                    }
                                }
                            />
                            <span class="slider-value">{move || cur_bass.get()}</span>
                        </div>
                    </div>
                }.into_any(),
                "set_treble" => view! {
                    <div class="control-row">
                        <span class="control-label">"Treble"</span>
                        <div class="slider-row">
                            <input type="range" min="-10" max="10" step="1"
                                prop:value=move || cur_treb.get()
                                on:change=move |ev| {
                                    if let Ok(val) = event_target_value(&ev).parse::<i64>() {
                                        media_action_set_key(rows, row_idx, aidx, "treble", serde_json::json!(val));
                                    }
                                }
                            />
                            <span class="slider-value">{move || cur_treb.get()}</span>
                        </div>
                    </div>
                }.into_any(),
                _ => view! { <span></span> }.into_any(),
            }}
        </div>
    }
}

// ── SceneDeviceEditor component ───────────────────────────────────────────────

#[component]
fn SceneDeviceEditor(
    device: Option<DeviceState>,
    idx: usize,
    rows: RwSignal<Vec<SceneMemberDraft>>,
    show_json_set: RwSignal<HashSet<usize>>,
) -> impl IntoView {
    let dtype = device
        .as_ref()
        .map(|d| presentation_device_type_key(d))
        .unwrap_or("unknown");

    // Static capability flags — derived once from live device
    let has_on = device
        .as_ref()
        .map(|d| bool_attr(d.attributes.get("on")).is_some())
        .unwrap_or(false);
    let has_bri = device
        .as_ref()
        .map(|d| {
            d.attributes
                .get("brightness_pct")
                .and_then(|v| v.as_f64())
                .is_some()
        })
        .unwrap_or(false);
    let has_ct = device
        .as_ref()
        .map(|d| {
            d.attributes
                .get("color_temp")
                .and_then(|v| v.as_f64())
                .is_some()
        })
        .unwrap_or(false);
    let has_position = device
        .as_ref()
        .map(|d| {
            d.attributes
                .get("position")
                .and_then(|v| v.as_f64())
                .is_some()
        })
        .unwrap_or(false);
    let has_lock = device
        .as_ref()
        .map(|d| bool_attr(d.attributes.get("locked")).is_some())
        .unwrap_or(false);
    let is_media = device.as_ref().map(|d| is_media_player(d)).unwrap_or(false);

    // Media player capabilities
    let has_vol = device
        .as_ref()
        .map(|d| {
            d.attributes
                .get("volume")
                .and_then(|v| v.as_f64())
                .is_some()
        })
        .unwrap_or(false);
    let has_bass = device
        .as_ref()
        .map(|d| {
            supports_action(d, "set_bass")
                && d.attributes.get("bass").and_then(|v| v.as_i64()).is_some()
        })
        .unwrap_or(false);
    let has_treble = device
        .as_ref()
        .map(|d| {
            supports_action(d, "set_treble")
                && d.attributes
                    .get("treble")
                    .and_then(|v| v.as_i64())
                    .is_some()
        })
        .unwrap_or(false);
    let has_mute = device
        .as_ref()
        .map(|d| supports_action(d, "set_mute") && bool_attr(d.attributes.get("muted")).is_some())
        .unwrap_or(false);
    let has_shuffle = device
        .as_ref()
        .map(|d| {
            supports_action(d, "set_shuffle") && bool_attr(d.attributes.get("shuffle")).is_some()
        })
        .unwrap_or(false);
    let sup_stop = device
        .as_ref()
        .map(|d| supports_action(d, "stop"))
        .unwrap_or(false);
    let favorites = device
        .as_ref()
        .map(|d| media_available_favorites(d))
        .unwrap_or_default();
    let playlists = device
        .as_ref()
        .map(|d| media_available_playlists(d))
        .unwrap_or_default();
    let has_favs = !favorites.is_empty();
    let has_pls = !playlists.is_empty();

    let is_sensor = matches!(
        dtype,
        "motion_sensor"
            | "occupancy_sensor"
            | "contact_sensor"
            | "leak_sensor"
            | "vibration_sensor"
            | "environment_sensor"
            | "temperature_sensor"
            | "humidity_sensor"
    );

    let has_controls = !is_sensor && (has_on || has_bri || has_position || has_lock || is_media);

    // Reactive reads from payload
    let cur_on =
        Memo::new(move |_| payload_get_bool(&payload_text_for(rows, idx), "on").unwrap_or(false));
    let cur_bri = Memo::new(move |_| {
        payload_get_f64(&payload_text_for(rows, idx), "brightness_pct").unwrap_or(0.0)
    });
    let cur_ct = Memo::new(move |_| {
        payload_get_f64(&payload_text_for(rows, idx), "color_temp").unwrap_or(2700.0)
    });
    let cur_pos = Memo::new(move |_| {
        payload_get_f64(&payload_text_for(rows, idx), "position").unwrap_or(50.0)
    });
    let cur_lock = Memo::new(move |_| {
        payload_get_bool(&payload_text_for(rows, idx), "locked").unwrap_or(false)
    });
    let show_json = Signal::derive(move || show_json_set.get().contains(&idx));

    view! {
        <div class="scene-member-controls">

            // ── Switch / Light (no brightness) ────────────────────────────
            {(has_controls && has_on && !has_bri && !is_media && !has_position && !has_lock).then(|| view! {
                <div class="control-row">
                    <span class="control-label">"Power"</span>
                    <div class="toggle-group">
                        <button class:active=move || cur_on.get()
                            on:click=move |_| payload_set_key(rows, idx, "on", Value::Bool(true))>
                            "On"
                        </button>
                        <button class:active=move || !cur_on.get()
                            on:click=move |_| payload_set_key(rows, idx, "on", Value::Bool(false))>
                            "Off"
                        </button>
                    </div>
                </div>
            })}

            // ── Dimmer — On/Off + Brightness + optional Color Temp ─────────
            {(has_controls && has_bri).then(|| view! {
                <div class="control-row">
                    <span class="control-label">"Power"</span>
                    <div class="toggle-group">
                        <button class:active=move || cur_on.get()
                            on:click=move |_| payload_set_key(rows, idx, "on", Value::Bool(true))>
                            "On"
                        </button>
                        <button class:active=move || !cur_on.get()
                            on:click=move |_| payload_set_key(rows, idx, "on", Value::Bool(false))>
                            "Off"
                        </button>
                    </div>
                </div>
                <div class="control-row">
                    <span class="control-label">"Brightness"</span>
                    <div class="slider-row">
                        <input type="range" min="0" max="100" step="1"
                            prop:value=move || cur_bri.get() as i64
                            on:change=move |ev| {
                                if let Ok(val) = event_target_value(&ev).parse::<f64>() {
                                    payload_set_key(rows, idx, "brightness_pct", serde_json::json!(val));
                                }
                            }
                        />
                        <span class="slider-value">{move || format!("{:.0}%", cur_bri.get())}</span>
                    </div>
                </div>
                {has_ct.then(|| view! {
                    <div class="control-row">
                        <span class="control-label">"Color Temp"</span>
                        <div class="slider-row">
                            <input type="range" min="2700" max="6500" step="50"
                                prop:value=move || cur_ct.get() as i64
                                on:change=move |ev| {
                                    if let Ok(val) = event_target_value(&ev).parse::<f64>() {
                                        payload_set_key(rows, idx, "color_temp", serde_json::json!(val));
                                    }
                                }
                            />
                            <span class="slider-value">{move || format!("{:.0}K", cur_ct.get())}</span>
                        </div>
                    </div>
                })}
            })}

            // ── Shade — Position slider + Open/Close shortcuts ─────────────
            {(has_controls && has_position).then(|| view! {
                <div class="control-row">
                    <span class="control-label">"Position"</span>
                    <div class="slider-row">
                        <input type="range" min="0" max="100" step="1"
                            prop:value=move || cur_pos.get() as i64
                            on:change=move |ev| {
                                if let Ok(val) = event_target_value(&ev).parse::<f64>() {
                                    payload_set_key(rows, idx, "position", serde_json::json!(val));
                                }
                            }
                        />
                        <span class="slider-value">{move || format!("{:.0}%", cur_pos.get())}</span>
                    </div>
                </div>
                <div class="control-row">
                    <span class="control-label"></span>
                    <div class="btn-group">
                        <button on:click=move |_| payload_set_key(rows, idx, "position", serde_json::json!(100.0))>
                            "Open"
                        </button>
                        <button on:click=move |_| payload_set_key(rows, idx, "position", serde_json::json!(0.0))>
                            "Close"
                        </button>
                    </div>
                </div>
            })}

            // ── Lock ───────────────────────────────────────────────────────
            {(has_controls && has_lock).then(|| view! {
                <div class="control-row">
                    <span class="control-label">"Lock"</span>
                    <div class="toggle-group">
                        <button class:active=move || cur_lock.get()
                            on:click=move |_| payload_set_key(rows, idx, "locked", Value::Bool(true))>
                            <span class="material-icons" style="font-size:16px;vertical-align:middle">"lock"</span>
                            " Lock"
                        </button>
                        <button class:active=move || !cur_lock.get()
                            on:click=move |_| payload_set_key(rows, idx, "locked", Value::Bool(false))>
                            <span class="material-icons" style="font-size:16px;vertical-align:middle">"lock_open"</span>
                            " Unlock"
                        </button>
                    </div>
                </div>
            })}

            // ── Media player — multi-action list ──────────────────────────
            {(has_controls && is_media).then(move || {
                let favorites_for_rows = favorites.clone();
                let playlists_for_rows = playlists.clone();
                let action_count = Memo::new(move |_| decode_media_actions(&payload_text_for(rows, idx)).len());
                view! {
                    <div class="media-action-list">
                        {move || (0..action_count.get()).map(|aidx| {
                            let favs = favorites_for_rows.clone();
                            let pls  = playlists_for_rows.clone();
                            view! {
                                <MediaActionRow rows=rows row_idx=idx aidx=aidx
                                    has_vol=has_vol has_favs=has_favs has_pls=has_pls
                                    sup_stop=sup_stop has_mute=has_mute has_shuffle=has_shuffle
                                    has_bass=has_bass has_treble=has_treble
                                    favorites=favs playlists=pls
                                />
                            }
                        }).collect_view()}
                    </div>
                    <button class="btn-outline scene-add-action"
                        on:click=move |_| media_actions_push(rows, idx)>
                        <span class="material-icons" style="font-size:16px;vertical-align:middle">"add"</span>
                        " Add Action"
                    </button>
                }
            })}

        </div>

        // Show/Hide JSON toggle — only when visual controls are present
        {has_controls.then(|| view! {
            <button class="scene-json-toggle"
                on:click=move |_| show_json_set.update(|s| {
                    if s.contains(&idx) { s.remove(&idx); } else { s.insert(idx); }
                })>
                {move || if show_json.get() { "Hide JSON" } else { "Show JSON" }}
            </button>
        })}

        // Raw JSON textarea — always visible when no controls, toggled when controls present
        {move || (!has_controls || show_json.get()).then(|| view! {
            <textarea
                class="search-input scene-json-editor"
                prop:value=move || payload_text_for(rows, idx)
                on:input=move |ev| {
                    let next = event_target_value(&ev);
                    rows.update(|items| {
                        if let Some(item) = items.get_mut(idx) {
                            item.payload_text = next;
                        }
                    });
                }
            />
            {move || payload_error(&payload_text_for(rows, idx))
                .map(|msg| view! { <p class="msg-error">{format!("Invalid JSON: {msg}")}</p> })
            }
        })}
    }
}

#[component]
pub fn NewScenePage() -> impl IntoView {
    view! { <NativeSceneEditorPage scene_id=None /> }
}

#[component]
pub fn NativeSceneDetailPage() -> impl IntoView {
    let params = use_params_map();
    let scene_id = params.with_untracked(|p| p.get("id").map(|s| s.to_string()));
    view! { <NativeSceneEditorPage scene_id /> }
}

#[component]
fn NativeSceneEditorPage(scene_id: Option<String>) -> impl IntoView {
    let auth = use_auth();
    let navigate = use_navigate();
    let is_existing = scene_id.is_some();

    let devices: RwSignal<Vec<DeviceState>> = RwSignal::new(vec![]);
    let loading = RwSignal::new(true);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);
    let name = RwSignal::new(String::new());
    let rows: RwSignal<Vec<SceneMemberDraft>> = RwSignal::new(vec![]);
    let original_name = RwSignal::new(String::new());
    let original_rows: RwSignal<Vec<SceneMemberDraft>> = RwSignal::new(vec![]);
    let add_device_id = RwSignal::new(String::new());
    let add_device_search = RwSignal::new(String::new());
    let show_json_set: RwSignal<HashSet<usize>> = RwSignal::new(HashSet::new());

    let scene_id_for_load = scene_id.clone();
    Effect::new(move |_| {
        let scene_id = scene_id_for_load.clone();
        let token = auth.token_str().unwrap_or_default();
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            let devices_result = fetch_devices(&token).await;
            let scene_result = if let Some(id) = scene_id.as_deref() {
                Some(fetch_scene(&token, id).await)
            } else {
                None
            };

            match devices_result {
                Ok(list) => devices.set(list),
                Err(e) => error.set(Some(e)),
            }

            if let Some(result) = scene_result {
                match result {
                    Ok(scene) => {
                        name.set(scene.name.clone());
                        let scene_rows = scene_to_rows(&scene);
                        rows.set(scene_rows.clone());
                        original_name.set(scene.name.clone());
                        original_rows.set(scene_rows);
                    }
                    Err(e) => error.set(Some(e)),
                }
            }

            loading.set(false);
        });
    });

    let addable_devices: Memo<Vec<DeviceState>> = Memo::new(move |_| {
        let query = add_device_search.get().trim().to_lowercase();
        let selected_ids: HashSet<String> =
            rows.get().into_iter().map(|row| row.device_id).collect();
        let mut list: Vec<DeviceState> = devices
            .get()
            .into_iter()
            .filter(|device| !is_scene_like(device))
            .filter(|device| !selected_ids.contains(&device.device_id))
            .filter(|device| {
                if query.is_empty() {
                    return true;
                }
                format!(
                    "{} {} {} {}",
                    device.name,
                    device.device_id,
                    display_area_value(device.area.as_deref()),
                    device.plugin_id,
                )
                .to_lowercase()
                .contains(&query)
            })
            .collect();
        list.sort_by(|a, b| sort_key_str(display_name(a)).cmp(&sort_key_str(display_name(b))));
        list
    });

    let invalid_row_count: Memo<usize> = Memo::new(move |_| {
        rows.get()
            .iter()
            .filter(|row| payload_error(&row.payload_text).is_some())
            .count()
    });
    let addable_count: Signal<usize> = Signal::derive(move || addable_devices.get().len());

    let is_dirty: Signal<bool> = Signal::derive(move || {
        name.get().trim() != original_name.get().trim() || rows.get() != original_rows.get()
    });
    let save_disabled: Signal<bool> =
        Signal::derive(move || busy.get() || loading.get() || invalid_row_count.get() > 0);

    let scene_id_for_save = scene_id.clone();
    let navigate_for_save = navigate.clone();
    let save = move |_| {
        let token = auth.token_str().unwrap_or_default();
        let scene_name = name.get().trim().to_string();
        if scene_name.is_empty() {
            error.set(Some("Scene name is required.".to_string()));
            return;
        }

        let current_rows = rows.get();
        let states = match rows_to_states(&current_rows) {
            Ok(states) => states,
            Err(e) => {
                error.set(Some(e));
                return;
            }
        };

        busy.set(true);
        error.set(None);
        notice.set(None);

        let nav = navigate_for_save.clone();
        let scene_id = scene_id_for_save.clone();
        spawn_local(async move {
            let result = match scene_id.as_deref() {
                Some(id) => update_scene(&token, id, &scene_name, &states).await,
                None => create_scene(&token, &scene_name, &states).await,
            };

            match result {
                Ok(scene) => {
                    notice.set(Some("Scene saved.".to_string()));
                    original_name.set(scene.name.clone());
                    let scene_rows = scene_to_rows(&scene);
                    rows.set(scene_rows.clone());
                    original_rows.set(scene_rows);
                    if scene_id.is_none() {
                        nav(&format!("/scenes/native/{}", scene.id), Default::default());
                    }
                }
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    let navigate_for_clone = navigate.clone();
    let clone_scene = move |_| {
        let token = auth.token_str().unwrap_or_default();
        let clone_name = format!("Copy of {}", name.get().trim());
        let current_rows = rows.get();
        let states = match rows_to_states(&current_rows) {
            Ok(s) => s,
            Err(e) => {
                error.set(Some(e));
                return;
            }
        };
        let nav = navigate_for_clone.clone();
        busy.set(true);
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match create_scene(&token, &clone_name, &states).await {
                Ok(scene) => nav(&format!("/scenes/native/{}", scene.id), Default::default()),
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    let scene_id_for_activate = scene_id.clone();
    let activate = move |_| {
        let Some(scene_id) = scene_id_for_activate.clone() else {
            return;
        };
        let token = auth.token_str().unwrap_or_default();
        busy.set(true);
        notice.set(None);
        spawn_local(async move {
            match activate_scene(&token, &scene_id).await {
                Ok(()) => notice.set(Some("Scene activated.".to_string())),
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    let scene_id_for_delete = scene_id.clone();
    let navigate_for_delete = navigate.clone();
    let delete_scene_click = move |_| {
        let Some(scene_id) = scene_id_for_delete.clone() else {
            return;
        };
        let token = auth.token_str().unwrap_or_default();
        let nav = navigate_for_delete.clone();
        busy.set(true);
        notice.set(None);
        spawn_local(async move {
            match delete_scene(&token, &scene_id).await {
                Ok(()) => nav("/scenes", Default::default()),
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="page device-detail-page scene-detail-page">
            <div class="detail-back-row">
                <a href="/scenes" class="back-link">
                    <span class="material-icons" style="font-size:18px;vertical-align:middle">"arrow_back"</span>
                    " Scenes"
                </a>
            </div>

            <div class="detail-heading">
                <div class="detail-title-row">
                    <span class="status-badge-lg tone-media scene-detail-badge">
                        <span class="material-icons" style="font-size:26px">"auto_awesome_motion"</span>
                    </span>
                    <div class="detail-name-block">
                        <h1>
                            {move || {
                                let trimmed = name.get().trim().to_string();
                                if is_existing {
                                    if trimmed.is_empty() { "Native Scene".to_string() } else { trimmed }
                                } else if trimmed.is_empty() {
                                    "New Scene".to_string()
                                } else {
                                    trimmed
                                }
                            }}
                        </h1>
                        <p class="subtitle scene-heading-copy">
                            "Edit HomeCore-managed scene membership and desired device state."
                        </p>
                        <div class="detail-meta-chips">
                            <span class="chip-neutral">"HomeCore"</span>
                            <span class="chip-neutral">
                                {move || format!("{} members", rows.get().len())}
                            </span>
                            <span class=move || {
                                if invalid_row_count.get() > 0 { "chip-offline" } else { "chip-neutral" }
                            }>
                                {move || format!("{} invalid payloads", invalid_row_count.get())}
                            </span>
                            <span class=move || {
                                if is_dirty.get() { "chip-neutral" } else { "chip-online" }
                            }>
                                {move || if is_dirty.get() { "Unsaved changes" } else { "Saved" }}
                            </span>
                        </div>
                    </div>
                    <div class="detail-heading-actions">
                        {is_existing.then(|| view! {
                            <button class="primary" disabled=move || busy.get() on:click=activate>
                                {move || if busy.get() { "Working…" } else { "Activate" }}
                            </button>
                        })}
                        <button
                            class="primary"
                            disabled=save_disabled
                            on:click=save
                        >
                            {move || if busy.get() { "Saving…" } else { "Save" }}
                        </button>
                        {is_existing.then(|| view! {
                            <button class="btn-outline" disabled=move || busy.get() on:click=clone_scene>
                                "Clone"
                            </button>
                        })}
                        {is_existing.then(|| view! {
                            <button class="btn-outline" disabled=move || busy.get() on:click=delete_scene_click>
                                "Delete"
                            </button>
                        })}
                    </div>
                </div>
            </div>

            {move || error.get().map(|e| view! { <p class="msg-error">{e}</p> })}
            {move || notice.get().map(|m| view! { <p class="msg-success">{m}</p> })}

            <div class="detail-grid">
                <div class="detail-card">
                    <div class="scene-editor-stack">
                        <div>
                            <h2>"Scene Settings"</h2>
                            <p class="subtitle">"Name the scene and confirm its saved state before activation."</p>
                        </div>
                        <div class="scene-field">
                            <label for="scene-name"><strong>"Scene Name"</strong></label>
                            <input
                                id="scene-name"
                                class="search-input"
                                type="text"
                                prop:value=move || name.get()
                                on:input=move |ev| name.set(event_target_value(&ev))
                            />
                        </div>
                    </div>
                </div>

                <div class="detail-card">
                    <div class="scene-editor-stack">
                        <div>
                            <h2>"Add Device"</h2>
                            <p class="subtitle">"Seed a new row with the device's current live attributes."</p>
                        </div>

                        <div class="detail-meta-chips">
                            <span class="chip-neutral">{move || format!("{} available", addable_count.get())}</span>
                            <span class="chip-neutral">"Scene devices excluded"</span>
                        </div>

                        <input
                            class="search-input"
                            type="search"
                            placeholder="Search devices by name, id, area, plugin…"
                            prop:value=move || add_device_search.get()
                            on:input=move |ev| add_device_search.set(event_target_value(&ev))
                        />

                        <div class="scene-add-device-row">
                            <select
                                class="scene-add-device-select"
                                prop:value=move || add_device_id.get()
                                on:change=move |ev| add_device_id.set(event_target_value(&ev))
                            >
                                <option value="">"Select a device"</option>
                                <For
                                    each=move || addable_devices.get()
                                    key=|device| device.device_id.clone()
                                    children=move |device| {
                                        let label = format!("{} ({})", device.name, device.device_id);
                                        view! { <option value=device.device_id.clone()>{label}</option> }
                                    }
                                />
                            </select>
                            <button
                                class="primary"
                                disabled=move || add_device_id.get().is_empty()
                                on:click=move |_| {
                                    let selected_id = add_device_id.get();
                                    if selected_id.is_empty() {
                                        return;
                                    }
                                    if let Some(device) = devices.get().into_iter().find(|device| device.device_id == selected_id) {
                                        let payload = serde_json::to_string_pretty(&device.attributes)
                                            .unwrap_or_else(|_| "{}".to_string());
                                        rows.update(|items| {
                                            items.push(SceneMemberDraft {
                                                device_id: selected_id.clone(),
                                                payload_text: payload,
                                            });
                                        });
                                        add_device_id.set(String::new());
                                        add_device_search.set(String::new());
                                    }
                                }
                            >
                                "Add"
                            </button>
                        </div>

                        {move || (addable_count.get() == 0).then(|| view! {
                            <p class="subtitle scene-inline-note">
                                "All eligible devices are already in this scene or filtered out by the current search."
                            </p>
                        })}
                    </div>
                </div>
            </div>

            <div class="detail-card scene-members-card">
                <div class="scene-members-heading">
                    <h2>"Scene Members"</h2>
                    <p class="subtitle">"Each row stores the desired command payload for one device."</p>
                </div>

                {move || {
                    if rows.get().is_empty() {
                        view! { <p class="cards-empty">"No devices in this scene yet."</p> }.into_any()
                    } else {
                        let device_list = devices.get();
                        view! {
                            <div class="scene-member-list">
                                {rows.get().into_iter().enumerate().map(|(idx, row)| {
                                    let current_device_id = row.device_id.clone();
                                    let display_label = device_display(&device_list, &current_device_id);
                                    let device_for_editor = device_list.iter().find(|d| d.device_id == current_device_id).cloned();
                                    let device_list_for_snapshot = device_list.clone();
                                    let device_meta = device_list.iter().find(|device| device.device_id == current_device_id);
                                    let area_label = device_meta
                                        .map(|device| display_area_value(device.area.as_deref()))
                                        .unwrap_or_else(|| "Unassigned".to_string());
                                    let plugin_label = device_meta
                                        .map(|device| device.plugin_id.clone())
                                        .unwrap_or_else(|| "Unknown plugin".to_string());
                                    view! {
                                        <div class="scene-member-card">
                                            <div class="scene-member-card-head">
                                                <div class="scene-member-card-copy">
                                                    <strong class="scene-member-title">{display_label}</strong>
                                                    <div class="detail-meta-chips">
                                                        <span class="chip-neutral">{area_label}</span>
                                                        <span class="chip-neutral">{plugin_label}</span>
                                                        <span class=move || {
                                                            if payload_error(&payload_text_for(rows, idx)).is_some() {
                                                                "chip-offline"
                                                            } else {
                                                                "chip-online"
                                                            }
                                                        }>
                                                            {move || if payload_error(&payload_text_for(rows, idx)).is_some() {
                                                                "Invalid JSON"
                                                            } else {
                                                                "Valid JSON"
                                                            }}
                                                        </span>
                                                    </div>
                                                </div>
                                                <div class="scene-member-actions">
                                                    <button
                                                        class="btn-outline"
                                                        on:click=move |_| {
                                                            if let Some(device) = device_list_for_snapshot
                                                                .iter()
                                                                .find(|device| device.device_id == current_device_id)
                                                            {
                                                                let payload = serde_json::to_string_pretty(&device.attributes)
                                                                    .unwrap_or_else(|_| "{}".to_string());
                                                                rows.update(|items| {
                                                                    if let Some(item) = items.get_mut(idx) {
                                                                        item.payload_text = payload.clone();
                                                                    }
                                                                });
                                                            }
                                                        }
                                                    >
                                                        "Use Live Snapshot"
                                                    </button>
                                                    <button
                                                        class="btn-outline"
                                                        on:click=move |_| {
                                                            rows.update(|items| {
                                                                if idx < items.len() {
                                                                    items.remove(idx);
                                                                }
                                                            });
                                                        }
                                                    >
                                                        "Remove"
                                                    </button>
                                                </div>
                                            </div>
                                            <SceneDeviceEditor
                                                device=device_for_editor
                                                idx=idx
                                                rows=rows
                                                show_json_set=show_json_set
                                            />
                                        </div>
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

#[component]
pub fn PluginSceneDetailPage() -> impl IntoView {
    let auth = use_auth();
    let ws = use_ws();
    let params = use_params_map();
    let device_id =
        params.with_untracked(|p| p.get("id").map(|s| s.to_string()).unwrap_or_default());
    let activate_device_id = device_id.clone();

    let loading = RwSignal::new(true);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);

    let did = device_id.clone();
    Effect::new(move |_| {
        let token = auth.token_str().unwrap_or_default();
        loading.set(true);
        error.set(None);
        let did = did.clone();
        spawn_local(async move {
            match fetch_device(&token, &did).await {
                Ok(device) => {
                    ws.devices.update(|m| {
                        m.insert(device.device_id.clone(), device);
                    });
                }
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    });

    let device: Memo<Option<DeviceState>> =
        Memo::new(move |_| ws.devices.get().get(&device_id).cloned());

    view! {
        <div class="page device-detail-page scene-detail-page">
            <div class="detail-back-row">
                <a href="/scenes" class="back-link">
                    <span class="material-icons" style="font-size:18px;vertical-align:middle">"arrow_back"</span>
                    " Scenes"
                </a>
            </div>

            {move || error.get().map(|e| view! { <p class="msg-error">{e}</p> })}
            {move || notice.get().map(|m| view! { <p class="msg-success">{m}</p> })}

            {move || {
                let Some(device) = device.get() else {
                    return view! {
                        <p class="cards-empty">
                            {if loading.get() { "Loading scene…" } else { "Scene not found." }}
                        </p>
                    }.into_any();
                };
                let activate_id = activate_device_id.clone();

                view! {
                    <div class="detail-heading">
                        <div class="detail-title-row">
                            <span class=format!(
                                "status-badge-lg scene-detail-badge {}",
                                if is_plugin_scene_active(&device) { "tone-good" } else { "tone-idle" }
                            )>
                                <span class="material-icons" style="font-size:26px">
                                    {if is_plugin_scene_active(&device) { "check_circle" } else { "radio_button_unchecked" }}
                                </span>
                            </span>
                            <div class="detail-name-block">
                                <h1>{device.name.clone()}</h1>
                                <p class="subtitle scene-heading-copy">
                                    "Inspect and activate a plugin-provided scene device."
                                </p>
                                <div class="detail-meta-chips">
                                    <span class:chip-online=is_plugin_scene_active(&device) class:chip-neutral=!is_plugin_scene_active(&device)>
                                        {if is_plugin_scene_active(&device) { "On" } else { "Off" }}
                                    </span>
                                    <span class="chip-neutral">{device.plugin_id.clone()}</span>
                                    <span class="chip-neutral">{display_area_value(device.area.as_deref())}</span>
                                </div>
                            </div>
                            <div class="detail-heading-actions">
                                <button
                                    class="primary"
                                    disabled=move || busy.get()
                                    on:click=move |_| {
                                        let token = auth.token_str().unwrap_or_default();
                                        let did = activate_id.clone();
                                        busy.set(true);
                                        notice.set(None);
                                        spawn_local(async move {
                                            match set_device_state(&token, &did, &serde_json::json!({ "activate": true })).await {
                                                Ok(()) => notice.set(Some("Plugin scene activated.".to_string())),
                                                Err(e) => error.set(Some(e)),
                                            }
                                            busy.set(false);
                                        });
                                    }
                                >
                                    {move || if busy.get() { "Activating…" } else { "Activate" }}
                                </button>
                            </div>
                        </div>
                    </div>

                    <div class="detail-grid">
                        <div class="detail-card">
                            <h2>"Scene Details"</h2>
                            <div class="scene-detail-facts">
                                <p><strong>"Device ID: "</strong>{device.device_id.clone()}</p>
                                <p><strong>"Type: "</strong>{raw_device_type_label(&device)}</p>
                                <p><strong>"Last Changed: "</strong>{format_abs(last_change_time(&device))}</p>
                            </div>
                        </div>

                        <div class="detail-card">
                            <h2>"Attributes"</h2>
                            <pre class="scene-attributes-pre">{
                                serde_json::to_string_pretty(&device.attributes)
                                    .unwrap_or_else(|_| "{}".to_string())
                            }</pre>
                        </div>
                    </div>
                }.into_any()
            }}
        </div>
    }
}
