//! Scenes page — unified cards view for native HomeCore scenes and plugin scenes.

use crate::api::{
    activate_scene, create_scene, fetch_devices, fetch_scene, fetch_scenes, set_device_state,
};
use crate::auth::use_auth;
use crate::models::*;
use crate::pages::shared::{
    ErrorBanner,
    card_size_canvas_class, common_card_prefs_map, json_str_set, load_common_card_prefs,
    load_pref_json, ls_set, set_to_json_array, CardSize, CardSizeSelect, CommonCardPrefs,
    LiveStatusBanner, MultiSelectDropdown, ResetFiltersButton, SearchField,
    SortDir, SortDirToggle, SortSelect,
};
use crate::ws::use_ws;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use std::collections::{HashMap, HashSet};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SceneSource {
    Native,
    Plugin,
}

#[derive(Debug, Clone, PartialEq)]
struct SceneRow {
    key: String,
    source: SceneSource,
    name: String,
    area_label: String,
    status_on: bool,
    type_label: String,
    plugin_id: String,
    member_count: usize,
    last_seen: Option<chrono::DateTime<chrono::Utc>>,
    search_text: String,
}

struct ScenePrefs {
    card_size: CardSize,
    search: String,
    status_filter: HashSet<String>,
    area_filter: HashSet<String>,
    type_filter: HashSet<String>,
    plugin_filter: HashSet<String>,
    sort_by: SortKey,
    sort_dir: SortDir,
}

impl Default for ScenePrefs {
    fn default() -> Self {
        Self {
            card_size: CardSize::Medium,
            search: String::new(),
            status_filter: HashSet::new(),
            area_filter: HashSet::new(),
            type_filter: HashSet::new(),
            plugin_filter: HashSet::new(),
            sort_by: SortKey::Name,
            sort_dir: SortDir::Asc,
        }
    }
}

const SCENES_PREFS_KEY: &str = "hc-leptos:scenes:prefs";

fn load_prefs() -> ScenePrefs {
    let Some(v) = load_pref_json(SCENES_PREFS_KEY) else {
        return ScenePrefs::default();
    };
    let common = load_common_card_prefs(&v, sort_key_from_str);

    ScenePrefs {
        card_size: common.card_size,
        search: common.search,
        status_filter: json_str_set(&v, "status_filter"),
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
    status_filter: &HashSet<String>,
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
    value.insert(
        "status_filter".to_string(),
        set_to_json_array(status_filter),
    );
    value.insert("area_filter".to_string(), set_to_json_array(area_filter));
    value.insert("type_filter".to_string(), set_to_json_array(type_filter));
    value.insert(
        "plugin_filter".to_string(),
        set_to_json_array(plugin_filter),
    );
    ls_set(
        SCENES_PREFS_KEY,
        &serde_json::Value::Object(value).to_string(),
    );
}

fn native_key(id: &str) -> String {
    format!("native:{id}")
}

fn plugin_key(id: &str) -> String {
    format!("plugin:{id}")
}

fn split_key(key: &str) -> Option<(SceneSource, &str)> {
    if let Some(id) = key.strip_prefix("native:") {
        Some((SceneSource::Native, id))
    } else {
        key.strip_prefix("plugin:")
            .map(|id| (SceneSource::Plugin, id))
    }
}

fn scene_area_label(scene: &Scene, devices: &HashMap<String, DeviceState>) -> String {
    let areas: HashSet<String> = scene
        .states
        .keys()
        .filter_map(|id| devices.get(id))
        .filter_map(|d| d.area.as_deref())
        .map(display_area_name)
        .collect();

    match areas.len() {
        0 => "Unassigned".to_string(),
        1 => areas
            .iter()
            .next()
            .cloned()
            .unwrap_or_else(|| "Unassigned".to_string()),
        _ => "Multiple Areas".to_string(),
    }
}

fn scene_search_text(scene: &Scene, devices: &HashMap<String, DeviceState>) -> String {
    let mut parts = vec![
        scene.name.clone(),
        scene.id.clone(),
        "native scene".to_string(),
        scene_area_label(scene, devices),
        "homecore".to_string(),
    ];

    for device_id in scene.states.keys() {
        parts.push(device_id.clone());
        if let Some(device) = devices.get(device_id) {
            parts.push(device.name.clone());
            parts.push(display_area_value(device.area.as_deref()));
        }
    }

    parts.join(" ").to_lowercase()
}

fn plugin_scene_search_text(device: &DeviceState) -> String {
    format!(
        "{} {} {} {} {} {} plugin scene",
        display_name(device),
        device.device_id,
        display_area_value(device.area.as_deref()),
        device.plugin_id,
        raw_device_type_label(device),
        status_text(device),
    )
    .to_lowercase()
}

fn activity_label(row: &SceneRow) -> String {
    match row.source {
        SceneSource::Native => {
            if row.last_seen.is_some() {
                format!("Activated {}", format_relative(row.last_seen.as_ref()))
            } else {
                "Never activated".to_string()
            }
        }
        SceneSource::Plugin => {
            if row.last_seen.is_some() {
                format!("State changed {}", format_relative(row.last_seen.as_ref()))
            } else {
                "No recent activity".to_string()
            }
        }
    }
}

fn status_badge_class(is_on: bool) -> &'static str {
    if is_on {
        "card-state-badge card-state-badge--tone-tone-good"
    } else {
        "card-state-badge card-state-badge--tone-tone-idle"
    }
}

fn scene_state_summary(row: &SceneRow) -> &'static str {
    match row.source {
        SceneSource::Native if row.status_on => "Matches live state",
        SceneSource::Native => "State drift",
        SceneSource::Plugin if row.status_on => "Plugin reports active",
        SceneSource::Plugin => "Plugin reports idle",
    }
}

fn sort_summary_label(sort_by: SortKey, sort_dir: SortDir) -> String {
    let label = match sort_by {
        SortKey::Name => "Name",
        SortKey::Area => "Area",
        SortKey::Status => "Status",
        SortKey::Type => "Type",
        SortKey::LastSeen => "Last Activity",
    };
    let direction = if sort_dir == SortDir::Desc {
        "↓"
    } else {
        "↑"
    };
    format!("Sort: {label} {direction}")
}

fn cmp_scene_name(a: &SceneRow, b: &SceneRow) -> std::cmp::Ordering {
    sort_key_str(&a.name).cmp(&sort_key_str(&b.name))
}

fn cmp_scene_status(a: &SceneRow, b: &SceneRow) -> std::cmp::Ordering {
    match (a.status_on, b.status_on) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => cmp_scene_name(a, b),
    }
}

fn cmp_scene_last_seen(a: &SceneRow, b: &SceneRow) -> std::cmp::Ordering {
    a.last_seen
        .cmp(&b.last_seen)
        .then_with(|| cmp_scene_name(a, b))
}

fn cmp_scene_rows(a: &SceneRow, b: &SceneRow, sort_by: SortKey) -> std::cmp::Ordering {
    match sort_by {
        SortKey::Name => cmp_scene_name(a, b),
        SortKey::Area => sort_key_str(&a.area_label)
            .cmp(&sort_key_str(&b.area_label))
            .then_with(|| cmp_scene_name(a, b)),
        SortKey::Status => cmp_scene_status(a, b),
        SortKey::Type => sort_key_str(&a.type_label)
            .cmp(&sort_key_str(&b.type_label))
            .then_with(|| cmp_scene_name(a, b)),
        SortKey::LastSeen => cmp_scene_last_seen(a, b),
    }
}

#[component]
fn SceneCard(scene_key: String, native_scenes: RwSignal<HashMap<String, Scene>>) -> impl IntoView {
    let auth = use_auth();
    let ws = use_ws();
    let navigate = use_navigate();
    let busy = RwSignal::new(false);
    let notice = RwSignal::new(Option::<String>::None);
    let error = RwSignal::new(Option::<String>::None);

    let key_for_memo = scene_key.clone();
    let card: Memo<Option<SceneRow>> = Memo::new(move |_| {
        let (source, id) = split_key(&key_for_memo)?;
        match source {
            SceneSource::Native => {
                let scenes = native_scenes.get();
                let scene = scenes.get(id)?.clone();
                let devices = ws.devices.get();
                Some(SceneRow {
                    key: native_key(&scene.id),
                    source,
                    name: scene.name.clone(),
                    area_label: scene_area_label(&scene, &devices),
                    status_on: scene_matches_live_state(&scene, &devices),
                    type_label: "Native Scene".to_string(),
                    plugin_id: "homecore".to_string(),
                    member_count: scene.states.len(),
                    last_seen: ws.scene_activations.get().get(&scene.id).copied(),
                    search_text: scene_search_text(&scene, &devices),
                })
            }
            SceneSource::Plugin => {
                let device = ws.devices.get().get(id)?.clone();
                Some(SceneRow {
                    key: plugin_key(&device.device_id),
                    source,
                    name: device.name.clone(),
                    area_label: display_area_value(device.area.as_deref()),
                    status_on: is_plugin_scene_active(&device),
                    type_label: "Plugin Scene".to_string(),
                    plugin_id: device.plugin_id.clone(),
                    member_count: 1,
                    last_seen: last_change_time(&device).copied(),
                    search_text: plugin_scene_search_text(&device),
                })
            }
        }
    });

    view! {
        <div class="card-slot" data-scene-key=scene_key.clone()>
            {move || {
                let Some(card_data) = card.get() else {
                    return view! { <div class="device-card device-card--ghost"></div> }.into_any();
                };

                let href = match card_data.source {
                    SceneSource::Native => {
                        format!("/scenes/native/{}", card_data.key.trim_start_matches("native:"))
                    }
                    SceneSource::Plugin => {
                        format!("/scenes/plugin/{}", card_data.key.trim_start_matches("plugin:"))
                    }
                };

                let activate_key = card_data.key.clone();
                let activate = move |_| {
                    let token = auth.token_str().unwrap_or_default();
                    let Some((source, id)) = split_key(&activate_key) else {
                        return;
                    };
                    let id = id.to_string();
                    notice.set(None);
                    error.set(None);
                    busy.set(true);
                    spawn_local(async move {
                        let result = match source {
                            SceneSource::Native => activate_scene(&token, &id).await,
                            SceneSource::Plugin => {
                                set_device_state(&token, &id, &serde_json::json!({ "activate": true })).await
                            }
                        };

                        match result {
                            Ok(()) => match source {
                                SceneSource::Native => {
                                    ws.scene_activations.update(|m| {
                                        m.insert(id.clone(), chrono::Utc::now());
                                    });
                                    notice.set(Some("Activated just now.".to_string()));
                                }
                                SceneSource::Plugin => {
                                    notice.set(Some("Activation sent.".to_string()));
                                }
                            },
                            Err(e) => error.set(Some(e)),
                        }
                        busy.set(false);
                    });
                };

                let clone_key = card_data.key.clone();
                let clone_name = card_data.name.clone();
                let nav = navigate.clone();
                let clone_scene = move |_| {
                    let Some((source, id)) = split_key(&clone_key) else { return; };
                    if source != SceneSource::Native { return; }
                    let id = id.to_string();
                    let new_name = format!("Copy of {}", clone_name.trim());
                    let token = auth.token_str().unwrap_or_default();
                    let nav = nav.clone();
                    notice.set(None);
                    error.set(None);
                    busy.set(true);
                    spawn_local(async move {
                        match fetch_scene(&token, &id).await {
                            Ok(scene) => {
                                let states: serde_json::Map<String, serde_json::Value> =
                                    scene.states.into_iter().collect();
                                match create_scene(&token, &new_name, &states).await {
                                    Ok(new_scene) => {
                                        native_scenes.update(|m| {
                                            m.insert(new_scene.id.clone(), new_scene.clone());
                                        });
                                        nav(&format!("/scenes/native/{}", new_scene.id), Default::default());
                                    }
                                    Err(e) => error.set(Some(e)),
                                }
                            }
                            Err(e) => error.set(Some(e)),
                        }
                        busy.set(false);
                    });
                };

                let source_label = match card_data.source {
                    SceneSource::Native => "HomeCore",
                    SceneSource::Plugin => "Plugin",
                };
                let kicker_label = match card_data.source {
                    SceneSource::Native => "Native Scene",
                    SceneSource::Plugin => "Plugin Scene",
                };
                let meta_line = match card_data.source {
                    SceneSource::Native => card_data.area_label.clone(),
                    SceneSource::Plugin => format!("{} · {}", card_data.area_label, card_data.plugin_id),
                };
                let secondary_chip = match card_data.source {
                    SceneSource::Native => format!("{} devices", card_data.member_count),
                    SceneSource::Plugin => card_data.plugin_id.clone(),
                };
                let activity_text = activity_label(&card_data);
                let status_summary = scene_state_summary(&card_data);

                view! {
                    <div
                        class="device-card device-card--scene"
                        class:device-card--scene-native=matches!(card_data.source, SceneSource::Native)
                        class:device-card--scene-plugin=matches!(card_data.source, SceneSource::Plugin)
                    >
                        <div class="card-header">
                            <span class=format!(
                                "card-status-icon status-badge-sm {}",
                                if card_data.status_on { "tone-good" } else { "tone-idle" }
                            )>
                                <i class=if card_data.status_on { "ph ph-check-circle" } else { "ph ph-circle" } style="font-size:18px"></i>
                            </span>
                            <div class="card-header-text">
                                <p class="scene-card-kicker">{kicker_label}</p>
                                <p class="card-name" title=card_data.name.clone()>{card_data.name.clone()}</p>
                                <p class="card-meta">{meta_line}</p>
                            </div>
                            <span class=status_badge_class(card_data.status_on)>
                                {if card_data.status_on { "On" } else { "Off" }}
                            </span>
                        </div>

                        <div class="card-body">
                            <div class="scene-card-chip-row">
                                <span class="card-state-badge card-state-badge--tone-tone-idle">
                                    {source_label}
                                </span>
                                <span class="card-state-badge card-state-badge--tone-tone-idle">
                                    {secondary_chip}
                                </span>
                            </div>

                            <div class="scene-card-activity">
                                <i class="ph ph-clock" style="font-size:16px"></i>
                                <span>{activity_text}</span>
                            </div>

                            <div class="card-state-row">
                                <span class=status_badge_class(card_data.status_on)>
                                    {status_summary}
                                </span>
                            </div>

                            <div class="card-controls">
                                <button
                                    class="card-ctrl-btn card-ctrl-btn--on"
                                    disabled=move || busy.get()
                                    on:click=activate
                                >
                                    <i class="ph ph-play" style="font-size:18px"></i>
                                    {move || if busy.get() { " Activating…" } else { " Activate" }}
                                </button>
                                {matches!(card_data.source, SceneSource::Native).then(|| view! {
                                    <button
                                        class="card-ctrl-btn"
                                        disabled=move || busy.get()
                                        on:click=clone_scene
                                    >
                                        <i class="ph ph-copy" style="font-size:18px"></i>
                                        " Clone"
                                    </button>
                                })}
                            </div>

                            {move || notice.get().map(|msg| view! {
                                <div class="scene-card-feedback scene-card-feedback--success">{msg}</div>
                            })}
                            {move || error.get().map(|msg| view! {
                                <div class="scene-card-feedback scene-card-feedback--error">{msg}</div>
                            })}
                        </div>

                        <div class="card-footer">
                            <div class="scene-card-footer-copy">
                                <span class="card-last-changed">
                                    {match card_data.source {
                                        SceneSource::Native => {
                                            if card_data.last_seen.is_some() {
                                                format!("Last activated {}", format_abs(card_data.last_seen.as_ref()))
                                            } else {
                                                "No activation history".to_string()
                                            }
                                        }
                                        SceneSource::Plugin => {
                                            if card_data.last_seen.is_some() {
                                                format!("Last change {}", format_abs(card_data.last_seen.as_ref()))
                                            } else {
                                                "No change history".to_string()
                                            }
                                        }
                                    }}
                                </span>
                                <span class="scene-card-footer-meta">{source_label}</span>
                            </div>
                            <a href=href class="card-detail-link" aria-label="Open scene details">
                                <i class="ph ph-arrow-square-out" style="font-size:15px"></i>
                            </a>
                        </div>
                    </div>
                }.into_any()
            }}
        </div>
    }
}

#[component]
pub fn ScenesPage() -> impl IntoView {
    let auth = use_auth();
    let ws = use_ws();
    let native_scenes: RwSignal<HashMap<String, Scene>> = RwSignal::new(HashMap::new());
    let loading = RwSignal::new(true);
    let error = RwSignal::new(Option::<String>::None);

    Effect::new(move |_| {
        let token = auth.token_str().unwrap_or_default();
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            let scenes_result = fetch_scenes(&token).await;
            let devices_result = fetch_devices(&token).await;

            match (scenes_result, devices_result) {
                (Ok(scenes), Ok(devices)) => {
                    native_scenes.set(
                        scenes
                            .into_iter()
                            .map(|scene| (scene.id.clone(), scene))
                            .collect(),
                    );
                    ws.devices.update(|m| {
                        for device in devices {
                            m.insert(device.device_id.clone(), device);
                        }
                    });
                }
                (Err(e), _) | (_, Err(e)) => error.set(Some(e)),
            }

            loading.set(false);
        });
    });

    let prefs = load_prefs();
    let card_size = RwSignal::new(prefs.card_size);
    let search = RwSignal::new(prefs.search);
    let status_filter = RwSignal::new(prefs.status_filter);
    let area_filter = RwSignal::new(prefs.area_filter);
    let type_filter = RwSignal::new(prefs.type_filter);
    let plugin_filter = RwSignal::new(prefs.plugin_filter);
    let sort_by = RwSignal::new(prefs.sort_by);
    let sort_dir = RwSignal::new(prefs.sort_dir);


    Effect::new(move |_| {
        save_prefs(
            card_size.get(),
            &search.get(),
            &status_filter.get(),
            &area_filter.get(),
            &type_filter.get(),
            &plugin_filter.get(),
            sort_by.get(),
            sort_dir.get(),
        );
    });

    let scene_rows: Memo<Vec<SceneRow>> = Memo::new(move |_| {
        let devices = ws.devices.get();
        let mut rows = Vec::new();

        for scene in native_scenes.get().values() {
            rows.push(SceneRow {
                key: native_key(&scene.id),
                source: SceneSource::Native,
                name: scene.name.clone(),
                area_label: scene_area_label(scene, &devices),
                status_on: scene_matches_live_state(scene, &devices),
                type_label: "Native Scene".to_string(),
                plugin_id: "homecore".to_string(),
                member_count: scene.states.len(),
                last_seen: ws.scene_activations.get().get(&scene.id).copied(),
                search_text: scene_search_text(scene, &devices),
            });
        }

        for device in devices.values().filter(|d| is_scene_like(d)) {
            rows.push(SceneRow {
                key: plugin_key(&device.device_id),
                source: SceneSource::Plugin,
                name: device.name.clone(),
                area_label: display_area_value(device.area.as_deref()),
                status_on: is_plugin_scene_active(device),
                type_label: "Plugin Scene".to_string(),
                plugin_id: device.plugin_id.clone(),
                member_count: 1,
                last_seen: last_change_time(device).copied(),
                search_text: plugin_scene_search_text(device),
            });
        }

        rows
    });

    let status_options: Signal<Vec<(String, String)>> = Signal::derive(|| {
        vec![
            ("on".to_string(), "On".to_string()),
            ("off".to_string(), "Off".to_string()),
        ]
    });

    let area_options: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let mut areas: Vec<String> = scene_rows
            .get()
            .into_iter()
            .map(|row| row.area_label)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        areas.sort();
        areas
            .into_iter()
            .map(|value| (value.clone(), value))
            .collect()
    });

    let type_options: Signal<Vec<(String, String)>> = Signal::derive(|| {
        vec![
            ("Native Scene".to_string(), "Native Scene".to_string()),
            ("Plugin Scene".to_string(), "Plugin Scene".to_string()),
        ]
    });

    let sort_options: Signal<Vec<(String, String)>> = Signal::derive(|| {
        vec![
            ("name".to_string(), "Sort: Name".to_string()),
            ("area".to_string(), "Sort: Area".to_string()),
            ("status".to_string(), "Sort: Status".to_string()),
            ("type".to_string(), "Sort: Type".to_string()),
            ("last_seen".to_string(), "Sort: Last Activity".to_string()),
        ]
    });

    let plugin_options: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let mut plugins: Vec<String> = scene_rows
            .get()
            .into_iter()
            .map(|row| row.plugin_id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        plugins.sort();
        plugins
            .into_iter()
            .map(|value| (value.clone(), value))
            .collect()
    });

    let active_filter_summary: Signal<Vec<String>> = Signal::derive(move || {
        let mut chips = Vec::new();
        let search_value = search.get();
        let trimmed_search = search_value.trim();
        if !trimmed_search.is_empty() {
            chips.push(format!("Search: {trimmed_search}"));
        }
        if !status_filter.get().is_empty() {
            chips.push(format!("Statuses: {}", status_filter.get().len()));
        }
        if !area_filter.get().is_empty() {
            chips.push(format!("Areas: {}", area_filter.get().len()));
        }
        if !type_filter.get().is_empty() {
            chips.push(format!("Types: {}", type_filter.get().len()));
        }
        if !plugin_filter.get().is_empty() {
            chips.push(format!("Plugins: {}", plugin_filter.get().len()));
        }
        chips.push(sort_summary_label(sort_by.get(), sort_dir.get()));
        chips
    });

    let card_keys: Memo<Vec<String>> = Memo::new(move |_| {
        let q = search.get().trim().to_lowercase();
        let status_f = status_filter.get();
        let area_f = area_filter.get();
        let type_f = type_filter.get();
        let plugin_f = plugin_filter.get();
        let sb = sort_by.get();
        let sd = sort_dir.get();

        let mut rows = scene_rows
            .get()
            .into_iter()
            .filter(|row| {
                status_f.is_empty() || status_f.contains(if row.status_on { "on" } else { "off" })
            })
            .filter(|row| area_f.is_empty() || area_f.contains(&row.area_label))
            .filter(|row| type_f.is_empty() || type_f.contains(&row.type_label))
            .filter(|row| plugin_f.is_empty() || plugin_f.contains(&row.plugin_id))
            .filter(|row| q.is_empty() || row.search_text.contains(&q))
            .collect::<Vec<_>>();

        rows.sort_by(|a, b| {
            let cmp = cmp_scene_rows(a, b, sb);
            if sd == SortDir::Desc {
                cmp.reverse()
            } else {
                cmp
            }
        });

        rows.into_iter().map(|row| row.key).collect()
    });

    let total = Signal::derive(move || card_keys.get().len());
    let active_count = Signal::derive(move || {
        scene_rows
            .get()
            .into_iter()
            .filter(|row| row.status_on)
            .count()
    });
    let native_count = Signal::derive(move || {
        scene_rows
            .get()
            .into_iter()
            .filter(|row| matches!(row.source, SceneSource::Native))
            .count()
    });
    let plugin_count = Signal::derive(move || {
        scene_rows
            .get()
            .into_iter()
            .filter(|row| matches!(row.source, SceneSource::Plugin))
            .count()
    });

    let canvas_class = move || card_size_canvas_class(card_size.get());

    view! {
        <div class="page">
            <div class="heading">
                <div>
                    <h1>"Scenes"</h1>
                    <p>
                        {move || format!("{} scenes", total.get())}
                        " · "
                        {move || format!("{} on", active_count.get())}
                    </p>
                    <div class="scene-summary-row">
                        <span class="summary-chip">{move || format!("{} native", native_count.get())}</span>
                        <span class="summary-chip">{move || format!("{} plugin", plugin_count.get())}</span>
                        <span class="summary-chip muted">"Cards update independently"</span>
                    </div>
                </div>
                <div>
                    <a
                        href="/scenes/new"
                        class="primary"
                        style="display:inline-flex;align-items:center;gap:0.35rem;padding:0.6rem 0.9rem;border-radius:0.8rem;"
                    >
                        <i class="ph ph-plus" style="font-size:18px"></i>
                        "New Scene"
                    </a>
                </div>
            </div>

            <LiveStatusBanner status=Signal::derive(move || ws.status.get()) />

            <ErrorBanner error=error />

            <div class="filter-panel panel">
                <div class="filter-bar">
                    <SearchField search placeholder="Search name, area, type, plugin, devices…" />

                    <CardSizeSelect card_size />

                    <SortSelect
                        current_value=Signal::derive(move || sort_key_to_str(sort_by.get()).to_string())
                        options=sort_options
                        on_change=Callback::new(move |value: String| {
                            sort_by.set(sort_key_from_str(Some(&value)));
                        })
                    />

                    <SortDirToggle sort_dir />

                </div>

                <div class="filter-summary">
                    <div class="filter-summary-count">
                        <strong>{move || total.get()}</strong>
                        <span>" scenes shown"</span>
                    </div>
                    <div class="filter-summary-chips">
                        {move || {
                            active_filter_summary
                                .get()
                                .into_iter()
                                .map(|chip| view! { <span class="summary-chip">{chip}</span> })
                                .collect_view()
                        }}
                    </div>
                </div>

                <div class="filter-body">
                    <div class="filter-multisel-row">
                        <MultiSelectDropdown
                            label="statuses"
                            placeholder="All statuses"
                            options=status_options
                            selected=status_filter
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
                            options=type_options
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
                            status_filter.set(HashSet::new());
                            area_filter.set(HashSet::new());
                            type_filter.set(HashSet::new());
                            plugin_filter.set(HashSet::new());
                            sort_by.set(SortKey::Name);
                            sort_dir.set(SortDir::Asc);
                        }) />
                    </div>
                </div>
            </div>

            <div class=canvas_class data-canvas="scenes-cards">
                {move || {
                    if card_keys.get().is_empty() {
                        if loading.get() {
                            view! {
                                <div class="cards-empty">
                                    <crate::pages::shared::SkeletonCards count=4 />
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <div class="cards-empty">
                                    <div class="hc-empty">
                                        <i class="ph ph-stack hc-empty__icon"></i>
                                        <div class="hc-empty__title">"No scenes yet"</div>
                                        <p class="hc-empty__body">
                                            "Try clearing filters, or create a new native scene to \
                                             group lights, switches, and other devices."
                                        </p>
                                    </div>
                                </div>
                            }.into_any()
                        }
                    } else {
                        view! {
                            <For
                                each=move || card_keys.get()
                                key=|id| id.clone()
                                children=move |scene_key| view! {
                                    <SceneCard scene_key native_scenes />
                                }
                            />
                        }.into_any()
                    }
                }}
            </div>
        </div>
    }
}
