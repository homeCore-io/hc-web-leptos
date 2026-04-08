//! Rule editor pages — create a new rule or edit an existing one.
//!
//! Architecture:
//!   One `RwSignal<Rule>` holds the complete typed rule.  The metadata section
//!   (name, enabled, priority, tags, etc.) uses typed field access directly.
//!
//!   Sub-editors (trigger, conditions, actions) still use `RwSignal<Value>`
//!   via a bridge layer and will be converted to typed signals incrementally.
//!
//!   Reference data (devices, areas, scenes, modes) is fetched once on page load
//!   and provided as read-only signals for searchable dropdowns.

use crate::api::{
    clone_rule, create_rule, delete_rule, fetch_areas, fetch_devices, fetch_modes,
    fetch_rule, fetch_rules, fetch_scenes, rule_fire_history, test_rule, update_rule,
};
use crate::auth::use_auth;
use crate::models::{
    is_media_player, is_scene_like, is_timer_device,
    media_available_favorites, media_available_playlists,
    Area, DeviceState, ModeRecord, Rule, RunMode, Scene, Trigger,
};
use hc_types::rule::{
    ButtonEventType, CompareOp, Condition, PeriodicUnit, SunEventType, ThresholdOp,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_params_map};
use serde_json::{json, Value};
use uuid::Uuid;

// ── Defaults ─────────────────────────────────────────────────────────────────

fn default_rule() -> Rule {
    use hc_types::rule::Trigger;
    Rule {
        id: Uuid::nil(),
        name: String::new(),
        enabled: true,
        priority: 0,
        tags: vec![],
        trigger: Trigger::ManualTrigger,
        conditions: vec![],
        actions: vec![],
        error: None,
        cooldown_secs: None,
        log_events: false,
        log_triggers: false,
        log_actions: false,
        required_expression: None,
        cancel_on_false: false,
        trigger_condition: None,
        variables: Default::default(),
        trigger_label: None,
        run_mode: RunMode::Parallel,
    }
}

fn default_trigger(t: &str) -> Value {
    match t {
        "device_state_changed" => json!({"type":"device_state_changed","device_id":""}),
        "device_availability_changed" => json!({"type":"device_availability_changed","device_id":""}),
        "button_event" => json!({"type":"button_event","device_id":"","event":"pushed"}),
        "numeric_threshold" => json!({"type":"numeric_threshold","device_id":"","attribute":"","op":"crosses_above","value":0.0}),
        "time_of_day" => json!({"type":"time_of_day","time":"08:00:00","days":["Mon","Tue","Wed","Thu","Fri","Sat","Sun"]}),
        "sun_event" => json!({"type":"sun_event","event":"sunset","offset_minutes":0}),
        "cron" => json!({"type":"cron","expression":"0 0 8 * * *"}),
        "periodic" => json!({"type":"periodic","every_n":15,"unit":"minutes"}),
        "calendar_event" => json!({"type":"calendar_event","offset_minutes":0}),
        "custom_event" => json!({"type":"custom_event","event_type":""}),
        "system_started" => json!({"type":"system_started"}),
        "hub_variable_changed" => json!({"type":"hub_variable_changed"}),
        "mode_changed" => json!({"type":"mode_changed"}),
        "webhook_received" => json!({"type":"webhook_received","path":"/hooks/"}),
        "mqtt_message" => json!({"type":"mqtt_message","topic_pattern":"homecore/devices/+/state"}),
        _ => json!({"type":"manual_trigger"}),
    }
}

fn default_condition(t: &str) -> Value {
    match t {
        "device_state" => json!({"type":"device_state","device_id":"","attribute":"on","op":"eq","value":true}),
        "time_window" => json!({"type":"time_window","start":"08:00:00","end":"22:00:00"}),
        "script_expression" => json!({"type":"script_expression","script":""}),
        "time_elapsed" => json!({"type":"time_elapsed","device_id":"","attribute":"","duration_secs":60}),
        "device_last_change" => json!({"type":"device_last_change","device_id":""}),
        "private_boolean_is" => json!({"type":"private_boolean_is","name":"","value":true}),
        "hub_variable" => json!({"type":"hub_variable","name":"","op":"eq","value":""}),
        "mode_is" => json!({"type":"mode_is","mode_id":"","on":true}),
        "or"  => json!({"type":"or","conditions":[]}),
        "and" => json!({"type":"and","conditions":[]}),
        "not" => json!({"type":"not","condition":{"type":"device_state","device_id":"","attribute":"on","op":"eq","value":true}}),
        "xor" => json!({"type":"xor","conditions":[]}),
        _ => json!({"type": t}),
    }
}

fn default_action(t: &str) -> Value {
    let mut base = match t {
        "set_device_state"   => json!({"type":"set_device_state","device_id":"","state":{}}),
        "publish_mqtt"       => json!({"type":"publish_mqtt","topic":"","payload":"","retain":false}),
        "call_service"       => json!({"type":"call_service","url":"","method":"POST","body":{}}),
        "fire_event"         => json!({"type":"fire_event","event_type":"","payload":{}}),
        "run_script"         => json!({"type":"run_script","script":""}),
        "notify"             => json!({"type":"notify","channel":"","message":""}),
        "delay"              => json!({"type":"delay","duration_secs":5,"cancelable":false}),
        "set_variable"       => json!({"type":"set_variable","name":"","value":""}),
        "set_hub_variable"   => json!({"type":"set_hub_variable","name":"","value":""}),
        "set_mode"           => json!({"type":"set_mode","mode_id":"","command":"on"}),
        "run_rule_actions"   => json!({"type":"run_rule_actions","rule_id":""}),
        "pause_rule"         => json!({"type":"pause_rule","rule_id":""}),
        "resume_rule"        => json!({"type":"resume_rule","rule_id":""}),
        "cancel_delays"      => json!({"type":"cancel_delays"}),
        "cancel_rule_timers" => json!({"type":"cancel_rule_timers"}),
        "set_private_boolean"=> json!({"type":"set_private_boolean","name":"","value":true}),
        "log_message"        => json!({"type":"log_message","message":""}),
        "comment"            => json!({"type":"comment","text":""}),
        "stop_rule_chain"    => json!({"type":"stop_rule_chain"}),
        "exit_rule"          => json!({"type":"exit_rule"}),
        "wait_for_event"     => json!({"type":"wait_for_event"}),
        "wait_for_expression"=> json!({"type":"wait_for_expression","expression":""}),
        "capture_device_state"  => json!({"type":"capture_device_state","key":"","device_ids":[]}),
        "restore_device_state"  => json!({"type":"restore_device_state","key":""}),
        "fade_device"        => json!({"type":"fade_device","device_id":"","target":{},"duration_secs":30}),
        "parallel"           => json!({"type":"parallel","actions":[]}),
        "conditional"        => json!({"type":"conditional","condition":"","then_actions":[],"else_actions":[]}),
        "repeat_until"       => json!({"type":"repeat_until","condition":"","actions":[],"max_iterations":10}),
        "repeat_while"       => json!({"type":"repeat_while","condition":"","actions":[],"max_iterations":10}),
        "repeat_count"       => json!({"type":"repeat_count","count":3,"actions":[]}),
        "ping_host"          => json!({"type":"ping_host","host":"","then_actions":[],"else_actions":[]}),
        "set_device_state_per_mode" => json!({"type":"set_device_state_per_mode","device_id":"","modes":[]}),
        "delay_per_mode"     => json!({"type":"delay_per_mode","modes":[]}),
        "activate_scene_per_mode" => json!({"type":"activate_scene_per_mode","modes":[]}),
        _                    => json!({"type":"log_message","message":""}),
    };
    base["enabled"] = json!(true);
    base
}

// ── Bridge: merge typed metadata with sub-editor Value data for save ────────

/// Build a Rule for saving by taking metadata + trigger + conditions from the
/// typed signal and actions from the Value-based sub-editor signal.
fn build_save_rule(rule: RwSignal<Rule>, rule_json: RwSignal<Value>) -> Rule {
    let mut r = rule.get_untracked();
    // Actions still use the Value-based sub-editor (rule_json).
    // Merge actions back from JSON bridge.
    let json = rule_json.get_untracked();
    if !json.is_null() {
        if let Ok(bridge) = serde_json::from_value::<Rule>(json) {
            r.actions = bridge.actions;
        }
    }
    r
}

// ── JSON field helpers ───────────────────────────────────────────────────────
// These operate on a RwSignal<Value> at any path depth.
// Used by sub-editors (trigger, condition, action) that still work with Value.
// Will be removed as each sub-editor is converted to typed signals.

fn jset(sig: RwSignal<Value>, path: &[&str], val: Value) {
    sig.update(|root| {
        let mut target = &mut *root;
        for &key in &path[..path.len() - 1] {
            target = &mut target[key];
        }
        if let Some(&last) = path.last() {
            target[last] = val;
        }
    });
}

fn jset_opt(sig: RwSignal<Value>, path: &[&str], raw: &str) {
    sig.update(|root| {
        let mut target = &mut *root;
        for &key in &path[..path.len() - 1] {
            target = &mut target[key];
        }
        if let Some(&last) = path.last() {
            if raw.trim().is_empty() {
                if let Some(obj) = target.as_object_mut() { obj.remove(last); }
            } else {
                let parsed = serde_json::from_str::<Value>(raw.trim())
                    .unwrap_or_else(|_| json!(raw.trim()));
                target[last] = parsed;
            }
        }
    });
}

fn jget_str(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_string()
}

fn jget_opt_str(v: &Value, key: &str) -> String {
    let val = &v[key];
    if val.is_null() { String::new() } else if let Some(s) = val.as_str() { s.to_string() } else { val.to_string() }
}

fn jget_u64_str(v: &Value, key: &str) -> String {
    v[key].as_u64().map(|n| n.to_string()).unwrap_or_default()
}

fn jget_i64_str(v: &Value, key: &str) -> String {
    v[key].as_i64().map(|n| n.to_string()).unwrap_or_default()
}

// ── Route entry points ───────────────────────────────────────────────────────

#[component]
pub fn NewRulePage() -> impl IntoView {
    view! { <RuleEditorPage id=None /> }
}

#[component]
pub fn EditRulePage() -> impl IntoView {
    let params = use_params_map();
    let id = Signal::derive(move || params.read().get("id").unwrap_or_default());
    view! { <RuleEditorPage id=Some(id) /> }
}

// ── Editor ───────────────────────────────────────────────────────────────────

#[component]
fn RuleEditorPage(id: Option<Signal<String>>) -> impl IntoView {
    let auth    = use_auth();
    let is_new  = id.is_none();
    let rule: RwSignal<Rule> = RwSignal::new(default_rule());
    // Bridge signal for sub-editors still using Value (trigger, conditions, actions).
    // Synced from `rule` on load; written back on save.
    let rule_json: RwSignal<Value> = RwSignal::new(
        serde_json::to_value(&default_rule()).unwrap_or_default()
    );
    let loading = RwSignal::new(!is_new);
    let saving  = RwSignal::new(false);
    let save_err: RwSignal<Option<String>> = RwSignal::new(None);
    let save_ok  = RwSignal::new(false);
    let advanced_open  = RwSignal::new(false);
    let confirm_delete = RwSignal::new(false);
    let tag_input: RwSignal<String> = RwSignal::new(String::new());

    // Test run + fire history panels (edit mode only)
    let test_loading   = RwSignal::new(false);
    let test_result: RwSignal<Option<Value>> = RwSignal::new(None);
    let test_err: RwSignal<Option<String>> = RwSignal::new(None);
    let history_loading = RwSignal::new(false);
    let history_data: RwSignal<Option<Value>> = RwSignal::new(None);
    let history_err: RwSignal<Option<String>> = RwSignal::new(None);
    let history_open   = RwSignal::new(false);

    // Reference data (fetched once, provided as context for sub-components).
    let devices: RwSignal<Vec<DeviceState>> = RwSignal::new(vec![]);
    let areas: RwSignal<Vec<Area>> = RwSignal::new(vec![]);
    let scenes: RwSignal<Vec<Scene>> = RwSignal::new(vec![]);
    let modes: RwSignal<Vec<ModeRecord>> = RwSignal::new(vec![]);
    let all_rules: RwSignal<Vec<crate::models::Rule>> = RwSignal::new(vec![]);
    provide_context(devices);
    provide_context(areas);
    provide_context(scenes);
    provide_context(modes);
    provide_context(all_rules);

    // ── Load rule (edit mode) + reference data ───────────────────────────────
    Effect::new(move |_| {
        let token = match auth.token.get() { Some(t) => t, None => return };
        // Fetch reference data.
        let t2 = token.clone();
        let t3 = token.clone();
        let t4 = token.clone();
        spawn_local(async move { if let Ok(d) = fetch_devices(&t2).await { devices.set(d); } });
        spawn_local(async move { if let Ok(a) = fetch_areas(&t3).await { areas.set(a); } });
        spawn_local(async move { if let Ok(s) = fetch_scenes(&t4).await { scenes.set(s); } });
        {
            let t5 = token.clone();
            spawn_local(async move { if let Ok(m) = fetch_modes(&t5).await { modes.set(m); } });
        }
        {
            let t6 = token.clone();
            spawn_local(async move {
                if let Ok(r) = fetch_rules(&t6).await { all_rules.set(r); }
            });
        }
        // Fetch rule (edit mode).
        if let Some(id_sig) = id {
            let rule_id = id_sig.get();
            if rule_id.is_empty() { return; }
            spawn_local(async move {
                match fetch_rule(&token, &rule_id).await {
                    Ok(r) => {
                        rule_json.set(serde_json::to_value(&r).unwrap_or_default());
                        rule.set(r);
                        loading.set(false);
                    }
                    Err(e) => { save_err.set(Some(format!("Load failed: {e}"))); loading.set(false); }
                }
            });
        }
    });

    // ── Tag commit ───────────────────────────────────────────────────────────
    let commit_tag = move || {
        let raw = tag_input.get_untracked().trim().to_string();
        if raw.is_empty() { return; }
        rule.update(|r| {
            if !r.tags.contains(&raw) { r.tags.push(raw); }
        });
        tag_input.set(String::new());
    };

    let navigate = use_navigate();

    view! {
        <div class="rule-editor">

            // ── Heading ──────────────────────────────────────────────────────
            <div class="detail-heading">
                <div class="detail-heading-actions">
                    {
                        let nav = navigate.clone();
                        view! {
                            <button class="hc-btn hc-btn--outline"
                                on:click=move |_| nav("/rules", Default::default())
                            >"← Rules"</button>
                        }
                    }
                    <h2 style="flex:1; margin:0; font-size:1.1rem">
                        {if is_new { "New Rule" } else { "Edit Rule" }}
                    </h2>
                </div>
            </div>

            // ── Status banners ───────────────────────────────────────────────
            {move || save_err.get().map(|e| view! { <p class="msg-error">{e}</p> })}
            {move || save_ok.get().then(|| view! {
                <p class="msg-ok save-ok-banner">
                    <span class="material-icons" style="font-size:16px;vertical-align:middle">"check_circle"</span>
                    " Saved successfully"
                </p>
            })}
            {move || loading.get().then(|| view! { <p class="msg-muted">"Loading…"</p> })}

            // ── Editor (hidden while loading) ────────────────────────────────
            <Show when=move || !loading.get()>

                // ── Rule header: name + top action bar ───────────────────────
                <section class="detail-card">
                    <div class="rule-header-row">
                        <input type="text" class="hc-input rule-name-input" placeholder="Rule name"
                            prop:value=move || rule.get().name.clone()
                            on:input=move |ev| {
                                let v = event_target_value(&ev);
                                rule.update(|r| r.name = v);
                            }
                        />
                        <div class="rule-header-actions">
                            {
                                let nav_save_top = navigate.clone();
                                let nav_cancel_top = navigate.clone();
                                let nav_clone_top = navigate.clone();
                                let nav_delete_top = navigate.clone();
                                view! {
                                    <button class="hc-btn hc-btn--primary hc-btn--sm"
                                        disabled=move || saving.get()
                                        on:click=move |_| {
                                            let token = match auth.token.get_untracked() { Some(t) => t, None => return };
                                            let typed = build_save_rule(rule, rule_json);
                                            if typed.name.trim().is_empty() { save_err.set(Some("Rule name is required.".into())); return; }
                                            save_err.set(None); save_ok.set(false); saving.set(true);
                                            let nav = nav_save_top.clone();
                                            let rule_id_str = id.map(|s| s.get_untracked()).unwrap_or_default();
                                            spawn_local(async move {
                                                let result = if rule_id_str.is_empty() { create_rule(&token, &typed).await }
                                                             else { update_rule(&token, &rule_id_str, &typed).await };
                                                match result {
                                                    Ok(saved) => {
                                                        if rule_id_str.is_empty() {
                                                            let new_id = saved.id.to_string();
                                                            if !new_id.is_empty() { nav(&format!("/rules/{new_id}"), Default::default()); }
                                                        } else {
                                                            rule_json.set(serde_json::to_value(&saved).unwrap_or_default());
                                                            rule.set(saved);
                                                            save_ok.set(true);
                                                            spawn_local(async move {
                                                                gloo_timers::future::TimeoutFuture::new(3000).await;
                                                                save_ok.set(false);
                                                            });
                                                        }
                                                    }
                                                    Err(e) => save_err.set(Some(e)),
                                                }
                                                saving.set(false);
                                            });
                                        }
                                    >{move || if saving.get() { "Saving…" } else { "Save" }}</button>
                                    <button class="hc-btn hc-btn--outline hc-btn--sm"
                                        disabled=move || saving.get()
                                        on:click=move |_| nav_cancel_top("/rules", Default::default())
                                    >"Cancel"</button>
                                    {(!is_new).then(|| {
                                        let nc = nav_clone_top.clone();
                                        let nd = nav_delete_top.clone();
                                        view! {
                                            <button class="hc-btn hc-btn--outline hc-btn--sm" title="Clone"
                                                disabled=move || saving.get()
                                                on:click=move |_| {
                                                    let token = match auth.token.get_untracked() { Some(t) => t, None => return };
                                                    let rule_id = id.map(|s| s.get_untracked()).unwrap_or_default();
                                                    if rule_id.is_empty() { return; }
                                                    let nav = nc.clone();
                                                    saving.set(true);
                                                    spawn_local(async move {
                                                        match clone_rule(&token, &rule_id).await {
                                                            Ok(new_rule) => {
                                                                let new_id = new_rule.id.to_string();
                                                                if !new_id.is_empty() { nav(&format!("/rules/{new_id}"), Default::default()); }
                                                            }
                                                            Err(e) => save_err.set(Some(e)),
                                                        }
                                                        saving.set(false);
                                                    });
                                                }
                                            >
                                                <span class="material-icons" style="font-size:14px;vertical-align:middle">"content_copy"</span>
                                            </button>
                                            <button class="hc-btn hc-btn--outline hc-btn--sm hc-btn--danger-outline" title="Delete"
                                                disabled=move || saving.get()
                                                on:click=move |_| {
                                                    let token = match auth.token.get_untracked() { Some(t) => t, None => return };
                                                    let rule_id = id.map(|s| s.get_untracked()).unwrap_or_default();
                                                    let nav = nd.clone();
                                                    saving.set(true);
                                                    spawn_local(async move {
                                                        match delete_rule(&token, &rule_id).await {
                                                            Ok(()) => nav("/rules", Default::default()),
                                                            Err(e) => { save_err.set(Some(e)); saving.set(false); }
                                                        }
                                                    });
                                                }
                                            >
                                                <span class="material-icons" style="font-size:14px;vertical-align:middle">"delete"</span>
                                            </button>
                                        }
                                    })}
                                }
                            }
                        </div>
                    </div>

                    <div class="rule-meta-row">
                        <label class="rule-meta-inline">
                            <input type="checkbox"
                                prop:checked=move || rule.get().enabled
                                on:change=move |ev| {
                                    use wasm_bindgen::JsCast;
                                    let checked = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).map(|el| el.checked()).unwrap_or(true);
                                    rule.update(|r| r.enabled = checked);
                                }
                            />
                            " Enabled"
                        </label>

                        <label class="field-label" style="margin:0">"Priority"</label>
                        <input type="number" class="hc-input hc-input--sm" style="width:5rem"
                            prop:value=move || rule.get().priority.to_string()
                            on:input=move |ev| {
                                if let Ok(n) = event_target_value(&ev).parse::<i32>() {
                                    rule.update(|r| r.priority = n);
                                }
                            }
                        />
                    </div>

                    // ── Tags ─────────────────────────────────────────────────
                    <label class="field-label">"Tags"</label>
                    <div class="tag-input-row">
                        {move || {
                            let tags = rule.get().tags.clone();
                            tags.into_iter().enumerate().map(|(i, tag)| {
                                view! {
                                    <span class="tag-chip">
                                        {tag}
                                        <button class="tag-chip-remove"
                                            on:click=move |_| rule.update(|r| {
                                                if i < r.tags.len() { r.tags.remove(i); }
                                            })
                                        >"×"</button>
                                    </span>
                                }
                            }).collect_view()
                        }}
                        <input type="text" class="hc-input hc-input--sm tag-chip-input"
                            placeholder="Add tag, press Enter"
                            prop:value=move || tag_input.get()
                            on:input=move |ev| tag_input.set(event_target_value(&ev))
                            on:keydown=move |ev| {
                                if ev.key() == "Enter" || ev.key() == "," {
                                    ev.prevent_default();
                                    commit_tag();
                                }
                            }
                            on:blur=move |_| commit_tag()
                        />
                    </div>

                    // ── Run mode ─────────────────────────────────────────────
                    <label class="field-label">"Run mode"</label>
                    <select class="hc-select"
                        on:change=move |ev| {
                            let t = event_target_value(&ev);
                            let rm = match t.as_str() {
                                "single"  => RunMode::Single,
                                "restart" => RunMode::Restart,
                                "queued"  => RunMode::Queued { max_queue: 10 },
                                _         => RunMode::Parallel,
                            };
                            rule.update(|r| r.run_mode = rm);
                        }
                    >
                        {[("parallel","Parallel (default)"),("single","Single — skip if running"),("restart","Restart — cancel and restart"),("queued","Queued")]
                            .map(|(v, label)| view! {
                                <option value=v selected=move || {
                                    let current = match &rule.get().run_mode {
                                        RunMode::Parallel => "parallel",
                                        RunMode::Single => "single",
                                        RunMode::Restart => "restart",
                                        RunMode::Queued { .. } => "queued",
                                    };
                                    current == v
                                }>{label}</option>
                            }).collect_view()}
                    </select>

                    // ── Cooldown ─────────────────────────────────────────────
                    <label class="field-label">"Cooldown (seconds)"</label>
                    <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="None"
                        prop:value=move || rule.get().cooldown_secs.map(|n| n.to_string()).unwrap_or_default()
                        on:input=move |ev| {
                            let raw = event_target_value(&ev);
                            rule.update(|r| {
                                r.cooldown_secs = raw.trim().parse::<u64>().ok();
                            });
                        }
                    />
                </section>

                // ── Trigger ──────────────────────────────────────────────────
                <section class="detail-card">
                    <h3 class="detail-card-title">"Trigger"</h3>
                    <TriggerEditor rule=rule />
                </section>

                // ── Conditions ───────────────────────────────────────────────
                <section class="detail-card">
                    <div class="rule-section-header">
                        <h3 class="detail-card-title">"Conditions"</h3>
                        <button class="hc-btn hc-btn--sm hc-btn--outline"
                            on:click=move |_| rule.update(|r| {
                                r.conditions.push(default_condition_typed("device_state"));
                            })
                        >"+ Add"</button>
                    </div>
                    <ConditionList rule=rule />
                </section>

                // ── Actions ──────────────────────────────────────────────────
                <section class="detail-card">
                    <div class="rule-section-header">
                        <h3 class="detail-card-title">"Actions"</h3>
                        <button class="hc-btn hc-btn--sm hc-btn--outline"
                            on:click=move |_| rule_json.update(|v| {
                                let arr = v["actions"].as_array_mut().expect("actions array");
                                arr.push(default_action("log_message"));
                            })
                        >"+ Add"</button>
                    </div>
                    <ItemList rule=rule_json key="actions" item_kind="action" />
                </section>

                // ── Advanced (collapsible) ───────────────────────────────────
                <section class="detail-card">
                    <button class="rule-advanced-toggle"
                        on:click=move |_| advanced_open.update(|v| *v = !*v)
                    >
                        {move || if advanced_open.get() { "▾ Advanced" } else { "▸ Advanced" }}
                    </button>

                    <Show when=move || advanced_open.get()>
                        <div class="rule-advanced-body">
                            <label class="field-label">"Trigger label"</label>
                            <input type="text" class="hc-input" placeholder="e.g. motion_hallway"
                                prop:value=move || rule.get().trigger_label.clone().unwrap_or_default()
                                on:input=move |ev| {
                                    let raw = event_target_value(&ev);
                                    rule.update(|r| r.trigger_label = if raw.trim().is_empty() { None } else { Some(raw) });
                                }
                            />

                            <label class="field-label">"Required expression (Rhai)"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="3"
                                placeholder=r#"e.g. mode_is("mode_night")"#
                                prop:value=move || rule.get().required_expression.clone().unwrap_or_default()
                                on:input=move |ev| {
                                    let raw = event_target_value(&ev);
                                    rule.update(|r| r.required_expression = if raw.trim().is_empty() { None } else { Some(raw) });
                                }
                            />

                            <label class="rule-meta-inline">
                                <input type="checkbox"
                                    prop:checked=move || rule.get().cancel_on_false
                                    on:change=move |ev| {
                                        use wasm_bindgen::JsCast;
                                        let checked = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).map(|el| el.checked()).unwrap_or(false);
                                        rule.update(|r| r.cancel_on_false = checked);
                                    }
                                />
                                " Cancel pending delays when required expression is false"
                            </label>

                            <label class="field-label">"Trigger condition (Rhai)"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="3"
                                placeholder="Per-event condition expression"
                                prop:value=move || rule.get().trigger_condition.clone().unwrap_or_default()
                                on:input=move |ev| {
                                    let raw = event_target_value(&ev);
                                    rule.update(|r| r.trigger_condition = if raw.trim().is_empty() { None } else { Some(raw) });
                                }
                            />

                            <label class="field-label">"Variables (JSON object)"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="4"
                                prop:value=move || {
                                    let vars = &rule.get().variables;
                                    if vars.is_empty() {
                                        "{}".to_string()
                                    } else {
                                        serde_json::to_string_pretty(vars).unwrap_or_default()
                                    }
                                }
                                on:input=move |ev| {
                                    let raw = event_target_value(&ev);
                                    if let Ok(parsed) = serde_json::from_str::<std::collections::HashMap<String, Value>>(&raw) {
                                        rule.update(|r| r.variables = parsed);
                                    }
                                }
                            />

                            <div class="rule-logging-row">
                                <span class="field-label" style="margin:0">"Logging:"</span>
                                <TypedCheckbox label="Events"
                                    checked=Signal::derive(move || rule.get().log_events)
                                    on_change=move |v| rule.update(|r| r.log_events = v) />
                                <TypedCheckbox label="Triggers"
                                    checked=Signal::derive(move || rule.get().log_triggers)
                                    on_change=move |v| rule.update(|r| r.log_triggers = v) />
                                <TypedCheckbox label="Actions"
                                    checked=Signal::derive(move || rule.get().log_actions)
                                    on_change=move |v| rule.update(|r| r.log_actions = v) />
                            </div>
                        </div>
                    </Show>
                </section>

                // ── Action bar ───────────────────────────────────────────────
                <div class="rule-action-bar">
                    {
                        let nav_save   = navigate.clone();
                        let nav_cancel = navigate.clone();
                        let nav_clone  = navigate.clone();
                        let nav_delete = navigate.clone();

                        view! {
                            // Save
                            <button class="hc-btn hc-btn--primary"
                                disabled=move || saving.get()
                                on:click=move |_| {
                                    let token = match auth.token.get_untracked() { Some(t) => t, None => return };
                                    let typed = build_save_rule(rule, rule_json);
                                    if typed.name.trim().is_empty() { save_err.set(Some("Rule name is required.".into())); return; }
                                    save_err.set(None); save_ok.set(false); saving.set(true);
                                    let nav = nav_save.clone();
                                    let rule_id_str = id.map(|s| s.get_untracked()).unwrap_or_default();
                                    spawn_local(async move {
                                        let result = if rule_id_str.is_empty() { create_rule(&token, &typed).await }
                                                     else { update_rule(&token, &rule_id_str, &typed).await };
                                        match result {
                                            Ok(saved) => {
                                                if rule_id_str.is_empty() {
                                                    let new_id = saved.id.to_string();
                                                    if !new_id.is_empty() { nav(&format!("/rules/{new_id}"), Default::default()); }
                                                } else {
                                                    rule_json.set(serde_json::to_value(&saved).unwrap_or_default());
                                                    rule.set(saved);
                                                    save_ok.set(true);
                                                    spawn_local(async move {
                                                        gloo_timers::future::TimeoutFuture::new(3000).await;
                                                        save_ok.set(false);
                                                    });
                                                }
                                            }
                                            Err(e) => save_err.set(Some(e)),
                                        }
                                        saving.set(false);
                                    });
                                }
                            >{move || if saving.get() { "Saving…" } else { "Save" }}</button>

                            // Cancel
                            <button class="hc-btn hc-btn--outline"
                                disabled=move || saving.get()
                                on:click=move |_| nav_cancel("/rules", Default::default())
                            >"Cancel"</button>

                            // Clone + Delete (edit mode only)
                            <Show when=move || !is_new>
                                {
                                    let nav_clone_btn = nav_clone.clone();
                                    let nav_delete_btn = nav_delete.clone();
                                    view! {
                                <button class="hc-btn hc-btn--outline"
                                    disabled=move || saving.get()
                                    title="Clone this rule"
                                    on:click=move |_| {
                                        let token = match auth.token.get_untracked() { Some(t) => t, None => return };
                                        let rule_id = id.map(|s| s.get_untracked()).unwrap_or_default();
                                        if rule_id.is_empty() { return; }
                                        let nav = nav_clone_btn.clone();
                                        saving.set(true);
                                        spawn_local(async move {
                                            match clone_rule(&token, &rule_id).await {
                                                Ok(new_rule) => {
                                                    let new_id = new_rule.id.to_string();
                                                    if !new_id.is_empty() { nav(&format!("/rules/{new_id}"), Default::default()); }
                                                }
                                                Err(e) => save_err.set(Some(e)),
                                            }
                                            saving.set(false);
                                        });
                                    }
                                >
                                    <span class="material-icons" style="font-size:15px;vertical-align:middle">"content_copy"</span>
                                    " Clone"
                                </button>

                                {move || if confirm_delete.get() {
                                    let nav = nav_delete_btn.clone();
                                    view! {
                                        <span class="rule-confirm-delete">
                                            "Delete rule? "
                                            <button class="hc-btn hc-btn--sm hc-btn--danger"
                                                on:click=move |_| {
                                                    let token = match auth.token.get_untracked() { Some(t) => t, None => return };
                                                    let rule_id = id.map(|s| s.get_untracked()).unwrap_or_default();
                                                    let nav = nav.clone();
                                                    saving.set(true);
                                                    spawn_local(async move {
                                                        match delete_rule(&token, &rule_id).await {
                                                            Ok(()) => nav("/rules", Default::default()),
                                                            Err(e) => { save_err.set(Some(e)); saving.set(false); }
                                                        }
                                                    });
                                                }
                                            >"Yes, delete"</button>
                                            " "
                                            <button class="hc-btn hc-btn--sm hc-btn--outline"
                                                on:click=move |_| confirm_delete.set(false)
                                            >"Cancel"</button>
                                        </span>
                                    }.into_any()
                                } else {
                                    view! {
                                        <button class="hc-btn hc-btn--outline hc-btn--danger-outline"
                                            on:click=move |_| confirm_delete.set(true)
                                        >"Delete"</button>
                                    }.into_any()
                                }}
                                    }
                                }
                            </Show>
                        }
                    }
                </div>

                // ── Test Run panel (edit mode only) ──────────────────────────
                <Show when=move || !is_new>
                    <section class="detail-card">
                        <div class="rule-section-header">
                            <h3 class="detail-card-title">"Test Run"</h3>
                            <button class="hc-btn hc-btn--sm hc-btn--outline"
                                disabled=move || test_loading.get()
                                on:click=move |_| {
                                    let token = match auth.token.get_untracked() { Some(t) => t, None => return };
                                    let rule_id = id.map(|s| s.get_untracked()).unwrap_or_default();
                                    if rule_id.is_empty() { return; }
                                    test_loading.set(true); test_err.set(None); test_result.set(None);
                                    spawn_local(async move {
                                        match test_rule(&token, &rule_id).await {
                                            Ok(r) => test_result.set(Some(r)),
                                            Err(e) => test_err.set(Some(e)),
                                        }
                                        test_loading.set(false);
                                    });
                                }
                            >{move || if test_loading.get() { "Running…" } else { "Run Test" }}</button>
                        </div>
                        <p class="msg-muted" style="font-size:0.78rem">
                            "Evaluates conditions and shows which actions would fire, without executing them."
                        </p>
                        {move || test_err.get().map(|e| view! { <p class="msg-error">{e}</p> })}
                        {move || test_result.get().map(|r| {
                            let pretty = serde_json::to_string_pretty(&r).unwrap_or_default();
                            view! { <pre class="test-result-pre">{pretty}</pre> }
                        })}
                    </section>
                </Show>

                // ── Fire History panel (edit mode only) ──────────────────────
                <Show when=move || !is_new>
                    <section class="detail-card">
                        <div class="rule-section-header">
                            <h3 class="detail-card-title">"Fire History"</h3>
                            <button class="hc-btn hc-btn--sm hc-btn--outline"
                                disabled=move || history_loading.get()
                                on:click=move |_| {
                                    let token = match auth.token.get_untracked() { Some(t) => t, None => return };
                                    let rule_id = id.map(|s| s.get_untracked()).unwrap_or_default();
                                    if rule_id.is_empty() { return; }
                                    history_loading.set(true); history_err.set(None); history_data.set(None); history_open.set(true);
                                    spawn_local(async move {
                                        match rule_fire_history(&token, &rule_id).await {
                                            Ok(h) => history_data.set(Some(h)),
                                            Err(e) => history_err.set(Some(e)),
                                        }
                                        history_loading.set(false);
                                    });
                                }
                            >{move || if history_loading.get() { "Loading…" } else { "Load History" }}</button>
                        </div>
                        <Show when=move || history_open.get()>
                            {move || history_err.get().map(|e| view! { <p class="msg-error">{e}</p> })}
                            {move || history_data.get().map(|data| {
                                let entries = data.as_array().cloned().unwrap_or_default();
                                if entries.is_empty() {
                                    view! { <p class="msg-muted" style="font-size:0.85rem">"No fire history."</p> }.into_any()
                                } else {
                                    view! {
                                        <div class="history-list">
                                            {entries.into_iter().map(|entry| {
                                                let ts = jget_str(&entry, "timestamp");
                                                let result = entry["result"].as_str().unwrap_or("ok").to_string();
                                                let actions_run = entry["actions_executed"].as_u64().unwrap_or(0);
                                                let elapsed = entry["elapsed_ms"].as_u64().unwrap_or(0);
                                                let is_err = result != "ok" && result != "success";
                                                view! {
                                                    <div class="history-entry" class:history-entry--error=is_err>
                                                        <span class="history-ts">{ts}</span>
                                                        <span class="history-result">{result}</span>
                                                        <span class="history-detail">{format!("{actions_run} actions, {elapsed}ms")}</span>
                                                    </div>
                                                }
                                            }).collect_view()}
                                        </div>
                                    }.into_any()
                                }
                            })}
                        </Show>
                    </section>
                </Show>
            </Show>
        </div>
    }
}

// ── ItemList ─────────────────────────────────────────────────────────────────
// Renders a list of conditions or actions from rule[key], with reorder/remove
// controls and the appropriate editor for each item.

#[component]
fn ItemList(
    rule: RwSignal<Value>,
    key: &'static str,
    item_kind: &'static str,
) -> impl IntoView {
    view! {
        {move || {
            let arr = rule.get()[key].as_array().cloned().unwrap_or_default();
            if arr.is_empty() {
                let msg = if key == "conditions" { "No conditions — rule fires unconditionally." } else { "No actions." };
                view! { <p class="msg-muted" style="font-size:0.85rem">{msg}</p> }.into_any()
            } else {
                let total = arr.len();
                arr.into_iter().enumerate().map(|(i, item)| {
                    let is_first = i == 0;
                    let is_last = i + 1 >= total;
                    let disabled_class = if item_kind == "action" && item["enabled"].as_bool() == Some(false) { " json-row--disabled" } else { "" };
                    view! {
                        <div class=format!("json-row{disabled_class}")>
                            <div class="json-row-controls">
                                <span class="json-row-index">{i + 1}</span>
                                <button class="hc-btn hc-btn--sm hc-btn--outline" title="Move up"
                                    disabled=is_first
                                    on:click=move |_| rule.update(|v| {
                                        if let Some(arr) = v[key].as_array_mut() { if i > 0 { arr.swap(i - 1, i); } }
                                    })
                                ><span class="material-icons" style="font-size:14px">"arrow_upward"</span></button>
                                <button class="hc-btn hc-btn--sm hc-btn--outline" title="Move down"
                                    disabled=is_last
                                    on:click=move |_| rule.update(|v| {
                                        if let Some(arr) = v[key].as_array_mut() { if i + 1 < arr.len() { arr.swap(i, i + 1); } }
                                    })
                                ><span class="material-icons" style="font-size:14px">"arrow_downward"</span></button>
                                <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove"
                                    on:click=move |_| rule.update(|v| {
                                        if let Some(arr) = v[key].as_array_mut() { arr.remove(i); }
                                    })
                                ><span class="material-icons" style="font-size:14px">"close"</span></button>
                            </div>
                            {if item_kind == "condition" {
                                view! { <ConditionEditor rule=rule index=i /> }.into_any()
                            } else {
                                view! { <ActionEditor rule=rule path=key index=i /> }.into_any()
                            }}
                        </div>
                    }
                }).collect_view().into_any()
            }
        }}
    }
}

// ── TriggerEditor ────────────────────────────────────────────────────────────

/// Helper to get a string identifying the current trigger variant.
fn trigger_variant_key(t: &Trigger) -> &'static str {
    match t {
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
    }
}

/// Create a default Trigger for a given variant key.
fn default_trigger_typed(key: &str) -> Trigger {
    use chrono::{NaiveTime, Weekday};
    match key {
        "device_state_changed" => Trigger::DeviceStateChanged {
            device_id: String::new(), device_ids: vec![], attribute: None,
            to: None, from: None, not_from: None, not_to: None,
            for_duration_secs: None, change_kind: None, change_source: None,
        },
        "device_availability_changed" => Trigger::DeviceAvailabilityChanged {
            device_id: String::new(), to: None, for_duration_secs: None,
        },
        "button_event" => Trigger::ButtonEvent {
            device_id: String::new(), button_number: None, event: ButtonEventType::Pushed,
        },
        "numeric_threshold" => Trigger::NumericThreshold {
            device_id: String::new(), attribute: String::new(),
            op: ThresholdOp::CrossesAbove, value: 0.0, for_duration_secs: None,
        },
        "time_of_day" => Trigger::TimeOfDay {
            time: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            days: vec![Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri, Weekday::Sat, Weekday::Sun],
        },
        "sun_event" => Trigger::SunEvent {
            event: SunEventType::Sunset, offset_minutes: 0,
        },
        "cron" => Trigger::Cron { expression: "0 0 8 * * *".into() },
        "periodic" => Trigger::Periodic { every_n: 15, unit: PeriodicUnit::Minutes },
        "calendar_event" => Trigger::CalendarEvent {
            calendar_id: None, title_contains: None, offset_minutes: 0,
        },
        "custom_event" => Trigger::CustomEvent { event_type: String::new() },
        "system_started" => Trigger::SystemStarted,
        "hub_variable_changed" => Trigger::HubVariableChanged { name: None },
        "mode_changed" => Trigger::ModeChanged { mode_id: None, to: None },
        "webhook_received" => Trigger::WebhookReceived { path: "/hooks/".into() },
        "mqtt_message" => Trigger::MqttMessage {
            topic_pattern: "homecore/devices/+/state".into(),
            payload: None, value_path: None, value_op: None, value_cmp: None,
        },
        _ => Trigger::ManualTrigger,
    }
}

#[component]
fn TriggerEditor(rule: RwSignal<Rule>) -> impl IntoView {
    let tg = move || rule.get().trigger.clone();
    let tset = move |new_trigger: Trigger| { rule.update(|r| r.trigger = new_trigger); };

    view! {
        <div class="trigger-editor">
            <label class="field-label">"Trigger type"</label>
            <select class="hc-select"
                on:change=move |ev| {
                    let t = event_target_value(&ev);
                    rule.update(|r| r.trigger = default_trigger_typed(&t));
                }
            >
                <optgroup label="Device">
                    {[("device_state_changed","Device state changed"),("device_availability_changed","Device availability changed"),("button_event","Button event"),("numeric_threshold","Numeric threshold")]
                        .map(|(v, label)| view! { <option value=v selected=move || trigger_variant_key(&tg()) == v>{label}</option> }).collect_view()}
                </optgroup>
                <optgroup label="Time">
                    {[("time_of_day","Time of day"),("sun_event","Sun event"),("cron","Cron schedule"),("periodic","Periodic"),("calendar_event","Calendar event")]
                        .map(|(v, label)| view! { <option value=v selected=move || trigger_variant_key(&tg()) == v>{label}</option> }).collect_view()}
                </optgroup>
                <optgroup label="Event">
                    {[("custom_event","Custom event"),("system_started","System started"),("hub_variable_changed","Hub variable changed"),("mode_changed","Mode changed"),("webhook_received","Webhook received"),("mqtt_message","MQTT message")]
                        .map(|(v, label)| view! { <option value=v selected=move || trigger_variant_key(&tg()) == v>{label}</option> }).collect_view()}
                </optgroup>
                <optgroup label="Manual">
                    <option value="manual_trigger" selected=move || trigger_variant_key(&tg()) == "manual_trigger">"Manual trigger"</option>
                </optgroup>
            </select>

            // Type-specific fields
            {move || {
                let trigger = tg();
                match trigger {
                    Trigger::DeviceStateChanged { ref device_id, ref attribute, ref to, ref from, ref for_duration_secs, .. } => {
                        let did = device_id.clone();
                        let attr = attribute.clone().unwrap_or_default();
                        let to_val = to.clone().map(|v| serde_json::to_value(&v).unwrap_or_default()).unwrap_or(Value::Null);
                        let from_val = from.clone().map(|v| serde_json::to_value(&v).unwrap_or_default()).unwrap_or(Value::Null);
                        let dur = for_duration_secs.map(|n| n.to_string()).unwrap_or_default();
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Device(s)"</label>
                                <DeviceMultiSelect rule=rule />
                                <label class="field-label">"Attribute (blank = any)"</label>
                                <AttributeSelect device_id=did.clone() value=attr.clone()
                                    on_select=Callback::new(move |a: String| rule.update(|r| {
                                        if let Trigger::DeviceStateChanged { ref mut attribute, .. } = r.trigger {
                                            *attribute = if a.is_empty() { None } else { Some(a) };
                                        }
                                    })) />
                                <div class="trigger-row-2">
                                    <AttrValueSelect device_id=did.clone() attribute=attr.clone() value=to_val label="To (blank = any)"
                                        on_select=Callback::new(move |raw: String| rule.update(|r| {
                                            if let Trigger::DeviceStateChanged { ref mut to, .. } = r.trigger {
                                                *to = if raw.trim().is_empty() { None } else {
                                                    Some(serde_json::from_str(&raw).unwrap_or_else(|_| json!(raw)))
                                                };
                                            }
                                        })) />
                                    <AttrValueSelect device_id=did.clone() attribute=attr.clone() value=from_val label="From (blank = any)"
                                        on_select=Callback::new(move |raw: String| rule.update(|r| {
                                            if let Trigger::DeviceStateChanged { ref mut from, .. } = r.trigger {
                                                *from = if raw.trim().is_empty() { None } else {
                                                    Some(serde_json::from_str(&raw).unwrap_or_else(|_| json!(raw)))
                                                };
                                            }
                                        })) />
                                </div>
                                <label class="field-label">"Hold duration (seconds, blank = immediate)"</label>
                                <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="None"
                                    prop:value=dur
                                    on:input=move |ev| {
                                        let raw = event_target_value(&ev);
                                        rule.update(|r| {
                                            if let Trigger::DeviceStateChanged { ref mut for_duration_secs, .. } = r.trigger {
                                                *for_duration_secs = raw.trim().parse::<u64>().ok();
                                            }
                                        });
                                    } />
                            </div>
                        }.into_any()
                    },

                    Trigger::DeviceAvailabilityChanged { ref device_id, ref to, .. } => {
                        let did = device_id.clone();
                        let to_val = *to;
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Device"</label>
                                <DeviceSelect value=did on_select=Callback::new(move |id: String| rule.update(|r| {
                                    if let Trigger::DeviceAvailabilityChanged { ref mut device_id, .. } = r.trigger { *device_id = id; }
                                })) />
                                <label class="field-label">"Direction"</label>
                                <select class="hc-select" on:change=move |ev| {
                                    let raw = event_target_value(&ev);
                                    rule.update(|r| {
                                        if let Trigger::DeviceAvailabilityChanged { ref mut to, .. } = r.trigger {
                                            *to = match raw.as_str() { "online" => Some(true), "offline" => Some(false), _ => None };
                                        }
                                    });
                                }>
                                    <option value="any" selected=to_val.is_none()>"Any"</option>
                                    <option value="online" selected=to_val==Some(true)>"Goes online"</option>
                                    <option value="offline" selected=to_val==Some(false)>"Goes offline"</option>
                                </select>
                            </div>
                        }.into_any()
                    },

                    Trigger::ButtonEvent { ref device_id, ref event, .. } => {
                        let did = device_id.clone();
                        let evt = format!("{:?}", event);
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Device"</label>
                                <DeviceSelect value=did on_select=Callback::new(move |id: String| rule.update(|r| {
                                    if let Trigger::ButtonEvent { ref mut device_id, .. } = r.trigger { *device_id = id; }
                                })) />
                                <label class="field-label">"Event type"</label>
                                <select class="hc-select" on:change=move |ev| {
                                    let raw = event_target_value(&ev);
                                    rule.update(|r| {
                                        if let Trigger::ButtonEvent { ref mut event, .. } = r.trigger {
                                            *event = match raw.as_str() {
                                                "Held" => ButtonEventType::Held,
                                                "DoubleTapped" => ButtonEventType::DoubleTapped,
                                                "Released" => ButtonEventType::Released,
                                                _ => ButtonEventType::Pushed,
                                            };
                                        }
                                    });
                                }>
                                    {[("Pushed","Pushed"),("Held","Held"),("DoubleTapped","Double-tapped"),("Released","Released")]
                                        .map(|(v,l)| view! { <option value=v selected=evt==v>{l}</option> }).collect_view()}
                                </select>
                            </div>
                        }.into_any()
                    },

                    Trigger::NumericThreshold { ref device_id, ref attribute, ref op, value, .. } => {
                        let did = device_id.clone();
                        let attr = attribute.clone();
                        let op_str = format!("{:?}", op);
                        let val = value;
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Device"</label>
                                <DeviceSelect value=did on_select=Callback::new(move |id: String| rule.update(|r| {
                                    if let Trigger::NumericThreshold { ref mut device_id, .. } = r.trigger { *device_id = id; }
                                })) />
                                <label class="field-label">"Attribute"</label>
                                <AttributeSelect device_id=device_id.clone() value=attr
                                    on_select=Callback::new(move |a: String| rule.update(|r| {
                                        if let Trigger::NumericThreshold { ref mut attribute, .. } = r.trigger { *attribute = a; }
                                    })) />
                                <div class="trigger-row-2">
                                    <div>
                                        <label class="field-label">"Operator"</label>
                                        <select class="hc-select" on:change=move |ev| {
                                            let raw = event_target_value(&ev);
                                            rule.update(|r| {
                                                if let Trigger::NumericThreshold { ref mut op, .. } = r.trigger {
                                                    *op = match raw.as_str() {
                                                        "CrossesBelow" => ThresholdOp::CrossesBelow,
                                                        "Above" => ThresholdOp::Above,
                                                        "Below" => ThresholdOp::Below,
                                                        _ => ThresholdOp::CrossesAbove,
                                                    };
                                                }
                                            });
                                        }>
                                            {[("CrossesAbove","Crosses above"),("CrossesBelow","Crosses below"),("Above","Is above"),("Below","Is below")]
                                                .map(|(v,l)| view! { <option value=v selected=op_str==v>{l}</option> }).collect_view()}
                                        </select>
                                    </div>
                                    <div>
                                        <label class="field-label">"Threshold"</label>
                                        <input type="number" class="hc-input" prop:value=val.to_string()
                                            on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<f64>() {
                                                rule.update(|r| { if let Trigger::NumericThreshold { ref mut value, .. } = r.trigger { *value = n; } });
                                            }} />
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    },

                    Trigger::TimeOfDay { ref time, ref days } => {
                        let time_str = time.format("%H:%M").to_string();
                        let days_clone = days.clone();
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Time (HH:MM)"</label>
                                <input type="time" class="hc-input hc-input--sm" style="width:10rem"
                                    prop:value=time_str
                                    on:input=move |ev| {
                                        let hm = event_target_value(&ev);
                                        if let Ok(t) = chrono::NaiveTime::parse_from_str(&format!("{hm}:00"), "%H:%M:%S") {
                                            rule.update(|r| { if let Trigger::TimeOfDay { ref mut time, .. } = r.trigger { *time = t; } });
                                        }
                                    } />
                                <label class="field-label">"Days"</label>
                                <div class="trigger-day-row">
                                    {[("Mon", chrono::Weekday::Mon),("Tue", chrono::Weekday::Tue),("Wed", chrono::Weekday::Wed),
                                      ("Thu", chrono::Weekday::Thu),("Fri", chrono::Weekday::Fri),("Sat", chrono::Weekday::Sat),("Sun", chrono::Weekday::Sun)]
                                        .map(|(label, wd)| {
                                            let checked = days_clone.contains(&wd);
                                            view! {
                                                <label class="day-chip">
                                                    <input type="checkbox" prop:checked=checked
                                                        on:change=move |ev| {
                                                            use wasm_bindgen::JsCast;
                                                            let on = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).map(|el| el.checked()).unwrap_or(false);
                                                            rule.update(|r| {
                                                                if let Trigger::TimeOfDay { ref mut days, .. } = r.trigger {
                                                                    if on { if !days.contains(&wd) { days.push(wd); } }
                                                                    else { days.retain(|d| *d != wd); }
                                                                }
                                                            });
                                                        } />
                                                    {label}
                                                </label>
                                            }
                                        }).collect_view()}
                                </div>
                            </div>
                        }.into_any()
                    },

                    Trigger::SunEvent { ref event, offset_minutes } => {
                        let evt = format!("{:?}", event);
                        let off = offset_minutes;
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Sun event"</label>
                                <select class="hc-select" on:change=move |ev| {
                                    let raw = event_target_value(&ev);
                                    rule.update(|r| {
                                        if let Trigger::SunEvent { ref mut event, .. } = r.trigger {
                                            *event = match raw.as_str() {
                                                "Sunrise" => SunEventType::Sunrise, "SolarNoon" => SunEventType::SolarNoon,
                                                "CivilDawn" => SunEventType::CivilDawn, "CivilDusk" => SunEventType::CivilDusk,
                                                _ => SunEventType::Sunset,
                                            };
                                        }
                                    });
                                }>
                                    {[("Sunrise","Sunrise"),("Sunset","Sunset"),("SolarNoon","Solar noon"),("CivilDawn","Civil dawn"),("CivilDusk","Civil dusk")]
                                        .map(|(v,l)| view! { <option value=v selected=evt==v>{l}</option> }).collect_view()}
                                </select>
                                <label class="field-label">"Offset (minutes)"</label>
                                <input type="number" class="hc-input hc-input--sm" style="width:8rem" prop:value=off.to_string()
                                    on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<i32>() {
                                        rule.update(|r| { if let Trigger::SunEvent { ref mut offset_minutes, .. } = r.trigger { *offset_minutes = n; } });
                                    }} />
                            </div>
                        }.into_any()
                    },

                    Trigger::Cron { ref expression } => {
                        let expr = expression.clone();
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Cron expression (6-field)"</label>
                                <input type="text" class="hc-input hc-textarea--code" placeholder="0 0 8 * * *" prop:value=expr
                                    on:input=move |ev| rule.update(|r| { if let Trigger::Cron { ref mut expression } = r.trigger { *expression = event_target_value(&ev); } }) />
                            </div>
                        }.into_any()
                    },

                    Trigger::Periodic { every_n, ref unit } => {
                        let n = every_n;
                        let u = format!("{:?}", unit);
                        view! {
                            <div class="trigger-fields">
                                <div class="trigger-row-2">
                                    <div>
                                        <label class="field-label">"Every N"</label>
                                        <input type="number" class="hc-input" min="1" prop:value=n.to_string()
                                            on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<u32>() {
                                                rule.update(|r| { if let Trigger::Periodic { ref mut every_n, .. } = r.trigger { *every_n = n; } });
                                            }} />
                                    </div>
                                    <div>
                                        <label class="field-label">"Unit"</label>
                                        <select class="hc-select" on:change=move |ev| {
                                            let raw = event_target_value(&ev);
                                            rule.update(|r| {
                                                if let Trigger::Periodic { ref mut unit, .. } = r.trigger {
                                                    *unit = match raw.as_str() { "Hours" => PeriodicUnit::Hours, "Days" => PeriodicUnit::Days, "Weeks" => PeriodicUnit::Weeks, _ => PeriodicUnit::Minutes };
                                                }
                                            });
                                        }>
                                            {[("Minutes","Minutes"),("Hours","Hours"),("Days","Days"),("Weeks","Weeks")]
                                                .map(|(v,l)| view! { <option value=v selected=u==v>{l}</option> }).collect_view()}
                                        </select>
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    },

                    Trigger::CalendarEvent { ref calendar_id, ref title_contains, offset_minutes } => {
                        let cal = calendar_id.clone().unwrap_or_default();
                        let title = title_contains.clone().unwrap_or_default();
                        let off = offset_minutes;
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Calendar ID (blank = any)"</label>
                                <input type="text" class="hc-input" prop:value=cal
                                    on:input=move |ev| { let v = event_target_value(&ev); rule.update(|r| {
                                        if let Trigger::CalendarEvent { ref mut calendar_id, .. } = r.trigger { *calendar_id = if v.is_empty() { None } else { Some(v) }; }
                                    }); } />
                                <label class="field-label">"Title contains (blank = any)"</label>
                                <input type="text" class="hc-input" prop:value=title
                                    on:input=move |ev| { let v = event_target_value(&ev); rule.update(|r| {
                                        if let Trigger::CalendarEvent { ref mut title_contains, .. } = r.trigger { *title_contains = if v.is_empty() { None } else { Some(v) }; }
                                    }); } />
                                <label class="field-label">"Offset (minutes)"</label>
                                <input type="number" class="hc-input hc-input--sm" style="width:8rem" prop:value=off.to_string()
                                    on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<i32>() {
                                        rule.update(|r| { if let Trigger::CalendarEvent { ref mut offset_minutes, .. } = r.trigger { *offset_minutes = n; } });
                                    }} />
                            </div>
                        }.into_any()
                    },

                    Trigger::CustomEvent { ref event_type } => {
                        let et = event_type.clone();
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Event type"</label>
                                <input type="text" class="hc-input" prop:value=et
                                    on:input=move |ev| rule.update(|r| { if let Trigger::CustomEvent { ref mut event_type } = r.trigger { *event_type = event_target_value(&ev); } }) />
                            </div>
                        }.into_any()
                    },

                    Trigger::HubVariableChanged { ref name } => {
                        let n = name.clone().unwrap_or_default();
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Variable name (blank = any)"</label>
                                <input type="text" class="hc-input" prop:value=n
                                    on:input=move |ev| { let v = event_target_value(&ev); rule.update(|r| {
                                        if let Trigger::HubVariableChanged { ref mut name } = r.trigger { *name = if v.is_empty() { None } else { Some(v) }; }
                                    }); } />
                            </div>
                        }.into_any()
                    },

                    Trigger::ModeChanged { ref mode_id, ref to } => {
                        let mid = mode_id.clone().unwrap_or_default();
                        let to_val = *to;
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Mode (blank = any)"</label>
                                <ModeSelect value=mid on_select=Callback::new(move |id: String| rule.update(|r| {
                                    if let Trigger::ModeChanged { ref mut mode_id, .. } = r.trigger { *mode_id = if id.is_empty() { None } else { Some(id) }; }
                                })) />
                                <label class="field-label">"Direction"</label>
                                <select class="hc-select" on:change=move |ev| {
                                    let raw = event_target_value(&ev);
                                    rule.update(|r| {
                                        if let Trigger::ModeChanged { ref mut to, .. } = r.trigger {
                                            *to = match raw.as_str() { "on" => Some(true), "off" => Some(false), _ => None };
                                        }
                                    });
                                }>
                                    <option value="any" selected=to_val.is_none()>"Any"</option>
                                    <option value="on" selected=to_val==Some(true)>"Turns on"</option>
                                    <option value="off" selected=to_val==Some(false)>"Turns off"</option>
                                </select>
                            </div>
                        }.into_any()
                    },

                    Trigger::WebhookReceived { ref path } => {
                        let p = path.clone();
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Webhook path"</label>
                                <input type="text" class="hc-input" prop:value=p
                                    on:input=move |ev| rule.update(|r| { if let Trigger::WebhookReceived { ref mut path } = r.trigger { *path = event_target_value(&ev); } }) />
                            </div>
                        }.into_any()
                    },

                    Trigger::MqttMessage { ref topic_pattern, ref payload, .. } => {
                        let tp = topic_pattern.clone();
                        let pl = payload.clone().unwrap_or_default();
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Topic pattern"</label>
                                <input type="text" class="hc-input hc-textarea--code" prop:value=tp
                                    on:input=move |ev| rule.update(|r| { if let Trigger::MqttMessage { ref mut topic_pattern, .. } = r.trigger { *topic_pattern = event_target_value(&ev); } }) />
                                <label class="field-label">"Exact payload (blank = any)"</label>
                                <input type="text" class="hc-input" prop:value=pl
                                    on:input=move |ev| { let v = event_target_value(&ev); rule.update(|r| {
                                        if let Trigger::MqttMessage { ref mut payload, .. } = r.trigger { *payload = if v.is_empty() { None } else { Some(v) }; }
                                    }); } />
                            </div>
                        }.into_any()
                    },

                    Trigger::SystemStarted => view! {
                        <div class="trigger-fields">
                            <p class="msg-muted" style="font-size:0.85rem">"Fires once when the rule engine starts."</p>
                        </div>
                    }.into_any(),

                    Trigger::ManualTrigger => view! {
                        <div class="trigger-fields">
                            <p class="msg-muted" style="font-size:0.85rem">"No configurable fields."</p>
                        </div>
                    }.into_any(),
                }
            }}
        </div>
    }
}

// ── Typed Condition Editor ───────────────────────────────────────────────────

fn condition_variant_key(c: &Condition) -> &'static str {
    match c {
        Condition::DeviceState { .. } => "device_state",
        Condition::TimeWindow { .. } => "time_window",
        Condition::ScriptExpression { .. } => "script_expression",
        Condition::TimeElapsed { .. } => "time_elapsed",
        Condition::DeviceLastChange { .. } => "device_last_change",
        Condition::Not { .. } => "not",
        Condition::And { .. } => "and",
        Condition::Or { .. } => "or",
        Condition::Xor { .. } => "xor",
        Condition::PrivateBooleanIs { .. } => "private_boolean_is",
        Condition::HubVariable { .. } => "hub_variable",
        Condition::ModeIs { .. } => "mode_is",
    }
}

fn default_condition_typed(key: &str) -> Condition {
    match key {
        "device_state" => Condition::DeviceState {
            device_id: String::new(), attribute: "on".into(), op: CompareOp::Eq, value: json!(true),
        },
        "time_window" => Condition::TimeWindow {
            start: chrono::NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            end: chrono::NaiveTime::from_hms_opt(22, 0, 0).unwrap(),
        },
        "script_expression" => Condition::ScriptExpression { script: String::new() },
        "time_elapsed" => Condition::TimeElapsed { device_id: String::new(), attribute: String::new(), duration_secs: 60 },
        "device_last_change" => Condition::DeviceLastChange { device_id: String::new(), kind: None, source: None, actor_id: None, actor_name: None },
        "not" => Condition::Not { condition: Box::new(default_condition_typed("device_state")) },
        "and" => Condition::And { conditions: vec![] },
        "or" => Condition::Or { conditions: vec![] },
        "xor" => Condition::Xor { conditions: vec![] },
        "private_boolean_is" => Condition::PrivateBooleanIs { name: String::new(), value: true },
        "hub_variable" => Condition::HubVariable { name: String::new(), op: CompareOp::Eq, value: json!("") },
        "mode_is" => Condition::ModeIs { mode_id: String::new(), on: true },
        _ => Condition::DeviceState { device_id: String::new(), attribute: "on".into(), op: CompareOp::Eq, value: json!(true) },
    }
}

/// Top-level condition list that reads/writes `rule.conditions`.
#[component]
fn ConditionList(rule: RwSignal<Rule>) -> impl IntoView {
    view! {
        {move || {
            let conditions = rule.get().conditions.clone();
            if conditions.is_empty() {
                view! { <p class="msg-muted" style="font-size:0.85rem">"No conditions — rule fires unconditionally."</p> }.into_any()
            } else {
                let total = conditions.len();
                conditions.into_iter().enumerate().map(|(i, _cond)| {
                    let is_first = i == 0;
                    let is_last = i + 1 >= total;
                    view! {
                        <div class="json-row">
                            <div class="json-row-controls">
                                <span class="json-row-index">{i + 1}</span>
                                <button class="hc-btn hc-btn--sm hc-btn--outline" title="Move up" disabled=is_first
                                    on:click=move |_| rule.update(|r| { if i > 0 { r.conditions.swap(i - 1, i); } })
                                ><span class="material-icons" style="font-size:14px">"arrow_upward"</span></button>
                                <button class="hc-btn hc-btn--sm hc-btn--outline" title="Move down" disabled=is_last
                                    on:click=move |_| rule.update(|r| { if i + 1 < r.conditions.len() { r.conditions.swap(i, i + 1); } })
                                ><span class="material-icons" style="font-size:14px">"arrow_downward"</span></button>
                                <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove"
                                    on:click=move |_| rule.update(|r| { r.conditions.remove(i); })
                                ><span class="material-icons" style="font-size:14px">"close"</span></button>
                            </div>
                            <TypedConditionEditor
                                get=Signal::derive(move || rule.get().conditions.get(i).cloned().unwrap_or_else(|| default_condition_typed("device_state")))
                                set=Callback::new(move |c: Condition| rule.update(|r| { if i < r.conditions.len() { r.conditions[i] = c; } }))
                            />
                        </div>
                    }
                }).collect_view().into_any()
            }
        }}
    }
}

/// Typed condition editor — takes get/set callbacks, works at any nesting depth.
#[component]
fn TypedConditionEditor(
    get: Signal<Condition>,
    set: Callback<Condition>,
) -> impl IntoView {
    view! {
        <div class="condition-editor">
            <label class="field-label">"Condition type"</label>
            <select class="hc-select"
                on:change=move |ev| {
                    let t = event_target_value(&ev);
                    set.run(default_condition_typed(&t));
                }
            >
                {[("device_state","Device state"),("time_window","Time window"),("script_expression","Script (Rhai)"),
                  ("time_elapsed","Time elapsed"),("device_last_change","Device last change"),("private_boolean_is","Private boolean"),
                  ("hub_variable","Hub variable"),("mode_is","Mode is on/off"),
                  ("not","NOT"),("and","AND"),("or","OR"),("xor","XOR")]
                    .map(|(v,l)| view! { <option value=v selected=move || condition_variant_key(&get.get()) == v>{l}</option> }).collect_view()}
            </select>

            {move || {
                let c = get.get();
                match c {
                    Condition::DeviceState { ref device_id, ref attribute, ref op, ref value } => {
                        let did = device_id.clone();
                        let attr = attribute.clone();
                        let op_str = format!("{:?}", op);
                        let val = value.clone();
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Device"</label>
                                <DeviceSelect value=did.clone() on_select=Callback::new(move |id: String| {
                                    let mut c = get.get_untracked(); if let Condition::DeviceState { ref mut device_id, .. } = c { *device_id = id; } set.run(c);
                                }) />
                                <div class="trigger-row-2">
                                    <div>
                                        <label class="field-label">"Attribute"</label>
                                        <AttributeSelect device_id=did.clone() value=attr.clone()
                                            on_select=Callback::new(move |a: String| {
                                                let mut c = get.get_untracked(); if let Condition::DeviceState { ref mut attribute, .. } = c { *attribute = a; } set.run(c);
                                            }) />
                                    </div>
                                    <div>
                                        <label class="field-label">"Operator"</label>
                                        <select class="hc-select" on:change=move |ev| {
                                            let raw = event_target_value(&ev);
                                            let mut c = get.get_untracked();
                                            if let Condition::DeviceState { ref mut op, .. } = c {
                                                *op = match raw.as_str() { "Ne" => CompareOp::Ne, "Gt" => CompareOp::Gt, "Gte" => CompareOp::Gte, "Lt" => CompareOp::Lt, "Lte" => CompareOp::Lte, _ => CompareOp::Eq };
                                            }
                                            set.run(c);
                                        }>
                                            {[("Eq","="),("Ne","≠"),("Gt",">"),("Gte","≥"),("Lt","<"),("Lte","≤")]
                                                .map(|(v,l)| view! { <option value=v selected=op_str==v>{l}</option> }).collect_view()}
                                        </select>
                                    </div>
                                </div>
                                <AttrValueSelect device_id=did attribute=attr value=val label="Value"
                                    on_select=Callback::new(move |raw: String| {
                                        let mut c = get.get_untracked();
                                        if let Condition::DeviceState { ref mut value, .. } = c {
                                            *value = if raw.trim().is_empty() { json!(true) } else {
                                                serde_json::from_str(&raw).unwrap_or_else(|_| json!(raw))
                                            };
                                        }
                                        set.run(c);
                                    }) />
                            </div>
                        }.into_any()
                    },

                    Condition::TimeWindow { ref start, ref end } => {
                        let s = start.format("%H:%M").to_string();
                        let e = end.format("%H:%M").to_string();
                        view! {
                            <div class="trigger-fields">
                                <div class="trigger-row-2">
                                    <div>
                                        <label class="field-label">"Start (HH:MM)"</label>
                                        <input type="time" class="hc-input hc-input--sm" prop:value=s
                                            on:input=move |ev| {
                                                let hm = event_target_value(&ev);
                                                if let Ok(t) = chrono::NaiveTime::parse_from_str(&format!("{hm}:00"), "%H:%M:%S") {
                                                    let mut c = get.get_untracked(); if let Condition::TimeWindow { ref mut start, .. } = c { *start = t; } set.run(c);
                                                }
                                            } />
                                    </div>
                                    <div>
                                        <label class="field-label">"End (HH:MM)"</label>
                                        <input type="time" class="hc-input hc-input--sm" prop:value=e
                                            on:input=move |ev| {
                                                let hm = event_target_value(&ev);
                                                if let Ok(t) = chrono::NaiveTime::parse_from_str(&format!("{hm}:00"), "%H:%M:%S") {
                                                    let mut c = get.get_untracked(); if let Condition::TimeWindow { ref mut end, .. } = c { *end = t; } set.run(c);
                                                }
                                            } />
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    },

                    Condition::ScriptExpression { ref script } => {
                        let s = script.clone();
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Rhai expression (→ bool)"</label>
                                <textarea class="hc-textarea hc-textarea--code" rows="4" prop:value=s
                                    on:input=move |ev| { let mut c = get.get_untracked(); if let Condition::ScriptExpression { ref mut script } = c { *script = event_target_value(&ev); } set.run(c); } />
                            </div>
                        }.into_any()
                    },

                    Condition::TimeElapsed { ref device_id, ref attribute, duration_secs } => {
                        let did = device_id.clone();
                        let attr = attribute.clone();
                        let dur = duration_secs;
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Device"</label>
                                <DeviceSelect value=did.clone() on_select=Callback::new(move |id: String| {
                                    let mut c = get.get_untracked(); if let Condition::TimeElapsed { ref mut device_id, .. } = c { *device_id = id; } set.run(c);
                                }) />
                                <label class="field-label">"Attribute"</label>
                                <AttributeSelect device_id=did value=attr on_select=Callback::new(move |a: String| {
                                    let mut c = get.get_untracked(); if let Condition::TimeElapsed { ref mut attribute, .. } = c { *attribute = a; } set.run(c);
                                }) />
                                <label class="field-label">"Duration (seconds)"</label>
                                <input type="number" class="hc-input hc-input--sm" style="width:8rem" prop:value=dur.to_string()
                                    on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<u64>() {
                                        let mut c = get.get_untracked(); if let Condition::TimeElapsed { ref mut duration_secs, .. } = c { *duration_secs = n; } set.run(c);
                                    }} />
                            </div>
                        }.into_any()
                    },

                    Condition::DeviceLastChange { ref device_id, ref source, .. } => {
                        let did = device_id.clone();
                        let src = source.clone().unwrap_or_default();
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Device"</label>
                                <DeviceSelect value=did on_select=Callback::new(move |id: String| {
                                    let mut c = get.get_untracked(); if let Condition::DeviceLastChange { ref mut device_id, .. } = c { *device_id = id; } set.run(c);
                                }) />
                                <label class="field-label">"Source (blank = any)"</label>
                                <input type="text" class="hc-input" prop:value=src
                                    on:input=move |ev| { let v = event_target_value(&ev);
                                        let mut c = get.get_untracked(); if let Condition::DeviceLastChange { ref mut source, .. } = c { *source = if v.is_empty() { None } else { Some(v) }; } set.run(c);
                                    } />
                            </div>
                        }.into_any()
                    },

                    Condition::PrivateBooleanIs { ref name, value } => {
                        let n = name.clone();
                        let v = value;
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Boolean name"</label>
                                <input type="text" class="hc-input" prop:value=n
                                    on:input=move |ev| { let mut c = get.get_untracked(); if let Condition::PrivateBooleanIs { ref mut name, .. } = c { *name = event_target_value(&ev); } set.run(c); } />
                                <label class="field-label">"Expected value"</label>
                                <select class="hc-select" on:change=move |ev| {
                                    let mut c = get.get_untracked(); if let Condition::PrivateBooleanIs { ref mut value, .. } = c { *value = event_target_value(&ev) == "true"; } set.run(c);
                                }>
                                    <option value="true" selected=v>"True"</option>
                                    <option value="false" selected=!v>"False"</option>
                                </select>
                            </div>
                        }.into_any()
                    },

                    Condition::HubVariable { ref name, ref op, ref value } => {
                        let n = name.clone();
                        let op_str = format!("{:?}", op);
                        let val_str = if value.is_string() { value.as_str().unwrap_or("").to_string() } else { value.to_string() };
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Variable name"</label>
                                <input type="text" class="hc-input" prop:value=n
                                    on:input=move |ev| { let mut c = get.get_untracked(); if let Condition::HubVariable { ref mut name, .. } = c { *name = event_target_value(&ev); } set.run(c); } />
                                <div class="trigger-row-2">
                                    <div>
                                        <label class="field-label">"Operator"</label>
                                        <select class="hc-select" on:change=move |ev| {
                                            let raw = event_target_value(&ev);
                                            let mut c = get.get_untracked();
                                            if let Condition::HubVariable { ref mut op, .. } = c {
                                                *op = match raw.as_str() { "Ne" => CompareOp::Ne, "Gt" => CompareOp::Gt, "Gte" => CompareOp::Gte, "Lt" => CompareOp::Lt, "Lte" => CompareOp::Lte, _ => CompareOp::Eq };
                                            }
                                            set.run(c);
                                        }>
                                            {[("Eq","="),("Ne","≠"),("Gt",">"),("Gte","≥"),("Lt","<"),("Lte","≤")]
                                                .map(|(v,l)| view! { <option value=v selected=op_str==v>{l}</option> }).collect_view()}
                                        </select>
                                    </div>
                                    <div>
                                        <label class="field-label">"Value (JSON)"</label>
                                        <input type="text" class="hc-input" prop:value=val_str
                                            on:input=move |ev| { let raw = event_target_value(&ev);
                                                let mut c = get.get_untracked();
                                                if let Condition::HubVariable { ref mut value, .. } = c {
                                                    *value = serde_json::from_str(&raw).unwrap_or_else(|_| json!(raw));
                                                }
                                                set.run(c);
                                            } />
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    },

                    Condition::ModeIs { ref mode_id, on } => {
                        let mid = mode_id.clone();
                        let is_on = on;
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Mode"</label>
                                <ModeSelect value=mid on_select=Callback::new(move |id: String| {
                                    let mut c = get.get_untracked(); if let Condition::ModeIs { ref mut mode_id, .. } = c { *mode_id = id; } set.run(c);
                                }) />
                                <label class="field-label">"Expected state"</label>
                                <select class="hc-select" on:change=move |ev| {
                                    let mut c = get.get_untracked(); if let Condition::ModeIs { ref mut on, .. } = c { *on = event_target_value(&ev) == "true"; } set.run(c);
                                }>
                                    <option value="true" selected=is_on>"On"</option>
                                    <option value="false" selected=!is_on>"Off"</option>
                                </select>
                            </div>
                        }.into_any()
                    },

                    // ── OR / AND / XOR — nested condition list ─────────
                    Condition::Or { ref conditions } | Condition::And { ref conditions } | Condition::Xor { ref conditions } => {
                        let variant_key = condition_variant_key(&c);
                        let label = match variant_key {
                            "or"  => "ANY of these must be true (OR)",
                            "and" => "ALL of these must be true (AND)",
                            _     => "EXACTLY ONE must be true (XOR)",
                        };
                        let border_class = match variant_key {
                            "or"  => "cond-group cond-group--or",
                            "and" => "cond-group cond-group--and",
                            _     => "cond-group cond-group--xor",
                        };
                        let total = conditions.len();
                        let vk = variant_key.to_string();
                        view! {
                            <div class=border_class>
                                <p class="cond-group-label">{label}</p>
                                {(0..total).map(|ci| {
                                    let is_first = ci == 0;
                                    let is_last = ci + 1 >= total;
                                    view! {
                                        <div class="json-row">
                                            <div class="json-row-controls">
                                                <span class="json-row-index">{ci + 1}</span>
                                                <button class="hc-btn hc-btn--sm hc-btn--outline" title="Move up" disabled=is_first
                                                    on:click=move |_| {
                                                        let mut c = get.get_untracked();
                                                        match &mut c {
                                                            Condition::Or { ref mut conditions } | Condition::And { ref mut conditions } | Condition::Xor { ref mut conditions } => {
                                                                if ci > 0 { conditions.swap(ci - 1, ci); }
                                                            }
                                                            _ => {}
                                                        }
                                                        set.run(c);
                                                    }
                                                ><span class="material-icons" style="font-size:14px">"arrow_upward"</span></button>
                                                <button class="hc-btn hc-btn--sm hc-btn--outline" title="Move down" disabled=is_last
                                                    on:click=move |_| {
                                                        let mut c = get.get_untracked();
                                                        match &mut c {
                                                            Condition::Or { ref mut conditions } | Condition::And { ref mut conditions } | Condition::Xor { ref mut conditions } => {
                                                                if ci + 1 < conditions.len() { conditions.swap(ci, ci + 1); }
                                                            }
                                                            _ => {}
                                                        }
                                                        set.run(c);
                                                    }
                                                ><span class="material-icons" style="font-size:14px">"arrow_downward"</span></button>
                                                <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove"
                                                    on:click=move |_| {
                                                        let mut c = get.get_untracked();
                                                        match &mut c {
                                                            Condition::Or { ref mut conditions } | Condition::And { ref mut conditions } | Condition::Xor { ref mut conditions } => {
                                                                conditions.remove(ci);
                                                            }
                                                            _ => {}
                                                        }
                                                        set.run(c);
                                                    }
                                                ><span class="material-icons" style="font-size:14px">"close"</span></button>
                                            </div>
                                            <TypedConditionEditor
                                                get=Signal::derive(move || {
                                                    match &get.get() {
                                                        Condition::Or { conditions } | Condition::And { conditions } | Condition::Xor { conditions } => {
                                                            conditions.get(ci).cloned().unwrap_or_else(|| default_condition_typed("device_state"))
                                                        }
                                                        _ => default_condition_typed("device_state"),
                                                    }
                                                })
                                                set=Callback::new(move |new_child: Condition| {
                                                    let mut c = get.get_untracked();
                                                    match &mut c {
                                                        Condition::Or { ref mut conditions } | Condition::And { ref mut conditions } | Condition::Xor { ref mut conditions } => {
                                                            if ci < conditions.len() { conditions[ci] = new_child; }
                                                        }
                                                        _ => {}
                                                    }
                                                    set.run(c);
                                                })
                                            />
                                        </div>
                                    }
                                }).collect_view()}
                                <button class="hc-btn hc-btn--sm hc-btn--outline" style="margin-top:0.25rem"
                                    on:click={
                                        let vk = vk.clone();
                                        move |_| {
                                            let mut c = get.get_untracked();
                                            match &mut c {
                                                Condition::Or { ref mut conditions } | Condition::And { ref mut conditions } | Condition::Xor { ref mut conditions } => {
                                                    conditions.push(default_condition_typed("device_state"));
                                                }
                                                _ => {}
                                            }
                                            set.run(c);
                                        }
                                    }
                                >"+ Add condition"</button>
                            </div>
                        }.into_any()
                    },

                    // ── NOT — single wrapped condition ───────────────────
                    Condition::Not { ref condition } => {
                        let has_inner = true; // NOT always has an inner condition
                        view! {
                            <div class="cond-group cond-group--not">
                                <p class="cond-group-label">"NOT — inverts the result"</p>
                                <div class="json-row">
                                    <div class="json-row-controls">
                                        <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Reset inner"
                                            on:click=move |_| {
                                                set.run(Condition::Not { condition: Box::new(default_condition_typed("device_state")) });
                                            }
                                        ><span class="material-icons" style="font-size:14px">"close"</span></button>
                                    </div>
                                    <TypedConditionEditor
                                        get=Signal::derive(move || {
                                            match &get.get() {
                                                Condition::Not { condition } => *condition.clone(),
                                                _ => default_condition_typed("device_state"),
                                            }
                                        })
                                        set=Callback::new(move |inner: Condition| {
                                            set.run(Condition::Not { condition: Box::new(inner) });
                                        })
                                    />
                                </div>
                            </div>
                        }.into_any()
                    },
                }
            }}
        </div>
    }
}

// ── Legacy ConditionEditor (kept for reference during migration) ─────────────
// TODO: Remove once all callers use TypedConditionEditor

#[component]
fn ConditionEditor(
    rule: RwSignal<Value>,
    index: usize,
    /// For nested conditions inside or/and/xor groups.
    #[prop(optional)] nested_path: Option<Vec<usize>>,
) -> impl IntoView {
    let np = nested_path.unwrap_or_default();
    let np_stored = StoredValue::new(np);

    // Helper: navigate to the right condition in the JSON tree.
    fn walk<'a>(v: &'a mut Value, index: usize, np: &[usize]) -> &'a mut Value {
        let mut target = &mut v["conditions"][index];
        for &ni in np { target = &mut target["conditions"][ni]; }
        target
    }

    // Read the condition value at the right depth.
    let cg = move || -> Value {
        let np = np_stored.get_value();
        let mut v = rule.get()["conditions"][index].clone();
        for &ni in np.iter() { v = v["conditions"][ni].clone(); }
        v
    };
    // Write helpers — use StoredValue to make them accessible from reactive closures.
    // StoredValue is non-reactive (doesn't trigger re-renders) but is Copy.
    let cset = move |key: &'static str, val: Value| {
        let np = np_stored.get_value();
        rule.update(move |v| { walk(v, index, &np)[key] = val; });
    };
    let cset_opt = move |key: &'static str, raw: &str| {
        let np = np_stored.get_value();
        let raw = raw.to_string();
        rule.update(move |v| {
            let target = walk(v, index, &np);
            if raw.trim().is_empty() { if let Some(o) = target.as_object_mut() { o.remove(key); } }
            else {
                let parsed = serde_json::from_str::<Value>(raw.trim()).unwrap_or_else(|_| json!(raw.trim()));
                target[key] = parsed;
            }
        });
    };
    let cset_whole = move |val: Value| {
        let np = np_stored.get_value();
        rule.update(move |v| { *walk(v, index, &np) = val; });
    };

    view! {
        <div class="condition-editor">
            <label class="field-label">"Condition type"</label>
            <select class="hc-select"
                on:change=move |ev| {
                    let t = event_target_value(&ev);
                    cset_whole(default_condition(&t));
                }
            >
                {[("device_state","Device state"),("time_window","Time window"),("script_expression","Script (Rhai)"),
                  ("time_elapsed","Time elapsed"),("device_last_change","Device last change"),("private_boolean_is","Private boolean"),
                  ("hub_variable","Hub variable"),("mode_is","Mode is on/off"),
                  ("not","NOT"),("and","AND"),("or","OR"),("xor","XOR")]
                    .map(|(v,l)| view! { <option value=v selected=move || cg()["type"].as_str()==Some(v)>{l}</option> }).collect_view()}
            </select>

            {move || {
                let c = cg();
                let t = c["type"].as_str().unwrap_or("").to_string();
                match t.as_str() {
                    "device_state" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device"</label>
                            <DeviceSelect value=jget_str(&c, "device_id")
                                on_select=Callback::new(move |id: String| cset("device_id", json!(id))) />
                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"Attribute"</label>
                                    <AttributeSelect device_id=jget_str(&c, "device_id") value=jget_str(&c, "attribute")
                                        on_select=Callback::new(move |attr: String| cset("attribute", json!(attr))) />
                                </div>
                                <div>
                                    <label class="field-label">"Operator"</label>
                                    <select class="hc-select" on:change=move |ev| cset("op", json!(event_target_value(&ev)))>
                                        {[("eq","="),("ne","≠"),("gt",">"),("gte","≥"),("lt","<"),("lte","≤")]
                                            .map(|(v,l)| view! { <option value=v selected=c["op"].as_str()==Some(v)>{l}</option> }).collect_view()}
                                    </select>
                                </div>
                            </div>
                            <AttrValueSelect
                                device_id=jget_str(&c, "device_id")
                                attribute=jget_str(&c, "attribute")
                                value=c["value"].clone()
                                label="Value"
                                on_select=Callback::new(move |raw: String| cset_opt("value", &raw))
                            />
                        </div>
                    }.into_any(),

                    "time_window" => view! {
                        <div class="trigger-fields">
                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"Start (HH:MM)"</label>
                                    <input type="time" class="hc-input hc-input--sm"
                                        prop:value=c["start"].as_str().unwrap_or("08:00:00").get(..5).unwrap_or("08:00").to_string()
                                        on:input=move |ev| { let hm = event_target_value(&ev); cset("start", json!(format!("{hm}:00"))); } />
                                </div>
                                <div>
                                    <label class="field-label">"End (HH:MM)"</label>
                                    <input type="time" class="hc-input hc-input--sm"
                                        prop:value=c["end"].as_str().unwrap_or("22:00:00").get(..5).unwrap_or("22:00").to_string()
                                        on:input=move |ev| { let hm = event_target_value(&ev); cset("end", json!(format!("{hm}:00"))); } />
                                </div>
                            </div>
                        </div>
                    }.into_any(),

                    "script_expression" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Rhai expression (→ bool)"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="4"
                                prop:value=jget_str(&c, "script") on:input=move |ev| cset("script", json!(event_target_value(&ev))) />
                        </div>
                    }.into_any(),

                    "time_elapsed" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device"</label>
                            <DeviceSelect value=jget_str(&c, "device_id")
                                on_select=Callback::new(move |id: String| cset("device_id", json!(id))) />
                            <label class="field-label">"Attribute"</label>
                            <AttributeSelect device_id=jget_str(&c, "device_id") value=jget_str(&c, "attribute")
                                on_select=Callback::new(move |attr: String| cset("attribute", json!(attr))) />
                            <label class="field-label">"Duration (seconds)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem"
                                prop:value=c["duration_secs"].as_u64().unwrap_or(60).to_string()
                                on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<u64>() { cset("duration_secs", json!(n)); } } />
                        </div>
                    }.into_any(),

                    "device_last_change" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device"</label>
                            <DeviceSelect value=jget_str(&c, "device_id")
                                on_select=Callback::new(move |id: String| cset("device_id", json!(id))) />
                            <label class="field-label">"Change kind (blank = any)"</label>
                            <input type="text" class="hc-input"
                                prop:value=jget_opt_str(&c, "kind") on:input=move |ev| cset_opt("kind", &event_target_value(&ev)) />
                            <label class="field-label">"Source (blank = any)"</label>
                            <input type="text" class="hc-input"
                                prop:value=jget_opt_str(&c, "source") on:input=move |ev| cset_opt("source", &event_target_value(&ev)) />
                        </div>
                    }.into_any(),

                    "private_boolean_is" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Boolean name"</label>
                            <input type="text" class="hc-input"
                                prop:value=jget_str(&c, "name") on:input=move |ev| cset("name", json!(event_target_value(&ev))) />
                            <label class="field-label">"Expected value"</label>
                            <select class="hc-select" on:change=move |ev| cset("value", json!(event_target_value(&ev)=="true"))>
                                <option value="true" selected=c["value"].as_bool()!=Some(false)>"True"</option>
                                <option value="false" selected=c["value"].as_bool()==Some(false)>"False"</option>
                            </select>
                        </div>
                    }.into_any(),

                    "hub_variable" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Variable name"</label>
                            <input type="text" class="hc-input"
                                prop:value=jget_str(&c, "name") on:input=move |ev| cset("name", json!(event_target_value(&ev))) />
                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"Operator"</label>
                                    <select class="hc-select" on:change=move |ev| cset("op", json!(event_target_value(&ev)))>
                                        {[("eq","="),("ne","≠"),("gt",">"),("gte","≥"),("lt","<"),("lte","≤")]
                                            .map(|(v,l)| view! { <option value=v selected=c["op"].as_str()==Some(v)>{l}</option> }).collect_view()}
                                    </select>
                                </div>
                                <div>
                                    <label class="field-label">"Value (JSON)"</label>
                                    <input type="text" class="hc-input"
                                        prop:value=jget_opt_str(&c, "value") on:input=move |ev| cset_opt("value", &event_target_value(&ev)) />
                                </div>
                            </div>
                        </div>
                    }.into_any(),

                    "mode_is" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Mode"</label>
                            <ModeSelect value=jget_str(&c, "mode_id")
                                on_select=Callback::new(move |id: String| cset("mode_id", json!(id))) />
                            <label class="field-label">"Expected state"</label>
                            <select class="hc-select" on:change=move |ev| cset("on", json!(event_target_value(&ev)=="true"))>
                                <option value="true" selected=c["on"].as_bool()!=Some(false)>"On"</option>
                                <option value="false" selected=c["on"].as_bool()==Some(false)>"Off"</option>
                            </select>
                        </div>
                    }.into_any(),

                    // ── OR / AND / XOR — nested condition list ─────────
                    "or" | "and" | "xor" => {
                        let label = match t.as_str() {
                            "or"  => "ANY of these must be true (OR)",
                            "and" => "ALL of these must be true (AND)",
                            "xor" => "EXACTLY ONE must be true (XOR)",
                            _     => "",
                        };
                        let border_class = match t.as_str() {
                            "or"  => "cond-group cond-group--or",
                            "and" => "cond-group cond-group--and",
                            _     => "cond-group cond-group--xor",
                        };
                        let np_list_sv = np_stored; // StoredValue is Copy
                        view! {
                            <div class=border_class>
                                <p class="cond-group-label">{label}</p>
                                {move || {
                                    // Read the nested conditions array
                                    let mut parent = rule.get()["conditions"][index].clone();
                                    for &ni in np_list_sv.get_value().iter() { parent = parent["conditions"][ni].clone(); }
                                    let arr = parent["conditions"].as_array().cloned().unwrap_or_default();
                                    let total = arr.len();

                                    let items = arr.into_iter().enumerate().map(|(ci, _)| {
                                        let is_first = ci == 0;
                                        let is_last = ci + 1 >= total;
                                        let mut child_path = np_list_sv.get_value();
                                        child_path.push(ci);
                                        view! {
                                            <div class="json-row">
                                                <div class="json-row-controls">
                                                    <span class="json-row-index">{ci + 1}</span>
                                                    <button class="hc-btn hc-btn--sm hc-btn--outline" title="Move up" disabled=is_first
                                                        on:click={
                                                            let np_c = np_list_sv.get_value();
                                                            move |_| {
                                                                let np_c = np_c.clone();
                                                                rule.update(|v| {
                                                                    let mut target = &mut v["conditions"][index];
                                                                    for &ni in np_c.iter() { target = &mut target["conditions"][ni]; }
                                                                    if let Some(arr) = target["conditions"].as_array_mut() { if ci > 0 { arr.swap(ci - 1, ci); } }
                                                                });
                                                            }
                                                        }
                                                    ><span class="material-icons" style="font-size:14px">"arrow_upward"</span></button>
                                                    <button class="hc-btn hc-btn--sm hc-btn--outline" title="Move down" disabled=is_last
                                                        on:click={
                                                            let np_c = np_list_sv.get_value();
                                                            move |_| {
                                                                let np_c = np_c.clone();
                                                                rule.update(|v| {
                                                                    let mut target = &mut v["conditions"][index];
                                                                    for &ni in np_c.iter() { target = &mut target["conditions"][ni]; }
                                                                    if let Some(arr) = target["conditions"].as_array_mut() { if ci + 1 < arr.len() { arr.swap(ci, ci + 1); } }
                                                                });
                                                            }
                                                        }
                                                    ><span class="material-icons" style="font-size:14px">"arrow_downward"</span></button>
                                                    <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove"
                                                        on:click={
                                                            let np_c = np_list_sv.get_value();
                                                            move |_| {
                                                                let np_c = np_c.clone();
                                                                rule.update(|v| {
                                                                    let mut target = &mut v["conditions"][index];
                                                                    for &ni in np_c.iter() { target = &mut target["conditions"][ni]; }
                                                                    if let Some(arr) = target["conditions"].as_array_mut() { arr.remove(ci); }
                                                                });
                                                            }
                                                        }
                                                    ><span class="material-icons" style="font-size:14px">"close"</span></button>
                                                </div>
                                                <ConditionEditor rule=rule index=index nested_path=child_path />
                                            </div>
                                        }
                                    }).collect_view();

                                    items
                                }}
                                <button class="hc-btn hc-btn--sm hc-btn--outline" style="margin-top:0.25rem"
                                    on:click={
                                        let np_c = np_list_sv.get_value();
                                        move |_| {
                                            let np_c = np_c.clone();
                                            rule.update(|v| {
                                                let mut target = &mut v["conditions"][index];
                                                for &ni in np_c.iter() { target = &mut target["conditions"][ni]; }
                                                if let Some(arr) = target["conditions"].as_array_mut() {
                                                    arr.push(default_condition("device_state"));
                                                } else {
                                                    target["conditions"] = json!([default_condition("device_state")]);
                                                }
                                            });
                                        }
                                    }
                                >"+ Add condition"</button>
                            </div>
                        }.into_any()
                    },

                    // ── NOT — single wrapped condition ───────────────────
                    "not" => {
                        let np_not_sv = np_stored;
                        view! {
                            <div class="cond-group cond-group--not">
                                <p class="cond-group-label">"NOT — inverts the result"</p>
                                {move || {
                                    let mut parent = rule.get()["conditions"][index].clone();
                                    for &ni in np_not_sv.get_value().iter() { parent = parent["conditions"][ni].clone(); }
                                    let has_inner = !parent["condition"].is_null();

                                    if has_inner {
                                        // Render a single ConditionEditor for the wrapped condition.
                                        // NOT uses "condition" (singular), not "conditions" array.
                                        // We handle this by rendering the inner JSON inline.
                                        let inner = parent["condition"].clone();
                                        let inner_type = inner["type"].as_str().unwrap_or("device_state").to_string();
                                        view! {
                                            <div class="json-row">
                                                <div class="json-row-controls">
                                                    <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove inner condition"
                                                        on:click={
                                                            let np_c = np_not_sv.get_value();
                                                            move |_| {
                                                                let np_c = np_c.clone();
                                                                rule.update(|v| {
                                                                    let mut target = &mut v["conditions"][index];
                                                                    for &ni in np_c.iter() { target = &mut target["conditions"][ni]; }
                                                                    target["condition"] = Value::Null;
                                                                });
                                                            }
                                                        }
                                                    ><span class="material-icons" style="font-size:14px">"close"</span></button>
                                                </div>
                                                // NOT's inner condition as inline JSON (single condition, not array-indexable)
                                                <label class="field-label">"Inner condition type"</label>
                                                <select class="hc-select" on:change={
                                                    let np_c = np_not_sv.get_value();
                                                    move |ev| {
                                                        let new_t = event_target_value(&ev);
                                                        let np_c = np_c.clone();
                                                        rule.update(|v| {
                                                            let mut target = &mut v["conditions"][index];
                                                            for &ni in np_c.iter() { target = &mut target["conditions"][ni]; }
                                                            target["condition"] = default_condition(&new_t);
                                                        });
                                                    }
                                                }>
                                                    {[("device_state","Device state"),("time_window","Time window"),("script_expression","Script"),
                                                      ("time_elapsed","Time elapsed"),("mode_is","Mode is"),("hub_variable","Hub variable")]
                                                        .map(|(v,l)| view! { <option value=v selected=inner_type==v>{l}</option> }).collect_view()}
                                                </select>
                                                <textarea class="hc-textarea hc-textarea--code" rows="3"
                                                    prop:value=serde_json::to_string_pretty(&inner).unwrap_or_default()
                                                    on:input={
                                                        let np_c = np_not_sv.get_value();
                                                        move |ev| {
                                                            if let Ok(parsed) = serde_json::from_str::<Value>(&event_target_value(&ev)) {
                                                                let np_c = np_c.clone();
                                                                rule.update(|v| {
                                                                    let mut target = &mut v["conditions"][index];
                                                                    for &ni in np_c.iter() { target = &mut target["conditions"][ni]; }
                                                                    target["condition"] = parsed;
                                                                });
                                                            }
                                                        }
                                                    }
                                                />
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <button class="hc-btn hc-btn--sm hc-btn--outline"
                                                on:click={
                                                    let np_c = np_not_sv.get_value();
                                                    move |_| {
                                                        let np_c = np_c.clone();
                                                        rule.update(|v| {
                                                            let mut target = &mut v["conditions"][index];
                                                            for &ni in np_c.iter() { target = &mut target["conditions"][ni]; }
                                                            target["condition"] = default_condition("device_state");
                                                        });
                                                    }
                                                }
                                            >"+ Set inner condition"</button>
                                        }.into_any()
                                    }
                                }}
                            </div>
                        }.into_any()
                    },

                    // Unknown → JSON fallback
                    _ => view! {
                        <div class="trigger-fields">
                            <JsonBlock rule=rule path_prefix="conditions" index=index rows=6 />
                        </div>
                    }.into_any(),
                }
            }}
        </div>
    }
}

// ── ActionEditor ─────────────────────────────────────────────────────────────

/// Map action type to category key for the category selector.
fn action_category(t: &str) -> &'static str {
    match t {
        "set_device_state" | "fade_device" | "capture_device_state" | "restore_device_state" => "device",
        "conditional" => "conditional",
        "notify" => "notify",
        "set_mode" => "mode",
        "delay" | "wait_for_event" | "wait_for_expression" => "timing",
        "run_script" => "script",
        "run_rule_actions" | "pause_rule" | "resume_rule" | "cancel_delays" | "cancel_rule_timers" => "rule_ctrl",
        _ => "more",
    }
}

/// Default action for a category.
fn category_default(cat: &str) -> &'static str {
    match cat {
        "device" => "set_device_state",
        "conditional" => "conditional",
        "notify" => "notify",
        "mode" => "set_mode",
        "timing" => "delay",
        "script" => "run_script",
        "rule_ctrl" => "run_rule_actions",
        _ => "log_message",
    }
}

const ACTION_CATEGORIES: &[(&str, &str, &str)] = &[
    ("device",      "Control device",  "devices"),
    ("conditional", "IF / ELSE",       "call_split"),
    ("notify",      "Notify",          "notifications"),
    ("mode",        "Set mode",        "tune"),
    ("timing",      "Delay / Wait",    "schedule"),
    ("script",      "Script",          "code"),
    ("rule_ctrl",   "Rule control",    "smart_toy"),
    ("more",        "More…",           "more_horiz"),
];

#[component]
fn ActionEditor(
    rule: RwSignal<Value>,
    path: &'static str,
    index: usize,
    /// For nested actions inside block actions (e.g. conditional then_actions).
    #[prop(optional)] nested_key: Option<&'static str>,
    #[prop(optional)] nested_index: Option<usize>,
) -> impl IntoView {
    // Path-aware accessors that handle both top-level and nested actions.
    let ag = move || -> Value {
        match (nested_key, nested_index) {
            (Some(nk), Some(ni)) => rule.get()[path][index][nk][ni].clone(),
            _ => rule.get()[path][index].clone(),
        }
    };
    let aset = move |key: &'static str, val: Value| {
        rule.update(|v| {
            match (nested_key, nested_index) {
                (Some(nk), Some(ni)) => { v[path][index][nk][ni][key] = val; }
                _ => { v[path][index][key] = val; }
            }
        });
    };
    let aset_opt = move |key: &'static str, raw: &str| {
        rule.update(|v| {
            let target = match (nested_key, nested_index) {
                (Some(nk), Some(ni)) => &mut v[path][index][nk][ni],
                _ => &mut v[path][index],
            };
            if raw.trim().is_empty() { if let Some(o) = target.as_object_mut() { o.remove(key); } }
            else {
                let parsed = serde_json::from_str::<Value>(raw.trim()).unwrap_or_else(|_| json!(raw.trim()));
                target[key] = parsed;
            }
        });
    };
    let aset_u64 = move |key: &'static str, raw: String| {
        rule.update(|v| {
            let target = match (nested_key, nested_index) {
                (Some(nk), Some(ni)) => &mut v[path][index][nk][ni],
                _ => &mut v[path][index],
            };
            if raw.trim().is_empty() { if let Some(o) = target.as_object_mut() { o.remove(key); } }
            else if let Ok(n) = raw.trim().parse::<u64>() { target[key] = json!(n); }
        });
    };
    // For replacing the entire action (category/type change)
    let aset_whole = move |val: Value| {
        rule.update(|v| {
            match (nested_key, nested_index) {
                (Some(nk), Some(ni)) => { v[path][index][nk][ni] = val; }
                _ => { v[path][index] = val; }
            }
        });
    };

    view! {
        <div class="action-editor">
            // ── Enabled toggle ───────────────────────────────────────────
            <div class="action-header-row">
                <label class="rule-meta-inline">
                    <input type="checkbox"
                        prop:checked=move || ag()["enabled"].as_bool().unwrap_or(true)
                        on:change=move |ev| {
                            use wasm_bindgen::JsCast;
                            let checked = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).map(|el| el.checked()).unwrap_or(true);
                            aset("enabled", json!(checked));
                        }
                    />
                    " Enabled"
                </label>
            </div>

            // ── Category selector ────────────────────────────────────────
            <div class="action-cat-row">
                {ACTION_CATEGORIES.iter().map(|(key, label, icon)| {
                    view! {
                        <button
                            class="action-cat-btn"
                            class:action-cat-btn--active=move || action_category(ag()["type"].as_str().unwrap_or("")) == *key
                            on:click=move |_| {
                                // Only reset if switching to a different category
                                let current_cat = action_category(ag()["type"].as_str().unwrap_or(""));
                                if current_cat == *key { return; }
                                let def = category_default(key);
                                let enabled = ag()["enabled"].as_bool().unwrap_or(true);
                                let mut new_action = default_action(def);
                                new_action["enabled"] = json!(enabled);
                                aset_whole(new_action);
                            }
                        >
                            <span class="material-icons" style="font-size:16px">{*icon}</span>
                            <span class="action-cat-label">{*label}</span>
                        </button>
                    }
                }).collect_view()}
            </div>

            // ── Category-specific editor ─────────────────────────────────
            {move || {
                let a = ag();
                let t = a["type"].as_str().unwrap_or("").to_string();
                let cat = action_category(&t);

                match cat {
                    // ── DEVICE ────────────────────────────────────────────
                    "device" => {
                        // Sub-type selector within device category
                        view! {
                            <div class="trigger-fields">
                                <select class="hc-select" on:change=move |ev| {
                                    let new_t = event_target_value(&ev);
                                    let enabled = ag()["enabled"].as_bool().unwrap_or(true);
                                    let mut a = default_action(&new_t); a["enabled"] = json!(enabled); aset_whole(a);
                                }>
                                    {[("set_device_state","Command device"),("fade_device","Fade device"),("capture_device_state","Capture state"),("restore_device_state","Restore state")]
                                        .map(|(v,l)| view! { <option value=v selected=t==v>{l}</option> }).collect_view()}
                                </select>
                                {match t.as_str() {
                                    "set_device_state" | "fade_device" => view! {
                                        <label class="field-label">"Device"</label>
                                        <DeviceSelect value=jget_str(&a,"device_id")
                                            on_select=Callback::new(move |id: String| aset("device_id", json!(id))) />
                                        <DeviceStateBuilder rule=rule path=path index=index nested_key=nested_key nested_index=nested_index />
                                        {(t == "fade_device").then(|| view! {
                                            <label class="field-label">"Duration (seconds)"</label>
                                            <input type="number" class="hc-input hc-input--sm" style="width:8rem"
                                                prop:value=a["duration_secs"].as_u64().unwrap_or(30).to_string()
                                                on:input=move |ev| aset_u64("duration_secs", event_target_value(&ev)) />
                                        })}
                                    }.into_any(),
                                    "capture_device_state" => view! {
                                        <label class="field-label">"Snapshot key"</label>
                                        <input type="text" class="hc-input" prop:value=jget_str(&a,"key")
                                            on:input=move |ev| aset("key", json!(event_target_value(&ev))) />
                                        <label class="field-label">"Devices"</label>
                                        <p class="msg-muted" style="font-size:0.78rem">"Select devices to capture:"</p>
                                        // TODO: multi-device chip selector
                                        <input type="text" class="hc-input" placeholder="device_id_1, device_id_2"
                                            prop:value=a["device_ids"].as_array().map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default()
                                            on:input=move |ev| {
                                                let ids: Vec<Value> = event_target_value(&ev).split(',').map(|s| json!(s.trim())).filter(|v| v.as_str()!=Some("")).collect();
                                                aset("device_ids", json!(ids));
                                            } />
                                    }.into_any(),
                                    "restore_device_state" => view! {
                                        <label class="field-label">"Snapshot key"</label>
                                        <input type="text" class="hc-input" prop:value=jget_str(&a,"key")
                                            on:input=move |ev| aset("key", json!(event_target_value(&ev))) />
                                    }.into_any(),
                                    _ => view! { <span /> }.into_any(),
                                }}
                            </div>
                        }.into_any()
                    },

                    // ── CONDITIONAL (IF/ELSE) ────────────────────────────
                    "conditional" => view! {
                        <div class="trigger-fields">
                            <div class="cond-branch cond-branch--if">
                                <span class="cond-branch-label">"IF"</span>
                                <label class="field-label">"Condition (Rhai expression)"</label>
                                <textarea class="hc-textarea hc-textarea--code" rows="2"
                                    prop:value=jget_str(&a, "condition")
                                    on:input=move |ev| aset("condition", json!(event_target_value(&ev))) />
                                <label class="field-label">"THEN actions:"</label>
                                <NestedItemList rule=rule path=path index=index key="then_actions" />
                            </div>

                            // ELSE-IF branches
                            {move || {
                                let branches = rule.get()[path][index]["else_if"].as_array().cloned().unwrap_or_default();
                                branches.into_iter().enumerate().map(|(bi, branch)| {
                                    let cond = branch["condition"].as_str().unwrap_or("").to_string();
                                    view! {
                                        <div class="cond-branch cond-branch--elseif">
                                            <div class="cond-branch-header">
                                                <span class="cond-branch-label">"ELSE IF"</span>
                                                <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove branch"
                                                    on:click=move |_| rule.update(|v| {
                                                        if let Some(arr) = v[path][index]["else_if"].as_array_mut() { arr.remove(bi); }
                                                    })
                                                ><span class="material-icons" style="font-size:14px">"close"</span></button>
                                            </div>
                                            <label class="field-label">"Condition"</label>
                                            <textarea class="hc-textarea hc-textarea--code" rows="2"
                                                prop:value=cond
                                                on:input=move |ev| rule.update(|v| {
                                                    v[path][index]["else_if"][bi]["condition"] = json!(event_target_value(&ev));
                                                }) />
                                            <label class="field-label">"THEN actions:"</label>
                                            <NestedElseIfActions rule=rule path=path index=index branch_index=bi />
                                        </div>
                                    }
                                }).collect_view()
                            }}

                            <button class="hc-btn hc-btn--sm hc-btn--outline" style="margin:0.25rem 0"
                                on:click=move |_| rule.update(|v| {
                                    let branch = json!({"condition": "", "actions": []});
                                    if let Some(arr) = v[path][index]["else_if"].as_array_mut() { arr.push(branch); }
                                    else { v[path][index]["else_if"] = json!([branch]); }
                                })
                            >"+ Add ELSE IF"</button>

                            <div class="cond-branch cond-branch--else">
                                <span class="cond-branch-label">"ELSE"</span>
                                <NestedItemList rule=rule path=path index=index key="else_actions" />
                            </div>
                        </div>
                    }.into_any(),

                    // ── NOTIFY ────────────────────────────────────────────
                    "notify" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Channel"</label>
                            <select class="hc-select" on:change=move |ev| aset("channel", json!(event_target_value(&ev)))>
                                {[("all","All channels"),("telegram","Telegram"),("pushover","Pushover"),("email","Email")]
                                    .map(|(v,l)| view! { <option value=v selected=a["channel"].as_str()==Some(v)>{l}</option> }).collect_view()}
                            </select>
                            <label class="field-label">"Title (optional)"</label>
                            <input type="text" class="hc-input" prop:value=jget_opt_str(&a,"title")
                                on:input=move |ev| aset_opt("title", &event_target_value(&ev)) />
                            <label class="field-label">"Message"</label>
                            <textarea class="hc-textarea" rows="2" prop:value=jget_str(&a,"message")
                                on:input=move |ev| aset("message", json!(event_target_value(&ev))) />
                        </div>
                    }.into_any(),

                    // ── MODE ──────────────────────────────────────────────
                    "mode" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Mode"</label>
                            <ModeSelect value=jget_str(&a,"mode_id")
                                on_select=Callback::new(move |id: String| aset("mode_id", json!(id))) />
                            <label class="field-label">"Command"</label>
                            <div class="toggle-group">
                                <button class:active=a["command"].as_str()==Some("on")
                                    on:click=move |_| aset("command", json!("on"))>"On"</button>
                                <button class:active=a["command"].as_str()==Some("off")
                                    on:click=move |_| aset("command", json!("off"))>"Off"</button>
                                <button class:active=a["command"].as_str()==Some("toggle")
                                    on:click=move |_| aset("command", json!("toggle"))>"Toggle"</button>
                            </div>
                        </div>
                    }.into_any(),

                    // ── TIMING / WAIT ─────────────────────────────────────
                    "timing" => view! {
                        <div class="trigger-fields">
                            <select class="hc-select" on:change=move |ev| {
                                let new_t = event_target_value(&ev);
                                let enabled = ag()["enabled"].as_bool().unwrap_or(true);
                                rule.update(|v| { v[path][index] = default_action(&new_t); v[path][index]["enabled"] = json!(enabled); });
                            }>
                                {[("delay","Delay"),("wait_for_event","Wait for event"),("wait_for_expression","Wait for expression")]
                                    .map(|(v,l)| view! { <option value=v selected=t==v>{l}</option> }).collect_view()}
                            </select>
                            {match t.as_str() {
                                "delay" => view! {
                                    <div class="control-row">
                                        <span class="control-label">"Duration"</span>
                                        <div class="state-slider-row">
                                            <input type="range" class="state-slider" min="1" max="300" step="1"
                                                prop:value=a["duration_secs"].as_u64().unwrap_or(5).to_string()
                                                on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<u64>() { aset("duration_secs", json!(n)); } } />
                                            <span class="state-slider-val">{format!("{}s", a["duration_secs"].as_u64().unwrap_or(5))}</span>
                                        </div>
                                    </div>
                                    <div class="control-row">
                                        <span class="control-label">"Cancelable"</span>
                                        <div class="toggle-group">
                                            <button class:active=a["cancelable"].as_bool()==Some(true)
                                                on:click=move |_| aset("cancelable", json!(true))>"Yes"</button>
                                            <button class:active=a["cancelable"].as_bool()!=Some(true)
                                                on:click=move |_| aset("cancelable", json!(false))>"No"</button>
                                        </div>
                                    </div>
                                }.into_any(),
                                "wait_for_event" => view! {
                                    <label class="field-label">"Device (optional)"</label>
                                    <DeviceSelect value=jget_opt_str(&a,"device_id")
                                        on_select=Callback::new(move |id: String| aset_opt("device_id", &id)) />
                                    <label class="field-label">"Attribute (optional)"</label>
                                    <AttributeSelect device_id=jget_opt_str(&a,"device_id") value=jget_opt_str(&a,"attribute")
                                        on_select=Callback::new(move |attr: String| aset_opt("attribute", &attr)) />
                                    <label class="field-label">"Timeout (ms, blank = no timeout)"</label>
                                    <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="none"
                                        prop:value=jget_u64_str(&a,"timeout_ms")
                                        on:input=move |ev| aset_u64("timeout_ms", event_target_value(&ev)) />
                                }.into_any(),
                                "wait_for_expression" => view! {
                                    <label class="field-label">"Rhai expression"</label>
                                    <textarea class="hc-textarea hc-textarea--code" rows="3"
                                        prop:value=jget_str(&a,"expression")
                                        on:input=move |ev| aset("expression", json!(event_target_value(&ev))) />
                                    <label class="field-label">"Timeout (ms, blank = no timeout)"</label>
                                    <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="none"
                                        prop:value=jget_u64_str(&a,"timeout_ms")
                                        on:input=move |ev| aset_u64("timeout_ms", event_target_value(&ev)) />
                                }.into_any(),
                                _ => view! { <span /> }.into_any(),
                            }}
                        </div>
                    }.into_any(),

                    // ── SCRIPT ────────────────────────────────────────────
                    "script" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Rhai script"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="6"
                                prop:value=jget_str(&a,"script")
                                on:input=move |ev| aset("script", json!(event_target_value(&ev))) />
                        </div>
                    }.into_any(),

                    // ── RULE CONTROL ──────────────────────────────────────
                    "rule_ctrl" => view! {
                        <div class="trigger-fields">
                            <select class="hc-select" on:change=move |ev| {
                                let new_t = event_target_value(&ev);
                                let enabled = ag()["enabled"].as_bool().unwrap_or(true);
                                rule.update(|v| { v[path][index] = default_action(&new_t); v[path][index]["enabled"] = json!(enabled); });
                            }>
                                {[("run_rule_actions","Run rule actions"),("pause_rule","Pause rule"),("resume_rule","Resume rule"),
                                  ("cancel_delays","Cancel delays"),("cancel_rule_timers","Cancel rule timers")]
                                    .map(|(v,l)| view! { <option value=v selected=t==v>{l}</option> }).collect_view()}
                            </select>
                            {match t.as_str() {
                                "run_rule_actions" | "pause_rule" | "resume_rule" => view! {
                                    <label class="field-label">"Rule"</label>
                                    <RuleSelect value=jget_str(&a,"rule_id")
                                        on_select=Callback::new(move |id: String| aset("rule_id", json!(id))) />
                                }.into_any(),
                                "cancel_delays" => view! {
                                    <label class="field-label">"Cancel key (blank = all)"</label>
                                    <input type="text" class="hc-input" prop:value=jget_opt_str(&a,"key")
                                        on:input=move |ev| aset_opt("key", &event_target_value(&ev)) />
                                }.into_any(),
                                "cancel_rule_timers" => view! {
                                    <label class="field-label">"Rule (blank = current rule)"</label>
                                    <RuleSelect value=jget_opt_str(&a,"rule_id")
                                        on_select=Callback::new(move |id: String| aset_opt("rule_id", &id)) />
                                }.into_any(),
                                _ => view! { <span /> }.into_any(),
                            }}
                        </div>
                    }.into_any(),

                    // ── MORE (expanded action types) ─────────────────────
                    _ => view! {
                        <div class="trigger-fields">
                            <select class="hc-select" on:change=move |ev| {
                                let new_t = event_target_value(&ev);
                                let enabled = ag()["enabled"].as_bool().unwrap_or(true);
                                rule.update(|v| { v[path][index] = default_action(&new_t); v[path][index]["enabled"] = json!(enabled); });
                            }>
                                {[("log_message","Log message"),("comment","Comment"),("fire_event","Fire event"),
                                  ("publish_mqtt","Publish MQTT"),("call_service","HTTP request"),
                                  ("set_variable","Set variable"),("set_hub_variable","Set hub variable"),
                                  ("set_private_boolean","Set private boolean"),
                                  ("stop_rule_chain","Stop rule chain"),("exit_rule","Exit rule"),
                                  ("parallel","Parallel"),("repeat_count","Repeat N times"),
                                  ("repeat_until","Repeat until"),("repeat_while","Repeat while"),
                                  ("ping_host","Ping host"),
                                  ("set_device_state_per_mode","Device per mode"),("delay_per_mode","Delay per mode"),
                                  ("activate_scene_per_mode","Scene per mode")]
                                    .map(|(v,l)| view! { <option value=v selected=t==v>{l}</option> }).collect_view()}
                            </select>
                            {match t.as_str() {
                                "log_message" => view! {
                                    <label class="field-label">"Level"</label>
                                    <select class="hc-select" on:change=move |ev| aset_opt("level", &event_target_value(&ev))>
                                        {[("","Info (default)"),("debug","Debug"),("warn","Warning"),("error","Error")]
                                            .map(|(v,l)| view! { <option value=v selected=a["level"].as_str().unwrap_or("")==v>{l}</option> }).collect_view()}
                                    </select>
                                    <label class="field-label">"Message"</label>
                                    <textarea class="hc-textarea" rows="2" prop:value=jget_str(&a,"message")
                                        on:input=move |ev| aset("message", json!(event_target_value(&ev))) />
                                }.into_any(),
                                "comment" => view! {
                                    <textarea class="hc-textarea" rows="2" placeholder="Comment text"
                                        prop:value=jget_str(&a,"text")
                                        on:input=move |ev| aset("text", json!(event_target_value(&ev))) />
                                }.into_any(),
                                "fire_event" => view! {
                                    <label class="field-label">"Event type"</label>
                                    <input type="text" class="hc-input" prop:value=jget_str(&a,"event_type")
                                        on:input=move |ev| aset("event_type", json!(event_target_value(&ev))) />
                                    <label class="field-label">"Payload (JSON)"</label>
                                    <textarea class="hc-textarea hc-textarea--code" rows="2"
                                        prop:value=serde_json::to_string_pretty(&a["payload"]).unwrap_or_default()
                                        on:input=move |ev| { if let Ok(p) = serde_json::from_str::<Value>(&event_target_value(&ev)) { aset("payload", p); } } />
                                }.into_any(),
                                "publish_mqtt" => view! {
                                    <label class="field-label">"Topic"</label>
                                    <input type="text" class="hc-input hc-textarea--code" prop:value=jget_str(&a,"topic")
                                        on:input=move |ev| aset("topic", json!(event_target_value(&ev))) />
                                    <label class="field-label">"Payload"</label>
                                    <textarea class="hc-textarea hc-textarea--code" rows="2" prop:value=jget_str(&a,"payload")
                                        on:input=move |ev| aset("payload", json!(event_target_value(&ev))) />
                                }.into_any(),
                                "call_service" => view! {
                                    <label class="field-label">"URL"</label>
                                    <input type="text" class="hc-input" prop:value=jget_str(&a,"url")
                                        on:input=move |ev| aset("url", json!(event_target_value(&ev))) />
                                    <label class="field-label">"Method"</label>
                                    <select class="hc-select" on:change=move |ev| aset("method", json!(event_target_value(&ev)))>
                                        {["GET","POST","PUT","PATCH","DELETE"].map(|m| view! { <option value=m selected=a["method"].as_str()==Some(m)>{m}</option> }).collect_view()}
                                    </select>
                                    <label class="field-label">"Body (JSON, optional)"</label>
                                    <textarea class="hc-textarea hc-textarea--code" rows="2"
                                        prop:value=serde_json::to_string_pretty(&a["body"]).unwrap_or_default()
                                        on:input=move |ev| { if let Ok(p) = serde_json::from_str::<Value>(&event_target_value(&ev)) { aset("body", p); } } />
                                }.into_any(),
                                "set_variable" | "set_hub_variable" => view! {
                                    <label class="field-label">"Variable name"</label>
                                    <input type="text" class="hc-input" prop:value=jget_str(&a,"name")
                                        on:input=move |ev| aset("name", json!(event_target_value(&ev))) />
                                    <label class="field-label">"Operation"</label>
                                    <select class="hc-select" on:change=move |ev| aset_opt("op", &event_target_value(&ev))>
                                        {[("","Set (replace)"),("add","Add"),("subtract","Subtract"),("multiply","Multiply"),("divide","Divide"),("toggle","Toggle")]
                                            .map(|(v,l)| view! { <option value=v selected=a["op"].as_str().unwrap_or("")==v>{l}</option> }).collect_view()}
                                    </select>
                                    <label class="field-label">"Value (JSON)"</label>
                                    <input type="text" class="hc-input" prop:value=jget_opt_str(&a,"value")
                                        on:input=move |ev| aset_opt("value", &event_target_value(&ev)) />
                                }.into_any(),
                                "set_private_boolean" => view! {
                                    <label class="field-label">"Name"</label>
                                    <input type="text" class="hc-input" prop:value=jget_str(&a,"name")
                                        on:input=move |ev| aset("name", json!(event_target_value(&ev))) />
                                    <label class="field-label">"Value"</label>
                                    <div class="toggle-group">
                                        <button class:active=a["value"].as_bool()==Some(true)
                                            on:click=move |_| aset("value", json!(true))>"True"</button>
                                        <button class:active=a["value"].as_bool()==Some(false)
                                            on:click=move |_| aset("value", json!(false))>"False"</button>
                                    </div>
                                }.into_any(),
                                "stop_rule_chain" | "exit_rule" => view! {
                                    <p class="msg-muted" style="font-size:0.85rem">
                                        {if t == "stop_rule_chain" { "Stops lower-priority rules." } else { "Halts remaining actions." }}
                                    </p>
                                }.into_any(),
                                // Block actions with nested action lists
                                "parallel" => view! {
                                    <NestedItemList rule=rule path=path index=index key="actions" />
                                }.into_any(),
                                "repeat_count" => view! {
                                    <div class="trigger-row-2">
                                        <div>
                                            <label class="field-label">"Count"</label>
                                            <input type="number" class="hc-input hc-input--sm" min="1"
                                                prop:value=a["count"].as_u64().unwrap_or(3).to_string()
                                                on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<u64>() { aset("count", json!(n)); } } />
                                        </div>
                                        <div>
                                            <label class="field-label">"Interval (ms)"</label>
                                            <input type="number" class="hc-input hc-input--sm" placeholder="none"
                                                prop:value=jget_u64_str(&a,"interval_ms")
                                                on:input=move |ev| aset_u64("interval_ms", event_target_value(&ev)) />
                                        </div>
                                    </div>
                                    <NestedItemList rule=rule path=path index=index key="actions" />
                                }.into_any(),
                                "repeat_until" | "repeat_while" => view! {
                                    <label class="field-label">"Condition (Rhai)"</label>
                                    <textarea class="hc-textarea hc-textarea--code" rows="2"
                                        prop:value=jget_str(&a,"condition")
                                        on:input=move |ev| aset("condition", json!(event_target_value(&ev))) />
                                    <div class="trigger-row-2">
                                        <div>
                                            <label class="field-label">"Max iterations"</label>
                                            <input type="number" class="hc-input hc-input--sm" placeholder="unlimited"
                                                prop:value=jget_u64_str(&a,"max_iterations")
                                                on:input=move |ev| aset_u64("max_iterations", event_target_value(&ev)) />
                                        </div>
                                        <div>
                                            <label class="field-label">"Interval (ms)"</label>
                                            <input type="number" class="hc-input hc-input--sm" placeholder="none"
                                                prop:value=jget_u64_str(&a,"interval_ms")
                                                on:input=move |ev| aset_u64("interval_ms", event_target_value(&ev)) />
                                        </div>
                                    </div>
                                    <NestedItemList rule=rule path=path index=index key="actions" />
                                }.into_any(),
                                "ping_host" => view! {
                                    <label class="field-label">"Host"</label>
                                    <input type="text" class="hc-input" prop:value=jget_str(&a,"host")
                                        on:input=move |ev| aset("host", json!(event_target_value(&ev))) />
                                    <div class="trigger-row-2">
                                        <div>
                                            <label class="field-label">"Ping count"</label>
                                            <input type="number" class="hc-input hc-input--sm" placeholder="1"
                                                prop:value=jget_u64_str(&a,"count")
                                                on:input=move |ev| aset_u64("count", event_target_value(&ev)) />
                                        </div>
                                        <div>
                                            <label class="field-label">"Timeout (ms)"</label>
                                            <input type="number" class="hc-input hc-input--sm" placeholder="3000"
                                                prop:value=jget_u64_str(&a,"timeout_ms")
                                                on:input=move |ev| aset_u64("timeout_ms", event_target_value(&ev)) />
                                        </div>
                                    </div>
                                    <div class="cond-branch cond-branch--if">
                                        <span class="cond-branch-label">"Reachable"</span>
                                        <NestedItemList rule=rule path=path index=index key="then_actions" />
                                    </div>
                                    <div class="cond-branch cond-branch--else">
                                        <span class="cond-branch-label">"Unreachable"</span>
                                        <NestedItemList rule=rule path=path index=index key="else_actions" />
                                    </div>
                                }.into_any(),
                                // Remaining block types → JSON
                                _ => view! {
                                    <JsonBlock rule=rule path_prefix=path index=index rows=8 />
                                }.into_any(),
                            }}
                        </div>
                    }.into_any(),
                }
            }}
        </div>
    }
}

// ── JsonBlock ────────────────────────────────────────────────────────────────
// Inline JSON textarea that reads/writes rule[path_prefix][index].

#[component]
fn JsonBlock(
    rule: RwSignal<Value>,
    path_prefix: &'static str,
    index: usize,
    #[prop(optional)] nested_key: Option<&'static str>,
    #[prop(optional)] nested_index: Option<usize>,
    #[prop(default = 6)] rows: u32,
) -> impl IntoView {
    let json_err: RwSignal<Option<String>> = RwSignal::new(None);
    let initial = match (nested_key, nested_index) {
        (Some(nk), Some(ni)) => rule.get_untracked()[path_prefix][index][nk][ni].clone(),
        _ => rule.get_untracked()[path_prefix][index].clone(),
    };
    let text: RwSignal<String> = RwSignal::new(
        serde_json::to_string_pretty(&initial).unwrap_or_default()
    );

    view! {
        <div class="json-editor">
            <textarea class="hc-textarea hc-textarea--code" rows=rows
                prop:value=move || text.get()
                on:input=move |ev| {
                    let raw = event_target_value(&ev);
                    text.set(raw.clone());
                    match serde_json::from_str::<Value>(&raw) {
                        Ok(parsed) => {
                            rule.update(|v| {
                                match (nested_key, nested_index) {
                                    (Some(nk), Some(ni)) => { v[path_prefix][index][nk][ni] = parsed; }
                                    _ => { v[path_prefix][index] = parsed; }
                                }
                            });
                            json_err.set(None);
                        }
                        Err(e) => json_err.set(Some(e.to_string())),
                    }
                }
            />
            {move || json_err.get().map(|e| view! {
                <p class="msg-error" style="font-size:0.78rem; margin:0.2rem 0 0">{e}</p>
            })}
        </div>
    }
}

// ── DeviceSelect ─────────────────────────────────────────────────────────────
// Dropdown that shows device names, stores device_id.

#[component]
fn DeviceSelect(
    /// Current device_id value.
    value: String,
    /// Called with the selected device_id.
    on_select: Callback<String>,
) -> impl IntoView {
    let devices = use_context::<RwSignal<Vec<DeviceState>>>().unwrap_or(RwSignal::new(vec![]));

    view! {
        <select class="hc-select"
            on:change=move |ev| on_select.run(event_target_value(&ev))
        >
            <option value="" disabled=true selected=value.is_empty()>"— Select device —"</option>
            {move || {
                let mut devs: Vec<DeviceState> = devices.get().into_iter()
                    .filter(|d| !is_scene_like(d))
                    .collect();
                devs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                let current = value.clone();
                let has_current = current.is_empty() || devs.iter().any(|d| d.device_id == current);
                let orphan = if !has_current {
                    Some(view! {
                        <option value=current.clone() selected=true>{format!("{current} (unknown)")}</option>
                    })
                } else { None };

                let options = devs.into_iter().map(|d| {
                    let sel = d.device_id == current;
                    view! {
                        <option value=d.device_id selected=sel>{d.name}</option>
                    }
                }).collect_view();

                view! { {orphan} {options} }
            }}
        </select>
    }
}

// ── DeviceMultiSelect ────────────────────────────────────────────────────────
// For triggers with device_ids: shows selected devices as chips + add dropdown.

#[component]
fn DeviceMultiSelect(
    rule: RwSignal<Rule>,
) -> impl IntoView {
    let devices = use_context::<RwSignal<Vec<DeviceState>>>().unwrap_or(RwSignal::new(vec![]));

    // Read primary device_id + additional device_ids from trigger
    let get_all_ids = move || -> Vec<String> {
        match &rule.get().trigger {
            Trigger::DeviceStateChanged { device_id, device_ids, .. } => {
                if device_ids.is_empty() {
                    if device_id.is_empty() { vec![] } else { vec![device_id.clone()] }
                } else {
                    let mut ids = device_ids.clone();
                    if !device_id.is_empty() && !ids.contains(device_id) {
                        ids.insert(0, device_id.clone());
                    }
                    ids
                }
            }
            _ => vec![],
        }
    };

    let set_ids = move |ids: Vec<String>| {
        rule.update(|r| {
            if let Trigger::DeviceStateChanged { ref mut device_id, ref mut device_ids, .. } = r.trigger {
                if ids.len() <= 1 {
                    *device_id = ids.first().cloned().unwrap_or_default();
                    *device_ids = vec![];
                } else {
                    *device_id = ids[0].clone();
                    *device_ids = ids;
                }
            }
        });
    };

    let device_name = move |id: &str| -> String {
        devices.get().iter().find(|d| d.device_id == id).map(|d| d.name.clone()).unwrap_or_else(|| id.to_string())
    };

    view! {
        <div class="device-multi-select">
            // Current devices as chips
            {move || {
                let ids = get_all_ids();
                ids.into_iter().enumerate().map(|(i, id)| {
                    let name = device_name(&id);
                    view! {
                        <span class="tag-chip">
                            {name}
                            <button class="tag-chip-remove" title="Remove device"
                                on:click=move |_| {
                                    let mut ids = get_all_ids();
                                    ids.remove(i);
                                    set_ids(ids);
                                }
                            >"×"</button>
                        </span>
                    }
                }).collect_view()
            }}

            // Add device dropdown
            {
                let add_device = move |id: String| {
                    if id.is_empty() { return; }
                    let mut ids = get_all_ids();
                    if !ids.contains(&id) { ids.push(id); }
                    set_ids(ids);
                };
                view! {
                    <DeviceSelect value=String::new() on_select=Callback::new(add_device) />
                }
            }
        </div>
    }
}

// ── DeviceStateBuilder ───────────────────────────────────────────────────────
// Each action = one command to the device. Dropdown of available commands,
// then the appropriate control for just that command.

/// Build the list of available commands for a device based on its capabilities.
fn device_commands(d: &DeviceState) -> Vec<(&'static str, &'static str)> {
    let mut cmds = Vec::new();
    let has = |k: &str| d.attributes.contains_key(k);
    let has_f = |k: &str| d.attributes.get(k).and_then(|v| v.as_f64()).is_some();

    // Timer devices
    if is_timer_device(d) {
        cmds.push(("timer_start",   "Start timer"));
        cmds.push(("timer_cancel",  "Cancel timer"));
        cmds.push(("timer_pause",   "Pause timer"));
        cmds.push(("timer_resume",  "Resume timer"));
        cmds.push(("timer_restart", "Restart timer"));
        return cmds;
    }

    // Scene devices (plugin scenes like Lutron/Hue)
    if is_scene_like(d) {
        cmds.push(("activate", "Activate scene"));
        return cmds;
    }

    if has("on") {
        cmds.push(("on_true",  "Turn on"));
        cmds.push(("on_false", "Turn off"));
    }
    if has_f("brightness_pct") { cmds.push(("brightness_pct", "Set brightness")); }
    if has_f("color_temp")     { cmds.push(("color_temp", "Set color temperature")); }
    if has_f("position")       { cmds.push(("position", "Set position")); }
    if has("locked") {
        cmds.push(("lock",   "Lock"));
        cmds.push(("unlock", "Unlock"));
    }
    if is_media_player(d) {
        cmds.push(("play",  "Play"));
        cmds.push(("pause", "Pause"));
        cmds.push(("stop",  "Stop"));
        cmds.push(("next",  "Next track"));
        cmds.push(("prev",  "Previous track"));
        if has_f("volume")  { cmds.push(("set_volume", "Set volume")); }
        if has("muted")     { cmds.push(("set_mute",   "Set mute")); }
        if has("shuffle")   { cmds.push(("set_shuffle", "Set shuffle")); }
        if has("loudness")  { cmds.push(("set_loudness","Set loudness")); }
        if d.attributes.get("bass").and_then(|v| v.as_i64()).is_some()   { cmds.push(("set_bass",   "Set bass")); }
        if d.attributes.get("treble").and_then(|v| v.as_i64()).is_some() { cmds.push(("set_treble", "Set treble")); }
        if !media_available_favorites(d).is_empty() { cmds.push(("play_favorite", "Play favorite")); }
        if !media_available_playlists(d).is_empty() { cmds.push(("play_playlist", "Play playlist")); }
    }
    cmds
}

/// Determine the current command key from the state JSON.
fn detect_command(state: &Value) -> String {
    let obj = match state.as_object() { Some(o) => o, None => return String::new() };
    // Timer commands
    if let Some(cmd) = obj.get("command").and_then(|v| v.as_str()) {
        return match cmd {
            "start"   => "timer_start",
            "cancel"  => "timer_cancel",
            "pause"   => "timer_pause",
            "resume"  => "timer_resume",
            "restart" => "timer_restart",
            _ => cmd,
        }.to_string();
    }
    // Scene activation
    if obj.get("activate").and_then(|v| v.as_bool()) == Some(true) {
        return "activate".to_string();
    }
    // Media action-based commands
    if let Some(act) = obj.get("action").and_then(|v| v.as_str()) {
        return match act {
            "play" => "play", "pause" => "pause", "stop" => "stop",
            "next" => "next", "previous" => "prev",
            "set_volume" => "set_volume", "set_mute" => "set_mute",
            "set_shuffle" => "set_shuffle", "set_bass" => "set_bass",
            "set_treble" => "set_treble", "set_loudness" => "set_loudness",
            "play_favorite" => "play_favorite", "play_playlist" => "play_playlist",
            _ => act,
        }.to_string();
    }
    // Direct attribute commands
    if let Some(v) = obj.get("on") {
        return if v.as_bool() == Some(true) { "on_true" } else { "on_false" }.to_string();
    }
    if obj.contains_key("locked") {
        return if obj["locked"].as_bool() == Some(true) { "lock" } else { "unlock" }.to_string();
    }
    if obj.contains_key("brightness_pct") { return "brightness_pct".to_string(); }
    if obj.contains_key("color_temp")     { return "color_temp".to_string(); }
    if obj.contains_key("position")       { return "position".to_string(); }
    String::new()
}

/// Build the state JSON for a given command key with a default value.
fn command_to_state(cmd: &str, d: &DeviceState) -> Value {
    match cmd {
        "timer_start"    => json!({"command": "start", "duration_secs": 300}),
        "timer_cancel"   => json!({"command": "cancel"}),
        "timer_pause"    => json!({"command": "pause"}),
        "timer_resume"   => json!({"command": "resume"}),
        "timer_restart"  => json!({"command": "restart"}),
        "activate"       => json!({"activate": true}),
        "on_true"        => json!({"on": true}),
        "on_false"       => json!({"on": false}),
        "brightness_pct" => json!({"brightness_pct": d.attributes.get("brightness_pct").and_then(|v| v.as_i64()).unwrap_or(50)}),
        "color_temp"     => json!({"color_temp": d.attributes.get("color_temp").and_then(|v| v.as_i64()).unwrap_or(2700)}),
        "position"       => json!({"position": d.attributes.get("position").and_then(|v| v.as_i64()).unwrap_or(50)}),
        "lock"           => json!({"locked": true}),
        "unlock"         => json!({"locked": false}),
        "play"           => json!({"action": "play"}),
        "pause"          => json!({"action": "pause"}),
        "stop"           => json!({"action": "stop"}),
        "next"           => json!({"action": "next"}),
        "prev"           => json!({"action": "previous"}),
        "set_volume"     => json!({"action": "set_volume", "volume": d.attributes.get("volume").and_then(|v| v.as_i64()).unwrap_or(20)}),
        "set_mute"       => json!({"action": "set_mute", "muted": false}),
        "set_shuffle"    => json!({"action": "set_shuffle", "shuffle": false}),
        "set_loudness"   => json!({"action": "set_loudness", "loudness": true}),
        "set_bass"       => json!({"action": "set_bass", "bass": d.attributes.get("bass").and_then(|v| v.as_i64()).unwrap_or(0)}),
        "set_treble"     => json!({"action": "set_treble", "treble": d.attributes.get("treble").and_then(|v| v.as_i64()).unwrap_or(0)}),
        "play_favorite"  => json!({"action": "play_favorite", "favorite": ""}),
        "play_playlist"  => json!({"action": "play_playlist", "playlist": ""}),
        _                => json!({}),
    }
}

#[component]
fn DeviceStateBuilder(
    rule: RwSignal<Value>,
    path: &'static str,
    index: usize,
    nested_key: Option<&'static str>,
    nested_index: Option<usize>,
) -> impl IntoView {
    let devices = use_context::<RwSignal<Vec<DeviceState>>>().unwrap_or(RwSignal::new(vec![]));
    let nk = nested_key;
    let ni = nested_index;

    // Read the action value at the right path depth
    let action_val = move || -> Value {
        match (nk, ni) {
            (Some(nk), Some(ni)) => rule.get()[path][index][nk][ni].clone(),
            _ => rule.get()[path][index].clone(),
        }
    };

    let state_key = move || {
        if action_val()["type"].as_str() == Some("fade_device") { "target" } else { "state" }
    };

    // Write to the state sub-object at the correct nested path
    let state_update = move |f: Box<dyn FnOnce(&mut Value)>| {
        let sk = state_key();
        rule.update(|v| {
            let target = match (nk, ni) {
                (Some(nk), Some(ni)) => &mut v[path][index][nk][ni][sk],
                _ => &mut v[path][index][sk],
            };
            f(target);
        });
    };
    // Set a single field in the state sub-object
    let sset = move |field: &'static str, val: Value| {
        state_update(Box::new(move |s| { s[field] = val; }));
    };
    let sset_opt = move |field: &'static str, raw: &str| {
        let raw = raw.to_string();
        state_update(Box::new(move |s| {
            if raw.trim().is_empty() { if let Some(o) = s.as_object_mut() { o.remove(field); } }
            else { s[field] = json!(raw.trim()); }
        }));
    };

    view! {
        <div class="state-builder">
            {move || {
                let av = action_val();
                let device_id = av["device_id"].as_str().unwrap_or("").to_string();
                let sk = state_key();
                let state = av[sk].clone();

                let dev = devices.get().into_iter().find(|d| d.device_id == device_id);
                if device_id.is_empty() {
                    return view! { <p class="msg-muted" style="font-size:0.85rem">"Select a device first."</p> }.into_any();
                }
                let d = match dev {
                    Some(d) => d,
                    None => return view! { <p class="msg-muted" style="font-size:0.85rem">"Device not found."</p> }.into_any(),
                };

                let cmds = device_commands(&d);
                let current_cmd = detect_command(&state);
                let favorites = media_available_favorites(&d);
                let playlists = media_available_playlists(&d);

                if cmds.is_empty() {
                    return view! { <p class="msg-muted" style="font-size:0.85rem">"No known commands for this device."</p> }.into_any();
                }

                // Command selector
                let d_for_change = d.clone();
                let cmd_select = view! {
                    <label class="field-label">"Command"</label>
                    <select class="hc-select"
                        on:change=move |ev| {
                            let cmd = event_target_value(&ev);
                            let new_state = command_to_state(&cmd, &d_for_change);
                            let _sk = state_key();
                            state_update(Box::new(|s| { *s = new_state; }));
                        }
                    >
                        <option value="" disabled=true selected=current_cmd.is_empty()>"— Select command —"</option>
                        {cmds.iter().map(|(k, label)| {
                            let sel = *k == current_cmd;
                            view! { <option value=*k selected=sel>{*label}</option> }
                        }).collect_view()}
                    </select>
                };

                // Command-specific control
                let control = match current_cmd.as_str() {
                    "brightness_pct" => view! {
                        <div class="control-row">
                            <span class="control-label">"Brightness"</span>
                            <div class="state-slider-row">
                                <input type="range" class="state-slider" min="0" max="100" step="1"
                                    prop:value=state["brightness_pct"].as_f64().unwrap_or(50.0).to_string()
                                    on:input=move |ev| {
                                        if let Ok(n) = event_target_value(&ev).parse::<i64>() {
                                            let _sk = state_key();
                                            sset("brightness_pct", json!(n));
                                        }
                                    }
                                />
                                <span class="state-slider-val">{format!("{}%", state["brightness_pct"].as_i64().unwrap_or(50))}</span>
                            </div>
                        </div>
                    }.into_any(),

                    "color_temp" => view! {
                        <div class="control-row">
                            <span class="control-label">"Color Temp"</span>
                            <div class="state-slider-row">
                                <input type="range" class="state-slider" min="2000" max="6500" step="100"
                                    prop:value=state["color_temp"].as_f64().unwrap_or(2700.0).to_string()
                                    on:input=move |ev| {
                                        if let Ok(n) = event_target_value(&ev).parse::<i64>() {
                                            let _sk = state_key();
                                            sset("color_temp", json!(n));
                                        }
                                    }
                                />
                                <span class="state-slider-val">{format!("{}K", state["color_temp"].as_i64().unwrap_or(2700))}</span>
                            </div>
                        </div>
                    }.into_any(),

                    "position" => view! {
                        <div class="control-row">
                            <span class="control-label">"Position"</span>
                            <div class="state-slider-row">
                                <input type="range" class="state-slider" min="0" max="100" step="1"
                                    prop:value=state["position"].as_f64().unwrap_or(50.0).to_string()
                                    on:input=move |ev| {
                                        if let Ok(n) = event_target_value(&ev).parse::<i64>() {
                                            let _sk = state_key();
                                            sset("position", json!(n));
                                        }
                                    }
                                />
                                <span class="state-slider-val">{format!("{}%", state["position"].as_i64().unwrap_or(50))}</span>
                            </div>
                        </div>
                    }.into_any(),

                    "set_volume" => view! {
                        <div class="control-row">
                            <span class="control-label">"Volume"</span>
                            <div class="state-slider-row">
                                <span class="material-icons" style="font-size:16px;color:var(--hc-text-muted)">"volume_down"</span>
                                <input type="range" class="state-slider" min="0" max="100" step="1"
                                    prop:value=state["volume"].as_f64().unwrap_or(20.0).to_string()
                                    on:input=move |ev| {
                                        if let Ok(n) = event_target_value(&ev).parse::<i64>() {
                                            let _sk = state_key();
                                            sset("volume", json!(n));
                                        }
                                    }
                                />
                                <span class="material-icons" style="font-size:16px;color:var(--hc-text-muted)">"volume_up"</span>
                                <span class="state-slider-val">{format!("{}%", state["volume"].as_i64().unwrap_or(20))}</span>
                            </div>
                        </div>
                    }.into_any(),

                    "set_bass" => view! {
                        <div class="control-row">
                            <span class="control-label">"Bass"</span>
                            <div class="state-slider-row">
                                <input type="range" class="state-slider" min="-10" max="10" step="1"
                                    prop:value=state["bass"].as_i64().unwrap_or(0).to_string()
                                    on:input=move |ev| {
                                        if let Ok(n) = event_target_value(&ev).parse::<i64>() {
                                            let _sk = state_key();
                                            sset("bass", json!(n));
                                        }
                                    }
                                />
                                <span class="state-slider-val">{state["bass"].as_i64().unwrap_or(0).to_string()}</span>
                            </div>
                        </div>
                    }.into_any(),

                    "set_treble" => view! {
                        <div class="control-row">
                            <span class="control-label">"Treble"</span>
                            <div class="state-slider-row">
                                <input type="range" class="state-slider" min="-10" max="10" step="1"
                                    prop:value=state["treble"].as_i64().unwrap_or(0).to_string()
                                    on:input=move |ev| {
                                        if let Ok(n) = event_target_value(&ev).parse::<i64>() {
                                            let _sk = state_key();
                                            sset("treble", json!(n));
                                        }
                                    }
                                />
                                <span class="state-slider-val">{state["treble"].as_i64().unwrap_or(0).to_string()}</span>
                            </div>
                        </div>
                    }.into_any(),

                    "set_mute" => view! {
                        <div class="control-row">
                            <span class="control-label">"Mute"</span>
                            <div class="toggle-group">
                                <button class:active=state["muted"].as_bool() == Some(true)
                                    on:click=move |_| sset("muted", json!(true))
                                >"Muted"</button>
                                <button class:active=state["muted"].as_bool() == Some(false)
                                    on:click=move |_| sset("muted", json!(false))
                                >"Unmuted"</button>
                            </div>
                        </div>
                    }.into_any(),

                    "set_shuffle" => view! {
                        <div class="control-row">
                            <span class="control-label">"Shuffle"</span>
                            <div class="toggle-group">
                                <button class:active=state["shuffle"].as_bool() == Some(true)
                                    on:click=move |_| sset("shuffle", json!(true))
                                >"On"</button>
                                <button class:active=state["shuffle"].as_bool() == Some(false)
                                    on:click=move |_| sset("shuffle", json!(false))
                                >"Off"</button>
                            </div>
                        </div>
                    }.into_any(),

                    "set_loudness" => view! {
                        <div class="control-row">
                            <span class="control-label">"Loudness"</span>
                            <div class="toggle-group">
                                <button class:active=state["loudness"].as_bool() == Some(true)
                                    on:click=move |_| sset("loudness", json!(true))
                                >"On"</button>
                                <button class:active=state["loudness"].as_bool() == Some(false)
                                    on:click=move |_| sset("loudness", json!(false))
                                >"Off"</button>
                            </div>
                        </div>
                    }.into_any(),

                    "play_favorite" => {
                        let cur = state["favorite"].as_str().unwrap_or("").to_string();
                        view! {
                            <div class="control-row">
                                <span class="control-label">"Favorite"</span>
                                <select class="hc-select"
                                    on:change=move |ev| {
                                        let _sk = state_key();
                                        sset("favorite", json!(event_target_value(&ev)));
                                    }
                                >
                                    <option value="" disabled=true selected=cur.is_empty()>"— Select —"</option>
                                    {favorites.into_iter().map(|f| {
                                        let sel = f == cur;
                                        let f2 = f.clone();
                                        view! { <option value=f selected=sel>{f2}</option> }
                                    }).collect_view()}
                                </select>
                            </div>
                        }.into_any()
                    },

                    "play_playlist" => {
                        let cur = state["playlist"].as_str().unwrap_or("").to_string();
                        view! {
                            <div class="control-row">
                                <span class="control-label">"Playlist"</span>
                                <select class="hc-select"
                                    on:change=move |ev| {
                                        let _sk = state_key();
                                        sset("playlist", json!(event_target_value(&ev)));
                                    }
                                >
                                    <option value="" disabled=true selected=cur.is_empty()>"— Select —"</option>
                                    {playlists.into_iter().map(|p| {
                                        let sel = p == cur;
                                        let p2 = p.clone();
                                        view! { <option value=p selected=sel>{p2}</option> }
                                    }).collect_view()}
                                </select>
                            </div>
                        }.into_any()
                    },

                    // Timer start — show duration + optional label
                    "timer_start" => view! {
                        <div class="control-row">
                            <span class="control-label">"Duration"</span>
                            <div class="state-slider-row">
                                <input type="number" class="hc-input hc-input--sm" style="width:6rem" min="1"
                                    prop:value=state["duration_secs"].as_u64().unwrap_or(300).to_string()
                                    on:input=move |ev| {
                                        if let Ok(n) = event_target_value(&ev).parse::<u64>() {
                                            let _sk = state_key();
                                            sset("duration_secs", json!(n));
                                        }
                                    }
                                />
                                <span class="state-slider-val">"seconds"</span>
                            </div>
                        </div>
                        <div class="control-row">
                            <span class="control-label">"Label"</span>
                            <input type="text" class="hc-input hc-input--sm" placeholder="optional"
                                prop:value=state["label"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| sset_opt("label", &event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    // Simple commands with no extra controls
                    "on_true" | "on_false" | "lock" | "unlock" | "activate"
                    | "play" | "pause" | "stop" | "next" | "prev"
                    | "timer_cancel" | "timer_pause" | "timer_resume" | "timer_restart" => {
                        view! { <span /> }.into_any()
                    },

                    _ => view! { <span /> }.into_any(),
                };

                view! {
                    {cmd_select}
                    {control}
                }.into_any()
            }}
        </div>
    }
}

// ── AttrValueSelect ──────────────────────────────────────────────────────────
// Dropdown for trigger to/from values based on device attribute type.
// Shows canonical labels: Open/Closed, On/Off, Locked/Unlocked, etc.

/// Canonical display labels for a boolean attribute's true/false values.
fn bool_labels(attr: &str) -> (&'static str, &'static str) {
    match attr {
        "open"       => ("Open",        "Closed"),
        "on"         => ("On",          "Off"),
        "locked"     => ("Locked",      "Unlocked"),
        "muted"      => ("Muted",       "Unmuted"),
        "shuffle"    => ("On",          "Off"),
        "loudness"   => ("On",          "Off"),
        "available"  => ("Online",      "Offline"),
        "motion"     => ("Active",      "Clear"),
        "occupied"   => ("Occupied",    "Vacant"),
        "leak"       => ("Leak detected","Dry"),
        "vibration"  => ("Active",      "Clear"),
        _            => ("True",        "False"),
    }
}

#[component]
fn AttrValueSelect(
    /// device_id to look up the attribute's current value type.
    device_id: String,
    /// Attribute name (e.g. "open", "on", "brightness_pct").
    attribute: String,
    /// Current value (may be null if "any").
    value: Value,
    /// Label for this field.
    label: &'static str,
    on_select: Callback<String>,
) -> impl IntoView {
    let devices = use_context::<RwSignal<Vec<DeviceState>>>().unwrap_or(RwSignal::new(vec![]));

    view! {
        <div>
            <label class="field-label">{label}</label>
            {move || {
                let dev = devices.get().into_iter().find(|d| d.device_id == device_id);
                let attr_val = dev.as_ref().and_then(|d| d.attributes.get(&attribute)).cloned();
                let is_bool = attr_val.as_ref().map(|v| v.is_boolean()).unwrap_or(false)
                    || matches!(attribute.as_str(), "on"|"open"|"locked"|"muted"|"shuffle"|"loudness"|"motion"|"occupied"|"leak"|"vibration"|"available");

                if is_bool || attribute.is_empty() && value.is_boolean() {
                    // Boolean attribute — show canonical labels (Open/Closed, On/Off, etc.)
                    let attr_for_labels = if attribute.is_empty() { "on" } else { &attribute };
                    let (true_label, false_label) = bool_labels(attr_for_labels);
                    let cur = if value.is_null() { "" }
                        else if value.as_bool() == Some(true) { "true" }
                        else { "false" };
                    view! {
                        <select class="hc-select"
                            on:change=move |ev| on_select.run(event_target_value(&ev))
                        >
                            <option value="" selected=cur.is_empty()>"— any —"</option>
                            <option value="true" selected=cur == "true">{true_label}</option>
                            <option value="false" selected=cur == "false">{false_label}</option>
                        </select>
                    }.into_any()
                } else if attr_val.as_ref().map(|v| v.is_number()).unwrap_or(false) {
                    // Numeric — show number input
                    let num_str = if value.is_null() { String::new() } else { value.to_string() };
                    view! {
                        <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="any"
                            prop:value=num_str
                            on:input=move |ev| on_select.run(event_target_value(&ev))
                        />
                    }.into_any()
                } else if attr_val.as_ref().map(|v| v.is_string()).unwrap_or(false) {
                    // String attribute — show current value + empty for "any"
                    let cur = if value.is_null() { String::new() } else { value.as_str().unwrap_or("").to_string() };
                    view! {
                        <input type="text" class="hc-input hc-input--sm" placeholder="any"
                            prop:value=cur
                            on:input=move |ev| on_select.run(event_target_value(&ev))
                        />
                    }.into_any()
                } else {
                    // Unknown / no device selected — fallback text
                    let cur = if value.is_null() { String::new() } else { value.to_string() };
                    view! {
                        <input type="text" class="hc-input hc-input--sm" placeholder="any (JSON)"
                            prop:value=cur
                            on:input=move |ev| on_select.run(event_target_value(&ev))
                        />
                    }.into_any()
                }
            }}
        </div>
    }
}

// ── AttributeSelect ──────────────────────────────────────────────────────────
// Dropdown of attribute names from a specific device.

#[component]
fn AttributeSelect(
    /// device_id to look up attributes for.
    device_id: String,
    /// Current attribute value.
    value: String,
    on_select: Callback<String>,
) -> impl IntoView {
    let devices = use_context::<RwSignal<Vec<DeviceState>>>().unwrap_or(RwSignal::new(vec![]));

    view! {
        <select class="hc-select"
            on:change=move |ev| on_select.run(event_target_value(&ev))
        >
            <option value="" selected=value.is_empty()>"— any attribute —"</option>
            {move || {
                let dev_id = device_id.clone();
                let current = value.clone();
                let devs = devices.get();
                let dev = devs.iter().find(|d| d.device_id == dev_id);
                let mut attrs: Vec<String> = dev
                    .map(|d| d.attributes.keys().cloned().collect())
                    .unwrap_or_default();
                attrs.sort();
                // If current value isn't in the list, show it as orphan
                let has_current = current.is_empty() || attrs.contains(&current);
                let orphan = if !has_current {
                    Some(view! { <option value=current.clone() selected=true>{format!("{current} (unknown)")}</option> })
                } else { None };
                let options = attrs.into_iter().map(|attr| {
                    let sel = attr == current;
                    let attr2 = attr.clone();
                    view! { <option value=attr selected=sel>{attr2}</option> }
                }).collect_view();
                view! { {orphan} {options} }
            }}
        </select>
    }
}

// ── ModeSelect ───────────────────────────────────────────────────────────────

#[component]
fn ModeSelect(value: String, on_select: Callback<String>) -> impl IntoView {
    let modes = use_context::<RwSignal<Vec<ModeRecord>>>().unwrap_or(RwSignal::new(vec![]));

    view! {
        <select class="hc-select"
            on:change=move |ev| on_select.run(event_target_value(&ev))
        >
            <option value="" selected=value.is_empty()>"— Select mode —"</option>
            {move || {
                let current = value.clone();
                let ms = modes.get();
                let has_current = current.is_empty() || ms.iter().any(|m| m.config.id == current);
                let orphan = if !has_current {
                    Some(view! { <option value=current.clone() selected=true>{format!("{current} (unknown)")}</option> })
                } else { None };
                let options = ms.into_iter().map(|m| {
                    let sel = m.config.id == current;
                    view! { <option value=m.config.id.clone() selected=sel>{m.config.name.clone()}</option> }
                }).collect_view();
                view! { {orphan} {options} }
            }}
        </select>
    }
}

// ── CheckboxField ────────────────────────────────────────────────────────────

#[component]
fn RuleSelect(value: String, on_select: Callback<String>) -> impl IntoView {
    let all_rules = use_context::<RwSignal<Vec<crate::models::Rule>>>().unwrap_or(RwSignal::new(vec![]));

    view! {
        <select class="hc-select"
            on:change=move |ev| on_select.run(event_target_value(&ev))
        >
            <option value="" selected=value.is_empty()>"— Select rule —"</option>
            {move || {
                let current = value.clone();
                let mut rules = all_rules.get();
                rules.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                let has_current = current.is_empty() || rules.iter().any(|r| r.id.to_string() == current);
                let orphan = if !has_current {
                    Some(view! { <option value=current.clone() selected=true>{format!("{current} (unknown)")}</option> })
                } else { None };
                let options = rules.into_iter().map(|r| {
                    let id = r.id.to_string();
                    let name = if r.name.is_empty() { "(unnamed)".to_string() } else { r.name.clone() };
                    let sel = id == current;
                    view! { <option value=id selected=sel>{name}</option> }
                }).collect_view();
                view! { {orphan} {options} }
            }}
        </select>
    }
}

/// Renders a nested action list inside a block action (e.g. conditional then/else,
/// parallel actions, repeat body). Reads/writes directly to rule[path][index][key].
#[component]
fn NestedItemList(
    rule: RwSignal<Value>,
    path: &'static str,
    index: usize,
    key: &'static str,
) -> impl IntoView {
    view! {
        <div class="nested-action-list">
            {move || {
                let arr = rule.get()[path][index][key].as_array().cloned().unwrap_or_default();
                let total = arr.len();
                if arr.is_empty() {
                    view! { <p class="msg-muted" style="font-size:0.78rem">"No actions."</p> }.into_any()
                } else {
                    arr.into_iter().enumerate().map(|(ai, _item)| {
                        let is_first = ai == 0;
                        let is_last = ai + 1 >= total;
                        view! {
                            <div class="json-row nested-action-row">
                                <div class="json-row-controls">
                                    <span class="json-row-index">{ai + 1}</span>
                                    <button class="hc-btn hc-btn--sm hc-btn--outline" title="Move up" disabled=is_first
                                        on:click=move |_| rule.update(|v| {
                                            if let Some(arr) = v[path][index][key].as_array_mut() { if ai > 0 { arr.swap(ai - 1, ai); } }
                                        })
                                    ><span class="material-icons" style="font-size:14px">"arrow_upward"</span></button>
                                    <button class="hc-btn hc-btn--sm hc-btn--outline" title="Move down" disabled=is_last
                                        on:click=move |_| rule.update(|v| {
                                            if let Some(arr) = v[path][index][key].as_array_mut() { if ai + 1 < arr.len() { arr.swap(ai, ai + 1); } }
                                        })
                                    ><span class="material-icons" style="font-size:14px">"arrow_downward"</span></button>
                                    <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove"
                                        on:click=move |_| rule.update(|v| {
                                            if let Some(arr) = v[path][index][key].as_array_mut() { arr.remove(ai); }
                                        })
                                    ><span class="material-icons" style="font-size:14px">"close"</span></button>
                                </div>
                                <ActionEditor rule=rule path=path index=index nested_key=key nested_index=ai />
                            </div>
                        }
                    }).collect_view().into_any()
                }
            }}
            <button class="hc-btn hc-btn--sm hc-btn--outline" style="margin-top:0.25rem"
                on:click=move |_| rule.update(|v| {
                    let entry = default_action("log_message");
                    if let Some(arr) = v[path][index][key].as_array_mut() { arr.push(entry); }
                    else { v[path][index][key] = json!([entry]); }
                })
            >"+ Add action"</button>
        </div>
    }
}

/// Renders nested actions inside an else_if branch.
#[component]
fn NestedElseIfActions(
    rule: RwSignal<Value>,
    path: &'static str,
    index: usize,
    branch_index: usize,
) -> impl IntoView {
    view! {
        <div class="nested-action-list">
            {move || {
                let arr = rule.get()[path][index]["else_if"][branch_index]["actions"].as_array().cloned().unwrap_or_default();
                if arr.is_empty() {
                    view! { <p class="msg-muted" style="font-size:0.78rem">"No actions."</p> }.into_any()
                } else {
                    arr.into_iter().enumerate().map(|(ai, a)| {
                        let a_type = a["type"].as_str().unwrap_or("log_message").to_string();
                        view! {
                            <div class="json-row nested-action-row">
                                <div class="json-row-controls">
                                    <span class="json-row-index">{ai + 1}</span>
                                    <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove"
                                        on:click=move |_| rule.update(|v| {
                                            if let Some(arr) = v[path][index]["else_if"][branch_index]["actions"].as_array_mut() { arr.remove(ai); }
                                        })
                                    ><span class="material-icons" style="font-size:14px">"close"</span></button>
                                </div>
                                <select class="hc-select" on:change=move |ev| {
                                    let new_t = event_target_value(&ev);
                                    rule.update(|v| { v[path][index]["else_if"][branch_index]["actions"][ai] = default_action(&new_t); });
                                }>
                                    {[("set_device_state","Device command"),("notify","Notify"),("set_mode","Set mode"),
                                      ("delay","Delay"),("log_message","Log message"),("run_script","Script"),
                                      ("fire_event","Fire event"),("stop_rule_chain","Stop chain")]
                                        .map(|(v,l)| view! { <option value=v selected=a_type==v>{l}</option> }).collect_view()}
                                </select>
                                <textarea class="hc-textarea hc-textarea--code" rows="2"
                                    prop:value=serde_json::to_string_pretty(&a).unwrap_or_default()
                                    on:input=move |ev| {
                                        if let Ok(parsed) = serde_json::from_str::<Value>(&event_target_value(&ev)) {
                                            rule.update(|v| { v[path][index]["else_if"][branch_index]["actions"][ai] = parsed; });
                                        }
                                    }
                                />
                            </div>
                        }
                    }).collect_view().into_any()
                }
            }}
            <button class="hc-btn hc-btn--sm hc-btn--outline" style="margin-top:0.25rem"
                on:click=move |_| rule.update(|v| {
                    let entry = default_action("log_message");
                    if let Some(arr) = v[path][index]["else_if"][branch_index]["actions"].as_array_mut() { arr.push(entry); }
                    else { v[path][index]["else_if"][branch_index]["actions"] = json!([entry]); }
                })
            >"+ Add action"</button>
        </div>
    }
}

/// Typed checkbox — takes a getter and setter instead of Value path.
#[component]
fn TypedCheckbox(
    label: &'static str,
    checked: Signal<bool>,
    on_change: impl Fn(bool) + 'static,
) -> impl IntoView {
    view! {
        <label class="rule-meta-inline">
            <input type="checkbox"
                prop:checked=move || checked.get()
                on:change=move |ev| {
                    use wasm_bindgen::JsCast;
                    let val = ev.target()
                        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                        .map(|el| el.checked())
                        .unwrap_or(false);
                    on_change(val);
                }
            />
            " "{label}
        </label>
    }
}

/// Legacy checkbox for Value-based sub-editors (trigger, condition, action).
#[component]
fn CheckboxField(label: &'static str, sig: RwSignal<Value>, key: &'static str) -> impl IntoView {
    view! {
        <label class="rule-meta-inline">
            <input type="checkbox"
                prop:checked=move || sig.get()[key].as_bool().unwrap_or(false)
                on:change=move |ev| {
                    use wasm_bindgen::JsCast;
                    let checked = ev.target()
                        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                        .map(|el| el.checked())
                        .unwrap_or(false);
                    sig.update(|v| { v[key] = json!(checked); });
                }
            />
            " "{label}
        </label>
    }
}
