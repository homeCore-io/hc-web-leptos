//! Admin page — user management, password change, system status, backup, log level,
//! stale device references, device cleanup.

use crate::pages::shared::ErrorBanner;
use crate::api::{
    bulk_delete_devices, change_password, create_user, delete_user, fetch_me,
    fetch_stale_refs, fetch_system_status, fetch_users, get_log_level, set_log_level,
    set_user_role, trigger_backup,
};
use crate::auth::use_auth;
use crate::models::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

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
        _ => "admin-badge admin-badge--readonly",
    }
}

fn role_display(role: &str) -> String {
    match role {
        "admin" => "Admin".to_string(),
        "user" => "User".to_string(),
        "read_only" => "Read Only".to_string(),
        _ => role.to_string(),
    }
}

// ── Component ────────────────────────────────────────────────────────────────

#[component]
pub fn AdminPage() -> impl IntoView {
    let auth = use_auth();

    // ── User management signals ──────────────────────────────────────────────
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

    // ── Password signals ─────────────────────────────────────────────────────
    let pw_current = RwSignal::new(String::new());
    let pw_new = RwSignal::new(String::new());
    let pw_confirm = RwSignal::new(String::new());
    let pw_error = RwSignal::new(Option::<String>::None);
    let pw_notice = RwSignal::new(Option::<String>::None);

    // ── System status signals ────────────────────────────────────────────────
    let system_status = RwSignal::new(Option::<SystemStatus>::None);
    let sys_loading = RwSignal::new(true);

    // ── Log level signals ────────────────────────────────────────────────────
    let log_level = RwSignal::new(String::new());
    let log_level_edit = RwSignal::new(String::new());

    // ── Refresh users ────────────────────────────────────────────────────────
    let refresh_users = move || {
        let token = auth.token_str().unwrap_or_default();
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            // Fetch current user id
            match fetch_me(&token).await {
                Ok(me) => {
                    if let Some(user) = me.get("user") {
                        if let Some(id) = user.get("id").and_then(|v| v.as_str()) {
                            current_user_id.set(id.to_string());
                        }
                    }
                }
                Err(_) => {} // non-fatal
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

    // ── Refresh system status ────────────────────────────────────────────────
    let refresh_system = move || {
        let token = auth.token_str().unwrap_or_default();
        sys_loading.set(true);
        spawn_local(async move {
            match fetch_system_status(&token).await {
                Ok(status) => system_status.set(Some(status)),
                Err(e) => error.set(Some(format!("System status: {e}"))),
            }
            sys_loading.set(false);
        });
    };

    // ── Refresh log level ────────────────────────────────────────────────────
    let refresh_log_level = move || {
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

    // ── Initial load ─────────────────────────────────────────────────────────
    Effect::new(move |_| {
        refresh_users();
        refresh_system();
        refresh_log_level();
    });

    // ── Sync edit_role when selection changes ────────────────────────────────
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
        <div class="page">
            <div class="heading">
                <div>
                    <h1>"Administration"</h1>
                    <p>"Manage users, system settings, and backups."</p>
                </div>
            </div>

            <ErrorBanner error=error />
            {move || notice.get().map(|n| view! { <p class="msg-notice">{n}</p> })}

            // ── System Status ────────────────────────────────────────────────
            <div class="detail-card">
                <div class="card-title-row">
                    <h2 class="card-title">"System Status"</h2>
                    <button
                        class="btn btn-outline"
                        on:click=move |_| refresh_system()
                        disabled=move || sys_loading.get()
                    >
                        {move || if sys_loading.get() { "Refreshing..." } else { "Refresh" }}
                    </button>
                </div>
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
                            </div>
                        }.into_any()
                    } else {
                        view! { <p class="no-controls-msg">"Loading system status..."</p> }.into_any()
                    }
                }}
            </div>

            // ── A. User Management ───────────────────────────────────────────
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

                // User table
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

            // ── B. Change Password ───────────────────────────────────────────
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

            // ── D. Backup ────────────────────────────────────────────────────
            <div class="detail-card">
                <h2 class="card-title">"Backup"</h2>
                <p class="cell-subtle">"Download a zip archive of the current HomeCore configuration and state databases."</p>
                <div class="edit-actions">
                    <button
                        class="btn btn-primary"
                        disabled=move || busy.get()
                        on:click=move |_| {
                            let token = auth.token_str().unwrap_or_default();
                            busy.set(true);
                            error.set(None);
                            notice.set(None);
                            spawn_local(async move {
                                match trigger_backup(&token).await {
                                    Ok(bytes) => {
                                        // Trigger browser download via Blob URL
                                        use js_sys::{Array, Uint8Array};
                                        use wasm_bindgen::JsCast;

                                        let uint8 = Uint8Array::from(bytes.as_slice());
                                        let array = Array::new();
                                        array.push(&uint8.buffer());
                                        if let Ok(blob) = web_sys::Blob::new_with_u8_array_sequence(&array) {
                                            if let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) {
                                                let document = web_sys::window().unwrap().document().unwrap();
                                                let a = document.create_element("a").unwrap();
                                                let _ = a.set_attribute("href", &url);
                                                let _ = a.set_attribute("download", "homecore-backup.zip");
                                                let _ = a.set_attribute("style", "display:none");
                                                let body = document.body().unwrap();
                                                let _ = body.append_child(&a);
                                                if let Some(el) = a.dyn_ref::<web_sys::HtmlElement>() {
                                                    el.click();
                                                }
                                                let _ = body.remove_child(&a);
                                                let _ = web_sys::Url::revoke_object_url(&url);
                                                notice.set(Some("Backup downloaded.".to_string()));
                                            }
                                        }
                                    }
                                    Err(e) => error.set(Some(format!("Backup failed: {e}"))),
                                }
                                busy.set(false);
                            });
                        }
                    >
                        {move || if busy.get() { "Downloading..." } else { "Download Backup" }}
                    </button>
                </div>
            </div>

            // ── E. Log Level ─────────────────────────────────────────────────
            <div class="detail-card">
                <h2 class="card-title">"Log Level"</h2>
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
        </div>

        // ── Stale Device References ──────────────────────────────────────
        <div class="detail-card">
            <h2 class="card-title">"Stale Device References"</h2>
            <p style="margin-bottom:0.75rem;color:var(--hc-text-muted,#888)">"Rules that reference device IDs no longer registered in the device store."</p>
            <StaleRefsSection />
        </div>

        // ── Device Cleanup ───────────────────────────────────────────────
        <div class="detail-card">
            <h2 class="card-title">"Device Cleanup"</h2>
            <p style="margin-bottom:0.75rem;color:var(--hc-text-muted,#888)">"Bulk delete devices by ID. Affected rules will have references nullified."</p>
            <DeviceCleanupSection />
        </div>
    }
}

// ── Stale Refs Sub-Component ─────────────────────────────────────────────

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
                view! {
                    <table class="admin-table">
                        <thead><tr>
                            <th>"Rule Name"</th>
                            <th>"Stale Device IDs"</th>
                        </tr></thead>
                        <tbody>
                            {rules.into_iter().map(|rule| {
                                let name = rule["rule_name"].as_str().unwrap_or("?").to_string();
                                let rule_id = rule["rule_id"].as_str().unwrap_or("").to_string();
                                let stale_ids: Vec<String> = rule["stale_device_ids"]
                                    .as_array()
                                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                                    .unwrap_or_default();
                                view! {
                                    <tr>
                                        <td>
                                            <a href=format!("/rules/{rule_id}") style="color:var(--hc-accent)">{name}</a>
                                        </td>
                                        <td>
                                            {stale_ids.into_iter().map(|id| view! {
                                                <code style="margin-right:0.5rem;color:var(--hc-danger,#e53935)">{id}</code>
                                            }).collect_view()}
                                        </td>
                                    </tr>
                                }
                            }).collect_view()}
                        </tbody>
                    </table>
                }.into_any()
            }
        }}
    }
}

// ── Device Cleanup Sub-Component ─────────────────────────────────────────

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
