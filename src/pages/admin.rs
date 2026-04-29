//! Admin page — user management, password change, system status, backup & data,
//! log level, calendars, stale device references, device cleanup.

use crate::api::{
    add_calendar_by_url, bulk_delete_devices, change_password, create_user, delete_calendar,
    delete_user, export_rules, export_scenes, fetch_calendars, fetch_calendar_events, fetch_me,
    fetch_plugins, fetch_stale_refs, fetch_system_config, fetch_system_status, fetch_users,
    get_log_level, import_rules, import_scenes, put_system_config_raw, restart_system,
    restore_backup, set_log_level, set_user_role, trigger_backup, upload_calendar,
};
use crate::auth::use_auth;
use crate::models::*;
use crate::pages::shared::{ErrorBanner, TabBar, TabSpec};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn role_badge_class(role: &str) -> &'static str {
    match role {
        "admin" => "admin-badge admin-badge--admin",
        "user" => "admin-badge admin-badge--user",
        "read_only" => "admin-badge admin-badge--readonly",
        "observer" => "admin-badge admin-badge--observer",
        "device_operator" => "admin-badge admin-badge--device-operator",
        "rule_editor" => "admin-badge admin-badge--rule-editor",
        "service_operator" => "admin-badge admin-badge--service-operator",
        _ => "admin-badge admin-badge--readonly",
    }
}

fn role_display(role: &str) -> String {
    match role {
        "admin" => "Admin".to_string(),
        "user" => "User".to_string(),
        "read_only" => "Read Only".to_string(),
        "observer" => "Observer".to_string(),
        "device_operator" => "Device Operator".to_string(),
        "rule_editor" => "Rule Editor".to_string(),
        "service_operator" => "Service Operator".to_string(),
        _ => role.to_string(),
    }
}

/// Trigger a browser download from an in-memory byte slice.
fn trigger_browser_download(bytes: &[u8], filename: &str, mime: &str) {
    use js_sys::{Array, Uint8Array};
    use wasm_bindgen::JsCast;

    let uint8 = Uint8Array::from(bytes);
    let array = Array::new();
    array.push(&uint8.buffer());
    let _ = mime; // MIME type noted for future BlobPropertyBag support
    if let Ok(blob) = web_sys::Blob::new_with_u8_array_sequence(&array) {
        if let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) {
            let document = web_sys::window().unwrap().document().unwrap();
            let a = document.create_element("a").unwrap();
            let _ = a.set_attribute("href", &url);
            let _ = a.set_attribute("download", filename);
            let _ = a.set_attribute("style", "display:none");
            let body = document.body().unwrap();
            let _ = body.append_child(&a);
            if let Some(el) = a.dyn_ref::<web_sys::HtmlElement>() {
                el.click();
            }
            let _ = body.remove_child(&a);
            let _ = web_sys::Url::revoke_object_url(&url);
        }
    }
}

/// Read a file chosen via `<input type="file">` and return its text content.
async fn read_file_input(input_id: &str) -> Result<String, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let document = web_sys::window().unwrap().document().unwrap();
    let el = document
        .get_element_by_id(input_id)
        .ok_or("File input not found")?;
    let input: web_sys::HtmlInputElement = el
        .dyn_into()
        .map_err(|_| "Element is not an input")?;
    let files = input.files().ok_or("No files property")?;
    let file = files.get(0).ok_or("No file selected")?;
    let text = JsFuture::from(file.text())
        .await
        .map_err(|_| "Failed to read file")?;
    text.as_string().ok_or_else(|| "File content not a string".to_string())
}

/// Read a file chosen via `<input type="file">` and return its raw bytes.
async fn read_file_input_bytes(input_id: &str) -> Result<Vec<u8>, String> {
    use js_sys::Uint8Array;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let document = web_sys::window().unwrap().document().unwrap();
    let el = document
        .get_element_by_id(input_id)
        .ok_or("File input not found")?;
    let input: web_sys::HtmlInputElement = el
        .dyn_into()
        .map_err(|_| "Element is not an input")?;
    let files = input.files().ok_or("No files property")?;
    let file = files.get(0).ok_or("No file selected")?;
    let array_buffer = JsFuture::from(file.array_buffer())
        .await
        .map_err(|_| "Failed to read file")?;
    let uint8 = Uint8Array::new(&array_buffer);
    Ok(uint8.to_vec())
}

// ── Main AdminPage ──────────────────────────────────────────────────────────
//
// Five tabs, each deep-linkable (/admin/system, /admin/config, etc.).
// /admin (no trailing slug) defaults to System.

const ADMIN_TABS: &[(&str, &str, &str)] = &[
    ("system",      "System",         "ph ph-pulse"),
    ("config",      "Configuration",  "ph ph-sliders"),
    ("users",       "Users",          "ph ph-users"),
    ("data",        "Data",           "ph ph-database"),
    ("maintenance", "Maintenance",    "ph ph-broom"),
];

#[component]
pub fn AdminPage() -> impl IntoView {
    let params = use_params_map();
    let active_tab = move || {
        params
            .read()
            .get("tab")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "system".to_string())
    };

    let tabs: Vec<TabSpec> = ADMIN_TABS
        .iter()
        .map(|(slug, label, icon)| TabSpec {
            slug,
            label,
            icon,
        })
        .collect();

    view! {
        <div class="page">
            <div class="heading">
                <div>
                    <h1>"Administration"</h1>
                    <p>"Manage users, system settings, and backups."</p>
                </div>
            </div>

            <TabBar tabs=tabs base_path="/admin" active_slug=active_tab />

            {move || match active_tab().as_str() {
                "config"      => view! { <ConfigurationTab /> }.into_any(),
                "users"       => view! { <UsersTab /> }.into_any(),
                "data"        => view! { <DataTab /> }.into_any(),
                "maintenance" => view! { <MaintenanceTab /> }.into_any(),
                _             => view! { <SystemTab /> }.into_any(),
            }}
        </div>
    }
}

// ── Tab content ─────────────────────────────────────────────────────────────

#[component]
fn SystemTab() -> impl IntoView {
    view! {
        <SystemStatusSection />
        <LogLevelSection />
    }
}

#[component]
fn UsersTab() -> impl IntoView {
    view! {
        <UserManagementSection />
        <ChangePasswordSection />
    }
}

#[component]
fn DataTab() -> impl IntoView {
    view! {
        <BackupDataSection />
        <CalendarsSection />
    }
}

#[component]
fn MaintenanceTab() -> impl IntoView {
    view! {
        <div class="detail-card">
            <h2 class="card-title">"Stale Device References"</h2>
            <p style="margin-bottom:0.75rem;color:var(--hc-text-muted,#888)">
                "Rules that reference device IDs no longer registered in the device store."
            </p>
            <StaleRefsSection />
        </div>

        <div class="detail-card">
            <h2 class="card-title">"Device Cleanup"</h2>
            <p style="margin-bottom:0.75rem;color:var(--hc-text-muted,#888)">
                "Bulk delete devices by ID. Affected rules will have references nullified."
            </p>
            <DeviceCleanupSection />
        </div>
    }
}

// ── Configuration tab ──────────────────────────────────────────────────────

#[component]
fn ConfigurationTab() -> impl IntoView {
    use crate::pages::admin_config::{all_sections, SectionCard};

    let auth = use_auth();
    let raw: RwSignal<String> = RwSignal::new(String::new());
    let path: RwSignal<String> = RwSignal::new(String::new());
    let (parsed, set_parsed) = signal(serde_json::Value::Null);
    let edit_text: RwSignal<String> = RwSignal::new(String::new());
    let editing = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let restart_required = RwSignal::new(false);
    let error: RwSignal<Option<String>> = RwSignal::new(None);
    let notice: RwSignal<Option<String>> = RwSignal::new(None);

    // Load current config on mount.
    Effect::new(move |_| {
        let token = match auth.token.get() { Some(t) => t, None => return };
        spawn_local(async move {
            match fetch_system_config(&token).await {
                Ok(v) => {
                    if let Some(s) = v["raw"].as_str() {
                        raw.set(s.to_string());
                        edit_text.set(s.to_string());
                    }
                    if let Some(p) = v["path"].as_str() {
                        path.set(p.to_string());
                    }
                    set_parsed.set(v["parsed"].clone());
                }
                Err(e) => error.set(Some(format!("Load failed: {e}"))),
            }
        });
    });

    let save_raw = move |_| {
        let token = match auth.token.get_untracked() { Some(t) => t, None => return };
        let body = edit_text.get_untracked();
        busy.set(true);
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match put_system_config_raw(&token, &body).await {
                Ok(v) => {
                    if let Some(s) = v["raw"].as_str() {
                        raw.set(s.to_string());
                    }
                    let needs_restart = v["restart_required"].as_bool().unwrap_or(true);
                    restart_required.set(needs_restart);
                    editing.set(false);
                    notice.set(Some("Saved.".into()));
                }
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    let do_restart = move |_| {
        let token = match auth.token.get_untracked() { Some(t) => t, None => return };
        busy.set(true);
        spawn_local(async move {
            match restart_system(&token).await {
                Ok(()) => {
                    notice.set(Some(
                        "Restart requested. The page will reconnect when hc-core is back up.".into(),
                    ));
                    restart_required.set(false);
                }
                Err(e) => error.set(Some(format!("Restart failed: {e}"))),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="detail-card">
            <h2 class="card-title">"Configuration"</h2>
            <p style="margin-bottom:0.75rem;color:var(--hc-text-muted,#888)">
                "Edit per-section settings below; the raw config editor at the bottom \
                 is the fallback for anything not exposed as a form. Most changes need a restart. "
                {move || (!path.get().is_empty()).then(|| view! {
                    <code style="font-size:0.85rem">{path.get()}</code>
                })}
            </p>

            <ErrorBanner error=error />

            {move || notice.get().map(|n| view! {
                <div class="msg-success" style="display:flex; align-items:center; gap:0.5rem; margin-bottom:0.75rem">
                    <span>{n}</span>
                </div>
            })}

            {move || restart_required.get().then(|| view! {
                <div class="msg-warning" style="display:flex; align-items:center; gap:0.5rem; margin-bottom:0.75rem">
                    <span>"Some changes require a restart of hc-core to take effect."</span>
                    <button
                        class="hc-btn hc-btn--sm hc-btn--primary"
                        disabled=move || busy.get()
                        on:click=do_restart
                    >"Restart Now"</button>
                </div>
            })}
        </div>

        // Per-section structured forms. Each card is collapsible; the
        // operator-friendly inputs live here. The raw editor below is
        // a fallback for fields not yet covered or for bulk pastes.
        <div style="margin:1rem 0">
            {
                all_sections().into_iter().map(|section| view! {
                    <SectionCard section=section parsed=parsed />
                }).collect_view()
            }
        </div>

        <div class="detail-card">
            <div style="display:flex; gap:0.5rem; margin-bottom:0.5rem; align-items:center">
                <h3 style="margin:0; font-size:1rem">"Raw config"</h3>
                <span style="flex:1"></span>
                {move || if editing.get() {
                    view! {
                        <button
                            class="hc-btn hc-btn--sm hc-btn--outline"
                            disabled=move || busy.get()
                            on:click=move |_| {
                                edit_text.set(raw.get_untracked());
                                editing.set(false);
                            }
                        >"Cancel"</button>
                        <button
                            class="hc-btn hc-btn--sm hc-btn--primary"
                            disabled=move || busy.get()
                            on:click=save_raw
                        >"Save"</button>
                    }.into_any()
                } else {
                    view! {
                        <button
                            class="hc-btn hc-btn--sm hc-btn--outline"
                            on:click=move |_| {
                                edit_text.set(raw.get_untracked());
                                editing.set(true);
                            }
                        >"Edit"</button>
                    }.into_any()
                }}
            </div>

            <textarea
                style="width:100%; min-height:30rem; font-family:monospace; font-size:0.85rem; padding:0.75rem; border:1px solid var(--hc-border); border-radius:6px"
                prop:value=move || if editing.get() { edit_text.get() } else { raw.get() }
                on:input=move |ev| edit_text.set(event_target_value(&ev))
                readonly=move || !editing.get()
            ></textarea>
        </div>
    }
}

// ── System Status ───────────────────────────────────────────────────────────

#[component]
fn SystemStatusSection() -> impl IntoView {
    let auth = use_auth();
    let system_status = RwSignal::new(Option::<SystemStatus>::None);
    let total_restarts = RwSignal::new(0u32);
    let last_restart_str = RwSignal::new(String::new());
    let loading = RwSignal::new(true);
    let error = RwSignal::new(Option::<String>::None);

    let refresh = move || {
        let token = auth.token_str().unwrap_or_default();
        let token2 = token.clone();
        loading.set(true);
        spawn_local(async move {
            match fetch_system_status(&token).await {
                Ok(status) => system_status.set(Some(status)),
                Err(e) => error.set(Some(format!("System status: {e}"))),
            }
            loading.set(false);
        });
        spawn_local(async move {
            if let Ok(plugins) = fetch_plugins(&token2).await {
                let total: u32 = plugins.iter().map(|p| p.restart_count).sum();
                total_restarts.set(total);
                let latest = plugins
                    .iter()
                    .filter_map(|p| p.last_restart)
                    .max()
                    .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "—".into());
                last_restart_str.set(latest);
            }
        });
    };

    Effect::new(move |_| refresh());

    view! {
        <div class="detail-card">
            <div class="card-title-row">
                <h2 class="card-title">"System Status"</h2>
                <button
                    class="btn btn-outline"
                    on:click=move |_| refresh()
                    disabled=move || loading.get()
                >
                    {move || if loading.get() { "Refreshing..." } else { "Refresh" }}
                </button>
            </div>
            <ErrorBanner error=error />
            {move || {
                if let Some(status) = system_status.get() {
                    view! {
                        <div class="admin-status-grid">
                            <div class="admin-stat">
                                <span class="admin-stat-label">"Uptime"</span>
                                <span class="admin-stat-value">{format_uptime(status.uptime_seconds as u64)}</span>
                            </div>
                            <div class="admin-stat">
                                <span class="admin-stat-label">"Version"</span>
                                <span class="admin-stat-value">{status.version.clone()}</span>
                            </div>
                            <div class="admin-stat">
                                <span class="admin-stat-label">"Started"</span>
                                <span class="admin-stat-value">{status.started_at.chars().take(19).collect::<String>()}</span>
                            </div>
                            <div class="admin-stat">
                                <span class="admin-stat-label">"Devices"</span>
                                <span class="admin-stat-value">{status.devices_total.to_string()}</span>
                            </div>
                            <div class="admin-stat">
                                <span class="admin-stat-label">"Rules"</span>
                                <span class="admin-stat-value">{format!("{} / {}", status.rules_enabled, status.rules_total)}</span>
                            </div>
                            <div class="admin-stat">
                                <span class="admin-stat-label">"Plugins"</span>
                                <span class="admin-stat-value">{status.plugins_active.to_string()}</span>
                            </div>
                            <div class="admin-stat">
                                <span class="admin-stat-label">"State DB"</span>
                                <span class="admin-stat-value">{format_bytes(status.state_db_bytes)}</span>
                            </div>
                            <div class="admin-stat">
                                <span class="admin-stat-label">"History DB"</span>
                                <span class="admin-stat-value">{format_bytes(status.history_db_bytes)}</span>
                            </div>
                            <div class="admin-stat">
                                <span class="admin-stat-label">"Plugin Restarts"</span>
                                <span class="admin-stat-value">{total_restarts.get().to_string()}</span>
                            </div>
                            <div class="admin-stat">
                                <span class="admin-stat-label">"Last Restart"</span>
                                <span class="admin-stat-value">{last_restart_str.get()}</span>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    view! { <p class="no-controls-msg">"Loading system status..."</p> }.into_any()
                }
            }}
        </div>
    }
}

// ── User Management ─────────────────────────────────────────────────────────

#[component]
fn UserManagementSection() -> impl IntoView {
    let auth = use_auth();

    let users: RwSignal<Vec<UserInfo>> = RwSignal::new(vec![]);
    let current_user_id = RwSignal::new(String::new());
    let loading = RwSignal::new(true);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);
    let busy = RwSignal::new(false);

    let create_username = RwSignal::new(String::new());
    let create_password = RwSignal::new(String::new());
    let create_role = RwSignal::new("user".to_string());

    let selected_user_id = RwSignal::new(Option::<String>::None);
    let edit_role = RwSignal::new(String::new());
    let delete_confirm = RwSignal::new(String::new());

    let refresh_users = move || {
        let token = auth.token_str().unwrap_or_default();
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match fetch_me(&token).await {
                Ok(me) => {
                    if let Some(user) = me.get("user") {
                        if let Some(id) = user.get("id").and_then(|v| v.as_str()) {
                            current_user_id.set(id.to_string());
                        }
                    }
                }
                Err(_) => {}
            }

            match fetch_users(&token).await {
                Ok(mut list) => {
                    list.sort_by(|a, b| a.username.cmp(&b.username));
                    users.set(list);
                }
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    };

    Effect::new(move |_| {
        refresh_users();
    });

    Effect::new(move |_| {
        let sel = selected_user_id.get();
        if let Some(id) = sel {
            if let Some(user) = users.get().iter().find(|u| u.id == id) {
                edit_role.set(user.role.clone());
            }
        }
        delete_confirm.set(String::new());
    });

    view! {
        <div class="detail-card">
            <div class="card-title-row">
                <h2 class="card-title">"User Management"</h2>
                <button
                    class="btn btn-outline"
                    on:click=move |_| refresh_users()
                    disabled=move || loading.get()
                >
                    {move || if loading.get() { "Refreshing..." } else { "Refresh" }}
                </button>
            </div>

            <ErrorBanner error=error />
            {move || notice.get().map(|n| view! { <p class="msg-notice">{n}</p> })}

            {move || {
                let list = users.get();
                if loading.get() && list.is_empty() {
                    view! { <p class="no-controls-msg">"Loading users..."</p> }.into_any()
                } else if list.is_empty() {
                    view! { <p class="no-controls-msg">"No users found."</p> }.into_any()
                } else {
                    view! {
                        <table class="admin-table">
                            <thead>
                                <tr>
                                    <th>"Username"</th>
                                    <th>"Role"</th>
                                    <th>"Created"</th>
                                    <th>"Actions"</th>
                                </tr>
                            </thead>
                            <tbody>
                                <For
                                    each=move || users.get()
                                    key=|u| u.id.clone()
                                    children=move |user| {
                                        let user_id = user.id.clone();
                                        let click_id = user_id.clone();
                                        let is_current = user_id == current_user_id.get();
                                        let role_class = role_badge_class(&user.role);
                                        let role_label = role_display(&user.role).to_string();
                                        let created = user.created_at.chars().take(10).collect::<String>();
                                        let username = user.username.clone();
                                        view! {
                                            <tr
                                                class:admin-row-current=is_current
                                                class:admin-row-selected=move || selected_user_id.get().as_deref() == Some(click_id.as_str())
                                            >
                                                <td>
                                                    {username}
                                                    {if is_current { " (you)" } else { "" }}
                                                </td>
                                                <td><span class={role_class}>{role_label}</span></td>
                                                <td>{created}</td>
                                                <td>
                                                    <button
                                                        class="btn-outline btn-sm"
                                                        on:click={
                                                            let uid = user_id.clone();
                                                            move |_| {
                                                                if selected_user_id.get().as_deref() == Some(uid.as_str()) {
                                                                    selected_user_id.set(None);
                                                                } else {
                                                                    selected_user_id.set(Some(uid.clone()));
                                                                }
                                                            }
                                                        }
                                                    >
                                                        {
                                                            let uid2 = user_id.clone();
                                                            move || if selected_user_id.get().as_deref() == Some(uid2.as_str()) { "Close" } else { "Edit" }
                                                        }
                                                    </button>
                                                </td>
                                            </tr>
                                            // Inline edit panel
                                            {
                                                let uid = user_id.clone();
                                                let uid_save = user_id.clone();
                                                let uid_del = user_id.clone();
                                                let is_self = user_id == current_user_id.get();
                                                move || {
                                                    let uid = uid.clone();
                                                    let uid_save = uid_save.clone();
                                                    let uid_del = uid_del.clone();
                                                    (selected_user_id.get().as_deref() == Some(uid.as_str())).then(|| {
                                                        view! {
                                                            <tr class="admin-edit-row">
                                                                <td colspan="4">
                                                                    <div class="admin-edit-panel">
                                                                        <div class="edit-grid">
                                                                            <div class="edit-field">
                                                                                <label>"Role"</label>
                                                                                <select
                                                                                    prop:value=move || edit_role.get()
                                                                                    on:change=move |ev| edit_role.set(event_target_value(&ev))
                                                                                >
                                                                                    <option value="admin" selected=move || edit_role.get() == "admin">"Admin"</option>
                                                                                    <option value="user" selected=move || edit_role.get() == "user">"User"</option>
                                                                                    <option value="read_only" selected=move || edit_role.get() == "read_only">"Read Only"</option>
                                                                                    <option value="observer" selected=move || edit_role.get() == "observer">"Observer"</option>
                                                                                    <option value="device_operator" selected=move || edit_role.get() == "device_operator">"Device Operator"</option>
                                                                                    <option value="rule_editor" selected=move || edit_role.get() == "rule_editor">"Rule Editor"</option>
                                                                                    <option value="service_operator" selected=move || edit_role.get() == "service_operator">"Service Operator"</option>
                                                                                </select>
                                                                            </div>
                                                                        </div>
                                                                        <div class="edit-actions">
                                                                            <button
                                                                                class="btn btn-primary"
                                                                                disabled=move || busy.get()
                                                                                on:click={
                                                                                    let uid_save = uid_save.clone();
                                                                                    move |_| {
                                                                                        let token = auth.token_str().unwrap_or_default();
                                                                                        let id = uid_save.clone();
                                                                                        let role = edit_role.get();
                                                                                        busy.set(true);
                                                                                        error.set(None);
                                                                                        notice.set(None);
                                                                                        spawn_local(async move {
                                                                                            match set_user_role(&token, &id, &role).await {
                                                                                                Ok(updated) => {
                                                                                                    notice.set(Some(format!("Updated role for {} to {}.", updated.username, role_display(&updated.role))));
                                                                                                    refresh_users();
                                                                                                }
                                                                                                Err(e) => error.set(Some(format!("Role update failed: {e}"))),
                                                                                            }
                                                                                            busy.set(false);
                                                                                        });
                                                                                    }
                                                                                }
                                                                            >
                                                                                {move || if busy.get() { "Saving..." } else { "Save role" }}
                                                                            </button>
                                                                        </div>
                                                                        {if !is_self {
                                                                            let uid_del = uid_del.clone();
                                                                            Some(view! {
                                                                                <div class="danger-zone">
                                                                                    <div class="danger-zone-copy">
                                                                                        <h3>"Delete User"</h3>
                                                                                        <p>"This action cannot be undone."</p>
                                                                                    </div>
                                                                                    <div class="danger-zone-controls">
                                                                                        <div class="edit-field">
                                                                                            <label>"Type DELETE to confirm"</label>
                                                                                            <input
                                                                                                class="input"
                                                                                                type="text"
                                                                                                prop:value=move || delete_confirm.get()
                                                                                                on:input=move |ev| delete_confirm.set(event_target_value(&ev))
                                                                                                placeholder="DELETE"
                                                                                            />
                                                                                        </div>
                                                                                        <button
                                                                                            class="danger"
                                                                                            disabled=move || busy.get() || delete_confirm.get().trim() != "DELETE"
                                                                                            on:click={
                                                                                                let uid_del = uid_del.clone();
                                                                                                move |_| {
                                                                                                    let token = auth.token_str().unwrap_or_default();
                                                                                                    let id = uid_del.clone();
                                                                                                    busy.set(true);
                                                                                                    error.set(None);
                                                                                                    notice.set(None);
                                                                                                    spawn_local(async move {
                                                                                                        match delete_user(&token, &id).await {
                                                                                                            Ok(()) => {
                                                                                                                notice.set(Some("User deleted.".to_string()));
                                                                                                                selected_user_id.set(None);
                                                                                                                refresh_users();
                                                                                                            }
                                                                                                            Err(e) => error.set(Some(format!("Delete failed: {e}"))),
                                                                                                        }
                                                                                                        busy.set(false);
                                                                                                    });
                                                                                                }
                                                                                            }
                                                                                        >
                                                                                            {move || if busy.get() { "Deleting..." } else { "Delete user" }}
                                                                                        </button>
                                                                                    </div>
                                                                                </div>
                                                                            })
                                                                        } else {
                                                                            None
                                                                        }}
                                                                    </div>
                                                                </td>
                                                            </tr>
                                                        }
                                                    })
                                                }
                                            }
                                        }
                                    }
                                />
                            </tbody>
                        </table>
                    }.into_any()
                }
            }}

            // Create user form
            <div class="card-title-row" style="margin-top: 1rem">
                <h3 class="card-title">"Create User"</h3>
            </div>
            <div class="admin-create-row">
                <input
                    class="input"
                    type="text"
                    prop:value=move || create_username.get()
                    on:input=move |ev| create_username.set(event_target_value(&ev))
                    placeholder="Username"
                />
                <input
                    class="input"
                    type="password"
                    prop:value=move || create_password.get()
                    on:input=move |ev| create_password.set(event_target_value(&ev))
                    placeholder="Password"
                />
                <select
                    prop:value=move || create_role.get()
                    on:change=move |ev| create_role.set(event_target_value(&ev))
                >
                    <option value="admin">"Admin"</option>
                    <option value="user" selected=true>"User"</option>
                    <option value="read_only">"Read Only"</option>
                    <option value="observer">"Observer"</option>
                    <option value="device_operator">"Device Operator"</option>
                    <option value="rule_editor">"Rule Editor"</option>
                    <option value="service_operator">"Service Operator"</option>
                </select>
                <button
                    class="btn btn-primary"
                    disabled=move || {
                        busy.get()
                            || create_username.get().trim().is_empty()
                            || create_password.get().trim().is_empty()
                    }
                    on:click=move |_| {
                        let token = auth.token_str().unwrap_or_default();
                        let username = create_username.get();
                        let password = create_password.get();
                        let role = create_role.get();
                        busy.set(true);
                        error.set(None);
                        notice.set(None);
                        spawn_local(async move {
                            match create_user(&token, &username, &password, &role).await {
                                Ok(user) => {
                                    notice.set(Some(format!("Created user {}.", user.username)));
                                    create_username.set(String::new());
                                    create_password.set(String::new());
                                    create_role.set("user".to_string());
                                    refresh_users();
                                }
                                Err(e) => error.set(Some(format!("Create failed: {e}"))),
                            }
                            busy.set(false);
                        });
                    }
                >
                    {move || if busy.get() { "Creating..." } else { "Create" }}
                </button>
            </div>
        </div>
    }
}

// ── Change Password ─────────────────────────────────────────────────────────

#[component]
fn ChangePasswordSection() -> impl IntoView {
    let auth = use_auth();
    let busy = RwSignal::new(false);
    let pw_current = RwSignal::new(String::new());
    let pw_new = RwSignal::new(String::new());
    let pw_confirm = RwSignal::new(String::new());
    let pw_error = RwSignal::new(Option::<String>::None);
    let pw_notice = RwSignal::new(Option::<String>::None);

    view! {
        <div class="detail-card">
            <h2 class="card-title">"Change Password"</h2>
            <ErrorBanner error=pw_error />
            {move || pw_notice.get().map(|n| view! { <p class="msg-notice">{n}</p> })}
            <div class="edit-grid">
                <div class="edit-field">
                    <label>"Current Password"</label>
                    <input
                        class="input"
                        type="password"
                        prop:value=move || pw_current.get()
                        on:input=move |ev| pw_current.set(event_target_value(&ev))
                        placeholder="Current password"
                    />
                </div>
                <div class="edit-field">
                    <label>"New Password"</label>
                    <input
                        class="input"
                        type="password"
                        prop:value=move || pw_new.get()
                        on:input=move |ev| pw_new.set(event_target_value(&ev))
                        placeholder="Minimum 8 characters"
                    />
                </div>
                <div class="edit-field">
                    <label>"Confirm New Password"</label>
                    <input
                        class="input"
                        type="password"
                        prop:value=move || pw_confirm.get()
                        on:input=move |ev| pw_confirm.set(event_target_value(&ev))
                        placeholder="Repeat new password"
                    />
                </div>
            </div>
            <div class="edit-actions">
                <button
                    class="btn btn-primary"
                    disabled=move || {
                        busy.get()
                            || pw_current.get().is_empty()
                            || pw_new.get().len() < 8
                            || pw_new.get() != pw_confirm.get()
                    }
                    on:click=move |_| {
                        let token = auth.token_str().unwrap_or_default();
                        let current = pw_current.get();
                        let new_pass = pw_new.get();
                        let confirm = pw_confirm.get();

                        pw_error.set(None);
                        pw_notice.set(None);

                        if new_pass.len() < 8 {
                            pw_error.set(Some("New password must be at least 8 characters.".to_string()));
                            return;
                        }
                        if new_pass != confirm {
                            pw_error.set(Some("Passwords do not match.".to_string()));
                            return;
                        }

                        busy.set(true);
                        spawn_local(async move {
                            match change_password(&token, &current, &new_pass).await {
                                Ok(()) => {
                                    pw_notice.set(Some("Password changed successfully.".to_string()));
                                    pw_current.set(String::new());
                                    pw_new.set(String::new());
                                    pw_confirm.set(String::new());
                                }
                                Err(e) => pw_error.set(Some(format!("Password change failed: {e}"))),
                            }
                            busy.set(false);
                        });
                    }
                >
                    {move || if busy.get() { "Changing..." } else { "Change Password" }}
                </button>
            </div>
        </div>
    }
}

// ── Backup & Data ───────────────────────────────────────────────────────────

#[component]
fn BackupDataSection() -> impl IntoView {
    let auth = use_auth();
    let busy = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);
    let import_result = RwSignal::new(Option::<String>::None);

    // ── Backup download ─────────────────────────────────────────────────────
    let on_backup = move |_| {
        let token = auth.token_str().unwrap_or_default();
        busy.set(true);
        error.set(None);
        notice.set(None);
        import_result.set(None);
        spawn_local(async move {
            match trigger_backup(&token).await {
                Ok(bytes) => {
                    trigger_browser_download(&bytes, "homecore-backup.zip", "application/zip");
                    notice.set(Some("Backup downloaded.".to_string()));
                }
                Err(e) => error.set(Some(format!("Backup failed: {e}"))),
            }
            busy.set(false);
        });
    };

    // ── Export rules ────────────────────────────────────────────────────────
    let on_export_rules = move |_| {
        let token = auth.token_str().unwrap_or_default();
        busy.set(true);
        error.set(None);
        notice.set(None);
        import_result.set(None);
        spawn_local(async move {
            match export_rules(&token).await {
                Ok(data) => {
                    let json = serde_json::to_string_pretty(&data).unwrap_or_default();
                    trigger_browser_download(json.as_bytes(), "homecore-rules.json", "application/json");
                    notice.set(Some("Rules exported.".to_string()));
                }
                Err(e) => error.set(Some(format!("Export rules failed: {e}"))),
            }
            busy.set(false);
        });
    };

    // ── Export scenes ───────────────────────────────────────────────────────
    let on_export_scenes = move |_| {
        let token = auth.token_str().unwrap_or_default();
        busy.set(true);
        error.set(None);
        notice.set(None);
        import_result.set(None);
        spawn_local(async move {
            match export_scenes(&token).await {
                Ok(data) => {
                    let json = serde_json::to_string_pretty(&data).unwrap_or_default();
                    trigger_browser_download(json.as_bytes(), "homecore-scenes.json", "application/json");
                    notice.set(Some("Scenes exported.".to_string()));
                }
                Err(e) => error.set(Some(format!("Export scenes failed: {e}"))),
            }
            busy.set(false);
        });
    };

    // ── Import rules ────────────────────────────────────────────────────────
    let on_import_rules = move |_| {
        let token = auth.token_str().unwrap_or_default();
        busy.set(true);
        error.set(None);
        notice.set(None);
        import_result.set(None);
        spawn_local(async move {
            match read_file_input("import-rules-file").await {
                Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(data) => match import_rules(&token, &data).await {
                        Ok(resp) => {
                            let imported = resp["imported"].as_u64().unwrap_or(0);
                            let skipped = resp["skipped"].as_u64().unwrap_or(0);
                            let errors = resp["errors"]
                                .as_array()
                                .map(|a| a.len() as u64)
                                .unwrap_or(0);
                            import_result.set(Some(format!(
                                "Rules import: {imported} imported, {skipped} skipped, {errors} errors."
                            )));
                        }
                        Err(e) => error.set(Some(format!("Import rules failed: {e}"))),
                    },
                    Err(e) => error.set(Some(format!("Invalid JSON: {e}"))),
                },
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    // ── Import scenes ───────────────────────────────────────────────────────
    let on_import_scenes = move |_| {
        let token = auth.token_str().unwrap_or_default();
        busy.set(true);
        error.set(None);
        notice.set(None);
        import_result.set(None);
        spawn_local(async move {
            match read_file_input("import-scenes-file").await {
                Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(data) => match import_scenes(&token, &data).await {
                        Ok(resp) => {
                            let imported = resp["imported"].as_u64().unwrap_or(0);
                            let skipped = resp["skipped"].as_u64().unwrap_or(0);
                            let errors = resp["errors"]
                                .as_array()
                                .map(|a| a.len() as u64)
                                .unwrap_or(0);
                            import_result.set(Some(format!(
                                "Scenes import: {imported} imported, {skipped} skipped, {errors} errors."
                            )));
                        }
                        Err(e) => error.set(Some(format!("Import scenes failed: {e}"))),
                    },
                    Err(e) => error.set(Some(format!("Invalid JSON: {e}"))),
                },
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    // ── Restore backup ──────────────────────────────────────────────────────
    let restore_confirm = RwSignal::new(false);
    let restore_result = RwSignal::new(Option::<String>::None);

    let on_restore = move |_| {
        if !restore_confirm.get() {
            restore_confirm.set(true);
            return;
        }
        let token = auth.token_str().unwrap_or_default();
        busy.set(true);
        error.set(None);
        notice.set(None);
        restore_result.set(None);
        restore_confirm.set(false);
        spawn_local(async move {
            match read_file_input_bytes("restore-backup-file").await {
                Ok(bytes) => match restore_backup(&token, &bytes).await {
                    Ok(resp) => {
                        let restored = resp["restored"]
                            .as_array()
                            .map(|a| a.len())
                            .unwrap_or(0);
                        let msg = resp["message"]
                            .as_str()
                            .unwrap_or("Restore complete.");
                        restore_result.set(Some(format!(
                            "{restored} file(s) restored. {msg}"
                        )));
                    }
                    Err(e) => error.set(Some(format!("Restore failed: {e}"))),
                },
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="detail-card">
            <h2 class="card-title">"Backup & Data"</h2>
            <ErrorBanner error=error />
            {move || notice.get().map(|n| view! { <p class="msg-notice">{n}</p> })}
            {move || import_result.get().map(|r| view! { <p class="msg-notice">{r}</p> })}
            {move || restore_result.get().map(|r| view! { <p class="msg-notice">{r}</p> })}

            // ── Full backup ─────────────────────────────────────────────────
            <div class="admin-data-group">
                <h3 class="admin-data-heading">"Full Backup"</h3>
                <p class="cell-subtle">"Download a zip archive of the current HomeCore configuration and state databases."</p>
                <div class="admin-data-actions">
                    <button
                        class="btn btn-primary"
                        disabled=move || busy.get()
                        on:click=on_backup
                    >
                        {move || if busy.get() { "Downloading..." } else { "Download Backup" }}
                    </button>
                </div>
            </div>

            // ── Export ───────────────────────────────────────────────────────
            <div class="admin-data-group">
                <h3 class="admin-data-heading">"Export"</h3>
                <p class="cell-subtle">"Download rules or scenes as JSON files for sharing or safekeeping."</p>
                <div class="admin-data-actions">
                    <button class="btn btn-outline" disabled=move || busy.get() on:click=on_export_rules>
                        "Export Rules"
                    </button>
                    <button class="btn btn-outline" disabled=move || busy.get() on:click=on_export_scenes>
                        "Export Scenes"
                    </button>
                </div>
            </div>

            // ── Import ──────────────────────────────────────────────────────
            <div class="admin-data-group">
                <h3 class="admin-data-heading">"Import"</h3>
                <p class="cell-subtle">"Import rules or scenes from a previously exported JSON file. Imported items receive new IDs."</p>

                <div class="admin-import-row">
                    <div class="admin-import-field">
                        <label>"Rules JSON"</label>
                        <input id="import-rules-file" class="input" type="file" accept=".json" />
                    </div>
                    <button class="btn btn-primary" disabled=move || busy.get() on:click=on_import_rules>
                        {move || if busy.get() { "Importing..." } else { "Import Rules" }}
                    </button>
                </div>

                <div class="admin-import-row">
                    <div class="admin-import-field">
                        <label>"Scenes JSON"</label>
                        <input id="import-scenes-file" class="input" type="file" accept=".json" />
                    </div>
                    <button class="btn btn-primary" disabled=move || busy.get() on:click=on_import_scenes>
                        {move || if busy.get() { "Importing..." } else { "Import Scenes" }}
                    </button>
                </div>
            </div>

            // ── Restore ─────────────────────────────────────────────────────
            <div class="admin-data-group">
                <h3 class="admin-data-heading">"Restore from Backup"</h3>
                <div class="danger-zone">
                    <div class="danger-zone-copy">
                        <p>"Upload a previously downloaded backup ZIP to restore configuration, rules, and databases. "</p>
                        <p><strong>"This will overwrite current data. A server restart is required after restore."</strong></p>
                    </div>
                    <div class="admin-import-row">
                        <div class="admin-import-field">
                            <label>"Backup ZIP"</label>
                            <input id="restore-backup-file" class="input" type="file" accept=".zip" />
                        </div>
                        <button
                            class="btn btn-danger"
                            disabled=move || busy.get()
                            on:click=on_restore
                        >
                            {move || {
                                if busy.get() {
                                    "Restoring..."
                                } else if restore_confirm.get() {
                                    "Confirm Restore"
                                } else {
                                    "Restore"
                                }
                            }}
                        </button>
                        {move || restore_confirm.get().then(|| view! {
                            <button
                                class="btn btn-outline"
                                on:click=move |_| restore_confirm.set(false)
                            >"Cancel"</button>
                        })}
                    </div>
                </div>
            </div>
        </div>
    }
}

// ── Log Level ───────────────────────────────────────────────────────────────

#[component]
fn LogLevelSection() -> impl IntoView {
    let auth = use_auth();
    let busy = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);
    let log_level = RwSignal::new(String::new());
    let log_level_edit = RwSignal::new(String::new());

    let refresh = move || {
        let token = auth.token_str().unwrap_or_default();
        spawn_local(async move {
            match get_log_level(&token).await {
                Ok(val) => {
                    let level = val
                        .get("level")
                        .and_then(|v| v.as_str())
                        .unwrap_or("info")
                        .to_string();
                    log_level.set(level.clone());
                    log_level_edit.set(level);
                }
                Err(e) => error.set(Some(format!("Log level: {e}"))),
            }
        });
    };

    Effect::new(move |_| refresh());

    view! {
        <div class="detail-card">
            <h2 class="card-title">"Log Level"</h2>
            <ErrorBanner error=error />
            {move || notice.get().map(|n| view! { <p class="msg-notice">{n}</p> })}
            <p class="cell-subtle">
                "Current level: "
                <strong>{move || log_level.get()}</strong>
            </p>
            <div class="admin-create-row">
                <select
                    prop:value=move || log_level_edit.get()
                    on:change=move |ev| log_level_edit.set(event_target_value(&ev))
                >
                    <option value="trace" selected=move || log_level_edit.get() == "trace">"trace"</option>
                    <option value="debug" selected=move || log_level_edit.get() == "debug">"debug"</option>
                    <option value="info" selected=move || log_level_edit.get() == "info">"info"</option>
                    <option value="warn" selected=move || log_level_edit.get() == "warn">"warn"</option>
                    <option value="error" selected=move || log_level_edit.get() == "error">"error"</option>
                </select>
                <button
                    class="btn btn-primary"
                    disabled=move || busy.get() || log_level_edit.get() == log_level.get()
                    on:click=move |_| {
                        let token = auth.token_str().unwrap_or_default();
                        let level = log_level_edit.get();
                        busy.set(true);
                        error.set(None);
                        notice.set(None);
                        spawn_local(async move {
                            match set_log_level(&token, &level).await {
                                Ok(()) => {
                                    log_level.set(level.clone());
                                    notice.set(Some(format!("Log level set to {level}.")));
                                }
                                Err(e) => error.set(Some(format!("Log level change failed: {e}"))),
                            }
                            busy.set(false);
                        });
                    }
                >
                    {move || if busy.get() { "Applying..." } else { "Apply" }}
                </button>
            </div>
        </div>
    }
}

// ── Calendars ───────────────────────────────────────────────────────────────

#[component]
fn CalendarsSection() -> impl IntoView {
    let auth = use_auth();
    let calendars: RwSignal<Vec<serde_json::Value>> = RwSignal::new(vec![]);
    let loading = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);

    // Add by URL fields
    let url_input = RwSignal::new(String::new());
    let url_name = RwSignal::new(String::new());

    // Events viewer
    let viewing_events: RwSignal<Option<String>> = RwSignal::new(None);
    let events: RwSignal<Vec<serde_json::Value>> = RwSignal::new(vec![]);

    let refresh = move || {
        let token = auth.token_str().unwrap_or_default();
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match fetch_calendars(&token).await {
                Ok(data) => calendars.set(data),
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    };

    Effect::new(move |_| refresh());

    // Add calendar by URL
    let on_add_url = move |_| {
        let token = auth.token_str().unwrap_or_default();
        let url = url_input.get();
        let name = url_name.get();
        let name_opt = if name.trim().is_empty() { None } else { Some(name.as_str().to_string()) };
        busy.set(true);
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match add_calendar_by_url(&token, &url, name_opt.as_deref(), None).await {
                Ok(resp) => {
                    let id = resp["calendar_id"].as_str().unwrap_or("?");
                    let count = resp["event_count"].as_u64().unwrap_or(0);
                    notice.set(Some(format!("Calendar '{id}' added with {count} events.")));
                    url_input.set(String::new());
                    url_name.set(String::new());
                    refresh();
                }
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    // Upload calendar file
    let on_upload = move |_| {
        let token = auth.token_str().unwrap_or_default();
        busy.set(true);
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match read_file_input("cal-upload-file").await {
                Ok(content) => {
                    match upload_calendar(&token, &content, None).await {
                        Ok(resp) => {
                            let id = resp["calendar_id"].as_str().unwrap_or("?");
                            let count = resp["event_count"].as_u64().unwrap_or(0);
                            notice.set(Some(format!("Calendar '{id}' uploaded with {count} events.")));
                            refresh();
                        }
                        Err(e) => error.set(Some(e)),
                    }
                }
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="detail-card">
            <div class="card-title-row">
                <h2 class="card-title">"Calendars"</h2>
                <button class="btn btn-outline" on:click=move |_| refresh() disabled=move || loading.get()>
                    {move || if loading.get() { "Refreshing..." } else { "Refresh" }}
                </button>
            </div>
            <p class="cell-subtle">"Manage .ics calendar subscriptions used as rule triggers and conditions."</p>
            <ErrorBanner error=error />
            {move || notice.get().map(|n| view! { <p class="msg-notice">{n}</p> })}

            // ── Calendar list ────────────────────────────────────────────────
            <div class="cal-list">
                {move || {
                    let cals = calendars.get();
                    if loading.get() && cals.is_empty() {
                        view! { <p class="no-controls-msg">"Loading..."</p> }.into_any()
                    } else if cals.is_empty() {
                        view! { <p class="no-controls-msg">"No calendars loaded."</p> }.into_any()
                    } else {
                        cals.into_iter().map(|cal| {
                            let id = cal["id"].as_str().unwrap_or("?").to_string();
                            let event_count = cal["event_count"].as_u64().unwrap_or(0);
                            let upcoming = cal["upcoming_count"].as_u64().unwrap_or(0);
                            let source = cal["source_url"].as_str().unwrap_or("uploaded").to_string();
                            let fetched = cal["fetched_at"].as_str().map(|s| s.chars().take(19).collect::<String>()).unwrap_or_default();
                            let del_id = id.clone();
                            let view_id = id.clone();
                            view! {
                                <div class="cal-item">
                                    <div class="cal-item-info">
                                        <span class="cal-item-name">{id}</span>
                                        <span class="cal-item-meta">
                                            {format!("{event_count} events, {upcoming} upcoming")}
                                            {(!fetched.is_empty()).then(|| format!(" — fetched {fetched}"))}
                                        </span>
                                        <span class="cal-item-meta">{source}</span>
                                    </div>
                                    <div class="cal-item-actions">
                                        <button
                                            class="btn btn-outline btn-sm"
                                            title="View events"
                                            on:click=move |_| {
                                                let token = auth.token_str().unwrap_or_default();
                                                let id = view_id.clone();
                                                viewing_events.set(Some(id.clone()));
                                                spawn_local(async move {
                                                    match fetch_calendar_events(&token, &id).await {
                                                        Ok(data) => events.set(data),
                                                        Err(_) => events.set(vec![]),
                                                    }
                                                });
                                            }
                                        >"Events"</button>
                                        <button
                                            class="btn btn-outline btn-sm hc-btn--danger-outline"
                                            title="Delete calendar"
                                            disabled=move || busy.get()
                                            on:click=move |_| {
                                                let token = auth.token_str().unwrap_or_default();
                                                let id = del_id.clone();
                                                busy.set(true);
                                                spawn_local(async move {
                                                    match delete_calendar(&token, &id).await {
                                                        Ok(()) => refresh(),
                                                        Err(e) => error.set(Some(e)),
                                                    }
                                                    busy.set(false);
                                                });
                                            }
                                        >"Delete"</button>
                                    </div>
                                </div>
                            }
                        }).collect_view().into_any()
                    }
                }}
            </div>

            // ── Events viewer ────────────────────────────────────────────────
            {move || viewing_events.get().map(|cal_id| {
                let ev_list = events.get();
                view! {
                    <div class="admin-data-group">
                        <div class="card-title-row">
                            <h3 class="admin-data-heading">{format!("Events — {cal_id}")}</h3>
                            <button class="btn btn-outline btn-sm" on:click=move |_| { viewing_events.set(None); events.set(vec![]); }>"Close"</button>
                        </div>
                        {if ev_list.is_empty() {
                            view! { <p class="cell-subtle">"No events."</p> }.into_any()
                        } else {
                            let total = ev_list.len();
                            view! {
                                <table class="admin-table">
                                    <thead><tr>
                                        <th>"Summary"</th>
                                        <th>"Start"</th>
                                        <th>"End"</th>
                                    </tr></thead>
                                    <tbody>
                                        {ev_list.into_iter().take(50).map(|ev| {
                                            let summary = ev["summary"].as_str().unwrap_or("?").to_string();
                                            let start = ev["start"].as_str().unwrap_or("").chars().take(19).collect::<String>();
                                            let end = ev["end"].as_str().unwrap_or("").chars().take(19).collect::<String>();
                                            view! {
                                                <tr>
                                                    <td>{summary}</td>
                                                    <td class="cell-subtle">{start}</td>
                                                    <td class="cell-subtle">{end}</td>
                                                </tr>
                                            }
                                        }).collect_view()}
                                    </tbody>
                                </table>
                                {(total > 50).then(|| view! {
                                    <p class="cell-subtle">{format!("Showing 50 of {total} events.")}</p>
                                })}
                            }.into_any()
                        }}
                    </div>
                }
            })}

            // ── Add by URL ───────────────────────────────────────────────────
            <div class="admin-data-group">
                <h3 class="admin-data-heading">"Add Calendar by URL"</h3>
                <div class="cal-add-row">
                    <input
                        class="input"
                        type="text"
                        prop:value=move || url_input.get()
                        on:input=move |ev| url_input.set(event_target_value(&ev))
                        placeholder="https://example.com/calendar.ics"
                    />
                    <input
                        class="input"
                        type="text"
                        style="max-width:180px"
                        prop:value=move || url_name.get()
                        on:input=move |ev| url_name.set(event_target_value(&ev))
                        placeholder="Name (optional)"
                    />
                    <button
                        class="btn btn-primary"
                        disabled=move || busy.get() || url_input.get().trim().is_empty()
                        on:click=on_add_url
                    >
                        {move || if busy.get() { "Adding..." } else { "Add" }}
                    </button>
                </div>
            </div>

            // ── Upload ICS file ──────────────────────────────────────────────
            <div class="admin-data-group">
                <h3 class="admin-data-heading">"Upload ICS File"</h3>
                <div class="cal-add-row">
                    <input id="cal-upload-file" class="input" type="file" accept=".ics" />
                    <button
                        class="btn btn-primary"
                        disabled=move || busy.get()
                        on:click=on_upload
                    >
                        {move || if busy.get() { "Uploading..." } else { "Upload" }}
                    </button>
                </div>
            </div>
        </div>
    }
}

// ── Stale Refs Sub-Component ────────────────────────────────────────────────

#[component]
fn StaleRefsSection() -> impl IntoView {
    let auth = use_auth();
    let stale_rules: RwSignal<Vec<serde_json::Value>> = RwSignal::new(vec![]);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);

    let refresh = move || {
        let token = auth.token_str().unwrap_or_default();
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match fetch_stale_refs(&token).await {
                Ok(data) => stale_rules.set(data),
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    };

    Effect::new(move |_| refresh());

    view! {
        <ErrorBanner error=error />
        <div style="margin-bottom:0.5rem">
            <button class="btn btn-outline" on:click=move |_| refresh()>
                {move || if loading.get() { "Checking..." } else { "Check Now" }}
            </button>
        </div>
        {move || {
            let rules = stale_rules.get();
            if loading.get() {
                view! { <p>"Loading..."</p> }.into_any()
            } else if rules.is_empty() {
                view! { <p style="color:var(--hc-success,#4caf50)">"No stale references found."</p> }.into_any()
            } else {
                let count = rules.len();
                view! {
                    <table class="admin-table">
                        <thead><tr>
                            <th>"Rule Name"</th>
                            <th>"Stale Device IDs"</th>
                            <th>"Action"</th>
                        </tr></thead>
                        <tbody>
                            {rules.into_iter().map(|rule| {
                                let name = rule["rule_name"].as_str().unwrap_or("?").to_string();
                                let rule_id = rule["rule_id"].as_str().unwrap_or("").to_string();
                                let stale_ids: Vec<String> = rule["stale_device_ids"]
                                    .as_array()
                                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                                    .unwrap_or_default();
                                let edit_href = format!("/rules/{rule_id}");
                                view! {
                                    <tr>
                                        <td>
                                            <a href=edit_href.clone() style="color:var(--hc-accent)">{name}</a>
                                        </td>
                                        <td>
                                            {stale_ids.into_iter().map(|id| view! {
                                                <code style="margin-right:0.5rem;color:var(--hc-danger,#e53935)">{id}</code>
                                            }).collect_view()}
                                        </td>
                                        <td>
                                            <a href=edit_href class="btn-outline btn-sm">"Edit Rule"</a>
                                        </td>
                                    </tr>
                                }
                            }).collect_view()}
                        </tbody>
                    </table>
                    <p class="cell-subtle" style="margin-top:0.5rem">
                        {format!("{count} rule(s) with stale references. Open each rule to update or remove the orphaned device IDs.")}
                    </p>
                }.into_any()
            }
        }}
    }
}

// ── Device Cleanup Sub-Component ────────────────────────────────────────────

#[component]
fn DeviceCleanupSection() -> impl IntoView {
    let auth = use_auth();
    let device_ids_input = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);

    view! {
        <ErrorBanner error=error />
        {move || notice.get().map(|n| view! { <p class="msg-notice">{n}</p> })}
        <div class="edit-field">
            <label>"Device IDs (comma-separated)"</label>
            <input
                class="input"
                type="text"
                prop:value=move || device_ids_input.get()
                on:input=move |ev| device_ids_input.set(event_target_value(&ev))
                placeholder="device_id_1, device_id_2, ..."
            />
        </div>
        <div class="edit-actions" style="margin-top:0.5rem">
            <button
                class="btn btn-danger"
                disabled=move || busy.get() || device_ids_input.get().trim().is_empty()
                on:click=move |_| {
                    let token = auth.token_str().unwrap_or_default();
                    let raw = device_ids_input.get();
                    let ids: Vec<String> = raw.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if ids.is_empty() { return; }
                    let count = ids.len();
                    busy.set(true);
                    error.set(None);
                    notice.set(None);
                    spawn_local(async move {
                        match bulk_delete_devices(&token, &ids).await {
                            Ok(resp) => {
                                let deleted = resp["deleted"].as_u64().unwrap_or(0);
                                let not_found = resp["not_found"].as_u64().unwrap_or(0);
                                let affected = resp["affected_rules"]
                                    .as_array()
                                    .map(|a| a.len())
                                    .unwrap_or(0);
                                notice.set(Some(format!(
                                    "Deleted {deleted}/{count} devices. {not_found} not found. {affected} rules affected."
                                )));
                                device_ids_input.set(String::new());
                            }
                            Err(e) => error.set(Some(format!("Bulk delete failed: {e}"))),
                        }
                        busy.set(false);
                    });
                }
            >
                {move || if busy.get() { "Deleting..." } else { "Delete Devices" }}
            </button>
        </div>
    }
}
