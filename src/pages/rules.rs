//! Rules page — list of automation rules with filters and inline operations.

use crate::api::{
    clone_rule, create_rule_group, delete_rule, delete_rule_group, fetch_rule_groups, fetch_rules,
    patch_rule, rule_group_action, rule_stale_refs,
};
use crate::auth::use_auth;
use crate::models::{Rule, RuleGroup};
use crate::pages::shared::{
    ErrorBanner, SkeletonRows,
    json_str_set, load_pref_json, ls_set, set_to_json_array,
    MultiSelectDropdown, ResetFiltersButton, SearchField,
    SortDir, SortDirToggle, SortSelect,
};
use hc_types::rule::Trigger;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use serde_json::{json, Value};
use std::collections::HashSet;

const RULES_PREFS_KEY: &str = "hc-leptos:rules:prefs";

/// A rule that references a deleted device.
#[derive(Debug, Clone, serde::Deserialize)]
struct StaleRef {
    rule_name: String,
    rule_id: String,
    #[serde(default)]
    stale_device_ids: Vec<String>,
}

// ── Trigger type helpers ──────────────────────────────────────────────────────

fn trigger_type(rule: &Rule) -> &'static str {
    match &rule.trigger {
        Trigger::DeviceStateChanged { .. } => "device_state_changed",
        Trigger::DeviceAvailabilityChanged { .. } => "device_availability_changed",
        Trigger::ButtonEvent { .. } => "button_event",
        Trigger::NumericThreshold { .. } => "numeric_threshold",
        Trigger::TimeOfDay { .. } => "time_of_day",
        Trigger::SunEvent { .. } => "sun_event",
        Trigger::Cron { .. } => "cron",
        Trigger::Periodic { .. } => "periodic",
        Trigger::CalendarEvent { .. } => "calendar_event",
        Trigger::CustomEvent { .. } => "custom_event",
        Trigger::SystemStarted => "system_started",
        Trigger::HubVariableChanged { .. } => "hub_variable_changed",
        Trigger::ModeChanged { .. } => "mode_changed",
        Trigger::WebhookReceived { .. } => "webhook_received",
        Trigger::ManualTrigger => "manual_trigger",
        Trigger::MqttMessage { .. } => "mqtt_message",
        Trigger::DeviceBatteryLow { .. } => "device_battery_low",
        Trigger::DeviceBatteryRecovered { .. } => "device_battery_recovered",
    }
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
        "device_battery_low" => "Battery Low",
        "device_battery_recovered" => "Battery Recovered",
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
        "device_battery_low" | "device_battery_recovered" => "tone-alert",
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
        "device_battery_low" | "device_battery_recovered" => "device",
        "manual_trigger" => "manual",
        _ => "other",
    }
}

// ── Rule field accessors ──────────────────────────────────────────────────────

fn rule_id(r: &Rule) -> String {
    r.id.to_string()
}

fn rule_name(r: &Rule) -> String {
    if r.name.is_empty() {
        "(unnamed)".to_string()
    } else {
        r.name.clone()
    }
}

fn rule_enabled(r: &Rule) -> bool {
    r.enabled
}

fn rule_priority(r: &Rule) -> i64 {
    r.priority as i64
}

fn rule_error(r: &Rule) -> Option<String> {
    r.error.clone()
}

fn rule_tags(r: &Rule) -> Vec<String> {
    r.tags.clone()
}

// ── Sort keys ────────────────────────────────────────────────────────────────

/// Default sort: priority descending, then name ascending.
fn sort_rules_default(rules: &mut [Rule]) {
    rules.sort_by(|a, b| {
        b.priority.cmp(&a.priority).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortKey { Name, Priority }

fn sort_key_from_str(s: Option<&str>) -> SortKey {
    match s { Some("priority") => SortKey::Priority, _ => SortKey::Name }
}
fn sort_key_to_str(k: SortKey) -> &'static str {
    match k { SortKey::Name => "name", SortKey::Priority => "priority" }
}

fn sort_options() -> Vec<(String, String)> {
    vec![("name".into(), "Name".into()), ("priority".into(), "Priority".into())]
}

// ── Page ──────────────────────────────────────────────────────────────────────

#[component]
pub fn RulesPage() -> impl IntoView {
    let auth = use_auth();
    let navigate = use_navigate();

    // ── State ─────────────────────────────────────────────────────────────────
    let rules: RwSignal<Vec<Rule>> = RwSignal::new(vec![]);
    let loading = RwSignal::new(true);
    let page_error: RwSignal<Option<String>> = RwSignal::new(None);

    let prefs = load_pref_json(RULES_PREFS_KEY).unwrap_or(Value::Null);
    let search = RwSignal::new(prefs["search"].as_str().unwrap_or("").to_string());
    let status_filter: RwSignal<HashSet<String>> = RwSignal::new(json_str_set(&prefs, "status"));
    let trigger_filter: RwSignal<HashSet<String>> = RwSignal::new(json_str_set(&prefs, "trigger"));
    let tag_filter: RwSignal<HashSet<String>> = RwSignal::new(json_str_set(&prefs, "tags"));
    let sort_by = RwSignal::new(sort_key_from_str(prefs["sort"].as_str()));
    let sort_dir = RwSignal::new(if prefs["sort_dir"].as_str() == Some("desc") { SortDir::Desc } else { SortDir::Asc });


    let confirm_delete: RwSignal<Option<String>> = RwSignal::new(None);
    let row_busy: RwSignal<Option<String>> = RwSignal::new(None);
    let stale_refs: RwSignal<Vec<StaleRef>> = RwSignal::new(vec![]);
    let selected: RwSignal<Vec<String>> = RwSignal::new(vec![]);
    let bulk_busy = RwSignal::new(false);

    // ── Group state ──────────────────────────────────────────────────────────
    let groups: RwSignal<Vec<RuleGroup>> = RwSignal::new(vec![]);
    let active_group: RwSignal<Option<String>> = RwSignal::new(None);
    let groups_expanded = RwSignal::new(false);

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
                    sort_rules_default(&mut data);
                    rules.set(data);
                }
                Err(e) => page_error.set(Some(e)),
            }
            loading.set(false);
        });

        // Also fetch stale refs and groups in parallel
        let token_for_groups = token_for_stale.clone();
        spawn_local(async move {
            if let Ok(data) = rule_stale_refs(&token_for_stale).await {
                if let Ok(refs) = serde_json::from_value::<Vec<StaleRef>>(data) {
                    stale_refs.set(refs);
                }
            }
        });
        spawn_local(async move {
            if let Ok(data) = fetch_rule_groups(&token_for_groups).await {
                groups.set(data);
            }
        });
    });

    // Persist filter prefs on change.
    Effect::new(move |_| {
        let v = json!({
            "search":   search.get(),
            "status":   set_to_json_array(&status_filter.get()),
            "trigger":  set_to_json_array(&trigger_filter.get()),
            "tags":     set_to_json_array(&tag_filter.get()),
            "sort":     sort_key_to_str(sort_by.get()),
            "sort_dir": if sort_dir.get() == SortDir::Desc { "desc" } else { "asc" },
        });
        ls_set(RULES_PREFS_KEY, &v.to_string());
    });

    // ── Dynamic filter options (computed from loaded rules) ─────────────────
    let status_options: Signal<Vec<(String, String)>> = Signal::derive(move || {
        vec![("active".into(), "Active".into()), ("disabled".into(), "Disabled".into())]
    });
    let trigger_options: Signal<Vec<(String, String)>> = Signal::derive(move || {
        vec![
            ("device".into(), "Device".into()),
            ("time".into(), "Time".into()),
            ("event".into(), "Event".into()),
            ("manual".into(), "Manual".into()),
        ]
    });
    let tag_options: Signal<Vec<(String, String)>> = Signal::derive(move || {
        let mut tags: Vec<String> = rules.get().iter()
            .flat_map(|r| rule_tags(r))
            .collect::<HashSet<_>>()
            .into_iter().collect();
        tags.sort();
        tags.into_iter().map(|t| (t.clone(), t)).collect()
    });

    // ── Filtered + sorted list ───────────────────────────────────────────────
    let filtered = Memo::new(move |_| {
        let q = search.get().to_lowercase();
        let st = status_filter.get();
        let tr = trigger_filter.get();
        let tf = tag_filter.get();
        let sk = sort_by.get();
        let sd = sort_dir.get();
        let grp = active_group.get();
        let grp_ids: Option<HashSet<String>> = grp.as_ref().and_then(|gid| {
            groups.get().iter().find(|g| g.id == *gid).map(|g| {
                g.rule_ids.iter().cloned().collect()
            })
        });
        let mut list: Vec<Rule> = rules
            .get()
            .into_iter()
            .filter(|r| {
                // Group filter
                if let Some(ref ids) = grp_ids {
                    if !ids.contains(&r.id.to_string()) { return false; }
                }
                let name = r.name.to_lowercase();
                let tags = rule_tags(r);
                let tags_lower = tags.join(" ").to_lowercase();
                if !q.is_empty() && !name.contains(&q) && !tags_lower.contains(&q) {
                    return false;
                }
                if !st.is_empty() {
                    let status = if rule_enabled(r) { "active" } else { "disabled" };
                    if !st.contains(status) { return false; }
                }
                if !tr.is_empty() {
                    let cat = trigger_category(trigger_type(r)).to_string();
                    if !tr.contains(&cat) { return false; }
                }
                if !tf.is_empty() {
                    if !tags.iter().any(|t| tf.contains(t)) { return false; }
                }
                true
            })
            .collect();
        list.sort_by(|a, b| {
            let cmp = match sk {
                SortKey::Priority => b.priority.cmp(&a.priority),
                SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            };
            if sd == SortDir::Desc { cmp.reverse() } else { cmp }
        });
        list
    });

    // ── Toolbar navigate clone ────────────────────────────────────────────────
    let nav_new = navigate.clone();

    view! {
        <div class="rules-page">
            // ── Page heading ─────────────────────────────────────────────────
            <div class="page-heading">
                <div>
                    <h1>"Rules"</h1>
                    <p>
                        {move || {
                            let f = filtered.get().len();
                            let t = rules.get().len();
                            if f == t { format!("{t} rules") } else { format!("{f} / {t} rules") }
                        }}
                    </p>
                </div>
                <button
                    class="hc-btn hc-btn--primary"
                    on:click=move |_| nav_new("/rules/new", Default::default())
                >"+ New Rule"</button>
            </div>

            // ── Rule Groups ──────────────────────────────────────────────────
            <RuleGroupsPanel groups=groups active_group=active_group expanded=groups_expanded />

            // ── Filter/sort toolbar ──────────────────────────────────────────
            <div class="filter-panel panel">
                <div class="filter-bar">
                    <SearchField search=search placeholder="Search name, tags…" />

                    <SortSelect
                        current_value=Signal::derive(move || sort_key_to_str(sort_by.get()).to_string())
                        options=Signal::derive(sort_options)
                        on_change=Callback::new(move |v: String| sort_by.set(sort_key_from_str(Some(&v))))
                    />
                    <SortDirToggle sort_dir />
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
                            label="triggers"
                            placeholder="All triggers"
                            options=trigger_options
                            selected=trigger_filter
                        />
                        <MultiSelectDropdown
                            label="tags"
                            placeholder="All tags"
                            options=tag_options
                            selected=tag_filter
                        />
                        <ResetFiltersButton on_reset=Callback::new(move |_| {
                            search.set(String::new());
                            status_filter.set(HashSet::new());
                            trigger_filter.set(HashSet::new());
                            tag_filter.set(HashSet::new());
                            sort_by.set(SortKey::Name);
                            sort_dir.set(SortDir::Asc);
                        }) />
                    </div>
                </div>
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
                                            sort_rules_default(&mut data);
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
                                            sort_rules_default(&mut data);
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
            <ErrorBanner error=page_error />

            // ── Stale-ref warnings ───────────────────────────────────────────
            {move || {
                let refs = stale_refs.get();
                (!refs.is_empty()).then(|| view! {
                    <div class="stale-refs-banner">
                        <i class="ph ph-warning" style="font-size:16px; vertical-align:middle"></i>
                        {format!(" {} rule(s) reference deleted devices:", refs.len())}
                        <ul class="stale-refs-list">
                            {refs.into_iter().map(|entry| {
                                let rule_name = entry.rule_name.clone();
                                let rule_id = entry.rule_id.clone();
                                let stale = entry.stale_device_ids.join(", ");
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
            {move || loading.get().then(|| view! { <SkeletonRows count=8 /> })}

            // ── Empty ─────────────────────────────────────────────────────────
            {move || {
                let list = filtered.get();
                (!loading.get() && list.is_empty()).then(|| view! {
                    <div class="hc-empty">
                        <i class="ph ph-robot hc-empty__icon"></i>
                        <div class="hc-empty__title">"No rules"</div>
                        <p class="hc-empty__body">
                            "Rules react to device state, time, and events. Try clearing filters, \
                             or create a new rule to automate your home."
                        </p>
                    </div>
                })
            }}

            // ── Rule list ─────────────────────────────────────────────────────
            <div class="rules-list">
                <For
                    each=move || filtered.get()
                    key=|r| r.id.to_string()
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
                                        Ok(_) => rules.update(|list| {
                                            if let Some(r) = list.iter_mut().find(|r| rule_id(r) == id) {
                                                r.enabled = !enabled;
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
                                            <i class="ph ph-warning-circle" style="font-size:14px;vertical-align:middle"></i>
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
                                                                    tag_filter.update(|s| { s.insert(t2.clone()); });
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
                                                <i class="ph ph-copy" style="font-size:15px"></i>
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
                                                            <i class="ph ph-trash" style="font-size:15px"></i>
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

// ── Rule Groups Panel ───────────────────────────────────────────────────────

#[component]
fn RuleGroupsPanel(
    groups: RwSignal<Vec<RuleGroup>>,
    active_group: RwSignal<Option<String>>,
    expanded: RwSignal<bool>,
) -> impl IntoView {
    let auth = use_auth();
    let busy = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let new_name = RwSignal::new(String::new());
    let _editing_group: RwSignal<Option<String>> = RwSignal::new(None);

    let refresh_groups = move || {
        let token = auth.token_str().unwrap_or_default();
        spawn_local(async move {
            if let Ok(data) = fetch_rule_groups(&token).await {
                groups.set(data);
            }
        });
    };

    view! {
        <div class="rule-groups-panel">
            <div class="rule-groups-header">
                <button
                    class="hc-btn hc-btn--sm hc-btn--outline"
                    on:click=move |_| expanded.update(|v| *v = !*v)
                >
                    <i class=move || if expanded.get() { "ph ph-caret-up" } else { "ph ph-caret-down" } style="font-size:16px"></i>
                    " Groups "
                    <span class="cell-subtle">
                        {move || {
                            let g = groups.get();
                            if g.is_empty() { String::new() } else { format!("({})", g.len()) }
                        }}
                    </span>
                </button>
                {move || active_group.get().map(|_| view! {
                    <button
                        class="hc-btn hc-btn--sm hc-btn--outline"
                        on:click=move |_| active_group.set(None)
                    >"Show All"</button>
                })}
            </div>

            {move || expanded.get().then(|| {
                let group_list = groups.get();
                view! {
                    <ErrorBanner error=error />
                    <div class="rule-groups-body">
                        {group_list.into_iter().map(|group| {
                            let gid = group.id.clone();
                            let gid_select = gid.clone();
                            let gid_enable = gid.clone();
                            let gid_disable = gid.clone();
                            let gid_delete = gid.clone();
                            let name = group.name.clone();
                            let desc = group.description.clone().unwrap_or_default();
                            let rule_count = group.rule_ids.len();
                            let is_active = active_group.get().as_deref() == Some(gid.as_str());

                            view! {
                                <div class="rule-group-chip" class:rule-group-chip--active=is_active>
                                    <button
                                        class="rule-group-chip-name"
                                        on:click=move |_| {
                                            if active_group.get().as_deref() == Some(gid_select.as_str()) {
                                                active_group.set(None);
                                            } else {
                                                active_group.set(Some(gid_select.clone()));
                                            }
                                        }
                                    >
                                        {name}
                                        <span class="cell-subtle">{format!(" ({rule_count})")}</span>
                                    </button>
                                    {(!desc.is_empty()).then(|| {
                                        let desc2 = desc.clone();
                                        view! {
                                            <span class="cell-subtle rule-group-desc" title=desc>{desc2}</span>
                                        }
                                    })}
                                    <div class="rule-group-actions">
                                        <button
                                            class="hc-btn hc-btn--sm hc-btn--outline"
                                            title="Enable all rules in group"
                                            disabled=move || busy.get()
                                            on:click=move |_| {
                                                let token = auth.token_str().unwrap_or_default();
                                                let id = gid_enable.clone();
                                                busy.set(true);
                                                spawn_local(async move {
                                                    match rule_group_action(&token, &id, "enable").await {
                                                        Ok(_) => {}
                                                        Err(e) => error.set(Some(e)),
                                                    }
                                                    busy.set(false);
                                                });
                                            }
                                        >
                                            <i class="ph ph-play" style="font-size:14px"></i>
                                        </button>
                                        <button
                                            class="hc-btn hc-btn--sm hc-btn--outline"
                                            title="Disable all rules in group"
                                            disabled=move || busy.get()
                                            on:click=move |_| {
                                                let token = auth.token_str().unwrap_or_default();
                                                let id = gid_disable.clone();
                                                busy.set(true);
                                                spawn_local(async move {
                                                    match rule_group_action(&token, &id, "disable").await {
                                                        Ok(_) => {}
                                                        Err(e) => error.set(Some(e)),
                                                    }
                                                    busy.set(false);
                                                });
                                            }
                                        >
                                            <i class="ph ph-pause" style="font-size:14px"></i>
                                        </button>
                                        <button
                                            class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline"
                                            title="Delete group"
                                            disabled=move || busy.get()
                                            on:click=move |_| {
                                                let token = auth.token_str().unwrap_or_default();
                                                let id = gid_delete.clone();
                                                busy.set(true);
                                                spawn_local(async move {
                                                    match delete_rule_group(&token, &id).await {
                                                        Ok(()) => refresh_groups(),
                                                        Err(e) => error.set(Some(e)),
                                                    }
                                                    if active_group.get_untracked().as_deref() == Some(id.as_str()) {
                                                        active_group.set(None);
                                                    }
                                                    busy.set(false);
                                                });
                                            }
                                        >
                                            <i class="ph ph-trash" style="font-size:14px"></i>
                                        </button>
                                    </div>
                                </div>
                            }
                        }).collect_view()}

                        // ── Create new group ─────────────────────────────────
                        <div class="rule-group-create">
                            <input
                                class="input"
                                type="text"
                                prop:value=move || new_name.get()
                                on:input=move |ev| new_name.set(event_target_value(&ev))
                                placeholder="New group name…"
                            />
                            <button
                                class="hc-btn hc-btn--sm hc-btn--primary"
                                disabled=move || busy.get() || new_name.get().trim().is_empty()
                                on:click=move |_| {
                                    let token = auth.token_str().unwrap_or_default();
                                    let name = new_name.get();
                                    busy.set(true);
                                    error.set(None);
                                    spawn_local(async move {
                                        match create_rule_group(&token, &name, None, &[]).await {
                                            Ok(_) => {
                                                new_name.set(String::new());
                                                refresh_groups();
                                            }
                                            Err(e) => error.set(Some(e)),
                                        }
                                        busy.set(false);
                                    });
                                }
                            >"Create"</button>
                        </div>
                    </div>
                }
            })}
        </div>
    }
}
