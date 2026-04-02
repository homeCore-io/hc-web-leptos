//! Device detail page — `/devices/:id`

use crate::api::{
    StreamEvent, delete_device as delete_device_request, fetch_areas, fetch_device,
    fetch_device_history, set_device_state, update_device_meta,
};
use crate::auth::{events_ws_url, use_auth};
use crate::models::*;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;
use leptos_shadcn_ui::{Button, ButtonVariant, Input};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistorySortKey {
    Time,
    Attribute,
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistorySortDir {
    Asc,
    Desc,
}

fn sync_edit_fields(
    device: &DeviceState,
    edit_name: RwSignal<String>,
    edit_area: RwSignal<String>,
    edit_canonical: RwSignal<String>,
    edit_icon: RwSignal<String>,
) {
    edit_name.set(device.name.clone());
    edit_area.set(device.area.clone().unwrap_or_default());
    edit_canonical.set(device.canonical_name.clone().unwrap_or_default());
    edit_icon.set(device.status_icon.clone().unwrap_or_default());
}

fn schedule_ws_retry(retry: RwSignal<u64>, generation: u64) {
    let callback = Closure::<dyn FnMut()>::new(move || {
        if retry.get_untracked() == generation {
            retry.update(|n| *n += 1);
        }
    });

    if let Some(window) = web_sys::window() {
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            1000,
        );
    }

    callback.forget();
}

#[component]
pub fn DeviceDetailPage() -> impl IntoView {
    let auth = use_auth();
    let params = use_params_map();
    let device_id: String =
        params.with_untracked(|p| p.get("id").map(|s| s.clone()).unwrap_or_default());

    // ── Signals (all RwSignal = Copy) ─────────────────────────────────────────
    let device: RwSignal<Option<DeviceState>> = RwSignal::new(None);
    let history: RwSignal<Vec<HistoryEntry>> = RwSignal::new(vec![]);
    let areas: RwSignal<Vec<Area>> = RwSignal::new(vec![]);
    let loading = RwSignal::new(true);
    let hist_loading = RwSignal::new(true);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);
    let busy = RwSignal::new(false);
    let show_edit = RwSignal::new(false);
    let edit_name = RwSignal::new(String::new());
    let edit_area = RwSignal::new(String::new());
    let edit_canonical = RwSignal::new(String::new());
    let edit_icon = RwSignal::new(String::new());
    let delete_confirm = RwSignal::new(String::new());
    let timer_secs = RwSignal::new("60".to_string());
    let selected_favorite = RwSignal::new(String::new());
    let selected_playlist = RwSignal::new(String::new());
    let timer_tick = RwSignal::new(0u64);
    let ws_retry = RwSignal::new(0u64);
    let history_sort_by = RwSignal::new(HistorySortKey::Time);
    let history_sort_dir = RwSignal::new(HistorySortDir::Desc);

    // Trigger signals — increment to re-run the matching effect.
    // All are RwSignal (Copy), safe to capture in any closure.
    let refresh_trigger = RwSignal::new(0u32);
    let hist_trigger = RwSignal::new(0u32);
    let save_trigger = RwSignal::new(0u32);

    let auth_token = auth.token; // RwSignal<Option<String>> — Copy

    Effect::new(move |_| {
        let callback = Closure::<dyn FnMut()>::new({
            move || timer_tick.update(|tick| *tick += 1)
        });
        let handle = web_sys::window().and_then(|window| {
            window
                .set_interval_with_callback_and_timeout_and_arguments_0(
                    callback.as_ref().unchecked_ref(),
                    1000,
                )
                .ok()
        });
        callback.forget();

        on_cleanup(move || {
            if let (Some(window), Some(handle)) = (web_sys::window(), handle) {
                window.clear_interval_with_handle(handle);
            }
        });
    });

    Effect::new(move |_| {
        let Some(d) = device.get() else { return };

        let favorites = media_available_favorites(&d);
        selected_favorite.update(|current| {
            if favorites.is_empty() {
                current.clear();
            } else if current.is_empty() || !favorites.iter().any(|item| item == current) {
                *current = favorites[0].clone();
            }
        });

        let playlists = media_available_playlists(&d);
        selected_playlist.update(|current| {
            if playlists.is_empty() {
                current.clear();
            } else if current.is_empty() || !playlists.iter().any(|item| item == current) {
                *current = playlists[0].clone();
            }
        });
    });

    // ── Areas fetch ───────────────────────────────────────────────────────────
    Effect::new(move |_| {
        let token = auth_token.get().unwrap_or_default();
        spawn_local(async move {
            if let Ok(mut list) = fetch_areas(&token).await {
                list.sort_by(|a, b| sort_key_str(&a.name).cmp(&sort_key_str(&b.name)));
                areas.set(list);
            }
        });
    });

    // ── Device fetch ──────────────────────────────────────────────────────────
    let did1 = device_id.clone();
    Effect::new(move |_| {
        let _ = refresh_trigger.get();
        let token = auth_token.get().unwrap_or_default();
        let id = did1.clone();
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match fetch_device(&token, &id).await {
                Ok(d) => {
                    if device.get_untracked().is_none() || !show_edit.get_untracked() {
                        sync_edit_fields(&d, edit_name, edit_area, edit_canonical, edit_icon);
                    }
                    device.set(Some(d));
                }
                Err(e) => error.set(Some(format!("Failed to load device: {e}"))),
            }
            loading.set(false);
        });
    });

    // ── History fetch ─────────────────────────────────────────────────────────
    let did2 = device_id.clone();
    Effect::new(move |_| {
        let _ = hist_trigger.get();
        let token = auth_token.get().unwrap_or_default();
        let id = did2.clone();
        hist_loading.set(true);
        spawn_local(async move {
            match fetch_device_history(&token, &id, 25).await {
                Ok(h) => history.set(h),
                Err(_) => {}
            }
            hist_loading.set(false);
        });
    });

    // ── Save metadata ─────────────────────────────────────────────────────────
    let did3 = device_id.clone();
    Effect::new(move |_| {
        let n = save_trigger.get();
        if n == 0 {
            return;
        }
        let token = auth_token.get().unwrap_or_default();
        let id = did3.clone();
        let name_val = edit_name.get();
        let area_val = edit_area.get();
        let canonical_val = edit_canonical.get();
        let icon_val = edit_icon.get();
        let body = serde_json::json!({
            "name": name_val.trim(),
            "area": if area_val.trim().is_empty() { serde_json::Value::Null } else { area_val.trim().into() },
            "canonical_name": if canonical_val.trim().is_empty() { serde_json::Value::Null } else { canonical_val.trim().into() },
            "status_icon": if icon_val.trim().is_empty() { serde_json::Value::Null } else { icon_val.trim().into() },
        });
        busy.set(true);
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match update_device_meta(&token, &id, &body).await {
                Ok(updated) => {
                    sync_edit_fields(&updated, edit_name, edit_area, edit_canonical, edit_icon);
                    device.set(Some(updated));
                    notice.set(Some("Device updated.".into()));
                    show_edit.set(false);
                }
                Err(e) => error.set(Some(format!("Save failed: {e}"))),
            }
            busy.set(false);
        });
    });

    let sorted_history: Memo<Vec<HistoryEntry>> = Memo::new(move |_| {
        let mut entries = history.get();
        let sort_key = history_sort_by.get();
        let sort_dir = history_sort_dir.get();

        entries.sort_by(|a, b| {
            let cmp = match sort_key {
                HistorySortKey::Time => a.recorded_at.cmp(&b.recorded_at),
                HistorySortKey::Attribute => {
                    sort_key_str(&a.attribute).cmp(&sort_key_str(&b.attribute))
                }
                HistorySortKey::Value => {
                    sort_key_str(&a.value_display()).cmp(&sort_key_str(&b.value_display()))
                }
            };

            let cmp = if cmp == std::cmp::Ordering::Equal {
                a.recorded_at.cmp(&b.recorded_at)
            } else {
                cmp
            };

            if sort_dir == HistorySortDir::Desc {
                cmp.reverse()
            } else {
                cmp
            }
        });

        entries
    });

    // ── WebSocket live updates ────────────────────────────────────────────────
    let did_ws = device_id.clone();
    Effect::new(move |_| {
        let generation = ws_retry.get();
        let token = match auth_token.get() {
            Some(t) => t,
            None => return,
        };
        let url = events_ws_url(&token);
        let ws = match web_sys::WebSocket::new(&url) {
            Ok(ws) => ws,
            Err(_) => return,
        };

        let on_open = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            if generation > 0 {
                refresh_trigger.update(|n| *n += 1);
                hist_trigger.update(|n| *n += 1);
            }
        });
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        on_open.forget();

        let id_ws = did_ws.clone();
        let cb =
            Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
                let text = match ev.data().as_string() {
                    Some(s) => s,
                    None => return,
                };
                let event: StreamEvent = match serde_json::from_str(&text) {
                    Ok(e) => e,
                    Err(_) => return,
                };
                match event {
                    StreamEvent::DeviceStateChanged {
                        device_id: eid,
                        current,
                        change,
                        ..
                    } if eid == id_ws => {
                        device.update(|d| {
                            if let Some(d) = d {
                                d.attributes = current;
                                if let Some(change) = change {
                                    d.last_seen = Some(change.changed_at);
                                    d.last_change = Some(change);
                                } else {
                                    d.last_seen = Some(chrono::Utc::now());
                                }
                            }
                        });
                        hist_trigger.update(|n| *n += 1);
                    }
                    StreamEvent::DeviceAvailabilityChanged {
                        device_id: eid,
                        available,
                    } if eid == id_ws => {
                        device.update(|d| {
                            if let Some(d) = d {
                                d.available = available;
                            }
                        });
                    }
                    _ => {}
                }
            });
        ws.set_onmessage(Some(cb.as_ref().unchecked_ref()));
        cb.forget();

        let on_close = Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |_| {
            schedule_ws_retry(ws_retry, generation);
        });
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        on_close.forget();

        let on_error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            schedule_ws_retry(ws_retry, generation);
        });
        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        on_error.forget();

        on_cleanup(move || {
            ws.set_onopen(None);
            ws.set_onmessage(None);
            ws.set_onclose(None);
            ws.set_onerror(None);
            let _ = ws.close();
        });
    });

    // ── View ──────────────────────────────────────────────────────────────────
    view! {
        <div class="page device-detail-page">

            // Back link
            <div>
                <a href="/devices" class="back-link">
                    <span class="material-icons" style="font-size:18px;vertical-align:middle">"arrow_back"</span>
                    " Devices"
                </a>
            </div>

            // Heading — reactive on device
            {move || device.get().map(|d| {
                let tone  = status_tone(&d);
                let icon  = status_icon_name(&d);
                let stext = status_text(&d);
                let avail = d.available;
                let name  = d.name.clone();
                let area  = d.area.as_deref().map(display_area_name);
                view! {
                    <div class="detail-title-row">
                        <span class=format!("status-badge-lg {}", tone.css_class())>
                            <span class="material-icons" style="font-size:26px">{icon}</span>
                        </span>
                        <div class="detail-name-block">
                            <h1>{name}</h1>
                            <div class="detail-meta-chips">
                                <span class:chip-online=avail class:chip-offline=!avail>
                                    {if avail { "Online" } else { "Offline" }}
                                </span>
                                <span class="chip-neutral">{stext}</span>
                                {area.map(|a| view! { <span class="chip-neutral">{a}</span> })}
                            </div>
                        </div>
                        <div class="detail-heading-actions">
                            <Button
                                variant=ButtonVariant::Secondary
                                on_click=Callback::new(move |_| show_edit.update(|v| *v = !*v))
                            >
                                <span class="material-icons" style="font-size:16px;vertical-align:middle">"edit"</span>
                                {move || if show_edit.get() { " Close editor" } else { " Edit" }}
                            </Button>
                        </div>
                    </div>
                }
            })}
            {move || (loading.get() && device.get().is_none()).then(move || view! {
                <p style="color:var(--hc-text-muted)">"Loading device…"</p>
            })}

            // Feedback
            {move || error.get().map(|e| view! { <p class="msg-error">{e}</p> })}
            {move || notice.get().map(|n| view! { <p class="msg-notice">{n}</p> })}

            // Main content
            {move || device.get().map(|d| {
                let _ = timer_tick.get();
                let id = d.device_id.clone();
                let is_timer  = d.plugin_id.starts_with("core.timer");
                let is_switch = d.plugin_id.starts_with("core.switch");
                let is_media  = is_media_player(&d);
                let has_on    = bool_attr(d.attributes.get("on")).is_some();
                let has_bri   = d.attributes.get("brightness_pct").and_then(|v| v.as_f64()).is_some();
                let has_ct    = d.attributes.get("color_temp").and_then(|v| v.as_f64()).is_some();
                let has_vol   = d.attributes.get("volume").and_then(|v| v.as_f64()).is_some();
                let has_lock  = bool_attr(d.attributes.get("locked")).is_some();
                let cur_on    = bool_attr(d.attributes.get("on")).unwrap_or(false);
                let cur_bri   = d.attributes.get("brightness_pct").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let cur_ct    = d.attributes.get("color_temp").and_then(|v| v.as_f64()).unwrap_or(2700.0);
                let cur_vol   = d.attributes.get("volume").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let cur_lock  = bool_attr(d.attributes.get("locked")).unwrap_or(false);
                let timer_state  = str_attr(d.attributes.get("state")).unwrap_or("idle").to_string();
                let timer_rem    = timer_remaining_secs(&d).unwrap_or(0);
                let timer_dur    = d.attributes.get("duration_secs").and_then(|v| v.as_u64())
                    .or_else(|| d.attributes.get("duration_ms").and_then(|v| v.as_u64()).map(|ms| ms / 1000))
                    .unwrap_or(0);
                let pb_state     = playback_state(&d);
                let media_sum    = media_summary(&d);
                let media_img    = media_image_url(&d).map(str::to_string);
                let media_enrichments = media_ui_enrichments(&d);
                let media_favorites = media_available_favorites(&d);
                let media_playlists = media_available_playlists(&d);
                let last_changed = last_change_time(&d);
                let change_text  = change_summary(&d);
                let correlation  = change_correlation_id(&d).map(str::to_string);
                let area_options = areas.get();

                let mut attr_pairs: Vec<(String, String)> = d.attributes.iter().map(|(k, v)| {
                    let disp = match v {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Number(n) => {
                            if let Some(f) = n.as_f64() {
                                if f.fract() == 0.0 { format!("{}", f as i64) } else { format!("{f:.2}") }
                            } else { n.to_string() }
                        }
                        other => other.to_string(),
                    };
                    (k.clone(), disp)
                }).collect();
                attr_pairs.sort_by(|a, b| a.0.cmp(&b.0));

                let is_running = timer_state == "running";
                let is_paused  = timer_state == "paused";
                let is_idle    = !is_running && !is_paused;
                let pct = if timer_dur > 0 {
                    ((timer_dur.saturating_sub(timer_rem)) as f64 / timer_dur as f64 * 100.0).min(100.0) as u32
                } else { 0 };
                let no_controls = !has_on && !is_switch && !has_bri && !has_ct
                    && !has_lock && !is_media && !is_timer;

                // ── id clones — one per on:click closure ─────────────────────
                let id_on    = id.clone();
                let id_off   = id.clone();
                let id_bri   = id.clone();
                let id_ct    = id.clone();
                let id_lock  = id.clone();
                let id_unlck = id.clone();
                let id_play  = id.clone();
                let id_play_favorite = id.clone();
                let id_play_playlist = id.clone();
                let id_pause = id.clone();
                let id_stop  = id.clone();
                let id_prev  = id.clone();
                let id_next  = id.clone();
                let id_vol   = id.clone();
                let id_tpaus = id.clone();
                let id_tcanc = id.clone();
                let id_tresu = id.clone();
                let id_tcanc2= id.clone();
                let id_start = id.clone();
                let ts_label = timer_state.clone();
                let delete_confirm_label = d.device_id.clone();
                let delete_confirm_target = d.device_id.clone();
                let delete_request_target = d.device_id.clone();

                view! {
                    // ── Edit form ─────────────────────────────────────────────
                    {move || show_edit.get().then({
                        let area_options = area_options.clone();
                        let delete_confirm_label = delete_confirm_label.clone();
                        let delete_confirm_target = delete_confirm_target.clone();
                        let delete_request_target = delete_request_target.clone();
                        move || view! {
                        <div class="detail-card edit-card">
                            <h2 class="card-title">"Edit Device"</h2>
                            <div class="edit-grid">
                                <div class="edit-field">
                                    <label>"Display Name"</label>
                                    <Input
                                        value=Signal::derive(move || edit_name.get())
                                        on_change=Callback::new(move |value| edit_name.set(value))
                                        placeholder="Display name"
                                    />
                                </div>
                                <div class="edit-field">
                                    <label>"Area"</label>
                                    <select
                                        on:change=move |ev| edit_area.set(event_target_value(&ev))
                                    >
                                        <option value="" selected=move || edit_area.get().is_empty()>
                                            "Unassigned"
                                        </option>
                                        <For
                                            each=move || area_options.clone()
                                            key=|area| area.id.clone()
                                            children=move |area| {
                                                let selected_name = area.name.clone();
                                                let label = display_area_name(&area.name);
                                                view! {
                                                    <option
                                                        value=selected_name.clone()
                                                        selected=move || edit_area.get() == selected_name
                                                    >
                                                        {label}
                                                    </option>
                                                }
                                            }
                                        />
                                    </select>
                                    <span class="cell-subtle">
                                        "Areas come from HomeCore’s defined areas list."
                                    </span>
                                </div>
                                <div class="edit-field">
                                    <label>"Canonical Name"</label>
                                    <Input
                                        value=Signal::derive(move || edit_canonical.get())
                                        on_change=Callback::new(move |value| edit_canonical.set(value))
                                        placeholder="e.g. living_room.floor_lamp (blank to clear)"
                                    />
                                </div>
                                <div class="edit-field">
                                    <label>"Status Icon"</label>
                                    <Input
                                        value=Signal::derive(move || edit_icon.get())
                                        on_change=Callback::new(move |value| edit_icon.set(value))
                                        placeholder="e.g. power, lock (blank to clear)"
                                    />
                                </div>
                            </div>
                            <div class="edit-actions">
                                <Button
                                    variant=ButtonVariant::Default
                                    on_click=Callback::new(move |_| save_trigger.update(|n| *n += 1))
                                    disabled=Signal::derive(move || busy.get() || edit_name.get().trim().is_empty())
                                >
                                    {move || if busy.get() { "Saving…" } else { "Save" }}
                                </Button>
                                <Button
                                    variant=ButtonVariant::Outline
                                    on_click=Callback::new(move |_| {
                                        if let Some(current) = device.get() {
                                            sync_edit_fields(
                                                &current,
                                                edit_name,
                                                edit_area,
                                                edit_canonical,
                                                edit_icon,
                                            );
                                        }
                                        delete_confirm.set(String::new());
                                    })
                                >
                                    "Reset fields"
                                </Button>
                                <Button
                                    variant=ButtonVariant::Outline
                                    on_click=Callback::new(move |_| {
                                        if let Some(current) = device.get() {
                                            sync_edit_fields(
                                                &current,
                                                edit_name,
                                                edit_area,
                                                edit_canonical,
                                                edit_icon,
                                            );
                                        }
                                        delete_confirm.set(String::new());
                                        show_edit.set(false);
                                    })
                                >
                                    "Cancel"
                                </Button>
                            </div>

                            <div class="danger-zone">
                                <div class="danger-zone-copy">
                                    <h3>"Delete Device"</h3>
                                    <p>
                                        "This removes the device from HomeCore. Rule references are rewritten to deleted placeholders on the backend."
                                    </p>
                                </div>
                                <div class="danger-zone-controls">
                                    <div class="edit-field">
                                        <label>{format!("Type {} to confirm", delete_confirm_label)}</label>
                                        <Input
                                            value=Signal::derive(move || delete_confirm.get())
                                            on_change=Callback::new(move |value| delete_confirm.set(value))
                                            placeholder="Device ID confirmation"
                                        />
                                    </div>
                                    <button
                                        class="danger"
                                        disabled=move || busy.get() || delete_confirm.get().trim() != delete_confirm_target
                                        on:click=move |_| {
                                            let token = auth_token.get().unwrap_or_default();
                                            let did = delete_request_target.clone();
                                            busy.set(true);
                                            error.set(None);
                                            notice.set(None);
                                            spawn_local(async move {
                                                match delete_device_request(&token, &did).await {
                                                    Ok(resp) if resp.deleted => {
                                                        let rule_note = if resp.affected_rules.is_empty() {
                                                            String::new()
                                                        } else {
                                                            format!(" {} rules updated.", resp.affected_rules.len())
                                                        };
                                                        notice.set(Some(format!("Device deleted.{rule_note}")));
                                                        if let Some(win) = web_sys::window() {
                                                            let _ = win.location().set_href("/devices");
                                                        }
                                                    }
                                                    Ok(_) => {
                                                        error.set(Some("Delete did not complete.".into()));
                                                        busy.set(false);
                                                    }
                                                    Err(e) => {
                                                        error.set(Some(format!("Delete failed: {e}")));
                                                        busy.set(false);
                                                    }
                                                }
                                            });
                                        }
                                    >
                                        {move || if busy.get() { "Deleting…" } else { "Delete device" }}
                                    </button>
                                </div>
                            </div>
                        </div>
                    }})}

                    // ── Two-column info grid ──────────────────────────────────
                    <div class="detail-grid">

                        // Overview card
                        <div class="detail-card">
                            <h2 class="card-title">"Overview"</h2>
                            <table class="info-table">
                                <tbody>
                                <tr><td class="info-label">"Device ID"</td>
                                    <td><code class="mono">{d.device_id.clone()}</code></td></tr>
                                <tr><td class="info-label">"Plugin"</td>
                                    <td>{d.plugin_id.clone()}</td></tr>
                                <tr><td class="info-label">"Area"</td>
                                    <td>{display_area_value(d.area.as_deref())}</td></tr>
                                <tr><td class="info-label">"Canonical"</td>
                                    <td><code class="mono">{d.canonical_name.as_deref().unwrap_or("—").to_string()}</code></td></tr>
                                <tr><td class="info-label">"Type"</td>
                                    <td>
                                        <div class="cell-primary">
                                            <span>{presentation_device_type_label(&d).to_string()}</span>
                                            <span class="cell-subtle">
                                                {"Raw type: "}{raw_device_type_label(&d)}
                                            </span>
                                        </div>
                                    </td></tr>
                                <tr><td class="info-label">"Availability"</td>
                                    <td>
                                        <span class:chip-online=d.available class:chip-offline=!d.available>
                                            {if d.available { "Online" } else { "Offline" }}
                                        </span>
                                    </td></tr>
                                <tr><td class="info-label">"Last seen"</td>
                                    <td>
                                        <div class="cell-primary">
                                            <span>{format_relative(d.last_seen.as_ref())}</span>
                                            <span class="cell-subtle">{format_abs(d.last_seen.as_ref())}</span>
                                        </div>
                                    </td></tr>
                                <tr><td class="info-label">"Last changed"</td>
                                    <td>
                                        <div class="cell-primary">
                                            <span>{format_relative(last_changed)}</span>
                                            <span class="cell-subtle">{format_abs(last_changed)}</span>
                                        </div>
                                    </td></tr>
                                <tr><td class="info-label">"Change source"</td>
                                    <td>
                                        <div class="cell-primary">
                                            <span>{change_text}</span>
                                            {correlation.map(|id| view! {
                                                <span class="cell-subtle">
                                                    {"Correlation: "}<code class="mono">{id}</code>
                                                </span>
                                            })}
                                        </div>
                                    </td></tr>
                                </tbody>
                            </table>
                        </div>

                        // Controls card
                        <div class="detail-card">
                            <h2 class="card-title">"Controls"</h2>

                            // On/Off toggle
                            {(has_on || is_switch).then(|| view! {
                                <div class="control-row">
                                    <span class="control-label">"Power"</span>
                                    <div class="toggle-group">
                                        <button class:active=cur_on disabled=move || busy.get()
                                            on:click=move |_| {
                                                let token = auth_token.get().unwrap_or_default();
                                                let did = id_on.clone();
                                                busy.set(true); error.set(None);
                                                spawn_local(async move {
                                                    let _ = set_device_state(&token, &did, &serde_json::json!({"on":true})).await;
                                                    busy.set(false);
                                                });
                                            }>"On"</button>
                                        <button class:active=!cur_on disabled=move || busy.get()
                                            on:click=move |_| {
                                                let token = auth_token.get().unwrap_or_default();
                                                let did = id_off.clone();
                                                busy.set(true); error.set(None);
                                                spawn_local(async move {
                                                    let _ = set_device_state(&token, &did, &serde_json::json!({"on":false})).await;
                                                    busy.set(false);
                                                });
                                            }>"Off"</button>
                                    </div>
                                </div>
                            })}

                            // Brightness slider
                            {has_bri.then(|| view! {
                                <div class="control-row">
                                    <span class="control-label">"Brightness"</span>
                                    <div class="slider-row">
                                        <input type="range" min="0" max="100" step="1"
                                            prop:value=cur_bri as i64
                                            on:change=move |ev| {
                                                if let Some(el) = ev.target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                {
                                                    let val: f64 = el.value().parse().unwrap_or(0.0);
                                                    let token = auth_token.get().unwrap_or_default();
                                                    let did = id_bri.clone();
                                                    busy.set(true); error.set(None);
                                                    spawn_local(async move {
                                                        let _ = set_device_state(&token, &did,
                                                            &serde_json::json!({"brightness_pct": val})).await;
                                                        busy.set(false);
                                                    });
                                                }
                                            }
                                        />
                                        <span class="slider-value">{format!("{:.0}%", cur_bri)}</span>
                                    </div>
                                </div>
                            })}

                            // Color temperature slider
                            {has_ct.then(|| view! {
                                <div class="control-row">
                                    <span class="control-label">"Color Temp"</span>
                                    <div class="slider-row">
                                        <input type="range" min="2700" max="6500" step="50"
                                            prop:value=cur_ct as i64
                                            style="accent-color:#1565c0"
                                            on:change=move |ev| {
                                                if let Some(el) = ev.target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                {
                                                    let val: f64 = el.value().parse().unwrap_or(2700.0);
                                                    let token = auth_token.get().unwrap_or_default();
                                                    let did = id_ct.clone();
                                                    busy.set(true); error.set(None);
                                                    spawn_local(async move {
                                                        let _ = set_device_state(&token, &did,
                                                            &serde_json::json!({"color_temp": val})).await;
                                                        busy.set(false);
                                                    });
                                                }
                                            }
                                        />
                                        <span class="slider-value">{format!("{:.0}K", cur_ct)}</span>
                                    </div>
                                </div>
                            })}

                            // Lock / Unlock
                            {has_lock.then(|| view! {
                                <div class="control-row">
                                    <span class="control-label">"Lock"</span>
                                    <div class="btn-group">
                                        <button class:active=cur_lock disabled=move || busy.get()
                                            on:click=move |_| {
                                                let token = auth_token.get().unwrap_or_default();
                                                let did = id_lock.clone();
                                                busy.set(true); error.set(None);
                                                spawn_local(async move {
                                                    let _ = set_device_state(&token, &did, &serde_json::json!({"locked":true})).await;
                                                    busy.set(false);
                                                });
                                            }>
                                            <span class="material-icons" style="font-size:16px;vertical-align:middle">"lock"</span>" Lock"
                                        </button>
                                        <button class:active=!cur_lock disabled=move || busy.get()
                                            on:click=move |_| {
                                                let token = auth_token.get().unwrap_or_default();
                                                let did = id_unlck.clone();
                                                busy.set(true); error.set(None);
                                                spawn_local(async move {
                                                    let _ = set_device_state(&token, &did, &serde_json::json!({"locked":false})).await;
                                                    busy.set(false);
                                                });
                                            }>
                                            <span class="material-icons" style="font-size:16px;vertical-align:middle">"lock_open_right"</span>" Unlock"
                                        </button>
                                    </div>
                                </div>
                            })}

                            // Media player controls
                            {is_media.then(|| {
                                let pb  = pb_state.clone();
                                let pb2 = pb_state.clone();
                                let img = media_img.clone();
                                let sum = media_sum.clone();
                                let show_play  = pb != "playing";
                                let show_pause = pb == "playing";
                                let sup_prev = supports_action(&d, "previous");
                                let sup_stop = supports_action(&d, "stop");
                                let sup_next = supports_action(&d, "next");
                                let sup_play_media = supports_action(&d, "play_media");
                                let show_favorites = media_enrichments.iter().any(|item| item == "favorites")
                                    && !media_favorites.is_empty();
                                let show_playlists = media_enrichments.iter().any(|item| item == "playlists")
                                    && !media_playlists.is_empty();
                                view! {
                                    <div class="control-section">
                                        {sum.map(|s| view! {
                                            <div class="media-now-playing">
                                                {img.map(|url| view! { <img src=url alt="" class="media-thumb" /> })}
                                                <div class="cell-primary">
                                                    <span class="media-title">{s}</span>
                                                    <span class="cell-subtle">{pb2}</span>
                                                </div>
                                            </div>
                                        })}
                                        {show_favorites.then(|| {
                                            let favorites = media_favorites.clone();
                                            view! {
                                                <div class="control-row">
                                                    <span class="control-label">"Favorites"</span>
                                                    <div class="timer-start-row">
                                                        <select
                                                            on:change=move |ev| selected_favorite.set(event_target_value(&ev))
                                                        >
                                                            {favorites.into_iter().map(|favorite| {
                                                                let selected_name = favorite.clone();
                                                                view! {
                                                                    <option
                                                                        value=selected_name.clone()
                                                                        selected=move || selected_favorite.get() == selected_name
                                                                    >
                                                                        {favorite}
                                                                    </option>
                                                                }
                                                            }).collect_view()}
                                                        </select>
                                                        <button disabled=move || busy.get() || selected_favorite.get().is_empty()
                                                            on:click=move |_| {
                                                                let token = auth_token.get().unwrap_or_default();
                                                                let did = id_play_favorite.clone();
                                                                let favorite = selected_favorite.get();
                                                                if favorite.is_empty() {
                                                                    return;
                                                                }
                                                                let body = if sup_play_media {
                                                                    serde_json::json!({"action":"play_media","media_type":"favorite","name": favorite})
                                                                } else {
                                                                    serde_json::json!({"action":"play_favorite","favorite": favorite})
                                                                };
                                                                busy.set(true); error.set(None);
                                                                spawn_local(async move {
                                                                    let _ = set_device_state(&token, &did, &body).await;
                                                                    busy.set(false);
                                                                });
                                                            }>
                                                            <span class="material-icons">"play_arrow"</span>" Play"
                                                        </button>
                                                    </div>
                                                </div>
                                            }
                                        })}
                                        {show_playlists.then(|| {
                                            let playlists = media_playlists.clone();
                                            view! {
                                                <div class="control-row">
                                                    <span class="control-label">"Playlists"</span>
                                                    <div class="timer-start-row">
                                                        <select
                                                            on:change=move |ev| selected_playlist.set(event_target_value(&ev))
                                                        >
                                                            {playlists.into_iter().map(|playlist| {
                                                                let selected_name = playlist.clone();
                                                                view! {
                                                                    <option
                                                                        value=selected_name.clone()
                                                                        selected=move || selected_playlist.get() == selected_name
                                                                    >
                                                                        {playlist}
                                                                    </option>
                                                                }
                                                            }).collect_view()}
                                                        </select>
                                                        <button disabled=move || busy.get() || selected_playlist.get().is_empty()
                                                            on:click=move |_| {
                                                                let token = auth_token.get().unwrap_or_default();
                                                                let did = id_play_playlist.clone();
                                                                let playlist = selected_playlist.get();
                                                                if playlist.is_empty() {
                                                                    return;
                                                                }
                                                                let body = if sup_play_media {
                                                                    serde_json::json!({"action":"play_media","media_type":"playlist","name": playlist})
                                                                } else {
                                                                    serde_json::json!({"action":"play_playlist","playlist": playlist})
                                                                };
                                                                busy.set(true); error.set(None);
                                                                spawn_local(async move {
                                                                    let _ = set_device_state(&token, &did, &body).await;
                                                                    busy.set(false);
                                                                });
                                                            }>
                                                            <span class="material-icons">"play_arrow"</span>" Play"
                                                        </button>
                                                    </div>
                                                </div>
                                            }
                                        })}
                                        <div class="control-row">
                                            <span class="control-label">"Playback"</span>
                                            <div class="btn-group">
                                                {sup_prev.then(|| view! {
                                                    <button disabled=move || busy.get()
                                                        on:click=move |_| {
                                                            let token = auth_token.get().unwrap_or_default();
                                                            let did = id_prev.clone();
                                                            spawn_local(async move { let _ = set_device_state(&token, &did, &serde_json::json!({"action":"previous"})).await; });
                                                        }>
                                                        <span class="material-icons">"skip_previous"</span>
                                                    </button>
                                                })}
                                                {show_play.then(|| view! {
                                                    <button disabled=move || busy.get()
                                                        on:click=move |_| {
                                                            let token = auth_token.get().unwrap_or_default();
                                                            let did = id_play.clone();
                                                            busy.set(true); error.set(None);
                                                            spawn_local(async move {
                                                                let _ = set_device_state(&token, &did, &serde_json::json!({"action":"play"})).await;
                                                                busy.set(false);
                                                            });
                                                        }>
                                                        <span class="material-icons">"play_arrow"</span>
                                                    </button>
                                                })}
                                                {show_pause.then(|| view! {
                                                    <button disabled=move || busy.get()
                                                        on:click=move |_| {
                                                            let token = auth_token.get().unwrap_or_default();
                                                            let did = id_pause.clone();
                                                            busy.set(true); error.set(None);
                                                            spawn_local(async move {
                                                                let _ = set_device_state(&token, &did, &serde_json::json!({"action":"pause"})).await;
                                                                busy.set(false);
                                                            });
                                                        }>
                                                        <span class="material-icons">"pause"</span>
                                                    </button>
                                                })}
                                                {sup_stop.then(|| view! {
                                                    <button disabled=move || busy.get()
                                                        on:click=move |_| {
                                                            let token = auth_token.get().unwrap_or_default();
                                                            let did = id_stop.clone();
                                                            spawn_local(async move { let _ = set_device_state(&token, &did, &serde_json::json!({"action":"stop"})).await; });
                                                        }>
                                                        <span class="material-icons">"stop"</span>
                                                    </button>
                                                })}
                                                {sup_next.then(|| view! {
                                                    <button disabled=move || busy.get()
                                                        on:click=move |_| {
                                                            let token = auth_token.get().unwrap_or_default();
                                                            let did = id_next.clone();
                                                            spawn_local(async move { let _ = set_device_state(&token, &did, &serde_json::json!({"action":"next"})).await; });
                                                        }>
                                                        <span class="material-icons">"skip_next"</span>
                                                    </button>
                                                })}
                                            </div>
                                        </div>
                                        {has_vol.then(|| view! {
                                            <div class="control-row">
                                                <span class="control-label">"Volume"</span>
                                                <div class="slider-row">
                                                    <span class="material-icons" style="font-size:18px;color:var(--hc-text-muted)">"volume_down"</span>
                                                    <input type="range" min="0" max="100" step="1"
                                                        prop:value=cur_vol as i64
                                                        on:change=move |ev| {
                                                            if let Some(el) = ev.target()
                                                                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                            {
                                                                let val: f64 = el.value().parse().unwrap_or(0.0);
                                                                let token = auth_token.get().unwrap_or_default();
                                                                let did = id_vol.clone();
                                                                spawn_local(async move {
                                                                    let _ = set_device_state(&token, &did, &serde_json::json!({"volume": val})).await;
                                                                });
                                                            }
                                                        }
                                                    />
                                                    <span class="material-icons" style="font-size:18px;color:var(--hc-text-muted)">"volume_up"</span>
                                                    <span class="slider-value">{format!("{:.0}%", cur_vol)}</span>
                                                </div>
                                            </div>
                                        })}
                                    </div>
                                }
                            })}

                            // Timer controls
                            {is_timer.then(|| view! {
                                <div class="control-section">
                                    <div class="timer-status">
                                        <span class="timer-state-label">{ts_label.clone()}</span>
                                        {(is_running || is_paused).then(|| view! {
                                            <span class="timer-remaining">
                                                {format_duration_secs(timer_rem)}" remaining"
                                            </span>
                                        })}
                                    </div>
                                    {(is_running || is_paused).then(|| view! {
                                        <div class="timer-progress-track">
                                            <div class="timer-progress-fill" style=format!("width:{}%", pct)></div>
                                        </div>
                                    })}
                                    {is_running.then(|| view! {
                                        <div class="btn-group">
                                            <button disabled=move || busy.get()
                                                on:click=move |_| {
                                                    let token = auth_token.get().unwrap_or_default();
                                                    let did = id_tpaus.clone();
                                                    spawn_local(async move { let _ = set_device_state(&token, &did, &serde_json::json!({"command":"pause"})).await; });
                                                }>
                                                <span class="material-icons">"pause"</span>" Pause"
                                            </button>
                                            <button class="danger" disabled=move || busy.get()
                                                on:click=move |_| {
                                                    let token = auth_token.get().unwrap_or_default();
                                                    let did = id_tcanc.clone();
                                                    spawn_local(async move { let _ = set_device_state(&token, &did, &serde_json::json!({"command":"cancel"})).await; });
                                                }>"Cancel"</button>
                                        </div>
                                    })}
                                    {is_paused.then(|| view! {
                                        <div class="btn-group">
                                            <button disabled=move || busy.get()
                                                on:click=move |_| {
                                                    let token = auth_token.get().unwrap_or_default();
                                                    let did = id_tresu.clone();
                                                    spawn_local(async move { let _ = set_device_state(&token, &did, &serde_json::json!({"command":"resume"})).await; });
                                                }>
                                                <span class="material-icons">"play_arrow"</span>" Resume"
                                            </button>
                                            <button class="danger" disabled=move || busy.get()
                                                on:click=move |_| {
                                                    let token = auth_token.get().unwrap_or_default();
                                                    let did = id_tcanc2.clone();
                                                    spawn_local(async move { let _ = set_device_state(&token, &did, &serde_json::json!({"command":"cancel"})).await; });
                                                }>"Cancel"</button>
                                        </div>
                                    })}
                                    {is_idle.then(|| view! {
                                        <div class="control-row">
                                            <span class="control-label">"Start"</span>
                                            <div class="timer-start-row">
                                                <Input
                                                    value=Signal::derive(move || timer_secs.get())
                                                    on_change=Callback::new(move |value| timer_secs.set(value))
                                                    input_type="text"
                                                    placeholder="Seconds"
                                                />
                                                <button disabled=move || busy.get()
                                                    on:click=move |_| {
                                                        let secs: u64 = timer_secs.get().trim().parse().unwrap_or(60);
                                                        let token = auth_token.get().unwrap_or_default();
                                                        let did = id_start.clone();
                                                        busy.set(true); error.set(None);
                                                        spawn_local(async move {
                                                            let _ = set_device_state(&token, &did,
                                                                &serde_json::json!({"command":"start","duration_ms": secs * 1000})).await;
                                                            busy.set(false);
                                                        });
                                                    }>
                                                    <span class="material-icons">"timer"</span>" Start"
                                                </button>
                                            </div>
                                        </div>
                                    })}
                                </div>
                            })}

                            // No controls fallback
                            {no_controls.then(|| view! {
                                <p class="no-controls-msg">
                                    <span class="material-icons" style="font-size:18px;vertical-align:middle">"info"</span>
                                    " Read-only device — no interactive controls."
                                </p>
                            })}
                        </div>
                    </div>

                    // ── Raw attributes ────────────────────────────────────────
                    <div class="detail-card">
                        <h2 class="card-title">"Attributes"
                            <span class="card-subtitle">" — live via WebSocket"</span>
                        </h2>
                        {if attr_pairs.is_empty() {
                            view! { <p class="no-controls-msg">"No attributes reported."</p> }.into_any()
                        } else {
                            view! {
                                <div class="attr-grid">
                                    {attr_pairs.into_iter().map(|(k, v)| view! {
                                        <div class="attr-row">
                                            <span class="attr-key">{k}</span>
                                            <span class="attr-val">{v}</span>
                                        </div>
                                    }).collect_view()}
                                </div>
                            }.into_any()
                        }}
                    </div>

                    // ── History ───────────────────────────────────────────────
                    <div class="detail-card">
                        <div class="card-title-row">
                            <h2 class="card-title">"State History"
                                <span class="card-subtitle">" — last 25 changes"</span>
                            </h2>
                            <Button
                                variant=ButtonVariant::Outline
                                on_click=Callback::new(move |_| hist_trigger.update(|n| *n += 1))
                                disabled=Signal::derive(move || hist_loading.get())
                            >
                                {move || if hist_loading.get() { "Reloading…" } else { "Reload" }}
                            </Button>
                        </div>
                        {move || {
                            let h = sorted_history.get();
                            if hist_loading.get() && h.is_empty() {
                                view! { <p style="padding:0.5rem 0;color:var(--hc-text-muted)">"Loading history…"</p> }.into_any()
                            } else if h.is_empty() {
                                view! { <p style="padding:0.5rem 0;color:var(--hc-text-muted)">"No history recorded yet."</p> }.into_any()
                            } else {
                                view! {
                                    <div class="history-toolbar">
                                        <div class="history-toolbar-meta">
                                            <strong>{move || sorted_history.get().len()}</strong>
                                            <span>" rows"</span>
                                        </div>
                                        <div class="history-sort-group">
                                            <button
                                                class="hist-sort-btn"
                                                class:active=move || history_sort_by.get() == HistorySortKey::Time
                                                on:click=move |_| {
                                                    if history_sort_by.get() == HistorySortKey::Time {
                                                        history_sort_dir.update(|dir| {
                                                            *dir = if *dir == HistorySortDir::Desc {
                                                                HistorySortDir::Asc
                                                            } else {
                                                                HistorySortDir::Desc
                                                            }
                                                        });
                                                    } else {
                                                        history_sort_by.set(HistorySortKey::Time);
                                                        history_sort_dir.set(HistorySortDir::Desc);
                                                    }
                                                }
                                            >
                                                "Time"
                                            </button>
                                            <button
                                                class="hist-sort-btn"
                                                class:active=move || history_sort_by.get() == HistorySortKey::Attribute
                                                on:click=move |_| {
                                                    if history_sort_by.get() == HistorySortKey::Attribute {
                                                        history_sort_dir.update(|dir| {
                                                            *dir = if *dir == HistorySortDir::Desc {
                                                                HistorySortDir::Asc
                                                            } else {
                                                                HistorySortDir::Desc
                                                            }
                                                        });
                                                    } else {
                                                        history_sort_by.set(HistorySortKey::Attribute);
                                                        history_sort_dir.set(HistorySortDir::Asc);
                                                    }
                                                }
                                            >
                                                "Attribute"
                                            </button>
                                            <button
                                                class="hist-sort-btn"
                                                class:active=move || history_sort_by.get() == HistorySortKey::Value
                                                on:click=move |_| {
                                                    if history_sort_by.get() == HistorySortKey::Value {
                                                        history_sort_dir.update(|dir| {
                                                            *dir = if *dir == HistorySortDir::Desc {
                                                                HistorySortDir::Asc
                                                            } else {
                                                                HistorySortDir::Desc
                                                            }
                                                        });
                                                    } else {
                                                        history_sort_by.set(HistorySortKey::Value);
                                                        history_sort_dir.set(HistorySortDir::Asc);
                                                    }
                                                }
                                            >
                                                "Value"
                                            </button>
                                            <button
                                                class="hist-sort-btn hist-sort-dir"
                                                on:click=move |_| {
                                                    history_sort_dir.update(|dir| {
                                                        *dir = if *dir == HistorySortDir::Desc {
                                                            HistorySortDir::Asc
                                                        } else {
                                                            HistorySortDir::Desc
                                                        }
                                                    });
                                                }
                                            >
                                                {move || if history_sort_dir.get() == HistorySortDir::Desc {
                                                    "Descending"
                                                } else {
                                                    "Ascending"
                                                }}
                                            </button>
                                        </div>
                                    </div>

                                    <div class="hist-wrap">
                                        <table class="hist-table">
                                            <thead><tr>
                                                <th>"Time"</th><th>"Attribute"</th><th>"Value"</th>
                                            </tr></thead>
                                            <tbody>
                                                <For
                                                    each=move || sorted_history.get()
                                                    key=|e| format!("{}{}{}", e.recorded_at, e.attribute, e.value)
                                                    children=|entry| {
                                                        let rel  = format_relative(Some(&entry.recorded_at));
                                                        let abs  = format_abs(Some(&entry.recorded_at));
                                                        let val  = entry.value_display();
                                                        let attr = entry.attribute.clone();
                                                        view! {
                                                            <tr>
                                                                <td><div class="cell-primary">
                                                                    <span>{rel}</span>
                                                                    <span class="cell-subtle">{abs}</span>
                                                                </div></td>
                                                                <td><code class="mono">{attr}</code></td>
                                                                <td class="hist-val">{val}</td>
                                                            </tr>
                                                        }
                                                    }
                                                />
                                            </tbody>
                                        </table>
                                    </div>
                                }.into_any()
                            }
                        }}
                    </div>

                }
            })}

        </div>
    }
}
