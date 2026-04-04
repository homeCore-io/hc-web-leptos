//! Glue devices management page — create, view, and delete helper devices.

use crate::api::{create_glue, delete_glue, fetch_glue};
use crate::auth::use_auth;
use crate::pages::shared::SearchField;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::{json, Value};

// ── Type metadata ────────────────────────────────────────────────────────────

const GLUE_TYPES: &[(&str, &str, &str)] = &[
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

fn type_icon(t: &str) -> &'static str {
    match t {
        "counter"   => "tag",
        "number"    => "123",
        "select"    => "list",
        "text"      => "text_fields",
        "button"    => "touch_app",
        "datetime"  => "schedule",
        "group"     => "workspaces",
        "threshold" => "thermostat",
        "schedule"  => "calendar_month",
        "timer"     => "timer",
        "virtual_switch" => "toggle_on",
        _ => "extension",
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
            let remaining = attrs["remaining_secs"].as_u64().unwrap_or(0);
            if state == "running" { format!("running ({remaining}s)") }
            else { state.to_string() }
        }
        "virtual_switch" => {
            if attrs["on"].as_bool() == Some(true) { "ON".to_string() } else { "off".to_string() }
        }
        _ => "—".to_string(),
    }
}

// ── Page ─────────────────────────────────────────────────────────────────────

#[component]
pub fn GluePage() -> impl IntoView {
    let auth = use_auth();
    let devices: RwSignal<Vec<Value>> = RwSignal::new(vec![]);
    let loading = RwSignal::new(true);
    let error: RwSignal<Option<String>> = RwSignal::new(None);
    let search = RwSignal::new(String::new());
    let show_create = RwSignal::new(false);
    let confirm_delete: RwSignal<Option<String>> = RwSignal::new(None);

    // Create form state
    let new_type = RwSignal::new("counter".to_string());
    let new_id = RwSignal::new(String::new());
    let new_name = RwSignal::new(String::new());
    let creating = RwSignal::new(false);

    // Fetch
    let reload = move || {
        let token = match auth.token.get_untracked() { Some(t) => t, None => return };
        loading.set(true);
        spawn_local(async move {
            match fetch_glue(&token).await {
                Ok(data) => devices.set(data),
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    };

    Effect::new(move |_| {
        let _ = auth.token.get();
        reload();
    });

    // Filtered list
    let filtered = Memo::new(move |_| {
        let q = search.get().to_lowercase();
        let mut list: Vec<Value> = devices.get().into_iter()
            .filter(|d| {
                if q.is_empty() { return true; }
                let name = d["name"].as_str().unwrap_or("").to_lowercase();
                let id = d["device_id"].as_str().unwrap_or("").to_lowercase();
                let dt = device_type_str(d).to_lowercase();
                name.contains(&q) || id.contains(&q) || dt.contains(&q)
            })
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
                    <p>{move || format!("{} devices", devices.get().len())}</p>
                </div>
                <button class="hc-btn hc-btn--primary"
                    on:click=move |_| show_create.update(|v| *v = !*v)
                >{move || if show_create.get() { "Cancel" } else { "+ New" }}</button>
            </div>

            {move || error.get().map(|e| view! { <p class="msg-error">{e}</p> })}

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
                                        devices.update(|list| list.push(dev));
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
                        view! { <p class="msg-muted">"No glue devices."</p> }.into_any()
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

                            view! {
                                <div class="glue-row">
                                    <span class="material-icons glue-icon" style="font-size:20px">{icon}</span>
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
                                                        on:click=move |_| {
                                                            let token = match auth.token.get_untracked() { Some(t) => t, None => return };
                                                            let id = id_del.clone();
                                                            confirm_delete.set(None);
                                                            spawn_local(async move {
                                                                match delete_glue(&token, &id).await {
                                                                    Ok(()) => devices.update(|list| list.retain(|d| d["device_id"].as_str() != Some(&id))),
                                                                    Err(e) => error.set(Some(e)),
                                                                }
                                                            });
                                                        }
                                                    >"Yes"</button>
                                                    " "
                                                    <button class="hc-btn hc-btn--sm hc-btn--outline"
                                                        on:click=move |_| confirm_delete.set(None)
                                                    >"No"</button>
                                                </span>
                                            }.into_any()
                                        } else {
                                            let id_set = id_for_confirm.clone();
                                            view! {
                                                <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Delete"
                                                    on:click=move |_| confirm_delete.set(Some(id_set.clone()))
                                                >
                                                    <span class="material-icons" style="font-size:15px">"delete"</span>
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
