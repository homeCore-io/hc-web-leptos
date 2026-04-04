//! Plugins management page — list all plugins with status, controls, and
//! navigation to detail pages.

use crate::api::{fetch_plugins, start_plugin, stop_plugin, restart_plugin};
use crate::auth::use_auth;
use crate::models::PluginInfo;
use crate::pages::shared::SearchField;
use crate::ws::use_ws;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
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

// ── Detail page placeholder (Phase 4) ───────────────────────────────────────

#[component]
pub fn PluginDetailPlaceholder() -> impl IntoView {
    let ws = use_ws();
    let params = leptos_router::hooks::use_params_map();
    let plugin_id = move || params.read().get("id").unwrap_or_default();
    let navigate = use_navigate();

    let plugin = Memo::new(move |_| {
        let id = plugin_id();
        ws.plugins.get().get(&id).cloned()
    });

    view! {
        <div class="plugin-detail">
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
                </div>
            </div>

            {move || plugin.get().map(|p| {
                let dot_cls = status_dot_class(&p.status);
                let label = status_label(&p.status);
                let uptime = p.uptime_str();
                view! {
                    <section class="detail-card">
                        <h3 class="detail-card-title">"Plugin Info"</h3>
                        <div class="plugin-detail-grid">
                            <span class="field-label">"Status"</span>
                            <span><span class=dot_cls></span>" "{label}</span>
                            <span class="field-label">"Plugin ID"</span>
                            <span><code>{p.plugin_id.clone()}</code></span>
                            <span class="field-label">"Type"</span>
                            <span>{type_label(p.managed)}</span>
                            <span class="field-label">"Uptime"</span>
                            <span>{uptime}</span>
                            <span class="field-label">"Devices"</span>
                            <span>{p.device_count.to_string()}</span>
                            <span class="field-label">"Restarts"</span>
                            <span>{p.restart_count.to_string()}</span>
                            {p.version.as_ref().map(|v| view! {
                                <span class="field-label">"Version"</span>
                                <span>{v.clone()}</span>
                            })}
                            {p.log_level.as_ref().map(|l| view! {
                                <span class="field-label">"Log Level"</span>
                                <span>{l.clone()}</span>
                            })}
                        </div>
                    </section>
                    <section class="detail-card">
                        <p class="msg-muted">"Full plugin detail page with config editor coming in Phase 4."</p>
                    </section>
                }
            })}
        </div>
    }
}
