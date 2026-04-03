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
    scene.states
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
        let selected_ids: HashSet<String> = rows.get().into_iter().map(|row| row.device_id).collect();
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
                                    let device_list_for_snapshot = device_list.clone();
                                    let device_meta = device_list.iter().find(|device| device.device_id == current_device_id);
                                    let area_label = device_meta
                                        .map(|device| display_area_value(device.area.as_deref()))
                                        .unwrap_or_else(|| "Unassigned".to_string());
                                    let plugin_label = device_meta
                                        .map(|device| device.plugin_id.clone())
                                        .unwrap_or_else(|| "Unknown plugin".to_string());
                                    let row_error = payload_error(&row.payload_text);
                                    view! {
                                        <div class="scene-member-card">
                                            <div class="scene-member-card-head">
                                                <div class="scene-member-card-copy">
                                                    <strong class="scene-member-title">{display_label}</strong>
                                                    <div class="detail-meta-chips">
                                                        <span class="chip-neutral">{area_label}</span>
                                                        <span class="chip-neutral">{plugin_label}</span>
                                                        <span class=if row_error.is_some() { "chip-offline" } else { "chip-online" }>
                                                            {if row_error.is_some() { "Invalid JSON" } else { "Valid JSON" }}
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
                                            <textarea
                                                class="search-input scene-json-editor"
                                                prop:value=row.payload_text.clone()
                                                on:input=move |ev| {
                                                    let next = event_target_value(&ev);
                                                    rows.update(|items| {
                                                        if let Some(item) = items.get_mut(idx) {
                                                            item.payload_text = next.clone();
                                                        }
                                                    });
                                                }
                                            />
                                            {row_error.map(|msg| view! {
                                                <p class="msg-error">{format!("Invalid JSON: {msg}")}</p>
                                            })}
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
    let device_id = params.with_untracked(|p| p.get("id").map(|s| s.to_string()).unwrap_or_default());
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

    let device: Memo<Option<DeviceState>> = Memo::new(move |_| ws.devices.get().get(&device_id).cloned());

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
