//! Glue devices management page — create, view, and delete helper devices.

use crate::api::{create_glue, delete_glue, fetch_glue, fetch_glue_device, send_glue_command, update_device_meta};
use crate::auth::use_auth;
use crate::pages::shared::{ErrorBanner, SearchField};
use crate::ws::use_ws;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_params_map};
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

// ── Type metadata ────────────────────────────────────────────────────────────

const GLUE_TYPES: &[(&str, &str, &str)] = &[
    ("switch",    "Switch",         "On/off toggle for automation flags"),
    ("timer",     "Timer",          "Countdown timer with start/pause/cancel"),
    ("counter",   "Counter",        "Tracks event counts with increment/decrement"),
    ("number",    "Input Number",   "Adjustable numeric value with min/max"),
    ("select",    "Input Select",   "Dropdown with predefined options"),
    ("text",      "Input Text",     "Stored string value"),
    ("button",    "Button",         "Stateless trigger — fires event on press"),
    ("datetime",  "Date/Time",      "Stored date and/or time"),
    ("group",     "Device Group",   "Combines devices into one (any/all logic)"),
    ("threshold", "Threshold",      "Binary sensor from numeric crossing"),
    ("schedule",  "Schedule",       "Weekly time blocks → on/off"),
];

fn type_label(t: &str) -> String {
    GLUE_TYPES.iter().find(|(k, _, _)| *k == t).map(|(_, l, _)| l.to_string()).unwrap_or_else(|| t.to_string())
}

/// Returns Phosphor icon names (slot into "ph ph-{name}" by the view).
fn type_icon(t: &str) -> &'static str {
    match t {
        "counter"   => "hash",
        "number"    => "number-square-one",
        "select"    => "list",
        "text"      => "text-aa",
        "button"    => "cursor-click",
        "datetime"  => "clock",
        "group"     => "squares-four",
        "threshold" => "thermometer-simple",
        "schedule"  => "calendar",
        "timer"     => "timer",
        "switch" | "virtual_switch" => "toggle-right",
        _ => "puzzle-piece",
    }
}

fn device_type_str(d: &Value) -> String {
    d["device_type"].as_str().unwrap_or("unknown").to_string()
}

fn device_value_summary(d: &Value) -> String {
    let attrs = &d["attributes"];
    let dt = device_type_str(d);
    match dt.as_str() {
        "counter" => format!("count: {}", attrs["count"].as_i64().unwrap_or(0)),
        "number" => {
            let v = attrs["value"].as_f64().unwrap_or(0.0);
            let unit = attrs["unit"].as_str().unwrap_or("");
            format!("{v}{unit}")
        }
        "select" => attrs["selected"].as_str().unwrap_or("—").to_string(),
        "text" => {
            let v = attrs["value"].as_str().unwrap_or("");
            if v.len() > 30 { format!("{}…", &v[..30]) } else { v.to_string() }
        }
        "button" => attrs["last_pressed"].as_str().unwrap_or("never").to_string(),
        "datetime" => attrs["value"].as_str().unwrap_or("—").to_string(),
        "group" => {
            let active = attrs["active_count"].as_u64().unwrap_or(0);
            let total = attrs["member_count"].as_u64().unwrap_or(0);
            let on = attrs["on"].as_bool().unwrap_or(false);
            format!("{}/{} {}", active, total, if on { "ON" } else { "off" })
        }
        "threshold" => {
            let above = attrs["above"].as_bool().unwrap_or(false);
            if above { "ABOVE".to_string() } else { "below".to_string() }
        }
        "schedule" => {
            let active = attrs["active"].as_bool().unwrap_or(false);
            if active { "ACTIVE".to_string() } else { "inactive".to_string() }
        }
        "timer" => {
            let state = attrs["state"].as_str().unwrap_or("idle");
            if state == "running" {
                let remaining = compute_remaining(attrs);
                let mins = remaining / 60;
                let secs = remaining % 60;
                format!("running ({mins}:{secs:02})")
            } else {
                state.to_string()
            }
        }
        "switch" | "virtual_switch" => {
            if attrs["on"].as_bool() == Some(true) { "ON".to_string() } else { "off".to_string() }
        }
        _ => "—".to_string(),
    }
}

/// For a running timer, compute remaining seconds from started_at + duration.
fn compute_remaining(attrs: &Value) -> u64 {
    let duration = attrs["duration_secs"].as_u64().unwrap_or(0);
    let started = attrs["started_at"].as_str().unwrap_or("");
    if started.is_empty() {
        return attrs["remaining_secs"].as_u64().unwrap_or(0);
    }
    let now_ms = js_sys::Date::now(); // millis since epoch
    let start_ms = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(started)).get_time();
    if !start_ms.is_finite() {
        return attrs["remaining_secs"].as_u64().unwrap_or(0);
    }
    let elapsed = ((now_ms - start_ms) / 1000.0).max(0.0) as u64;
    duration.saturating_sub(elapsed)
}

fn is_glue_device(plugin_id: &str) -> bool {
    matches!(plugin_id, "core.glue" | "core.timer" | "core.switch")
}


// ── Page ─────────────────────────────────────────────────────────────────────

#[component]
pub fn GluePage() -> impl IntoView {
    let auth = use_auth();
    let ws = use_ws();
    let loading = RwSignal::new(true);
    let error: RwSignal<Option<String>> = RwSignal::new(None);
    let search = RwSignal::new(String::new());
    let show_create = RwSignal::new(false);
    let confirm_delete: RwSignal<Option<String>> = RwSignal::new(None);

    // Create form state
    let new_type = RwSignal::new("switch".to_string());
    let new_id = RwSignal::new(String::new());
    let new_name = RwSignal::new(String::new());
    let creating = RwSignal::new(false);

    // 1-second tick for timer countdown display
    let timer_tick = RwSignal::new(0u64);
    Effect::new(move |_| {
        let callback = Closure::<dyn FnMut()>::new(move || {
            timer_tick.update(|t| *t += 1);
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

    // Seed WS device map from glue REST endpoint
    Effect::new(move |_| {
        let token = match auth.token.get() { Some(t) => t, None => return };
        loading.set(true);
        spawn_local(async move {
            match fetch_glue(&token).await {
                Ok(data) => {
                    ws.devices.update(|m| {
                        for d in data {
                            if let Ok(dev) = serde_json::from_value::<crate::models::DeviceState>(d) {
                                m.insert(dev.device_id.clone(), dev);
                            }
                        }
                    });
                }
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    });

    // Derive glue devices from the live WS device map
    let filtered = Memo::new(move |_| {
        let _ = timer_tick.get(); // subscribe to tick for timer countdowns
        let q = search.get().to_lowercase();
        let all = ws.devices.get();
        let mut list: Vec<Value> = all.values()
            .filter(|d| is_glue_device(&d.plugin_id))
            .filter(|d| {
                if q.is_empty() { return true; }
                let dt = d.device_type.as_deref().unwrap_or("");
                d.name.to_lowercase().contains(&q)
                    || d.device_id.to_lowercase().contains(&q)
                    || dt.to_lowercase().contains(&q)
            })
            .map(|d| json!({
                "device_id": d.device_id,
                "name": d.name,
                "device_type": d.device_type,
                "available": d.available,
                "plugin_id": d.plugin_id,
                "attributes": d.attributes,
            }))
            .collect();
        list.sort_by(|a, b| {
            let ta = device_type_str(a);
            let tb = device_type_str(b);
            ta.cmp(&tb).then_with(|| {
                a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
            })
        });
        list
    });

    view! {
        <div class="glue-page">
            // ── Heading ──────────────────────────────────────────────────────
            <div class="page-heading">
                <div>
                    <h1>"Glue Devices"</h1>
                    <p>{move || {
                        let count = ws.devices.get().values().filter(|d| is_glue_device(&d.plugin_id)).count();
                        format!("{count} devices")
                    }}</p>
                </div>
                <button class="hc-btn hc-btn--primary"
                    on:click=move |_| show_create.update(|v| *v = !*v)
                >{move || if show_create.get() { "Cancel" } else { "+ New" }}</button>
            </div>

            <ErrorBanner error=error />

            // ── Create form ──────────────────────────────────────────────────
            <Show when=move || show_create.get()>
                <div class="detail-card glue-create-form">
                    <h3 class="detail-card-title">"Create Glue Device"</h3>

                    <label class="field-label">"Type"</label>
                    <select class="hc-select"
                        on:change=move |ev| new_type.set(event_target_value(&ev))
                    >
                        {GLUE_TYPES.iter().map(|(k, label, desc)| view! {
                            <option value=*k selected=move || new_type.get() == *k>
                                {format!("{label} — {desc}")}
                            </option>
                        }).collect_view()}
                    </select>

                    <label class="field-label">"ID (slug)"</label>
                    <input type="text" class="hc-input" placeholder="e.g. deck_door_count"
                        prop:value=move || new_id.get()
                        on:input=move |ev| new_id.set(event_target_value(&ev))
                    />

                    <label class="field-label">"Display Name"</label>
                    <input type="text" class="hc-input" placeholder="e.g. Deck Door Open Count"
                        prop:value=move || new_name.get()
                        on:input=move |ev| new_name.set(event_target_value(&ev))
                    />

                    <button class="hc-btn hc-btn--primary" style="margin-top:0.5rem"
                        disabled=move || creating.get() || new_id.get().trim().is_empty() || new_name.get().trim().is_empty()
                        on:click=move |_| {
                            let token = match auth.token.get_untracked() { Some(t) => t, None => return };
                            let body = json!({
                                "type": new_type.get_untracked(),
                                "id": new_id.get_untracked().trim(),
                                "name": new_name.get_untracked().trim(),
                                "config": {}
                            });
                            creating.set(true);
                            spawn_local(async move {
                                match create_glue(&token, &body).await {
                                    Ok(dev) => {
                                        if let Ok(d) = serde_json::from_value::<crate::models::DeviceState>(dev) {
                                            ws.devices.update(|m| { m.insert(d.device_id.clone(), d); });
                                        }
                                        new_id.set(String::new());
                                        new_name.set(String::new());
                                        show_create.set(false);
                                    }
                                    Err(e) => error.set(Some(e)),
                                }
                                creating.set(false);
                            });
                        }
                    >{move || if creating.get() { "Creating…" } else { "Create" }}</button>
                </div>
            </Show>

            // ── Search ───────────────────────────────────────────────────────
            <div class="filter-panel panel">
                <div class="filter-bar">
                    <SearchField search=search placeholder="Search glue devices…" />
                </div>
            </div>

            // ── Loading ──────────────────────────────────────────────────────
            {move || loading.get().then(|| view! { <p class="msg-muted">"Loading…"</p> })}

            // ── Device list ──────────────────────────────────────────────────
            <div class="glue-list">
                {move || {
                    let list = filtered.get();
                    if list.is_empty() && !loading.get() {
                        view! {
                            <div class="hc-empty">
                                <i class="ph ph-puzzle-piece hc-empty__icon"></i>
                                <div class="hc-empty__title">"No glue devices yet"</div>
                                <p class="hc-empty__body">
                                    "Glue devices are HomeCore's built-in primitives — counters, \
                                     timers, threshold sensors, schedules, and switches. Create one \
                                     to wire automations together."
                                </p>
                            </div>
                        }.into_any()
                    } else {
                        list.into_iter().map(|d| {
                            let id = d["device_id"].as_str().unwrap_or("").to_string();
                            let name = d["name"].as_str().unwrap_or("").to_string();
                            let dt = device_type_str(&d);
                            let icon = type_icon(&dt);
                            let label = type_label(&dt);
                            let summary = device_value_summary(&d);
                            let id_for_delete = id.clone();
                            let id_for_confirm = id.clone();
                            let id_for_nav = id.clone();
                            let nav = use_navigate();

                            view! {
                                <div class="glue-row" style="cursor:pointer"
                                    on:click=move |_| {
                                        let path = format!("/glue/{}", id_for_nav);
                                        nav(&path, Default::default());
                                    }
                                >
                                    <i class={format!("ph ph-{} glue-icon", icon)} style="font-size:20px"></i>
                                    <div class="glue-info">
                                        <span class="glue-name">{name}</span>
                                        <span class="glue-meta">{label}" · "{summary}</span>
                                    </div>
                                    <span class="glue-id">{id.clone()}</span>

                                    {move || {
                                        if confirm_delete.get().as_deref() == Some(&id_for_confirm) {
                                            let id_del = id_for_delete.clone();
                                            view! {
                                                <span class="rule-confirm-delete">
                                                    "Delete? "
                                                    <button class="hc-btn hc-btn--sm hc-btn--danger"
                                                        on:click=move |ev: web_sys::MouseEvent| {
                                                            ev.stop_propagation();
                                                            let token = match auth.token.get_untracked() { Some(t) => t, None => return };
                                                            let id = id_del.clone();
                                                            confirm_delete.set(None);
                                                            spawn_local(async move {
                                                                match delete_glue(&token, &id).await {
                                                                    Ok(()) => ws.devices.update(|m| { m.remove(&id); }),
                                                                    Err(e) => error.set(Some(e)),
                                                                }
                                                            });
                                                        }
                                                    >"Yes"</button>
                                                    " "
                                                    <button class="hc-btn hc-btn--sm hc-btn--outline"
                                                        on:click=move |ev: web_sys::MouseEvent| { ev.stop_propagation(); confirm_delete.set(None); }
                                                    >"No"</button>
                                                </span>
                                            }.into_any()
                                        } else {
                                            let id_set = id_for_confirm.clone();
                                            view! {
                                                <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Delete"
                                                    on:click=move |ev: web_sys::MouseEvent| { ev.stop_propagation(); confirm_delete.set(Some(id_set.clone())); }
                                                >
                                                    <i class="ph ph-trash" style="font-size:15px"></i>
                                                </button>
                                            }.into_any()
                                        }
                                    }}
                                </div>
                            }
                        }).collect_view().into_any()
                    }
                }}
            </div>
        </div>
    }
}

// ── Detail / Edit Page ───────────────────────────────────────────────────────

#[component]
pub fn GlueDetailPage() -> impl IntoView {
    let auth = use_auth();
    let params = use_params_map();
    let device_id = move || params.read().get("id").unwrap_or_default();
    let device: RwSignal<Option<Value>> = RwSignal::new(None);
    let loading = RwSignal::new(true);
    let error: RwSignal<Option<String>> = RwSignal::new(None);
    let busy = RwSignal::new(false);
    let navigate = use_navigate();

    // Fetch device
    Effect::new(move |_| {
        let token = match auth.token.get() { Some(t) => t, None => return };
        let did = device_id();
        if did.is_empty() { return; }
        loading.set(true);
        spawn_local(async move {
            match fetch_glue_device(&token, &did).await {
                Ok(d) => device.set(Some(d)),
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    });

    // Send command helper
    let send_cmd = move |cmd: Value| {
        let token = match auth.token.get_untracked() { Some(t) => t, None => return };
        let did = device_id();
        busy.set(true);
        spawn_local(async move {
            if let Err(e) = send_glue_command(&token, &did, &cmd).await {
                error.set(Some(e));
            }
            // Refresh device state
            match fetch_glue_device(&token, &did).await {
                Ok(d) => device.set(Some(d)),
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="glue-detail">
            // ── Back + heading ────────────────────────────────────────────────
            <div class="detail-heading">
                <div class="detail-heading-actions">
                    {
                        let nav = navigate.clone();
                        view! {
                            <button class="hc-btn hc-btn--outline"
                                on:click=move |_| nav("/glue", Default::default())
                            >"← Glue Devices"</button>
                        }
                    }
                    <h2 style="flex:1; margin:0; font-size:1.1rem">
                        {move || device.get().map(|d| d["name"].as_str().unwrap_or("").to_string()).unwrap_or_default()}
                    </h2>
                </div>
            </div>

            <ErrorBanner error=error />
            {move || loading.get().then(|| view! { <p class="msg-muted">"Loading…"</p> })}

            // ── Device info + controls ────────────────────────────────────────
            {move || device.get().map(|d| {
                let dt = device_type_str(&d);
                let icon = type_icon(&dt);
                let label = type_label(&dt);
                let did = d["device_id"].as_str().unwrap_or("").to_string();
                let current_name = d["name"].as_str().unwrap_or("").to_string();
                let attrs = d["attributes"].clone();

                view! {
                    <section class="detail-card">
                        <div class="glue-detail-header">
                            <i class={format!("ph ph-{}", icon)} style="font-size:28px; color:var(--hc-text-muted)"></i>
                            <div style="flex:1">
                                <div class="rule-header-row">
                                    <input type="text" class="hc-input rule-name-input"
                                        prop:value=current_name.clone()
                                        on:change=move |ev| {
                                            let new_name = event_target_value(&ev);
                                            if new_name.trim().is_empty() || new_name == current_name { return; }
                                            let token = match auth.token.get_untracked() { Some(t) => t, None => return };
                                            let did = device_id();
                                            busy.set(true);
                                            spawn_local(async move {
                                                let _ = update_device_meta(&token, &did, &json!({"name": new_name})).await;
                                                match fetch_glue_device(&token, &did).await {
                                                    Ok(d) => device.set(Some(d)),
                                                    Err(e) => error.set(Some(e)),
                                                }
                                                busy.set(false);
                                            });
                                        }
                                    />
                                </div>
                                <span class="glue-meta">{label}" · "<code>{did}</code></span>
                            </div>
                        </div>
                    </section>

                    // ── Type-specific controls ───────────────────────────────
                    <section class="detail-card">
                        <h3 class="detail-card-title">"Controls"</h3>
                        {match dt.as_str() {
                            "counter" => {
                                let count = attrs["count"].as_i64().unwrap_or(0);
                                let step = attrs["step"].as_i64().unwrap_or(1);
                                view! {
                                    <div class="glue-ctrl-row">
                                        <span class="glue-ctrl-value">{count.to_string()}</span>
                                        <div class="glue-ctrl-btns">
                                            <button class="hc-btn hc-btn--sm" disabled=move || busy.get()
                                                on:click=move |_| send_cmd(json!({"command":"decrement"}))
                                            >"-"{step.to_string()}</button>
                                            <button class="hc-btn hc-btn--sm" disabled=move || busy.get()
                                                on:click=move |_| send_cmd(json!({"command":"increment"}))
                                            >"+"{step.to_string()}</button>
                                            <button class="hc-btn hc-btn--sm hc-btn--outline" disabled=move || busy.get()
                                                on:click=move |_| send_cmd(json!({"command":"reset"}))
                                            >"Reset"</button>
                                        </div>
                                    </div>
                                }.into_any()
                            }
                            "switch" | "virtual_switch" | "vswitch" => {
                                let on = attrs["on"].as_bool().unwrap_or(false);
                                view! {
                                    <div class="glue-ctrl-row">
                                        <div class="toggle-group">
                                            <button class:active=on disabled=move || busy.get()
                                                on:click=move |_| send_cmd(json!({"command":"on"}))
                                            >"On"</button>
                                            <button class:active=!on disabled=move || busy.get()
                                                on:click=move |_| send_cmd(json!({"command":"off"}))
                                            >"Off"</button>
                                        </div>
                                    </div>
                                }.into_any()
                            }
                            "number" => {
                                let val = attrs["value"].as_f64().unwrap_or(0.0);
                                let min = attrs["min"].as_f64().unwrap_or(0.0);
                                let max = attrs["max"].as_f64().unwrap_or(100.0);
                                let step = attrs["step"].as_f64().unwrap_or(1.0);
                                let unit = attrs["unit"].as_str().unwrap_or("").to_string();
                                view! {
                                    <div class="glue-ctrl-row">
                                        <span class="glue-ctrl-value">{format!("{val}{unit}")}</span>
                                        <div class="state-slider-row" style="flex:1">
                                            <input type="range" class="state-slider"
                                                min=min.to_string() max=max.to_string() step=step.to_string()
                                                prop:value=val.to_string()
                                                on:change=move |ev| {
                                                    if let Ok(n) = event_target_value(&ev).parse::<f64>() {
                                                        send_cmd(json!({"command":"set","value":n}));
                                                    }
                                                }
                                            />
                                        </div>
                                    </div>
                                }.into_any()
                            }
                            "select" => {
                                let selected = attrs["selected"].as_str().unwrap_or("").to_string();
                                let options: Vec<String> = attrs["options"].as_array()
                                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                                    .unwrap_or_default();
                                view! {
                                    <div class="glue-ctrl-row">
                                        <select class="hc-select" on:change=move |ev| {
                                            send_cmd(json!({"command":"select","option":event_target_value(&ev)}));
                                        }>
                                            {options.into_iter().map(|opt| {
                                                let sel = opt == selected;
                                                let opt2 = opt.clone();
                                                view! { <option value=opt selected=sel>{opt2}</option> }
                                            }).collect_view()}
                                        </select>
                                        <button class="hc-btn hc-btn--sm hc-btn--outline" disabled=move || busy.get()
                                            on:click=move |_| send_cmd(json!({"command":"previous"}))
                                        >"◀"</button>
                                        <button class="hc-btn hc-btn--sm hc-btn--outline" disabled=move || busy.get()
                                            on:click=move |_| send_cmd(json!({"command":"next"}))
                                        >"▶"</button>
                                    </div>
                                }.into_any()
                            }
                            "button" => view! {
                                <div class="glue-ctrl-row">
                                    <button class="hc-btn hc-btn--primary" disabled=move || busy.get()
                                        on:click=move |_| send_cmd(json!({"command":"press"}))
                                    >"Press"</button>
                                    <span class="glue-meta">{format!("Last: {}", attrs["last_pressed"].as_str().unwrap_or("never"))}</span>
                                </div>
                            }.into_any(),
                            "text" => {
                                let val = attrs["value"].as_str().unwrap_or("").to_string();
                                view! {
                                    <div class="glue-ctrl-row" style="flex-direction:column; align-items:stretch">
                                        <textarea class="hc-textarea" rows="3" prop:value=val.clone()
                                            on:change=move |ev| {
                                                send_cmd(json!({"command":"set","value":event_target_value(&ev)}));
                                            }
                                        />
                                    </div>
                                }.into_any()
                            }
                            "timer" => {
                                let state = attrs["state"].as_str().unwrap_or("idle").to_string();
                                let remaining = attrs["remaining_secs"].as_u64().unwrap_or(0);
                                view! {
                                    <div class="glue-ctrl-row">
                                        <span class="glue-ctrl-value">{format!("{state} ({remaining}s)")}</span>
                                        <div class="glue-ctrl-btns">
                                            <button class="hc-btn hc-btn--sm" disabled=move || busy.get()
                                                on:click=move |_| send_cmd(json!({"command":"start","duration_secs":300}))
                                            >"Start 5m"</button>
                                            <button class="hc-btn hc-btn--sm hc-btn--outline" disabled=move || busy.get()
                                                on:click=move |_| send_cmd(json!({"command":"pause"}))
                                            >"Pause"</button>
                                            <button class="hc-btn hc-btn--sm hc-btn--outline" disabled=move || busy.get()
                                                on:click=move |_| send_cmd(json!({"command":"resume"}))
                                            >"Resume"</button>
                                            <button class="hc-btn hc-btn--sm hc-btn--outline" disabled=move || busy.get()
                                                on:click=move |_| send_cmd(json!({"command":"cancel"}))
                                            >"Cancel"</button>
                                        </div>
                                    </div>
                                }.into_any()
                            }
                            "group" | "threshold" | "schedule" => {
                                let summary = device_value_summary(&d);
                                view! {
                                    <div class="glue-ctrl-row">
                                        <span class="glue-ctrl-value">{summary}</span>
                                        <span class="glue-meta">"Read-only computed device"</span>
                                    </div>
                                }.into_any()
                            }
                            _ => view! {
                                <p class="msg-muted">"No controls for this device type."</p>
                            }.into_any(),
                        }}
                    </section>

                    // ── Raw attributes ────────────────────────────────────────
                    <section class="detail-card">
                        <h3 class="detail-card-title">"Attributes"</h3>
                        <pre class="activity-detail" style="max-height:30rem">
                            {serde_json::to_string_pretty(&attrs).unwrap_or_default()}
                        </pre>
                    </section>
                }
            })}
        </div>
    }
}
