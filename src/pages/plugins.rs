//! Plugins management page — list all plugins with status, controls, and
//! navigation to detail pages.

use crate::api::{
    fetch_plugin, fetch_plugin_config, fetch_plugins, patch_plugin, restart_plugin,
    send_plugin_command, start_plugin, stop_plugin, update_plugin_config,
};
use crate::auth::{plugin_stream_sse_url, use_auth};
use crate::models::PluginInfo;
use crate::pages::shared::{ErrorBanner, SearchField};
use crate::ws::use_ws;
use hc_types::{Action as CapAction, Capabilities, RequiresRole};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

// ── Helpers ─────────────────────────────────────────────────────────────────

fn status_dot_class(status: &str) -> &'static str {
    match status {
        "active" => "plugin-dot plugin-dot--active",
        "starting" => "plugin-dot plugin-dot--starting",
        "stopped" => "plugin-dot plugin-dot--stopped",
        _ => "plugin-dot plugin-dot--offline",
    }
}

fn status_label(status: &str) -> &'static str {
    match status {
        "active" => "Active",
        "starting" => "Starting",
        "stopped" => "Stopped",
        "offline" => "Offline",
        _ => "Unknown",
    }
}

fn type_label(managed: bool) -> &'static str {
    if managed { "Local" } else { "Remote" }
}

// ── Page ────────────────────────────────────────────────────────────────────

#[component]
pub fn PluginsPage() -> impl IntoView {
    let auth = use_auth();
    let ws = use_ws();
    let loading = RwSignal::new(true);
    let error: RwSignal<Option<String>> = RwSignal::new(None);
    let search = RwSignal::new(String::new());
    let status_filter: RwSignal<HashSet<String>> = RwSignal::new(HashSet::new());
    let busy_id: RwSignal<Option<String>> = RwSignal::new(None);

    // Fetch plugins and seed WS map
    Effect::new(move |_| {
        let token = match auth.token.get() { Some(t) => t, None => return };
        loading.set(true);
        spawn_local(async move {
            match fetch_plugins(&token).await {
                Ok(list) => {
                    ws.plugins.update(|m| {
                        for p in list {
                            m.insert(p.plugin_id.clone(), p);
                        }
                    });
                }
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    });

    // Filtered + sorted list
    let filtered = Memo::new(move |_| {
        let q = search.get().to_lowercase();
        let sf = status_filter.get();
        let all = ws.plugins.get();
        let mut list: Vec<PluginInfo> = all
            .values()
            .filter(|p| {
                if !q.is_empty() {
                    let name = p.display_name().to_lowercase();
                    let id = p.plugin_id.to_lowercase();
                    if !name.contains(&q) && !id.contains(&q) {
                        return false;
                    }
                }
                if !sf.is_empty() && !sf.contains(&p.status) {
                    return false;
                }
                true
            })
            .cloned()
            .collect();
        list.sort_by(|a, b| a.display_name().cmp(&b.display_name()));
        list
    });

    // Action helpers
    let do_action = move |id: String, action: &'static str| {
        let token = match auth.token.get_untracked() { Some(t) => t, None => return };
        busy_id.set(Some(id.clone()));
        spawn_local(async move {
            let result = match action {
                "start" => start_plugin(&token, &id).await,
                "stop" => stop_plugin(&token, &id).await,
                "restart" => restart_plugin(&token, &id).await,
                _ => Ok(()),
            };
            if let Err(e) = result {
                error.set(Some(e));
            }
            // Refresh plugin list
            if let Ok(list) = fetch_plugins(&token).await {
                ws.plugins.update(|m| {
                    for p in list {
                        m.insert(p.plugin_id.clone(), p);
                    }
                });
            }
            busy_id.set(None);
        });
    };

    view! {
        <div class="plugins-page">
            // ── Heading ─────────────────────────────────────────────────────
            <div class="page-heading">
                <div>
                    <h1>"Plugins"</h1>
                    <p>{move || {
                        let all = ws.plugins.get();
                        let total = all.len();
                        let active = all.values().filter(|p| p.status == "active").count();
                        format!("{total} plugins, {active} active")
                    }}</p>
                </div>
            </div>

            <ErrorBanner error=error />

            // ── Filters ─────────────────────────────────────────────────────
            <div class="filter-panel panel">
                <div class="filter-bar">
                    <SearchField search=search placeholder="Search plugins…" />
                    <div class="plugin-status-filters">
                        {["active", "offline", "stopped", "starting"].into_iter().map(|s| {
                            let s_str = s.to_string();
                            let s_str2 = s_str.clone();
                            let label = status_label(s);
                            view! {
                                <button
                                    class="hc-btn hc-btn--sm"
                                    class:hc-btn--primary=move || status_filter.get().contains(&s_str)
                                    class:hc-btn--outline=move || !status_filter.get().contains(&s_str2)
                                    on:click={
                                        let s_str = s.to_string();
                                        move |_| {
                                            status_filter.update(|set| {
                                                if !set.remove(&s_str) {
                                                    set.insert(s_str.clone());
                                                }
                                            });
                                        }
                                    }
                                >{label}</button>
                            }
                        }).collect_view()}
                    </div>
                </div>
            </div>

            // ── Loading ─────────────────────────────────────────────────────
            {move || loading.get().then(|| view! { <p class="msg-muted">"Loading…"</p> })}

            // ── Plugin rows ─────────────────────────────────────────────────
            <div class="plugin-list">
                {move || {
                    let list = filtered.get();
                    let current_busy = busy_id.get();
                    if list.is_empty() && !loading.get() {
                        view! { <p class="msg-muted">"No plugins found."</p> }.into_any()
                    } else {
                        list.into_iter().map(|p| {
                            let id = p.plugin_id.clone();
                            let id_nav = id.clone();
                            let name = p.display_name();
                            let status = p.status.clone();
                            let dot_cls = status_dot_class(&status);
                            let label = status_label(&status);
                            let managed = p.managed;
                            let device_count = p.device_count;
                            let uptime = p.uptime_str();
                            let version = p.version.clone().unwrap_or_default();
                            let restart_count = p.restart_count;
                            let is_busy = current_busy.as_deref() == Some(&id);
                            let nav = use_navigate();

                            let id_start = id.clone();
                            let id_stop = id.clone();
                            let id_restart = id.clone();

                            view! {
                                <div class="plugin-row" on:click=move |_| {
                                    let path = format!("/plugins/{}", id_nav);
                                    nav(&path, Default::default());
                                }>
                                    // Left: status + name + badges
                                    <span class=dot_cls></span>
                                    <span class="plugin-row-name">{name}</span>
                                    <span class="plugin-badge plugin-status-badge">{label}</span>
                                    {(!version.is_empty()).then(|| view! {
                                        <span class="plugin-badge plugin-version-badge">{"v"}{version.clone()}</span>
                                    })}
                                    <span class="plugin-badge plugin-type-badge">{type_label(managed)}</span>

                                    // Center: meta
                                    <div class="plugin-row-meta">
                                        <span title="Devices"><span class="material-icons" style="font-size:14px">"devices"</span>" "{device_count.to_string()}</span>
                                        <span title="Uptime"><span class="material-icons" style="font-size:14px">"schedule"</span>" "{uptime}</span>
                                        {(restart_count > 0).then(|| view! {
                                            <span title="Restarts"><span class="material-icons" style="font-size:14px">"refresh"</span>" "{restart_count.to_string()}</span>
                                        })}
                                    </div>

                                    // Right: actions
                                    {managed.then(|| {
                                        view! {
                                            <div class="plugin-row-actions" on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()>
                                                {(status == "stopped" || status == "offline").then(|| {
                                                    let id = id_start.clone();
                                                    let do_action = do_action.clone();
                                                    view! {
                                                        <button class="hc-btn hc-btn--sm hc-btn--primary" disabled=is_busy
                                                            on:click=move |_| do_action(id.clone(), "start")
                                                        >"Start"</button>
                                                    }
                                                })}
                                                {(status == "active" || status == "starting").then(|| {
                                                    let id = id_stop.clone();
                                                    let do_action = do_action.clone();
                                                    view! {
                                                        <button class="hc-btn hc-btn--sm hc-btn--outline" disabled=is_busy
                                                            on:click=move |_| do_action(id.clone(), "stop")
                                                        >"Stop"</button>
                                                    }
                                                })}
                                                {(status == "active").then(|| {
                                                    let id = id_restart.clone();
                                                    let do_action = do_action.clone();
                                                    view! {
                                                        <button class="hc-btn hc-btn--sm hc-btn--outline" disabled=is_busy
                                                            on:click=move |_| do_action(id.clone(), "restart")
                                                        >"Restart"</button>
                                                    }
                                                })}
                                            </div>
                                        }
                                    })}
                                </div>
                            }
                        }).collect_view().into_any()
                    }
                }}
            </div>
        </div>
    }
}

// ── Detail page ─────────────────────────────────────────────────────────────

const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

#[component]
pub fn PluginDetailPage() -> impl IntoView {
    let auth = use_auth();
    let ws = use_ws();
    let params = leptos_router::hooks::use_params_map();
    let plugin_id = move || params.read().get("id").unwrap_or_default();
    let navigate = use_navigate();

    let error: RwSignal<Option<String>> = RwSignal::new(None);
    let notice: RwSignal<Option<String>> = RwSignal::new(None);
    let busy = RwSignal::new(false);

    // Config editor state
    let config_raw: RwSignal<Option<String>> = RwSignal::new(None);
    let config_loading = RwSignal::new(false);
    let editing = RwSignal::new(false);
    let edit_text = RwSignal::new(String::new());
    let config_save_busy = RwSignal::new(false);

    // Live plugin data from WS map
    let plugin = Memo::new(move |_| {
        let id = plugin_id();
        ws.plugins.get().get(&id).cloned()
    });

    // Seed plugin into WS map if not already present (direct navigation)
    Effect::new(move |_| {
        let token = match auth.token.get() { Some(t) => t, None => return };
        let id = plugin_id();
        if id.is_empty() { return; }
        if ws.plugins.get_untracked().contains_key(&id) { return; }
        spawn_local(async move {
            if let Ok(p) = fetch_plugin(&token, &id).await {
                ws.plugins.update(|m| { m.insert(p.plugin_id.clone(), p); });
            }
        });
    });

    // Fetch config on mount
    Effect::new(move |_| {
        let token = match auth.token.get() { Some(t) => t, None => return };
        let id = plugin_id();
        if id.is_empty() { return; }
        config_loading.set(true);
        spawn_local(async move {
            match fetch_plugin_config(&token, &id).await {
                Ok(resp) => {
                    if let Some(raw) = resp["raw"].as_str() {
                        config_raw.set(Some(raw.to_string()));
                    }
                }
                Err(e) => {
                    if !e.contains("not found") && !e.contains("not available") {
                        error.set(Some(format!("Config: {e}")));
                    }
                }
            }
            config_loading.set(false);
        });
    });

    // Action helper
    let do_action = move |action: &'static str| {
        let token = match auth.token.get_untracked() { Some(t) => t, None => return };
        let id = plugin_id();
        busy.set(true);
        spawn_local(async move {
            let result = match action {
                "start" => start_plugin(&token, &id).await,
                "stop" => stop_plugin(&token, &id).await,
                "restart" => restart_plugin(&token, &id).await,
                _ => Ok(()),
            };
            if let Err(e) = result { error.set(Some(e)); }
            // Refresh plugin list
            if let Ok(list) = fetch_plugins(&token).await {
                ws.plugins.update(|m| { for p in list { m.insert(p.plugin_id.clone(), p); } });
            }
            busy.set(false);
        });
    };

    // Log level change
    let on_log_level_change = move |level: String| {
        let token = match auth.token.get_untracked() { Some(t) => t, None => return };
        let id = plugin_id();
        spawn_local(async move {
            if let Err(e) = patch_plugin(&token, &id, &json!({ "log_level": level })).await {
                error.set(Some(e));
            } else {
                ws.plugins.update(|m| {
                    if let Some(p) = m.get_mut(&id) {
                        p.log_level = Some(level);
                    }
                });
            }
        });
    };

    // Thermostat-specific wizard state — recalculate_all and reload_config
    // come from the manifest now (see ActionsCard); the wizard remains
    // hardcoded because it needs live device pickers.
    let therm_wizard_open = RwSignal::new(false);
    let therm_new_id = RwSignal::new(String::new());
    let therm_new_name = RwSignal::new(String::new());
    let therm_new_sensors: RwSignal<Vec<String>> = RwSignal::new(Vec::new());
    let therm_new_actuator = RwSignal::new(String::new());
    let therm_new_setpoint = RwSignal::new(70.0f64);
    let therm_new_hyst = RwSignal::new(1.0f64);
    let therm_new_mode = RwSignal::new("off".to_string());
    let therm_new_min_on = RwSignal::new(0u64);
    let therm_new_min_off = RwSignal::new(0u64);
    let therm_create_busy = RwSignal::new(false);

    let do_therm_create = move || {
        let token = match auth.token.get_untracked() { Some(t) => t, None => return };
        let id = plugin_id();
        let config = json!({
            "id": therm_new_id.get_untracked().trim(),
            "name": therm_new_name.get_untracked().trim(),
            "sensor_device_ids": therm_new_sensors.get_untracked(),
            "sensor_attribute": "temperature",
            "aggregation": "average",
            "setpoint": therm_new_setpoint.get_untracked(),
            "hysteresis": therm_new_hyst.get_untracked(),
            "mode": therm_new_mode.get_untracked(),
            "actuator_device_id": therm_new_actuator.get_untracked(),
            "min_on_secs": therm_new_min_on.get_untracked(),
            "min_off_secs": therm_new_min_off.get_untracked(),
        });
        therm_create_busy.set(true);
        spawn_local(async move {
            match send_plugin_command(&token, &id, "add_thermostat", json!({"config": config})).await {
                Ok(_) => {
                    notice.set(Some("Thermostat created.".into()));
                    therm_wizard_open.set(false);
                    therm_new_id.set(String::new());
                    therm_new_name.set(String::new());
                    therm_new_sensors.set(Vec::new());
                    therm_new_actuator.set(String::new());
                }
                Err(e) => error.set(Some(format!("Create failed: {e}"))),
            }
            therm_create_busy.set(false);
        });
    };

    // Save config
    let save_config = move || {
        let token = match auth.token.get_untracked() { Some(t) => t, None => return };
        let id = plugin_id();
        config_save_busy.set(true);
        let text = edit_text.get_untracked();
        spawn_local(async move {
            match update_plugin_config(&token, &id, &json!({ "raw": text })).await {
                Ok(()) => {
                    // Update the stored raw config and exit editing mode
                    config_raw.set(Some(edit_text.get_untracked()));
                    editing.set(false);
                    notice.set(Some("Config saved. Restart plugin to apply changes.".into()));
                }
                Err(e) => error.set(Some(format!("Save failed: {e}"))),
            }
            config_save_busy.set(false);
        });
    };

    // Devices from this plugin
    let plugin_devices = Memo::new(move |_| {
        let id = plugin_id();
        let devs = ws.devices.get();
        let mut list: Vec<_> = devs.values()
            .filter(|d| d.plugin_id == id)
            .cloned()
            .collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    });

    view! {
        <div class="plugin-detail">
            // ── Header ──────────────────────────────────────────────────────
            <div class="detail-heading">
                <div class="detail-heading-actions">
                    {
                        let nav = navigate.clone();
                        view! {
                            <button class="hc-btn hc-btn--outline"
                                on:click=move |_| nav("/plugins", Default::default())
                            >"← Plugins"</button>
                        }
                    }
                    <h2 style="flex:1; margin:0; font-size:1.1rem">
                        {move || plugin.get().map(|p| p.display_name()).unwrap_or_default()}
                    </h2>
                    {move || plugin.get().map(|p| {
                        let status = p.status.clone();
                        let managed = p.managed;
                        let do_action = do_action.clone();
                        view! {
                            <div class="plugin-detail-actions">
                                {managed.then(|| {
                                    let status2 = status.clone();
                                    let status3 = status.clone();
                                    let do_action2 = do_action.clone();
                                    let do_action3 = do_action.clone();
                                    view! {
                                        {(status == "stopped" || status == "offline").then(|| view! {
                                            <button class="hc-btn hc-btn--sm hc-btn--primary" disabled=move || busy.get()
                                                on:click=move |_| do_action("start")
                                            >"Start"</button>
                                        })}
                                        {(status2 == "active" || status2 == "starting").then(|| view! {
                                            <button class="hc-btn hc-btn--sm hc-btn--outline" disabled=move || busy.get()
                                                on:click=move |_| do_action2("stop")
                                            >"Stop"</button>
                                        })}
                                        {(status3 == "active").then(|| view! {
                                            <button class="hc-btn hc-btn--sm hc-btn--outline" disabled=move || busy.get()
                                                on:click=move |_| do_action3("restart")
                                            >"Restart"</button>
                                        })}
                                    }
                                })}
                            </div>
                        }
                    })}
                </div>
            </div>

            <ErrorBanner error=error />
            {move || notice.get().map(|n| view! {
                <div class="msg-warning" style="display:flex; align-items:center; gap:0.5rem">
                    <span>{n}</span>
                    {move || plugin.get().map(|p| p.managed).unwrap_or(false).then(|| {
                        let do_action = do_action.clone();
                        view! {
                            <button class="hc-btn hc-btn--sm hc-btn--primary" disabled=move || busy.get()
                                on:click=move |_| { notice.set(None); do_action("restart"); }
                            >"Restart Now"</button>
                        }
                    })}
                </div>
            })}

            // ── Health & Info ────────────────────────────────────────────────
            {move || plugin.get().map(|p| {
                let dot_cls = status_dot_class(&p.status);
                let label = status_label(&p.status);
                let uptime = p.uptime_str();
                let hb = p.last_heartbeat.map(|t| {
                    let secs = (chrono::Utc::now() - t).num_seconds().max(0);
                    if secs < 60 { format!("{secs}s ago") }
                    else { format!("{}m ago", secs / 60) }
                }).unwrap_or_else(|| "—".into());
                let restart_ts = p.last_restart.map(|t| t.format("%H:%M:%S").to_string()).unwrap_or_else(|| "—".into());
                let current_level = p.log_level.clone().unwrap_or_else(|| "info".into());

                view! {
                    <section class="detail-card">
                        <h3 class="detail-card-title">"Health & Status"</h3>
                        <div class="plugin-detail-grid">
                            <span class="field-label">"Status"</span>
                            <span><span class=dot_cls></span>" "{label}</span>
                            <span class="field-label">"Plugin ID"</span>
                            <span><code>{p.plugin_id.clone()}</code></span>
                            <span class="field-label">"Type"</span>
                            <span>{type_label(p.managed)}</span>
                            <span class="field-label">"Uptime"</span>
                            <span>{uptime}</span>
                            <span class="field-label">"Last Heartbeat"</span>
                            <span>{hb}</span>
                            <span class="field-label">"Last Restart"</span>
                            <span>{restart_ts}</span>
                            <span class="field-label">"Restart Count"</span>
                            <span>{p.restart_count.to_string()}</span>
                            <span class="field-label">"Devices"</span>
                            <span>{p.device_count.to_string()}</span>
                            {p.version.as_ref().map(|v| view! {
                                <span class="field-label">"Version"</span>
                                <span>{v.clone()}</span>
                            })}
                            <span class="field-label">"Management"</span>
                            <span>{if p.supports_management { "Supported" } else { "Not available" }}</span>
                        </div>
                    </section>

                    // ── Log Level ────────────────────────────────────────────
                    <section class="detail-card">
                        <h3 class="detail-card-title">"Log Level"</h3>
                        <div style="display:flex; align-items:center; gap:0.75rem">
                            <select class="hc-select" style="width:auto"
                                on:change=move |ev| on_log_level_change(event_target_value(&ev))
                            >
                                {LOG_LEVELS.iter().map(|l| {
                                    let selected = *l == current_level;
                                    view! {
                                        <option value=*l selected=selected>{*l}</option>
                                    }
                                }).collect_view()}
                            </select>
                            <span class="msg-muted" style="font-size:0.8rem">
                                {if p.supports_management { "Takes effect immediately" } else { "Requires restart" }}
                            </span>
                        </div>
                    </section>
                }
            })}

            // ── Plugin actions (manifest-driven) ────────────────────────────
            {move || plugin.get().and_then(|p| p.capabilities).and_then(|caps| {
                if caps.actions.is_empty() { return None; }
                let pid = plugin_id();
                Some(view! {
                    <PluginActionsCard plugin_id=pid capabilities=caps notice=notice error=error />
                })
            })}

            // ── Thermostat: New-thermostat wizard ───────────────────────────
            // Recalculate / Reload are now manifest actions — see the
            // ActionsCard above. The wizard stays hardcoded because it
            // needs live device pickers (sensors + actuator), which the
            // v1 capability schema subset can't represent.
            {move || (plugin.get().map(|p| p.plugin_id).as_deref() == Some("plugin.thermostat")).then(|| view! {
                <section class="detail-card">
                    <h3 class="detail-card-title">"New thermostat"</h3>
                    <div style="display:flex; gap:0.5rem; flex-wrap:wrap">
                        <button class="hc-btn hc-btn--sm hc-btn--primary"
                            on:click=move |_| therm_wizard_open.update(|v| *v = !*v)
                        >
                            {move || if therm_wizard_open.get() { "Cancel" } else { "+ New thermostat" }}
                        </button>
                    </div>

                    <Show when=move || therm_wizard_open.get()>
                        <div class="thermostat-wizard" style="margin-top:0.75rem">
                            <h4 class="field-label">"Create Thermostat"</h4>

                            <label class="field-label">"ID (slug)"</label>
                            <input type="text" class="hc-input" placeholder="e.g. living_room"
                                prop:value=move || therm_new_id.get()
                                on:input=move |ev| therm_new_id.set(event_target_value(&ev))
                            />

                            <label class="field-label">"Display Name"</label>
                            <input type="text" class="hc-input" placeholder="e.g. Living Room Thermostat"
                                prop:value=move || therm_new_name.get()
                                on:input=move |ev| therm_new_name.set(event_target_value(&ev))
                            />

                            <label class="field-label">"Temperature Sensors"</label>
                            {move || {
                                let devmap = ws.devices.get();
                                let candidates = therm_sensor_candidates(&devmap);
                                let selected = therm_new_sensors.get();
                                view! {
                                    <div class="thermostat-sensor-list">
                                        {candidates.into_iter().map(|(id, label)| {
                                            let id_for_cb = id.clone();
                                            let checked = selected.contains(&id);
                                            view! {
                                                <label class="thermostat-check-row">
                                                    <input type="checkbox" prop:checked=checked
                                                        on:change=move |ev| {
                                                            let on = event_target_checked(&ev);
                                                            therm_new_sensors.update(|v| {
                                                                if on {
                                                                    if !v.contains(&id_for_cb) { v.push(id_for_cb.clone()); }
                                                                } else {
                                                                    v.retain(|s| s != &id_for_cb);
                                                                }
                                                            });
                                                        }
                                                    />
                                                    <span>{label}</span>
                                                </label>
                                            }
                                        }).collect_view()}
                                    </div>
                                }
                            }}

                            <div class="thermostat-wizard-row">
                                <div style="flex:1">
                                    <label class="field-label">"Setpoint"</label>
                                    <input type="number" class="hc-input" step="0.5"
                                        prop:value=move || therm_new_setpoint.get().to_string()
                                        on:input=move |ev| {
                                            if let Ok(v) = event_target_value(&ev).parse::<f64>() { therm_new_setpoint.set(v); }
                                        }
                                    />
                                </div>
                                <div style="flex:1">
                                    <label class="field-label">"Hysteresis"</label>
                                    <input type="number" class="hc-input" step="0.5" min="0"
                                        prop:value=move || therm_new_hyst.get().to_string()
                                        on:input=move |ev| {
                                            if let Ok(v) = event_target_value(&ev).parse::<f64>() { therm_new_hyst.set(v.max(0.0)); }
                                        }
                                    />
                                </div>
                                <div style="flex:1">
                                    <label class="field-label">"Mode"</label>
                                    <select class="hc-select"
                                        on:change=move |ev| therm_new_mode.set(event_target_value(&ev))
                                    >
                                        {["off", "heat", "cool"].iter().map(|m| view! {
                                            <option value=*m selected=move || therm_new_mode.get() == *m>{*m}</option>
                                        }).collect_view()}
                                    </select>
                                </div>
                            </div>

                            <label class="field-label">"Actuator Device"</label>
                            {move || {
                                let devmap = ws.devices.get();
                                let candidates = therm_actuator_candidates(&devmap);
                                view! {
                                    <select class="hc-select"
                                        on:change=move |ev| therm_new_actuator.set(event_target_value(&ev))
                                    >
                                        <option value="" selected=move || therm_new_actuator.get().is_empty()>"— none —"</option>
                                        {candidates.into_iter().map(|(id, label)| {
                                            let id_for_selected = id.clone();
                                            view! {
                                                <option value=id.clone()
                                                    selected=move || therm_new_actuator.get() == id_for_selected
                                                >{label}</option>
                                            }
                                        }).collect_view()}
                                    </select>
                                }
                            }}

                            <div class="thermostat-wizard-row">
                                <div style="flex:1">
                                    <label class="field-label">"Min on (sec)"</label>
                                    <input type="number" class="hc-input" min="0"
                                        prop:value=move || therm_new_min_on.get().to_string()
                                        on:input=move |ev| {
                                            if let Ok(v) = event_target_value(&ev).parse::<u64>() { therm_new_min_on.set(v); }
                                        }
                                    />
                                </div>
                                <div style="flex:1">
                                    <label class="field-label">"Min off (sec)"</label>
                                    <input type="number" class="hc-input" min="0"
                                        prop:value=move || therm_new_min_off.get().to_string()
                                        on:input=move |ev| {
                                            if let Ok(v) = event_target_value(&ev).parse::<u64>() { therm_new_min_off.set(v); }
                                        }
                                    />
                                </div>
                            </div>

                            <button class="hc-btn hc-btn--primary" style="margin-top:0.5rem"
                                disabled=move || therm_create_busy.get()
                                    || therm_new_id.get().trim().is_empty()
                                    || therm_new_name.get().trim().is_empty()
                                on:click=move |_| do_therm_create()
                            >
                                {move || if therm_create_busy.get() { "Creating…" } else { "Create" }}
                            </button>
                        </div>
                    </Show>
                </section>
            })}

            // ── Configuration ───────────────────────────────────────────────
            <section class="detail-card">
                <div style="display:flex; align-items:center; justify-content:space-between">
                    <h3 class="detail-card-title" style="margin:0">"Configuration"</h3>
                    <div style="display:flex; gap:0.35rem; align-items:center">
                        <Show when=move || !editing.get()>
                            <button class="hc-btn hc-btn--sm hc-btn--outline"
                                disabled=move || config_raw.get().is_none()
                                on:click=move |_| {
                                    edit_text.set(config_raw.get().unwrap_or_default());
                                    editing.set(true);
                                }
                            >"Edit"</button>
                        </Show>
                        <Show when=move || editing.get()>
                            <button class="hc-btn hc-btn--sm hc-btn--primary"
                                disabled=move || config_save_busy.get()
                                on:click=move |_| save_config()
                            >{move || if config_save_busy.get() { "Saving…" } else { "Save" }}</button>
                            <button class="hc-btn hc-btn--sm hc-btn--outline"
                                disabled=move || config_save_busy.get()
                                on:click=move |_| editing.set(false)
                            >"Cancel"</button>
                        </Show>
                    </div>
                </div>

                {move || config_loading.get().then(|| view! { <p class="msg-muted">"Loading config…"</p> })}

                {move || {
                    if config_loading.get() { return None; }
                    if config_raw.get().is_none() {
                        return Some(view! {
                            <p class="msg-muted" style="margin-top:0.5rem">
                                "No configuration available for this plugin."
                            </p>
                        }.into_any());
                    }

                    if editing.get() {
                        Some(view! {
                            <textarea class="hc-textarea plugin-config-editor"
                                rows="24"
                                prop:value=move || edit_text.get()
                                on:input=move |ev| edit_text.set(event_target_value(&ev))
                            />
                        }.into_any())
                    } else {
                        Some(view! {
                            <pre class="plugin-config-viewer">{move || config_raw.get().unwrap_or_default()}</pre>
                        }.into_any())
                    }
                }}
            </section>

            // ── Devices ─────────────────────────────────────────────────────
            <section class="detail-card">
                <h3 class="detail-card-title">"Devices"</h3>
                {move || {
                    let devs = plugin_devices.get();
                    if devs.is_empty() {
                        view! { <p class="msg-muted">"No devices registered by this plugin."</p> }.into_any()
                    } else {
                        let nav = use_navigate();
                        devs.into_iter().map(|d| {
                            let id = d.device_id.clone();
                            let id_nav = id.clone();
                            let name = d.name.clone();
                            let avail_cls = if d.available { "plugin-dot plugin-dot--active" } else { "plugin-dot plugin-dot--offline" };
                            let dt = d.device_type.as_deref().unwrap_or("").to_string();
                            let nav = nav.clone();
                            view! {
                                <div class="plugin-device-row" style="cursor:pointer"
                                    on:click=move |_| {
                                        let path = format!("/devices/{}", id_nav);
                                        nav(&path, Default::default());
                                    }
                                >
                                    <span class=avail_cls></span>
                                    <span class="plugin-device-name">{name}</span>
                                    {(!dt.is_empty()).then(|| view! {
                                        <span class="plugin-badge" style="font-size:0.7rem">{dt}</span>
                                    })}
                                    <span class="plugin-device-id">{id}</span>
                                </div>
                            }
                        }).collect_view().into_any()
                    }
                }}
            </section>
        </div>
    }
}

/// Candidates for thermostat sensor picker — devices with a numeric
/// `temperature` attribute, excluding other thermostats.
fn therm_sensor_candidates(
    devices: &std::collections::HashMap<String, crate::models::DeviceState>,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = devices
        .values()
        .filter(|d| d.device_type.as_deref() != Some("thermostat"))
        .filter(|d| {
            d.attributes
                .get("temperature")
                .and_then(|v| v.as_f64())
                .is_some()
        })
        .map(|d| {
            let t = d.attributes.get("temperature")
                .and_then(|v| v.as_f64())
                .map(|v| format!("{v:.1}°"))
                .unwrap_or_default();
            (d.device_id.clone(), format!("{} ({}) — {}", d.name, d.device_id, t))
        })
        .collect();
    out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    out
}

/// Candidates for thermostat actuator picker — devices with an `on` boolean.
fn therm_actuator_candidates(
    devices: &std::collections::HashMap<String, crate::models::DeviceState>,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = devices
        .values()
        .filter(|d| d.device_type.as_deref() != Some("thermostat"))
        .filter(|d| d.attributes.get("on").and_then(|v| v.as_bool()).is_some())
        .map(|d| (d.device_id.clone(), format!("{} ({})", d.name, d.device_id)))
        .collect();
    out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    out
}

// ── Manifest-driven actions ─────────────────────────────────────────────────

/// Renders the "Actions" card driven by the plugin's published capability
/// manifest. Non-streaming actions become live buttons; streaming actions
/// are shown disabled until the ActionDrawer ships (Phase 3 Slice C).
#[component]
fn PluginActionsCard(
    plugin_id: String,
    capabilities: Capabilities,
    notice: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
) -> impl IntoView {
    let pid = plugin_id;
    view! {
        <section class="detail-card">
            <h3 class="detail-card-title">"Actions"</h3>
            <div class="plugin-actions-list">
                {capabilities.actions.into_iter().map(|action| {
                    let pid = pid.clone();
                    view! { <ActionRow plugin_id=pid action=action notice=notice error=error /> }
                }).collect_view()}
            </div>
        </section>
    }
}

/// Parsed form definition for a single param field, derived from the
/// JSON-schema subset that `pluginCapabilitiesPlan.md` §2 allows.
#[derive(Clone, Debug)]
struct ParamDef {
    name: String,
    ty: String,
    default: Option<Value>,
    enum_values: Option<Vec<Value>>,
    required: bool,
    minimum: Option<f64>,
    maximum: Option<f64>,
    description: Option<String>,
}

fn parse_params(params: Option<&Value>) -> Vec<ParamDef> {
    let Some(obj) = params.and_then(|v| v.as_object()) else { return Vec::new() };
    let mut out = Vec::with_capacity(obj.len());
    for (name, spec) in obj {
        let ty = spec
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("string")
            .to_string();
        out.push(ParamDef {
            name: name.clone(),
            ty,
            default: spec.get("default").cloned(),
            enum_values: spec
                .get("enum")
                .and_then(Value::as_array)
                .cloned(),
            required: spec.get("required").and_then(Value::as_bool).unwrap_or(false),
            minimum: spec.get("minimum").and_then(Value::as_f64),
            maximum: spec.get("maximum").and_then(Value::as_f64),
            description: spec
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn initial_form_state(params: &[ParamDef]) -> HashMap<String, Value> {
    let mut m = HashMap::new();
    for p in params {
        if let Some(d) = &p.default {
            m.insert(p.name.clone(), d.clone());
        }
    }
    m
}

#[component]
fn ParamsForm(
    params: Vec<ParamDef>,
    state: RwSignal<HashMap<String, Value>>,
) -> impl IntoView {
    view! {
        <div class="plugin-action-row__form">
            {params.into_iter().map(|p| view! {
                <ParamField def=p state=state />
            }).collect_view()}
        </div>
    }
}

#[component]
fn ActionRow(
    plugin_id: String,
    action: CapAction,
    notice: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
) -> impl IntoView {
    let auth = use_auth();
    let busy = RwSignal::new(false);
    let form_open = RwSignal::new(false);
    let drawer_open = RwSignal::new(false);

    let params = parse_params(action.params.as_ref());
    let has_params = !params.is_empty();
    let form_state: RwSignal<HashMap<String, Value>> =
        RwSignal::new(initial_form_state(&params));

    let is_streaming = action.stream;
    let is_admin = matches!(action.requires_role, RequiresRole::Admin);
    let label = action.label.clone();
    let description = action.description.clone();
    let action_id = action.id.clone();

    // Non-streaming submit path.
    let submit = {
        let plugin_id = plugin_id.clone();
        let action_id = action_id.clone();
        let label = label.clone();
        move || {
            let Some(token) = auth.token.get_untracked() else { return };
            let pid = plugin_id.clone();
            let aid = action_id.clone();
            let label = label.clone();
            let body = Value::Object(
                form_state
                    .get_untracked()
                    .into_iter()
                    .collect::<serde_json::Map<_, _>>(),
            );
            busy.set(true);
            spawn_local(async move {
                match send_plugin_command(&token, &pid, &aid, body).await {
                    Ok(_) => {
                        notice.set(Some(format!("{label} — done.")));
                        form_open.set(false);
                    }
                    Err(e) => error.set(Some(format!("{label}: {e}"))),
                }
                busy.set(false);
            });
        }
    };

    let submit_click = submit.clone();
    let submit_run = submit;

    let button_label = move || {
        if busy.get() {
            "Running…".to_string()
        } else if is_streaming {
            "Run".to_string()
        } else if has_params {
            if form_open.get() { "Hide".to_string() } else { "Configure…".to_string() }
        } else {
            "Run".to_string()
        }
    };

    let params_for_form = params.clone();
    let drawer_plugin_id = plugin_id.clone();
    let drawer_action = action.clone();

    view! {
        <div class="plugin-action-row">
            <div class="plugin-action-row__head">
                <div class="plugin-action-row__meta">
                    <div class="plugin-action-row__label">
                        {label.clone()}
                        {is_admin.then(|| view! {
                            <span class="plugin-badge" style="margin-left:0.5rem; font-size:0.7rem">"admin"</span>
                        })}
                        {is_streaming.then(|| view! {
                            <span class="plugin-badge" style="margin-left:0.35rem; font-size:0.7rem">"streaming"</span>
                        })}
                    </div>
                    {description.clone().map(|d| view! {
                        <div class="msg-muted" style="font-size:0.8rem; margin-top:0.15rem">{d}</div>
                    })}
                </div>
                <div class="plugin-action-row__controls">
                    <button
                        class="hc-btn hc-btn--sm hc-btn--primary"
                        disabled=move || busy.get()
                        on:click=move |_| {
                            if is_streaming {
                                drawer_open.set(true);
                            } else if has_params {
                                form_open.update(|v| *v = !*v);
                            } else {
                                submit_click();
                            }
                        }
                    >
                        {button_label}
                    </button>
                </div>
            </div>

            <Show when=move || has_params && !is_streaming && form_open.get()>
                <div class="plugin-action-row__form">
                    {params_for_form.iter().cloned().map(|p| view! {
                        <ParamField def=p state=form_state />
                    }).collect_view()}
                    <div style="display:flex; gap:0.5rem; margin-top:0.5rem">
                        <button
                            class="hc-btn hc-btn--sm hc-btn--primary"
                            disabled=move || busy.get()
                            on:click={
                                let submit_run = submit_run.clone();
                                move |_| submit_run()
                            }
                        >
                            {move || if busy.get() { "Running…" } else { "Run" }}
                        </button>
                        <button
                            class="hc-btn hc-btn--sm hc-btn--outline"
                            disabled=move || busy.get()
                            on:click=move |_| form_open.set(false)
                        >"Cancel"</button>
                    </div>
                </div>
            </Show>

            {is_streaming.then(move || view! {
                <ActionDrawer
                    plugin_id=drawer_plugin_id.clone()
                    action=drawer_action.clone()
                    open=drawer_open
                    error=error
                />
            })}
        </div>
    }
}

/// Single input for one `ParamDef`. Writes directly into the shared
/// `form_state` map under the param's name. Arrays and objects fall back
/// to a raw-JSON textarea in v1.
#[component]
fn ParamField(def: ParamDef, state: RwSignal<HashMap<String, Value>>) -> impl IntoView {
    let name = def.name.clone();
    let label_text = if def.required {
        format!("{} *", def.name)
    } else {
        def.name.clone()
    };

    let current = {
        let name = name.clone();
        move || state.get().get(&name).cloned().unwrap_or(Value::Null)
    };

    let set_val = {
        let name = name.clone();
        move |v: Value| {
            state.update(|m| {
                if v.is_null() {
                    m.remove(&name);
                } else {
                    m.insert(name.clone(), v);
                }
            });
        }
    };

    let help = def.description.clone().map(|d| view! {
        <div class="msg-muted" style="font-size:0.75rem; margin-top:0.15rem">{d}</div>
    });

    let input = match def.ty.as_str() {
        "boolean" => {
            let set_val = set_val.clone();
            let current = current.clone();
            view! {
                <label class="plugin-action-row__check">
                    <input
                        type="checkbox"
                        prop:checked=move || current().as_bool().unwrap_or(false)
                        on:change=move |ev| set_val(Value::Bool(event_target_checked(&ev)))
                    />
                    <span>{def.name.clone()}</span>
                </label>
            }.into_any()
        }
        "integer" | "number" => {
            let is_int = def.ty == "integer";
            let min_attr = def.minimum.map(|v| v.to_string()).unwrap_or_default();
            let max_attr = def.maximum.map(|v| v.to_string()).unwrap_or_default();
            let set_val = set_val.clone();
            let current = current.clone();
            view! {
                <input
                    type="number"
                    class="hc-input"
                    step=if is_int { "1" } else { "any" }
                    min=min_attr
                    max=max_attr
                    prop:value=move || match current() {
                        Value::Number(n) => n.to_string(),
                        _ => String::new(),
                    }
                    on:input=move |ev| {
                        let raw = event_target_value(&ev);
                        if raw.is_empty() {
                            set_val(Value::Null);
                        } else if is_int {
                            if let Ok(n) = raw.parse::<i64>() { set_val(json!(n)); }
                        } else if let Ok(n) = raw.parse::<f64>() {
                            set_val(json!(n));
                        }
                    }
                />
            }.into_any()
        }
        "string" if def.enum_values.is_some() => {
            let options = def.enum_values.clone().unwrap_or_default();
            let set_val = set_val.clone();
            let current = current.clone();
            view! {
                <select
                    class="hc-select"
                    on:change=move |ev| set_val(Value::String(event_target_value(&ev)))
                >
                    {options.into_iter().map(|opt| {
                        let s = opt.as_str().unwrap_or("").to_string();
                        let s_for_selected = s.clone();
                        let s_for_label = s.clone();
                        let current = current.clone();
                        let selected = move || current().as_str() == Some(s_for_selected.as_str());
                        view! { <option value=s prop:selected=selected>{s_for_label}</option> }
                    }).collect_view()}
                </select>
            }.into_any()
        }
        "string" => {
            let set_val = set_val.clone();
            let current = current.clone();
            view! {
                <input
                    type="text"
                    class="hc-input"
                    prop:value=move || current().as_str().unwrap_or("").to_string()
                    on:input=move |ev| {
                        let s = event_target_value(&ev);
                        if s.is_empty() { set_val(Value::Null); }
                        else { set_val(Value::String(s)); }
                    }
                />
            }.into_any()
        }
        _ => {
            let set_val = set_val.clone();
            let current = current.clone();
            view! {
                <textarea
                    class="hc-textarea"
                    rows="3"
                    placeholder="JSON"
                    prop:value=move || match current() {
                        Value::Null => String::new(),
                        v => serde_json::to_string(&v).unwrap_or_default(),
                    }
                    on:input=move |ev| {
                        let raw = event_target_value(&ev);
                        if raw.trim().is_empty() {
                            set_val(Value::Null);
                        } else if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                            set_val(v);
                        }
                    }
                />
            }.into_any()
        }
    };

    view! {
        <div class="plugin-action-row__field">
            <label class="field-label">{label_text}</label>
            {input}
            {help}
        </div>
    }
}

// ── Streaming ActionDrawer ──────────────────────────────────────────────────

/// Holds the live `EventSource` for a streaming action. Dropping the
/// holder closes the connection; the registered `Closure`s keep themselves
/// alive by being `.forget()`-ed into JS on registration.
struct StreamHandle {
    es: web_sys::EventSource,
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        let _ = self.es.close();
    }
}

fn terminal_stage(s: &str) -> bool {
    matches!(s, "complete" | "error" | "canceled" | "timeout")
}

/// Streaming drawer for a single action. POSTs the command, opens an SSE
/// connection, and renders stage events live. The component lifecycle
/// matches the `open` signal; closing the drawer tears down the SSE.
#[component]
fn ActionDrawer(
    plugin_id: String,
    action: CapAction,
    open: RwSignal<bool>,
    error: RwSignal<Option<String>>,
) -> impl IntoView {
    use std::rc::Rc;
    let auth = use_auth();

    // Run state — all Copy.
    let starting = RwSignal::new(false);
    let request_id: RwSignal<Option<String>> = RwSignal::new(None);
    let items: RwSignal<Vec<Value>> = RwSignal::new(Vec::new());
    let last_progress: RwSignal<Option<Value>> = RwSignal::new(None);
    let warnings: RwSignal<Vec<Value>> = RwSignal::new(Vec::new());
    let pending_prompt: RwSignal<Option<Value>> = RwSignal::new(None);
    let terminal: RwSignal<Option<Value>> = RwSignal::new(None);
    let started = RwSignal::new(false);

    // Param form state (same shape as ActionRow's non-streaming form).
    let params = parse_params(action.params.as_ref());
    let has_params = !params.is_empty();
    let form_state: RwSignal<HashMap<String, Value>> =
        RwSignal::new(initial_form_state(&params));
    let respond_state: RwSignal<HashMap<String, Value>> = RwSignal::new(HashMap::new());

    // Non-Copy props go into StoredValue::new_local so click closures
    // stay Fn without per-clone ceremony at every use site.
    let plugin_id_sv: StoredValue<String, leptos::reactive::owner::LocalStorage> =
        StoredValue::new_local(plugin_id);
    let action_id_sv: StoredValue<String, leptos::reactive::owner::LocalStorage> =
        StoredValue::new_local(action.id.clone());
    let item_key_sv: StoredValue<Option<String>, leptos::reactive::owner::LocalStorage> =
        StoredValue::new_local(action.item_key.clone());
    let params_sv: StoredValue<Vec<ParamDef>, leptos::reactive::owner::LocalStorage> =
        StoredValue::new_local(params.clone());

    let cancelable = action.cancelable;
    let label = action.label.clone();
    let description = action.description.clone();
    let is_admin = matches!(action.requires_role, RequiresRole::Admin);

    // Keep the SSE handle + callbacks alive for the lifetime of the run.
    let stream_holder: StoredValue<Option<StreamHandle>, leptos::reactive::owner::LocalStorage> =
        StoredValue::new_local(None);

    // Shared event apply logic; reads item_key from the StoredValue.
    let apply_event = move |ev: Value| {
        let stage = ev
            .get("stage")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        match stage.as_str() {
            "progress" => last_progress.set(Some(ev)),
            "item" => {
                let op = ev.get("op").and_then(Value::as_str).unwrap_or("").to_string();
                let data = ev.get("data").cloned().unwrap_or(Value::Null);
                let key = item_key_sv
                    .with_value(|k| k.clone())
                    .unwrap_or_else(|| "id".to_string());
                let key_val = data.get(&key).cloned();
                items.update(|list| {
                    let matches_existing = |v: &Value| -> bool {
                        match (&key_val, v.get(&key)) {
                            (Some(k), Some(existing)) => k == existing,
                            _ => false,
                        }
                    };
                    match op.as_str() {
                        "add" | "update" => {
                            if let Some(pos) = list.iter().position(matches_existing) {
                                list[pos] = data;
                            } else {
                                list.push(data);
                            }
                        }
                        "remove" => list.retain(|v| !matches_existing(v)),
                        _ => {}
                    }
                });
            }
            "awaiting_user" => {
                respond_state.set(HashMap::new());
                pending_prompt.set(Some(ev));
            }
            "warning" => warnings.update(|w| w.push(ev)),
            s if terminal_stage(s) => {
                terminal.set(Some(ev));
                stream_holder.update_value(|h| *h = None);
            }
            _ => {}
        }
    };

    // Start the action: POST command, open SSE on returned request_id.
    let start_action: Rc<dyn Fn()> = Rc::new(move || {
        let Some(token) = auth.token.get_untracked() else {
            error.set(Some("not authenticated".into()));
            return;
        };
        let pid = plugin_id_sv.with_value(|v| v.clone());
        let aid = action_id_sv.with_value(|v| v.clone());
        let body = Value::Object(
            form_state
                .get_untracked()
                .into_iter()
                .collect::<serde_json::Map<_, _>>(),
        );
        let apply_for_spawn = apply_event;
        starting.set(true);
        started.set(true);
        spawn_local(async move {
            match send_plugin_command(&token, &pid, &aid, body).await {
                Ok(resp) => {
                    // Concurrency:single busy response: server returned the
                    // active_request_id of the in-flight invocation.
                    // Surface it as a soft error and reset to the pre-start
                    // form so the user can wait or hit Cancel separately.
                    if resp.get("status").and_then(Value::as_str) == Some("busy") {
                        let active = resp
                            .get("active_request_id")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        error.set(Some(format!(
                            "another invocation is already running (request_id {active}); \
                             wait for it to finish or cancel it first"
                        )));
                        started.set(false);
                        starting.set(false);
                        return;
                    }
                    let rid = resp
                        .get("request_id")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    match rid {
                        Some(rid) => {
                            request_id.set(Some(rid.clone()));
                            let url = plugin_stream_sse_url(&pid, &rid, &token);
                            match web_sys::EventSource::new(&url) {
                                Ok(es) => {
                                    let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
                                        move |ev: web_sys::MessageEvent| {
                                            let raw = ev.data().as_string().unwrap_or_default();
                                            if raw.is_empty() { return; }
                                            if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                                                apply_for_spawn(v);
                                            }
                                        },
                                    );
                                    let _ = es.add_event_listener_with_callback(
                                        "stream",
                                        on_message.as_ref().unchecked_ref(),
                                    );
                                    on_message.forget();

                                    let on_err = Closure::<dyn FnMut(web_sys::Event)>::new(
                                        move |_ev: web_sys::Event| {},
                                    );
                                    es.set_onerror(Some(on_err.as_ref().unchecked_ref()));
                                    on_err.forget();

                                    stream_holder.update_value(|h| *h = Some(StreamHandle { es }));
                                }
                                Err(e) => {
                                    error.set(Some(format!(
                                        "failed to open stream: {}",
                                        e.as_string().unwrap_or_else(|| "EventSource error".into())
                                    )));
                                    started.set(false);
                                }
                            }
                        }
                        None => {
                            error.set(Some("plugin did not return request_id".into()));
                            started.set(false);
                        }
                    }
                }
                Err(e) => {
                    error.set(Some(format!("start failed: {e}")));
                    started.set(false);
                }
            }
            starting.set(false);
        });
    });

    let cancel_action: Rc<dyn Fn()> = Rc::new(move || {
        let Some(token) = auth.token.get_untracked() else { return };
        let Some(rid) = request_id.get_untracked() else { return };
        let pid = plugin_id_sv.with_value(|v| v.clone());
        spawn_local(async move {
            let body = json!({ "target_request_id": rid });
            if let Err(e) = send_plugin_command(&token, &pid, "cancel", body).await {
                error.set(Some(format!("cancel failed: {e}")));
            }
        });
    });

    let respond_action: Rc<dyn Fn()> = Rc::new(move || {
        let Some(token) = auth.token.get_untracked() else { return };
        let Some(rid) = request_id.get_untracked() else { return };
        let pid = plugin_id_sv.with_value(|v| v.clone());
        let response = Value::Object(
            respond_state
                .get_untracked()
                .into_iter()
                .collect::<serde_json::Map<_, _>>(),
        );
        spawn_local(async move {
            let body = json!({ "target_request_id": rid, "response": response });
            match send_plugin_command(&token, &pid, "respond", body).await {
                Ok(_) => pending_prompt.set(None),
                Err(e) => error.set(Some(format!("respond failed: {e}"))),
            }
        });
    });

    let close_drawer: Rc<dyn Fn()> = Rc::new(move || {
        started.set(false);
        starting.set(false);
        request_id.set(None);
        items.set(Vec::new());
        last_progress.set(None);
        warnings.set(Vec::new());
        pending_prompt.set(None);
        terminal.set(None);
        respond_state.set(HashMap::new());
        stream_holder.update_value(|h| *h = None);
        open.set(false);
    });

    // Store the Rc-wrapped closures so use sites can clone them cheaply.
    let start_sv: StoredValue<Rc<dyn Fn()>, leptos::reactive::owner::LocalStorage> =
        StoredValue::new_local(start_action);
    let cancel_sv: StoredValue<Rc<dyn Fn()>, leptos::reactive::owner::LocalStorage> =
        StoredValue::new_local(cancel_action);
    let respond_sv: StoredValue<Rc<dyn Fn()>, leptos::reactive::owner::LocalStorage> =
        StoredValue::new_local(respond_action);
    let close_sv: StoredValue<Rc<dyn Fn()>, leptos::reactive::owner::LocalStorage> =
        StoredValue::new_local(close_drawer);

    on_cleanup(move || {
        stream_holder.update_value(|h| *h = None);
    });

    view! {
        <Show when=move || open.get()>
            <div
                class="action-drawer-backdrop"
                on:click=move |_| close_sv.with_value(|f| f())
            >
                <div
                    class="action-drawer"
                    on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
                >
                    <header class="action-drawer__head">
                        <div>
                            <div class="action-drawer__title">{label.clone()}</div>
                            {is_admin.then(|| view! {
                                <span class="plugin-badge" style="margin-right:0.35rem; font-size:0.7rem">"admin"</span>
                            })}
                            <span class="plugin-badge" style="font-size:0.7rem">"streaming"</span>
                        </div>
                        <button
                            class="hc-btn hc-btn--sm hc-btn--outline"
                            on:click=move |_| close_sv.with_value(|f| f())
                        >"Close"</button>
                    </header>

                    {description.clone().map(|d| view! {
                        <p class="msg-muted action-drawer__desc">{d}</p>
                    })}

                    // Pre-start: show params form.
                    <Show when=move || !started.get()>
                        <div class="action-drawer__body">
                            {move || has_params.then(|| {
                                let params = params_sv.with_value(|v| v.clone());
                                view! { <ParamsForm params=params state=form_state /> }
                            })}
                            <div class="action-drawer__footer">
                                <button
                                    class="hc-btn hc-btn--sm hc-btn--primary"
                                    disabled=move || starting.get()
                                    on:click=move |_| start_sv.with_value(|f| f())
                                >
                                    {move || if starting.get() { "Starting…" } else { "Run" }}
                                </button>
                            </div>
                        </div>
                    </Show>

                    // Running: show live state.
                    <Show when=move || started.get()>
                        <div class="action-drawer__body">
                            {move || last_progress.get().map(|ev| view! {
                                <ProgressBanner ev=ev />
                            })}

                            {move || pending_prompt.get().map(|prompt_ev| view! {
                                <AwaitingUserCard
                                    ev=prompt_ev
                                    state=respond_state
                                    submit=move || respond_sv.with_value(|f| f())
                                />
                            })}

                            {move || {
                                let items = items.get();
                                if items.is_empty() { return None; }
                                let key = item_key_sv.with_value(|k| k.clone());
                                Some(view! {
                                    <div class="action-drawer__items">
                                        <div class="field-label">"Items"</div>
                                        <ul class="action-drawer__item-list">
                                            {items.into_iter().map(|it| {
                                                let key = key.clone();
                                                view! { <StreamItemRow item=it item_key=key /> }
                                            }).collect_view()}
                                        </ul>
                                    </div>
                                })
                            }}

                            {move || {
                                let ws = warnings.get();
                                if ws.is_empty() { return None; }
                                Some(view! {
                                    <div class="action-drawer__warnings">
                                        {ws.into_iter().map(|w| {
                                            let msg = w.get("message")
                                                .and_then(Value::as_str)
                                                .unwrap_or("")
                                                .to_string();
                                            view! { <div class="action-drawer__warning">{msg}</div> }
                                        }).collect_view()}
                                    </div>
                                })
                            }}

                            {move || terminal.get().map(|t| view! {
                                <TerminalCard ev=t />
                            })}

                            <div class="action-drawer__footer">
                                <Show when=move || cancelable && terminal.get().is_none()>
                                    <button
                                        class="hc-btn hc-btn--sm hc-btn--outline"
                                        on:click=move |_| cancel_sv.with_value(|f| f())
                                    >"Cancel"</button>
                                </Show>
                                <Show when=move || terminal.get().is_some()>
                                    <button
                                        class="hc-btn hc-btn--sm hc-btn--primary"
                                        on:click=move |_| close_sv.with_value(|f| f())
                                    >"Done"</button>
                                </Show>
                            </div>
                        </div>
                    </Show>
                </div>
            </div>
        </Show>
    }
}

#[component]
fn ProgressBanner(ev: Value) -> impl IntoView {
    let pct = ev.get("percent").and_then(Value::as_u64).map(|v| v as u8);
    let label = ev
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let message = ev
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string);
    let width = pct.map(|p| format!("{p}%")).unwrap_or_else(|| "0%".into());
    view! {
        <div class="action-drawer__progress">
            <div class="action-drawer__progress-row">
                <span class="action-drawer__progress-label">{label}</span>
                {pct.map(|p| view! { <span class="action-drawer__progress-pct">{format!("{p}%")}</span> })}
            </div>
            <div class="action-drawer__progress-bar">
                <div class="action-drawer__progress-bar-fill" style=format!("width:{width}")></div>
            </div>
            {message.map(|m| view! { <div class="msg-muted" style="font-size:0.8rem">{m}</div> })}
        </div>
    }
}

#[component]
fn AwaitingUserCard(
    ev: Value,
    state: RwSignal<HashMap<String, Value>>,
    submit: impl Fn() + Clone + 'static,
) -> impl IntoView {
    let prompt = ev
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let schema = ev.get("response_schema").cloned();
    let params = schema
        .as_ref()
        .map(|s| parse_params(Some(s)))
        .unwrap_or_default();
    let has_schema = !params.is_empty();

    let submit_for_click = submit.clone();
    view! {
        <div class="action-drawer__prompt">
            <div class="action-drawer__prompt-title">"Awaiting response"</div>
            <div class="action-drawer__prompt-body">{prompt}</div>
            {has_schema.then(move || view! {
                <div class="plugin-action-row__form" style="margin-top:0.5rem">
                    {params.iter().cloned().map(|p| view! {
                        <ParamField def=p state=state />
                    }).collect_view()}
                </div>
            })}
            <div style="display:flex; gap:0.5rem; margin-top:0.5rem">
                <button
                    class="hc-btn hc-btn--sm hc-btn--primary"
                    on:click=move |_| submit_for_click()
                >"Submit response"</button>
            </div>
        </div>
    }
}

#[component]
fn TerminalCard(ev: Value) -> impl IntoView {
    let stage = ev
        .get("stage")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let class = match stage.as_str() {
        "complete" => "action-drawer__terminal action-drawer__terminal--ok",
        _ => "action-drawer__terminal action-drawer__terminal--err",
    };
    let message = ev
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string);
    let data = ev.get("data").cloned();
    view! {
        <div class=class>
            <div class="action-drawer__terminal-stage">{stage}</div>
            {message.map(|m| view! { <div>{m}</div> })}
            {data.and_then(|d| if d.is_null() { None } else {
                Some(view! {
                    <pre class="action-drawer__terminal-data">
                        {serde_json::to_string_pretty(&d).unwrap_or_default()}
                    </pre>
                })
            })}
        </div>
    }
}

/// One row in the streaming-item list. Renders the manifest's `item_key`
/// field as the primary identifier, lifts `label`/`name` and `status` if
/// present, and tucks the full JSON behind a click-to-expand twisty.
/// Generic — works for every plugin's item shape.
#[component]
fn StreamItemRow(item: Value, item_key: Option<String>) -> impl IntoView {
    let key = item_key.as_deref().unwrap_or("id");
    let id_str = item
        .get(key)
        .map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            other => other.to_string(),
        })
        .unwrap_or_default();
    let title = item
        .get("label")
        .and_then(Value::as_str)
        .or_else(|| item.get("name").and_then(Value::as_str))
        .or_else(|| item.get("manufacturer").and_then(Value::as_str))
        .map(str::to_string);
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string);
    let pretty = serde_json::to_string_pretty(&item).unwrap_or_default();
    let expanded = RwSignal::new(false);

    let status_class = status
        .as_deref()
        .map(|s| {
            // Sanitize status into a CSS-safe modifier; unknown values
            // still get a generic pill.
            let safe: String = s
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
                .collect();
            format!("action-drawer__item-status action-drawer__item-status--{safe}")
        })
        .unwrap_or_else(|| "action-drawer__item-status".into());

    view! {
        <li class="action-drawer__item">
            <div
                class="action-drawer__item-row"
                on:click=move |_| expanded.update(|e| *e = !*e)
            >
                <span class="action-drawer__item-id">{id_str}</span>
                {title.map(|t| view! {
                    <span class="action-drawer__item-title">{t}</span>
                })}
                {status.map(|s| view! {
                    <span class=status_class.clone()>{s}</span>
                })}
                <span class="action-drawer__item-twisty">
                    {move || if expanded.get() { "▾" } else { "▸" }}
                </span>
            </div>
            <Show when=move || expanded.get()>
                <pre class="action-drawer__item-details">{pretty.clone()}</pre>
            </Show>
        </li>
    }
}
