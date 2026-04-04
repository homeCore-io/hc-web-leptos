//! Plugins management page — list all plugins with status, controls, and
//! navigation to detail pages.

use crate::api::{
    fetch_plugin_config, fetch_plugins, patch_plugin, restart_plugin, start_plugin, stop_plugin,
    update_plugin_config,
};
use crate::auth::use_auth;
use crate::models::PluginInfo;
use crate::pages::shared::SearchField;
use crate::ws::use_ws;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use serde_json::{json, Value};
use std::collections::HashSet;

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

            {move || error.get().map(|e| view! { <p class="msg-error">{e}</p> })}

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

            // ── Plugin cards ────────────────────────────────────────────────
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

                            // Action button closures
                            let id_start = id.clone();
                            let id_stop = id.clone();
                            let id_restart = id.clone();

                            view! {
                                <div class="plugin-card" style="cursor:pointer"
                                    on:click=move |_| {
                                        let path = format!("/plugins/{}", id_nav);
                                        nav(&path, Default::default());
                                    }
                                >
                                    <div class="plugin-card-header">
                                        <span class=dot_cls></span>
                                        <span class="plugin-name">{name}</span>
                                        <span class="plugin-badge plugin-status-badge">{label}</span>
                                        {(!version.is_empty()).then(|| view! {
                                            <span class="plugin-badge plugin-version-badge">{"v"}{version.clone()}</span>
                                        })}
                                        <span class="plugin-badge plugin-type-badge">{type_label(managed)}</span>
                                    </div>
                                    <div class="plugin-card-meta">
                                        <span title="Devices"><span class="material-icons" style="font-size:14px">"devices"</span>" "{device_count.to_string()}</span>
                                        <span title="Uptime"><span class="material-icons" style="font-size:14px">"schedule"</span>" "{uptime}</span>
                                        {(restart_count > 0).then(|| view! {
                                            <span title="Restarts"><span class="material-icons" style="font-size:14px">"refresh"</span>" "{restart_count.to_string()}</span>
                                        })}
                                    </div>
                                    {managed.then(|| {
                                        view! {
                                            <div class="plugin-card-actions" on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()>
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
    let config_json: RwSignal<Option<Value>> = RwSignal::new(None);
    let config_loading = RwSignal::new(false);
    let config_dirty = RwSignal::new(false);
    let raw_mode = RwSignal::new(false);
    let raw_text = RwSignal::new(String::new());
    let config_save_busy = RwSignal::new(false);

    // Live plugin data from WS map
    let plugin = Memo::new(move |_| {
        let id = plugin_id();
        ws.plugins.get().get(&id).cloned()
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
                    if let Some(config) = resp.get("config") {
                        config_json.set(Some(config.clone()));
                        // Convert to TOML string for raw editor
                        if let Ok(toml_val) = serde_json::from_value::<toml::Value>(config.clone()) {
                            let toml_str = toml::to_string_pretty(&toml_val).unwrap_or_default();
                            raw_text.set(toml_str.clone());
                            config_raw.set(Some(toml_str));
                        }
                    } else if let Some(raw) = resp["raw"].as_str() {
                        raw_text.set(raw.to_string());
                        config_raw.set(Some(raw.to_string()));
                    }
                }
                Err(e) => {
                    // Config unavailable is not fatal — remote plugins may not support it
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

    // Save config
    let save_config = move || {
        let token = match auth.token.get_untracked() { Some(t) => t, None => return };
        let id = plugin_id();
        config_save_busy.set(true);
        let body = if raw_mode.get_untracked() {
            json!({ "raw": raw_text.get_untracked() })
        } else if let Some(cfg) = config_json.get_untracked() {
            json!({ "config": cfg })
        } else {
            json!({ "raw": raw_text.get_untracked() })
        };
        spawn_local(async move {
            match update_plugin_config(&token, &id, &body).await {
                Ok(()) => {
                    notice.set(Some("Config saved. Restart plugin to apply changes.".into()));
                    config_dirty.set(false);
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

            {move || error.get().map(|e| view! { <p class="msg-error">{e}</p> })}
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

            // ── Configuration Editor ────────────────────────────────────────
            <section class="detail-card">
                <div style="display:flex; align-items:center; justify-content:space-between">
                    <h3 class="detail-card-title" style="margin:0">"Configuration"</h3>
                    <div style="display:flex; gap:0.5rem; align-items:center">
                        <button class="hc-btn hc-btn--sm"
                            class:hc-btn--outline=move || !raw_mode.get()
                            class:hc-btn--primary=move || raw_mode.get()
                            on:click=move |_| raw_mode.update(|v| *v = !*v)
                        >{move || if raw_mode.get() { "Structured" } else { "Raw TOML" }}</button>
                        <button class="hc-btn hc-btn--sm hc-btn--primary"
                            disabled=move || config_save_busy.get() || !config_dirty.get()
                            on:click=move |_| save_config()
                        >{move || if config_save_busy.get() { "Saving…" } else { "Save" }}</button>
                    </div>
                </div>

                {move || config_loading.get().then(|| view! { <p class="msg-muted">"Loading config…"</p> })}

                {move || {
                    if config_loading.get() { return None; }
                    if config_json.get().is_none() && config_raw.get().is_none() {
                        return Some(view! {
                            <p class="msg-muted" style="margin-top:0.5rem">
                                "No configuration available for this plugin."
                            </p>
                        }.into_any());
                    }

                    if raw_mode.get() {
                        Some(view! {
                            <textarea class="hc-textarea plugin-config-editor"
                                rows="20"
                                prop:value=move || raw_text.get()
                                on:input=move |ev| {
                                    raw_text.set(event_target_value(&ev));
                                    config_dirty.set(true);
                                }
                            />
                        }.into_any())
                    } else if let Some(cfg) = config_json.get() {
                        // Structured view: render each top-level TOML section
                        if let Some(obj) = cfg.as_object() {
                            let sections: Vec<_> = obj.iter().map(|(k, v)| {
                                let key = k.clone();
                                let val = v.clone();
                                view! { <ConfigSection key=key.clone() value=val config_json config_dirty /> }
                            }).collect();
                            Some(view! {
                                <div class="plugin-config-structured">{sections}</div>
                            }.into_any())
                        } else {
                            Some(view! {
                                <pre class="activity-detail">{serde_json::to_string_pretty(&cfg).unwrap_or_default()}</pre>
                            }.into_any())
                        }
                    } else {
                        // Raw-only (parse failed)
                        Some(view! {
                            <textarea class="hc-textarea plugin-config-editor"
                                rows="20"
                                prop:value=move || raw_text.get()
                                on:input=move |ev| {
                                    raw_text.set(event_target_value(&ev));
                                    config_dirty.set(true);
                                }
                            />
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

// ── Structured config section ───────────────────────────────────────────────

#[component]
fn ConfigSection(
    key: String,
    value: Value,
    config_json: RwSignal<Option<Value>>,
    config_dirty: RwSignal<bool>,
) -> impl IntoView {
    let collapsed = RwSignal::new(false);

    view! {
        <div class="config-section">
            <div class="config-section-header" on:click=move |_| collapsed.update(|v| *v = !*v)>
                <span class="material-icons" style="font-size:16px">
                    {move || if collapsed.get() { "expand_more" } else { "expand_less" }}
                </span>
                <span class="config-section-title">{"["}{key.clone()}{"]"}</span>
            </div>
            <Show when=move || !collapsed.get()>
                <div class="config-section-body">
                    {if let Some(obj) = value.as_object() {
                        obj.iter().map(|(field_key, field_val)| {
                            let fk = field_key.clone();
                            let section = key.clone();
                            render_config_field(fk, field_val.clone(), section, config_json, config_dirty)
                        }).collect_view().into_any()
                    } else {
                        // Non-object value at section level — show as read-only
                        view! {
                            <div class="config-field">
                                <span class="config-field-value">{value.to_string()}</span>
                            </div>
                        }.into_any()
                    }}
                </div>
            </Show>
        </div>
    }
}

fn render_config_field(
    key: String,
    value: Value,
    section: String,
    config_json: RwSignal<Option<Value>>,
    config_dirty: RwSignal<bool>,
) -> impl IntoView {
    let display_key = key.clone();
    let update_field = move |new_val: Value| {
        config_json.update(|cfg| {
            if let Some(ref mut c) = cfg {
                if let Some(sec) = c.get_mut(&section) {
                    if let Some(obj) = sec.as_object_mut() {
                        obj.insert(key.clone(), new_val);
                    }
                }
            }
        });
        config_dirty.set(true);
    };

    view! {
        <div class="config-field">
            <label class="config-field-label">{display_key}</label>
            {match &value {
                Value::Bool(b) => {
                    let checked = *b;
                    view! {
                        <input type="checkbox" class="hc-checkbox"
                            prop:checked=checked
                            on:change=move |ev| {
                                let v = event_target_checked(&ev);
                                update_field(Value::Bool(v));
                            }
                        />
                    }.into_any()
                }
                Value::Number(n) => {
                    let s = n.to_string();
                    view! {
                        <input type="text" class="hc-input hc-input--sm"
                            prop:value=s
                            on:change=move |ev| {
                                let text = event_target_value(&ev);
                                let v = text.parse::<f64>()
                                    .map(|f| serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::String(text.clone())))
                                    .unwrap_or_else(|_| {
                                        text.parse::<i64>()
                                            .map(|i| Value::Number(i.into()))
                                            .unwrap_or(Value::String(text))
                                    });
                                update_field(v);
                            }
                        />
                    }.into_any()
                }
                Value::String(s) => {
                    let s = s.clone();
                    view! {
                        <input type="text" class="hc-input hc-input--sm"
                            prop:value=s
                            on:change=move |ev| {
                                update_field(Value::String(event_target_value(&ev)));
                            }
                        />
                    }.into_any()
                }
                _ => {
                    // Arrays, nested objects — show as JSON text
                    let pretty = serde_json::to_string_pretty(&value).unwrap_or_default();
                    view! {
                        <pre class="config-field-complex">{pretty}</pre>
                    }.into_any()
                }
            }}
        </div>
    }
}

fn event_target_checked(ev: &web_sys::Event) -> bool {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.checked())
        .unwrap_or(false)
}
