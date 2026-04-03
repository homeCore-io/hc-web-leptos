//! Rules page — list of automation rules with filters and inline operations.

use crate::api::{clone_rule, delete_rule, fetch_rules, patch_rule, rule_stale_refs};
use crate::auth::use_auth;
use crate::pages::shared::{ls_get, ls_set, SearchField};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use serde_json::{json, Value};

const RULES_PREFS_KEY: &str = "hc-leptos:rules:prefs";

// ── Trigger type helpers ──────────────────────────────────────────────────────

fn trigger_type(rule: &Value) -> &str {
    rule["trigger"]["type"].as_str().unwrap_or("unknown")
}

fn trigger_label(t: &str) -> &'static str {
    match t {
        "device_state_changed" => "Device State",
        "device_availability_changed" => "Availability",
        "button_event" => "Button",
        "numeric_threshold" => "Threshold",
        "time_of_day" => "Time of Day",
        "sun_event" => "Sun Event",
        "cron" => "Cron",
        "periodic" => "Periodic",
        "calendar_event" => "Calendar",
        "custom_event" => "Custom Event",
        "system_started" => "System Start",
        "hub_variable_changed" => "Hub Variable",
        "mode_changed" => "Mode Changed",
        "webhook_received" => "Webhook",
        "manual_trigger" => "Manual",
        _ => "Unknown",
    }
}

fn trigger_tone(t: &str) -> &'static str {
    match t {
        "device_state_changed"
        | "device_availability_changed"
        | "button_event"
        | "numeric_threshold" => "tone-good",
        "time_of_day" | "sun_event" | "cron" | "periodic" | "calendar_event" => "tone-warn",
        "custom_event"
        | "system_started"
        | "hub_variable_changed"
        | "mode_changed"
        | "webhook_received" => "tone-media",
        "manual_trigger" | _ => "tone-idle",
    }
}

fn trigger_category(t: &str) -> &'static str {
    match t {
        "device_state_changed"
        | "device_availability_changed"
        | "button_event"
        | "numeric_threshold" => "device",
        "time_of_day" | "sun_event" | "cron" | "periodic" | "calendar_event" => "time",
        "custom_event"
        | "system_started"
        | "hub_variable_changed"
        | "mode_changed"
        | "webhook_received" => "event",
        "manual_trigger" => "manual",
        _ => "other",
    }
}

// ── Rule field accessors ──────────────────────────────────────────────────────

fn rule_id(r: &Value) -> String {
    r["id"].as_str().unwrap_or("").to_string()
}

fn rule_name(r: &Value) -> String {
    r["name"].as_str().unwrap_or("(unnamed)").to_string()
}

fn rule_enabled(r: &Value) -> bool {
    r["enabled"].as_bool().unwrap_or(false)
}

fn rule_priority(r: &Value) -> i64 {
    r["priority"].as_i64().unwrap_or(0)
}

fn rule_error(r: &Value) -> Option<String> {
    r["error"].as_str().map(str::to_string)
}

fn rule_tags(r: &Value) -> Vec<String> {
    r["tags"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

// ── Prefs ─────────────────────────────────────────────────────────────────────

fn load_prefs() -> (String, String, String) {
    let raw = ls_get(RULES_PREFS_KEY).unwrap_or_default();
    let v: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    let search = v["search"].as_str().unwrap_or("").to_string();
    let status = v["status"].as_str().unwrap_or("all").to_string();
    let trigger = v["trigger"].as_str().unwrap_or("all").to_string();
    (search, status, trigger)
}

fn save_prefs(search: &str, status: &str, trigger: &str) {
    let v = json!({ "search": search, "status": status, "trigger": trigger });
    ls_set(RULES_PREFS_KEY, &v.to_string());
}

// ── Page ──────────────────────────────────────────────────────────────────────

#[component]
pub fn RulesPage() -> impl IntoView {
    let auth = use_auth();
    let navigate = use_navigate();

    // ── State ─────────────────────────────────────────────────────────────────
    let rules: RwSignal<Vec<Value>> = RwSignal::new(vec![]);
    let loading = RwSignal::new(true);
    let page_error: RwSignal<Option<String>> = RwSignal::new(None);

    let (init_search, init_status, init_trigger) = load_prefs();
    let search = RwSignal::new(init_search);
    let status_filter = RwSignal::new(init_status);
    let trigger_filter = RwSignal::new(init_trigger);
    let tag_filter: RwSignal<Option<String>> = RwSignal::new(None);

    // Inline confirm-delete: stores the rule id being confirmed.
    let confirm_delete: RwSignal<Option<String>> = RwSignal::new(None);
    // Per-row busy: rule id currently being operated on.
    let row_busy: RwSignal<Option<String>> = RwSignal::new(None);
    // Stale-ref warnings
    let stale_refs: RwSignal<Vec<Value>> = RwSignal::new(vec![]);
    // Bulk selection
    let selected: RwSignal<Vec<String>> = RwSignal::new(vec![]);
    let bulk_busy = RwSignal::new(false);

    // ── Initial fetch ─────────────────────────────────────────────────────────
    Effect::new(move |_| {
        let token = match auth.token.get() {
            Some(t) => t,
            None => return,
        };
        let token_for_stale = token.clone();
        spawn_local(async move {
            match fetch_rules(&token).await {
                Ok(mut data) => {
                    data.sort_by(|a, b| {
                        let pa = a["priority"].as_i64().unwrap_or(0);
                        let pb = b["priority"].as_i64().unwrap_or(0);
                        pb.cmp(&pa).then_with(|| {
                            a["name"]
                                .as_str()
                                .unwrap_or("")
                                .cmp(b["name"].as_str().unwrap_or(""))
                        })
                    });
                    rules.set(data);
                }
                Err(e) => page_error.set(Some(e)),
            }
            loading.set(false);
        });

        // Also fetch stale refs in parallel
        spawn_local(async move {
            if let Ok(data) = rule_stale_refs(&token_for_stale).await {
                if let Some(arr) = data.as_array() {
                    stale_refs.set(arr.clone());
                }
            }
        });
    });

    // Persist filter prefs on change.
    Effect::new(move |_| {
        save_prefs(
            &search.get(),
            &status_filter.get(),
            &trigger_filter.get(),
        );
    });

    // ── Filtered list ─────────────────────────────────────────────────────────
    let filtered = Memo::new(move |_| {
        let q = search.get().to_lowercase();
        let st = status_filter.get();
        let tr = trigger_filter.get();
        let tf = tag_filter.get();
        rules
            .get()
            .into_iter()
            .filter(|r| {
                let name = r["name"].as_str().unwrap_or("").to_lowercase();
                let tags = rule_tags(r);
                let tags_lower = tags.join(" ").to_lowercase();
                if !q.is_empty() && !name.contains(&q) && !tags_lower.contains(&q) {
                    return false;
                }
                match st.as_str() {
                    "active" if !rule_enabled(r) => return false,
                    "disabled" if rule_enabled(r) => return false,
                    _ => {}
                }
                if tr != "all" && trigger_category(trigger_type(r)) != tr.as_str() {
                    return false;
                }
                if let Some(ref tag) = tf {
                    if !tags.iter().any(|t| t == tag) {
                        return false;
                    }
                }
                true
            })
            .collect::<Vec<_>>()
    });

    // ── Toolbar navigate clone ────────────────────────────────────────────────
    let nav_new = navigate.clone();

    view! {
        <div class="rules-page">
            // ── Toolbar ───────────────────────────────────────────────────────
            <div class="rules-toolbar">
                <SearchField search=search placeholder="Search rules…" />

                <div class="rules-filter-group">
                    {["all", "active", "disabled"].map(|v| {
                        let label = match v { "active" => "Active", "disabled" => "Disabled", _ => "All" };
                        view! {
                            <button
                                class="filter-chip"
                                class:filter-chip--active=move || status_filter.get() == v
                                on:click=move |_| status_filter.set(v.to_string())
                            >{label}</button>
                        }
                    }).collect_view()}
                </div>

                <div class="rules-filter-group">
                    {[
                        ("all", "All Triggers"),
                        ("device", "Device"),
                        ("time", "Time"),
                        ("event", "Event"),
                        ("manual", "Manual"),
                    ].map(|(v, label)| {
                        view! {
                            <button
                                class="filter-chip"
                                class:filter-chip--active=move || trigger_filter.get() == v
                                on:click=move |_| trigger_filter.set(v.to_string())
                            >{label}</button>
                        }
                    }).collect_view()}
                </div>

                <button
                    class="hc-btn hc-btn--primary rules-new-btn"
                    on:click=move |_| nav_new("/rules/new", Default::default())
                >"+ New Rule"</button>
            </div>

            // ── Active tag filter + count ────────────────────────────────────
            <div class="rules-sub-toolbar">
                {move || tag_filter.get().map(|tag| view! {
                    <span class="filter-chip filter-chip--active">
                        "tag: " {tag.clone()}
                        <button class="tag-chip-remove" on:click=move |_| tag_filter.set(None)>"×"</button>
                    </span>
                })}
                <span class="rules-count">
                    {move || {
                        let f = filtered.get().len();
                        let t = rules.get().len();
                        if f == t { format!("{t} rules") } else { format!("{f} / {t} rules") }
                    }}
                </span>
            </div>

            // ── Bulk action bar ───────────────────────────────────────────────
            {move || {
                let sel = selected.get();
                (!sel.is_empty()).then(|| {
                    let count = sel.len();
                    view! {
                        <div class="rules-bulk-bar">
                            <span>{format!("{count} selected")}</span>
                            <button
                                class="hc-btn hc-btn--sm"
                                disabled=move || bulk_busy.get()
                                on:click=move |_| {
                                    let token = match auth.token.get_untracked() { Some(t) => t, None => return };
                                    let ids = selected.get_untracked();
                                    bulk_busy.set(true);
                                    spawn_local(async move {
                                        for id in &ids {
                                            let _ = patch_rule(&token, id, &json!({"enabled": true})).await;
                                        }
                                        // Refresh rules list
                                        if let Ok(mut data) = fetch_rules(&token).await {
                                            data.sort_by(|a, b| {
                                                let pa = a["priority"].as_i64().unwrap_or(0);
                                                let pb = b["priority"].as_i64().unwrap_or(0);
                                                pb.cmp(&pa).then_with(|| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")))
                                            });
                                            rules.set(data);
                                        }
                                        selected.set(vec![]);
                                        bulk_busy.set(false);
                                    });
                                }
                            >"Enable All"</button>
                            <button
                                class="hc-btn hc-btn--sm hc-btn--outline"
                                disabled=move || bulk_busy.get()
                                on:click=move |_| {
                                    let token = match auth.token.get_untracked() { Some(t) => t, None => return };
                                    let ids = selected.get_untracked();
                                    bulk_busy.set(true);
                                    spawn_local(async move {
                                        for id in &ids {
                                            let _ = patch_rule(&token, id, &json!({"enabled": false})).await;
                                        }
                                        if let Ok(mut data) = fetch_rules(&token).await {
                                            data.sort_by(|a, b| {
                                                let pa = a["priority"].as_i64().unwrap_or(0);
                                                let pb = b["priority"].as_i64().unwrap_or(0);
                                                pb.cmp(&pa).then_with(|| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")))
                                            });
                                            rules.set(data);
                                        }
                                        selected.set(vec![]);
                                        bulk_busy.set(false);
                                    });
                                }
                            >"Disable All"</button>
                            <button
                                class="hc-btn hc-btn--sm hc-btn--outline"
                                on:click=move |_| selected.set(vec![])
                            >"Clear"</button>
                        </div>
                    }
                })
            }}

            // ── Page error ────────────────────────────────────────────────────
            {move || page_error.get().map(|e| view! { <p class="msg-error">{e}</p> })}

            // ── Stale-ref warnings ───────────────────────────────────────────
            {move || {
                let refs = stale_refs.get();
                (!refs.is_empty()).then(|| view! {
                    <div class="stale-refs-banner">
                        <span class="material-icons" style="font-size:16px; vertical-align:middle">"warning"</span>
                        {format!(" {} rule(s) reference deleted devices:", refs.len())}
                        <ul class="stale-refs-list">
                            {refs.into_iter().map(|entry| {
                                let rule_name = entry["rule_name"].as_str().unwrap_or("Unknown").to_string();
                                let rule_id = entry["rule_id"].as_str().unwrap_or("").to_string();
                                let stale = entry["stale_device_ids"].as_array()
                                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
                                    .unwrap_or_default();
                                view! {
                                    <li>
                                        <a href=format!("/rules/{rule_id}")>{rule_name}</a>
                                        " — stale: " <code>{stale}</code>
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    </div>
                })
            }}

            // ── Loading ───────────────────────────────────────────────────────
            {move || loading.get().then(|| view! { <p class="msg-muted">"Loading rules…"</p> })}

            // ── Empty ─────────────────────────────────────────────────────────
            {move || {
                let list = filtered.get();
                (!loading.get() && list.is_empty()).then(||
                    view! { <p class="msg-muted">"No rules match the current filters."</p> }
                )
            }}

            // ── Rule list ─────────────────────────────────────────────────────
            <div class="rules-list">
                <For
                    each=move || filtered.get()
                    key=|r| r["id"].as_str().unwrap_or("").to_string()
                    children={
                        // Clone navigate once; rows clone it again inside children.
                        let nav = navigate.clone();
                        move |rule| {
                            let id      = rule_id(&rule);
                            let name    = rule_name(&rule);
                            let prio    = rule_priority(&rule);
                            let enabled = rule_enabled(&rule);
                            let ttype   = trigger_type(&rule).to_string();
                            let tlabel  = trigger_label(&ttype);
                            let ttone   = trigger_tone(&ttype);
                            let tags    = rule_tags(&rule);
                            let err     = rule_error(&rule);

                            // Per-row navigate clone.
                            let nav_edit  = nav.clone();
                            let edit_path = format!("/rules/{id}");

                            // ── Toggle enabled ────────────────────────────────
                            let id_toggle = id.clone();
                            let toggle = move |_: web_sys::MouseEvent| {
                                let token = match auth.token.get_untracked() {
                                    Some(t) => t,
                                    None => return,
                                };
                                let id = id_toggle.clone();
                                row_busy.set(Some(id.clone()));
                                spawn_local(async move {
                                    match patch_rule(&token, &id, &json!({ "enabled": !enabled })).await {
                                        Ok(updated) => rules.update(|list| {
                                            if let Some(r) = list.iter_mut().find(|r| rule_id(r) == id) {
                                                *r = updated;
                                            }
                                        }),
                                        Err(e) => page_error.set(Some(e)),
                                    }
                                    row_busy.set(None);
                                });
                            };

                            // ── Clone ─────────────────────────────────────────
                            let id_clone  = id.clone();
                            let nav_clone = nav.clone();
                            let do_clone  = move |_: web_sys::MouseEvent| {
                                let token = match auth.token.get_untracked() {
                                    Some(t) => t,
                                    None => return,
                                };
                                let id  = id_clone.clone();
                                let nav = nav_clone.clone();
                                row_busy.set(Some(id.clone()));
                                spawn_local(async move {
                                    match clone_rule(&token, &id).await {
                                        Ok(new_rule) => {
                                            let new_id = rule_id(&new_rule);
                                            rules.update(|list| list.insert(0, new_rule));
                                            if !new_id.is_empty() {
                                                nav(&format!("/rules/{new_id}"), Default::default());
                                            }
                                        }
                                        Err(e) => page_error.set(Some(e)),
                                    }
                                    row_busy.set(None);
                                });
                            };

                            // ── Delete ────────────────────────────────────────
                            let id_del = id.clone();
                            let do_delete = move |_: web_sys::MouseEvent| {
                                let token = match auth.token.get_untracked() {
                                    Some(t) => t,
                                    None => return,
                                };
                                let id = id_del.clone();
                                confirm_delete.set(None);
                                row_busy.set(Some(id.clone()));
                                spawn_local(async move {
                                    match delete_rule(&token, &id).await {
                                        Ok(()) => rules.update(|list| list.retain(|r| rule_id(r) != id)),
                                        Err(e) => page_error.set(Some(e)),
                                    }
                                    row_busy.set(None);
                                });
                            };

                            let id_confirm = id.clone();
                            let id_busy    = id.clone();

                            view! {
                                <div
                                    class="rule-row"
                                    class:rule-row--disabled=move || !enabled
                                    class:rule-row--error=err.is_some()
                                >
                                    {err.clone().map(|msg| view! {
                                        <div class="rule-row-error">
                                            <span class="material-icons" style="font-size:14px;vertical-align:middle">"error"</span>
                                            " "{msg}
                                        </div>
                                    })}

                                    <div class="rule-row-main">
                                        {
                                            let id_check = id.clone();
                                            let id_change = id.clone();
                                            view! {
                                                <input type="checkbox" class="rule-select-cb"
                                                    prop:checked=move || selected.get().contains(&id_check)
                                                    on:change=move |ev| {
                                                        use wasm_bindgen::JsCast;
                                                        let checked = ev.target()
                                                            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                            .map(|el| el.checked())
                                                            .unwrap_or(false);
                                                        let id = id_change.clone();
                                                        selected.update(|s| {
                                                            if checked { if !s.contains(&id) { s.push(id); } }
                                                            else { s.retain(|x| x != &id); }
                                                        });
                                                    }
                                                />
                                            }
                                        }
                                        <span class="rule-priority">{prio}</span>

                                        <div class="rule-name-col">
                                            <span class="rule-name">{name}</span>
                                            {(!tags.is_empty()).then(|| view! {
                                                <span class="rule-tags">
                                                    {tags.into_iter().map(|t| {
                                                        let t2 = t.clone();
                                                        view! {
                                                            <button
                                                                class="rule-tag rule-tag--clickable"
                                                                title="Filter by this tag"
                                                                on:click=move |ev: web_sys::MouseEvent| {
                                                                    ev.stop_propagation();
                                                                    tag_filter.set(Some(t2.clone()));
                                                                }
                                                            >{t}</button>
                                                        }
                                                    }).collect_view()}
                                                </span>
                                            })}
                                        </div>

                                        <span class=format!("rule-trigger-badge {ttone}")>
                                            {tlabel}
                                        </span>

                                        <div class="rule-row-actions">
                                            <button
                                                class="hc-btn hc-btn--sm"
                                                class:hc-btn--outline=!enabled
                                                disabled=move || row_busy.get().as_deref() == Some(&id_busy)
                                                on:click=toggle
                                                title=if enabled { "Disable rule" } else { "Enable rule" }
                                            >
                                                {if enabled { "Enabled" } else { "Disabled" }}
                                            </button>

                                            <button
                                                class="hc-btn hc-btn--sm hc-btn--outline"
                                                on:click=move |_| nav_edit(&edit_path, Default::default())
                                            >"Edit"</button>

                                            <button
                                                class="hc-btn hc-btn--sm hc-btn--outline"
                                                title="Clone rule"
                                                on:click=do_clone
                                            >
                                                <span class="material-icons" style="font-size:15px">"content_copy"</span>
                                            </button>

                                            {move || {
                                                if confirm_delete.get().as_deref() == Some(&id_confirm) {
                                                    view! {
                                                        <span class="rule-confirm-delete">
                                                            "Delete? "
                                                            <button
                                                                class="hc-btn hc-btn--sm hc-btn--danger"
                                                                on:click=do_delete.clone()
                                                            >"Yes"</button>
                                                            " "
                                                            <button
                                                                class="hc-btn hc-btn--sm hc-btn--outline"
                                                                on:click=move |_| confirm_delete.set(None)
                                                            >"No"</button>
                                                        </span>
                                                    }.into_any()
                                                } else {
                                                    let id_set = id_confirm.clone();
                                                    view! {
                                                        <button
                                                            class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline"
                                                            title="Delete rule"
                                                            on:click=move |_| confirm_delete.set(Some(id_set.clone()))
                                                        >
                                                            <span class="material-icons" style="font-size:15px">"delete"</span>
                                                        </button>
                                                    }.into_any()
                                                }
                                            }}
                                        </div>
                                    </div>
                                </div>
                            }
                        }
                    }
                />
            </div>
        </div>
    }
}
