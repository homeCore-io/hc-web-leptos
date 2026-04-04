//! Rule editor pages — create a new rule or edit an existing one.
//!
//! Architecture:
//!   One `RwSignal<Value>` holds the complete rule JSON.  Every editor component
//!   reads fields via `rule.get()["path"]` and writes via `rule.update(|v| ...)`.
//!   No inter-signal synchronisation, no nested signal materialisation.
//!
//!   Reference data (devices, areas, scenes, modes) is fetched once on page load
//!   and provided as read-only signals for searchable dropdowns.

use crate::api::{
    clone_rule, create_rule, delete_rule, fetch_areas, fetch_devices, fetch_modes,
    fetch_rule, fetch_scenes, rule_fire_history, test_rule, update_rule,
};
use crate::auth::use_auth;
use crate::models::{
    is_media_player, is_scene_like, media_available_favorites, media_available_playlists,
    Area, DeviceState, ModeRecord, Scene,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_params_map};
use serde_json::{json, Value};

// ── Default JSON skeletons ───────────────────────────────────────────────────

fn default_rule() -> Value {
    json!({
        "name": "",
        "enabled": true,
        "priority": 0,
        "tags": [],
        "trigger": {"type": "manual_trigger"},
        "conditions": [],
        "actions": [],
        "run_mode": {"type": "parallel"},
        "log_events": false,
        "log_triggers": false,
        "log_actions": false,
        "cancel_on_false": false,
    })
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

// ── JSON field helpers ───────────────────────────────────────────────────────
// These operate on a RwSignal<Value> at any path depth.

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
    let rule: RwSignal<Value> = RwSignal::new(default_rule());
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
    provide_context(devices);
    provide_context(areas);
    provide_context(scenes);
    provide_context(modes);

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
        // Fetch rule (edit mode).
        if let Some(id_sig) = id {
            let rule_id = id_sig.get();
            if rule_id.is_empty() { return; }
            spawn_local(async move {
                match fetch_rule(&token, &rule_id).await {
                    Ok(r) => { rule.set(r); loading.set(false); }
                    Err(e) => { save_err.set(Some(format!("Load failed: {e}"))); loading.set(false); }
                }
            });
        }
    });

    // ── Tag commit ───────────────────────────────────────────────────────────
    let commit_tag = move || {
        let raw = tag_input.get_untracked().trim().to_string();
        if raw.is_empty() { return; }
        rule.update(|v| {
            let arr = v["tags"].as_array_mut().expect("tags is array");
            if !arr.iter().any(|t| t.as_str() == Some(&raw)) { arr.push(json!(raw)); }
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
            {move || save_ok.get().then(|| view! { <p class="msg-ok">"Saved."</p> })}
            {move || loading.get().then(|| view! { <p class="msg-muted">"Loading…"</p> })}

            // ── Editor (hidden while loading) ────────────────────────────────
            <Show when=move || !loading.get()>

                // ── Rule metadata ────────────────────────────────────────────
                <section class="detail-card">
                    <h3 class="detail-card-title">"Rule"</h3>

                    <label class="field-label">"Name"</label>
                    <input type="text" class="hc-input" placeholder="Rule name"
                        prop:value=move || jget_str(&rule.get(), "name")
                        on:input=move |ev| jset(rule, &["name"], json!(event_target_value(&ev)))
                    />

                    <div class="rule-meta-row">
                        <CheckboxField label="Enabled" sig=rule key="enabled" />

                        <label class="field-label" style="margin:0">"Priority"</label>
                        <input type="number" class="hc-input hc-input--sm" style="width:5rem"
                            prop:value=move || rule.get()["priority"].as_i64().unwrap_or(0).to_string()
                            on:input=move |ev| {
                                if let Ok(n) = event_target_value(&ev).parse::<i64>() {
                                    jset(rule, &["priority"], json!(n));
                                }
                            }
                        />
                    </div>

                    // ── Tags ─────────────────────────────────────────────────
                    <label class="field-label">"Tags"</label>
                    <div class="tag-input-row">
                        {move || {
                            let tags = rule.get()["tags"].as_array().cloned().unwrap_or_default();
                            tags.into_iter().enumerate().map(|(i, tag)| {
                                let label = tag.as_str().unwrap_or("").to_string();
                                view! {
                                    <span class="tag-chip">
                                        {label}
                                        <button class="tag-chip-remove"
                                            on:click=move |_| rule.update(|v| {
                                                if let Some(arr) = v["tags"].as_array_mut() { arr.remove(i); }
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
                                "single"  => json!({"type":"single"}),
                                "restart" => json!({"type":"restart"}),
                                "queued"  => json!({"type":"queued","max_queue":10}),
                                _         => json!({"type":"parallel"}),
                            };
                            jset(rule, &["run_mode"], rm);
                        }
                    >
                        {[("parallel","Parallel (default)"),("single","Single — skip if running"),("restart","Restart — cancel and restart"),("queued","Queued")]
                            .map(|(v, label)| view! {
                                <option value=v selected=move || rule.get()["run_mode"]["type"].as_str().unwrap_or("parallel") == v>{label}</option>
                            }).collect_view()}
                    </select>

                    // ── Cooldown ─────────────────────────────────────────────
                    <label class="field-label">"Cooldown (seconds)"</label>
                    <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="None"
                        prop:value=move || jget_u64_str(&rule.get(), "cooldown_secs")
                        on:input=move |ev| {
                            let raw = event_target_value(&ev);
                            rule.update(|v| {
                                if raw.trim().is_empty() { if let Some(o) = v.as_object_mut() { o.remove("cooldown_secs"); } }
                                else if let Ok(n) = raw.trim().parse::<u64>() { v["cooldown_secs"] = json!(n); }
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
                            on:click=move |_| rule.update(|v| {
                                let arr = v["conditions"].as_array_mut().expect("conditions array");
                                arr.push(default_condition("device_state"));
                            })
                        >"+ Add"</button>
                    </div>
                    <ItemList rule=rule key="conditions" item_kind="condition" />
                </section>

                // ── Actions ──────────────────────────────────────────────────
                <section class="detail-card">
                    <div class="rule-section-header">
                        <h3 class="detail-card-title">"Actions"</h3>
                        <button class="hc-btn hc-btn--sm hc-btn--outline"
                            on:click=move |_| rule.update(|v| {
                                let arr = v["actions"].as_array_mut().expect("actions array");
                                arr.push(default_action("log_message"));
                            })
                        >"+ Add"</button>
                    </div>
                    <ItemList rule=rule key="actions" item_kind="action" />
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
                                prop:value=move || jget_opt_str(&rule.get(), "trigger_label")
                                on:input=move |ev| jset_opt(rule, &["trigger_label"], &event_target_value(&ev))
                            />

                            <label class="field-label">"Required expression (Rhai)"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="3"
                                placeholder=r#"e.g. mode_is("mode_night")"#
                                prop:value=move || jget_opt_str(&rule.get(), "required_expression")
                                on:input=move |ev| jset_opt(rule, &["required_expression"], &event_target_value(&ev))
                            />

                            <CheckboxField label="Cancel pending delays when required expression is false" sig=rule key="cancel_on_false" />

                            <label class="field-label">"Trigger condition (Rhai)"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="3"
                                placeholder="Per-event condition expression"
                                prop:value=move || jget_opt_str(&rule.get(), "trigger_condition")
                                on:input=move |ev| jset_opt(rule, &["trigger_condition"], &event_target_value(&ev))
                            />

                            <label class="field-label">"Variables (JSON object)"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="4"
                                prop:value=move || {
                                    let v = &rule.get()["variables"];
                                    if v.is_null() || v.as_object().map(|m| m.is_empty()).unwrap_or(true) {
                                        "{}".to_string()
                                    } else {
                                        serde_json::to_string_pretty(v).unwrap_or_default()
                                    }
                                }
                                on:input=move |ev| {
                                    let raw = event_target_value(&ev);
                                    if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
                                        jset(rule, &["variables"], parsed);
                                    }
                                }
                            />

                            <div class="rule-logging-row">
                                <span class="field-label" style="margin:0">"Logging:"</span>
                                <CheckboxField label="Events"   sig=rule key="log_events" />
                                <CheckboxField label="Triggers" sig=rule key="log_triggers" />
                                <CheckboxField label="Actions"  sig=rule key="log_actions" />
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
                                    let body = rule.get_untracked();
                                    let name = body["name"].as_str().unwrap_or("");
                                    if name.trim().is_empty() { save_err.set(Some("Rule name is required.".into())); return; }
                                    save_err.set(None); save_ok.set(false); saving.set(true);
                                    let nav = nav_save.clone();
                                    let rule_id = id.map(|s| s.get_untracked()).unwrap_or_default();
                                    spawn_local(async move {
                                        let result = if rule_id.is_empty() { create_rule(&token, &body).await }
                                                     else { update_rule(&token, &rule_id, &body).await };
                                        match result {
                                            Ok(saved) => {
                                                if rule_id.is_empty() {
                                                    let new_id = saved["id"].as_str().unwrap_or("").to_string();
                                                    if !new_id.is_empty() { nav(&format!("/rules/{new_id}"), Default::default()); }
                                                } else {
                                                    rule.set(saved);
                                                    save_ok.set(true);
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
                                                    let new_id = new_rule["id"].as_str().unwrap_or("").to_string();
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

#[component]
fn TriggerEditor(rule: RwSignal<Value>) -> impl IntoView {
    let tset = move |key: &'static str, val: Value| {
        rule.update(|v| { v["trigger"][key] = val; });
    };
    let tset_opt = move |key: &'static str, raw: &str| {
        rule.update(|v| {
            if raw.trim().is_empty() { if let Some(o) = v["trigger"].as_object_mut() { o.remove(key); } }
            else {
                let parsed = serde_json::from_str::<Value>(raw.trim()).unwrap_or_else(|_| json!(raw.trim()));
                v["trigger"][key] = parsed;
            }
        });
    };
    let tg = move || rule.get()["trigger"].clone();

    view! {
        <div class="trigger-editor">
            <label class="field-label">"Trigger type"</label>
            <select class="hc-select"
                on:change=move |ev| {
                    let t = event_target_value(&ev);
                    jset(rule, &["trigger"], default_trigger(&t));
                }
            >
                <optgroup label="Device">
                    {[("device_state_changed","Device state changed"),("device_availability_changed","Device availability changed"),("button_event","Button event"),("numeric_threshold","Numeric threshold")]
                        .map(|(v, label)| view! { <option value=v selected=move || tg()["type"].as_str() == Some(v)>{label}</option> }).collect_view()}
                </optgroup>
                <optgroup label="Time">
                    {[("time_of_day","Time of day"),("sun_event","Sun event"),("cron","Cron schedule"),("periodic","Periodic"),("calendar_event","Calendar event")]
                        .map(|(v, label)| view! { <option value=v selected=move || tg()["type"].as_str() == Some(v)>{label}</option> }).collect_view()}
                </optgroup>
                <optgroup label="Event">
                    {[("custom_event","Custom event"),("system_started","System started"),("hub_variable_changed","Hub variable changed"),("mode_changed","Mode changed"),("webhook_received","Webhook received"),("mqtt_message","MQTT message")]
                        .map(|(v, label)| view! { <option value=v selected=move || tg()["type"].as_str() == Some(v)>{label}</option> }).collect_view()}
                </optgroup>
                <optgroup label="Manual">
                    <option value="manual_trigger" selected=move || tg()["type"].as_str() == Some("manual_trigger")>"Manual trigger"</option>
                </optgroup>
            </select>

            // Type-specific fields
            {move || {
                let trigger = tg();
                let t = trigger["type"].as_str().unwrap_or("").to_string();
                match t.as_str() {
                    "device_state_changed" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device(s)"</label>
                            <DeviceMultiSelect rule=rule />
                            <label class="field-label">"Attribute (blank = any)"</label>
                            <AttributeSelect
                                device_id=jget_str(&trigger, "device_id")
                                value=jget_opt_str(&trigger, "attribute")
                                on_select=Callback::new(move |attr: String| tset_opt("attribute", &attr))
                            />
                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"To (JSON, blank = any)"</label>
                                    <input type="text" class="hc-input"
                                        prop:value=jget_opt_str(&trigger, "to")
                                        on:input=move |ev| tset_opt("to", &event_target_value(&ev))
                                    />
                                </div>
                                <div>
                                    <label class="field-label">"From (JSON, blank = any)"</label>
                                    <input type="text" class="hc-input"
                                        prop:value=jget_opt_str(&trigger, "from")
                                        on:input=move |ev| tset_opt("from", &event_target_value(&ev))
                                    />
                                </div>
                            </div>
                            <label class="field-label">"Hold duration (seconds, blank = immediate)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="None"
                                prop:value=jget_u64_str(&trigger, "for_duration_secs")
                                on:input=move |ev| {
                                    let raw = event_target_value(&ev);
                                    rule.update(|v| {
                                        if raw.trim().is_empty() { if let Some(o) = v["trigger"].as_object_mut() { o.remove("for_duration_secs"); } }
                                        else if let Ok(n) = raw.trim().parse::<u64>() { v["trigger"]["for_duration_secs"] = json!(n); }
                                    });
                                }
                            />
                        </div>
                    }.into_any(),

                    "device_availability_changed" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device"</label>
                            <DeviceSelect value=jget_str(&trigger, "device_id")
                                on_select=Callback::new(move |id: String| tset("device_id", json!(id))) />
                            <label class="field-label">"Direction"</label>
                            <select class="hc-select" on:change=move |ev| {
                                let raw = event_target_value(&ev);
                                rule.update(|v| { match raw.as_str() {
                                    "online" => v["trigger"]["to"] = json!(true),
                                    "offline" => v["trigger"]["to"] = json!(false),
                                    _ => { if let Some(o) = v["trigger"].as_object_mut() { o.remove("to"); } }
                                }});
                            }>
                                <option value="any" selected=move || tg().get("to").map(|v| v.is_null()).unwrap_or(true)>"Any"</option>
                                <option value="online" selected=move || tg()["to"].as_bool() == Some(true)>"Goes online"</option>
                                <option value="offline" selected=move || tg()["to"].as_bool() == Some(false)>"Goes offline"</option>
                            </select>
                        </div>
                    }.into_any(),

                    "button_event" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device"</label>
                            <DeviceSelect value=jget_str(&trigger, "device_id")
                                on_select=Callback::new(move |id: String| tset("device_id", json!(id))) />
                            <label class="field-label">"Event type"</label>
                            <select class="hc-select" on:change=move |ev| tset("event", json!(event_target_value(&ev)))>
                                {[("pushed","Pushed"),("held","Held"),("double_tapped","Double-tapped"),("released","Released")]
                                    .map(|(v,l)| view! { <option value=v selected=trigger["event"].as_str()==Some(v)>{l}</option> }).collect_view()}
                            </select>
                        </div>
                    }.into_any(),

                    "numeric_threshold" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device"</label>
                            <DeviceSelect value=jget_str(&trigger, "device_id")
                                on_select=Callback::new(move |id: String| tset("device_id", json!(id))) />
                            <label class="field-label">"Attribute"</label>
                            <AttributeSelect
                                device_id=jget_str(&trigger, "device_id")
                                value=jget_str(&trigger, "attribute")
                                on_select=Callback::new(move |attr: String| tset("attribute", json!(attr)))
                            />
                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"Operator"</label>
                                    <select class="hc-select" on:change=move |ev| tset("op", json!(event_target_value(&ev)))>
                                        {[("crosses_above","Crosses above"),("crosses_below","Crosses below"),("above","Is above"),("below","Is below")]
                                            .map(|(v,l)| view! { <option value=v selected=trigger["op"].as_str()==Some(v)>{l}</option> }).collect_view()}
                                    </select>
                                </div>
                                <div>
                                    <label class="field-label">"Threshold"</label>
                                    <input type="number" class="hc-input"
                                        prop:value=trigger["value"].as_f64().unwrap_or(0.0).to_string()
                                        on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<f64>() { tset("value", json!(n)); } }
                                    />
                                </div>
                            </div>
                        </div>
                    }.into_any(),

                    "time_of_day" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Time (HH:MM)"</label>
                            <input type="time" class="hc-input hc-input--sm" style="width:10rem"
                                prop:value=trigger["time"].as_str().unwrap_or("08:00:00").get(..5).unwrap_or("08:00").to_string()
                                on:input=move |ev| { let hm = event_target_value(&ev); tset("time", json!(format!("{hm}:00"))); }
                            />
                            <label class="field-label">"Days"</label>
                            <div class="trigger-day-row">
                                {["Mon","Tue","Wed","Thu","Fri","Sat","Sun"].map(|day| {
                                    let checked = trigger["days"].as_array().map(|a| a.iter().any(|d| d.as_str()==Some(day))).unwrap_or(false);
                                    view! {
                                        <label class="day-chip">
                                            <input type="checkbox" prop:checked=checked
                                                on:change=move |ev| {
                                                    use wasm_bindgen::JsCast;
                                                    let on = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).map(|el| el.checked()).unwrap_or(false);
                                                    rule.update(|v| {
                                                        if let Some(arr) = v["trigger"]["days"].as_array_mut() {
                                                            if on { if !arr.iter().any(|d| d.as_str()==Some(day)) { arr.push(json!(day)); } }
                                                            else { arr.retain(|d| d.as_str()!=Some(day)); }
                                                        }
                                                    });
                                                }
                                            />
                                            {day}
                                        </label>
                                    }
                                }).collect_view()}
                            </div>
                        </div>
                    }.into_any(),

                    "sun_event" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Sun event"</label>
                            <select class="hc-select" on:change=move |ev| tset("event", json!(event_target_value(&ev)))>
                                {[("sunrise","Sunrise"),("sunset","Sunset"),("solar_noon","Solar noon"),("civil_dawn","Civil dawn"),("civil_dusk","Civil dusk")]
                                    .map(|(v,l)| view! { <option value=v selected=trigger["event"].as_str()==Some(v)>{l}</option> }).collect_view()}
                            </select>
                            <label class="field-label">"Offset (minutes)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem"
                                prop:value=jget_i64_str(&trigger, "offset_minutes")
                                on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<i64>() { tset("offset_minutes", json!(n)); } }
                            />
                        </div>
                    }.into_any(),

                    "cron" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Cron expression (6-field)"</label>
                            <input type="text" class="hc-input hc-textarea--code" placeholder="0 0 8 * * *"
                                prop:value=jget_str(&trigger, "expression")
                                on:input=move |ev| tset("expression", json!(event_target_value(&ev)))
                            />
                        </div>
                    }.into_any(),

                    "periodic" => view! {
                        <div class="trigger-fields">
                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"Every N"</label>
                                    <input type="number" class="hc-input" min="1"
                                        prop:value=trigger["every_n"].as_u64().unwrap_or(15).to_string()
                                        on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<u64>() { tset("every_n", json!(n)); } }
                                    />
                                </div>
                                <div>
                                    <label class="field-label">"Unit"</label>
                                    <select class="hc-select" on:change=move |ev| tset("unit", json!(event_target_value(&ev)))>
                                        {[("minutes","Minutes"),("hours","Hours"),("days","Days"),("weeks","Weeks")]
                                            .map(|(v,l)| view! { <option value=v selected=trigger["unit"].as_str()==Some(v)>{l}</option> }).collect_view()}
                                    </select>
                                </div>
                            </div>
                        </div>
                    }.into_any(),

                    "calendar_event" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Calendar ID (blank = any)"</label>
                            <input type="text" class="hc-input"
                                prop:value=jget_opt_str(&trigger, "calendar_id")
                                on:input=move |ev| tset_opt("calendar_id", &event_target_value(&ev))
                            />
                            <label class="field-label">"Title contains (blank = any)"</label>
                            <input type="text" class="hc-input"
                                prop:value=jget_opt_str(&trigger, "title_contains")
                                on:input=move |ev| tset_opt("title_contains", &event_target_value(&ev))
                            />
                            <label class="field-label">"Offset (minutes)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem"
                                prop:value=jget_i64_str(&trigger, "offset_minutes")
                                on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<i64>() { tset("offset_minutes", json!(n)); } }
                            />
                        </div>
                    }.into_any(),

                    "custom_event" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Event type"</label>
                            <input type="text" class="hc-input"
                                prop:value=jget_str(&trigger, "event_type")
                                on:input=move |ev| tset("event_type", json!(event_target_value(&ev)))
                            />
                        </div>
                    }.into_any(),

                    "hub_variable_changed" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Variable name (blank = any)"</label>
                            <input type="text" class="hc-input"
                                prop:value=jget_opt_str(&trigger, "name")
                                on:input=move |ev| tset_opt("name", &event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "mode_changed" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Mode (blank = any)"</label>
                            <ModeSelect value=jget_opt_str(&trigger, "mode_id")
                                on_select=Callback::new(move |id: String| tset_opt("mode_id", &id))
                            />
                            <label class="field-label">"Direction"</label>
                            <select class="hc-select" on:change=move |ev| {
                                let raw = event_target_value(&ev);
                                rule.update(|v| { match raw.as_str() {
                                    "on" => v["trigger"]["to"] = json!(true),
                                    "off" => v["trigger"]["to"] = json!(false),
                                    _ => { if let Some(o) = v["trigger"].as_object_mut() { o.remove("to"); } }
                                }});
                            }>
                                <option value="any" selected=move || !tg().get("to").is_some_and(|v| !v.is_null())>"Any"</option>
                                <option value="on" selected=move || tg()["to"].as_bool()==Some(true)>"Turns on"</option>
                                <option value="off" selected=move || tg()["to"].as_bool()==Some(false)>"Turns off"</option>
                            </select>
                        </div>
                    }.into_any(),

                    "webhook_received" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Webhook path"</label>
                            <input type="text" class="hc-input"
                                prop:value=jget_str(&trigger, "path")
                                on:input=move |ev| tset("path", json!(event_target_value(&ev)))
                            />
                        </div>
                    }.into_any(),

                    "mqtt_message" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Topic pattern"</label>
                            <input type="text" class="hc-input hc-textarea--code"
                                prop:value=jget_str(&trigger, "topic_pattern")
                                on:input=move |ev| tset("topic_pattern", json!(event_target_value(&ev)))
                            />
                            <label class="field-label">"Exact payload (blank = any)"</label>
                            <input type="text" class="hc-input"
                                prop:value=jget_opt_str(&trigger, "payload")
                                on:input=move |ev| tset_opt("payload", &event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    _ => view! {
                        <div class="trigger-fields">
                            <p class="msg-muted" style="font-size:0.85rem">
                                {if t == "system_started" { "Fires once when the rule engine starts." } else { "No configurable fields." }}
                            </p>
                        </div>
                    }.into_any(),
                }
            }}
        </div>
    }
}

// ── ConditionEditor ──────────────────────────────────────────────────────────

#[component]
fn ConditionEditor(rule: RwSignal<Value>, index: usize) -> impl IntoView {
    let cg = move || rule.get()["conditions"][index].clone();
    let cset = move |key: &'static str, val: Value| {
        rule.update(|v| { v["conditions"][index][key] = val; });
    };
    let cset_opt = move |key: &'static str, raw: &str| {
        rule.update(|v| {
            if raw.trim().is_empty() { if let Some(o) = v["conditions"][index].as_object_mut() { o.remove(key); } }
            else {
                let parsed = serde_json::from_str::<Value>(raw.trim()).unwrap_or_else(|_| json!(raw.trim()));
                v["conditions"][index][key] = parsed;
            }
        });
    };

    view! {
        <div class="condition-editor">
            <label class="field-label">"Condition type"</label>
            <select class="hc-select"
                on:change=move |ev| {
                    let t = event_target_value(&ev);
                    rule.update(|v| { v["conditions"][index] = default_condition(&t); });
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
                            <label class="field-label">"Value (JSON)"</label>
                            <input type="text" class="hc-input"
                                prop:value=jget_opt_str(&c, "value") on:input=move |ev| cset_opt("value", &event_target_value(&ev)) />
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

                    // Nested types → JSON fallback
                    _ => view! {
                        <div class="trigger-fields">
                            <p class="msg-muted" style="font-size:0.78rem">"Nested condition — edit as JSON:"</p>
                            <JsonBlock rule=rule path_prefix="conditions" index=index rows=6 />
                        </div>
                    }.into_any(),
                }
            }}
        </div>
    }
}

// ── ActionEditor ─────────────────────────────────────────────────────────────

#[component]
fn ActionEditor(
    rule: RwSignal<Value>,
    path: &'static str,
    index: usize,
) -> impl IntoView {
    let ag = move || rule.get()[path][index].clone();
    let aset = move |key: &'static str, val: Value| {
        rule.update(|v| { v[path][index][key] = val; });
    };
    let aset_opt = move |key: &'static str, raw: &str| {
        rule.update(|v| {
            if raw.trim().is_empty() { if let Some(o) = v[path][index].as_object_mut() { o.remove(key); } }
            else {
                let parsed = serde_json::from_str::<Value>(raw.trim()).unwrap_or_else(|_| json!(raw.trim()));
                v[path][index][key] = parsed;
            }
        });
    };

    view! {
        <div class="action-editor">
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

            <label class="field-label">"Action type"</label>
            <select class="hc-select"
                on:change=move |ev| {
                    let t = event_target_value(&ev);
                    rule.update(|v| { v[path][index] = default_action(&t); });
                }
            >
                <optgroup label="Device">
                    {[("set_device_state","Set device state"),("fade_device","Fade device"),("capture_device_state","Capture state"),("restore_device_state","Restore state")]
                        .map(|(v,l)| view! { <option value=v selected=move || ag()["type"].as_str()==Some(v)>{l}</option> }).collect_view()}
                </optgroup>
                <optgroup label="Communication">
                    {[("publish_mqtt","Publish MQTT"),("call_service","Call HTTP service"),("fire_event","Fire event"),("notify","Notify"),("log_message","Log message"),("comment","Comment")]
                        .map(|(v,l)| view! { <option value=v selected=move || ag()["type"].as_str()==Some(v)>{l}</option> }).collect_view()}
                </optgroup>
                <optgroup label="Script & Variables">
                    {[("run_script","Run script (Rhai)"),("set_variable","Set variable"),("set_hub_variable","Set hub variable"),("set_private_boolean","Set private boolean")]
                        .map(|(v,l)| view! { <option value=v selected=move || ag()["type"].as_str()==Some(v)>{l}</option> }).collect_view()}
                </optgroup>
                <optgroup label="Timing & Flow">
                    {[("delay","Delay"),("wait_for_event","Wait for event"),("wait_for_expression","Wait for expression"),("stop_rule_chain","Stop rule chain"),("exit_rule","Exit rule")]
                        .map(|(v,l)| view! { <option value=v selected=move || ag()["type"].as_str()==Some(v)>{l}</option> }).collect_view()}
                </optgroup>
                <optgroup label="Mode & Rule Control">
                    {[("set_mode","Set mode"),("run_rule_actions","Run rule actions"),("pause_rule","Pause rule"),("resume_rule","Resume rule"),("cancel_delays","Cancel delays"),("cancel_rule_timers","Cancel rule timers")]
                        .map(|(v,l)| view! { <option value=v selected=move || ag()["type"].as_str()==Some(v)>{l}</option> }).collect_view()}
                </optgroup>
                <optgroup label="Block actions">
                    {[("parallel","Parallel"),("conditional","Conditional"),("repeat_until","Repeat until"),("repeat_while","Repeat while"),("repeat_count","Repeat count"),
                      ("ping_host","Ping host"),("set_device_state_per_mode","Set state per mode"),("delay_per_mode","Delay per mode"),("activate_scene_per_mode","Scene per mode")]
                        .map(|(v,l)| view! { <option value=v selected=move || ag()["type"].as_str()==Some(v)>{l}</option> }).collect_view()}
                </optgroup>
            </select>

            // Type-specific fields
            {move || {
                let a = ag();
                let t = a["type"].as_str().unwrap_or("").to_string();
                match t.as_str() {
                    "set_device_state" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device"</label>
                            <DeviceSelect value=jget_str(&a, "device_id")
                                on_select=Callback::new(move |id: String| aset("device_id", json!(id))) />
                            <DeviceStateBuilder rule=rule path=path index=index />
                        </div>
                    }.into_any(),

                    "notify" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Channel"</label>
                            <input type="text" class="hc-input" prop:value=jget_str(&a,"channel") on:input=move |ev| aset("channel", json!(event_target_value(&ev))) />
                            <label class="field-label">"Message"</label>
                            <textarea class="hc-textarea" rows="2" prop:value=jget_str(&a,"message") on:input=move |ev| aset("message", json!(event_target_value(&ev))) />
                            <label class="field-label">"Title (optional)"</label>
                            <input type="text" class="hc-input" prop:value=jget_opt_str(&a,"title") on:input=move |ev| aset_opt("title", &event_target_value(&ev)) />
                        </div>
                    }.into_any(),

                    "delay" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Duration (seconds)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem"
                                prop:value=a["duration_secs"].as_u64().unwrap_or(5).to_string()
                                on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<u64>() { aset("duration_secs", json!(n)); } } />
                        </div>
                    }.into_any(),

                    "set_mode" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Mode"</label>
                            <ModeSelect value=jget_str(&a, "mode_id")
                                on_select=Callback::new(move |id: String| aset("mode_id", json!(id))) />
                            <label class="field-label">"Command"</label>
                            <select class="hc-select" on:change=move |ev| aset("command", json!(event_target_value(&ev)))>
                                {[("on","On"),("off","Off"),("toggle","Toggle")].map(|(v,l)| view! { <option value=v selected=a["command"].as_str()==Some(v)>{l}</option> }).collect_view()}
                            </select>
                        </div>
                    }.into_any(),

                    "run_script" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Rhai script"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="6" prop:value=jget_str(&a,"script") on:input=move |ev| aset("script", json!(event_target_value(&ev))) />
                        </div>
                    }.into_any(),

                    "log_message" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Message"</label>
                            <textarea class="hc-textarea" rows="2" prop:value=jget_str(&a,"message") on:input=move |ev| aset("message", json!(event_target_value(&ev))) />
                        </div>
                    }.into_any(),

                    "comment" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Comment"</label>
                            <textarea class="hc-textarea" rows="2" prop:value=jget_str(&a,"text") on:input=move |ev| aset("text", json!(event_target_value(&ev))) />
                        </div>
                    }.into_any(),

                    "fire_event" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Event type"</label>
                            <input type="text" class="hc-input" prop:value=jget_str(&a,"event_type") on:input=move |ev| aset("event_type", json!(event_target_value(&ev))) />
                            <label class="field-label">"Payload (JSON)"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="2"
                                prop:value=serde_json::to_string_pretty(&a["payload"]).unwrap_or_default()
                                on:input=move |ev| { if let Ok(p) = serde_json::from_str::<Value>(&event_target_value(&ev)) { aset("payload", p); } } />
                        </div>
                    }.into_any(),

                    "publish_mqtt" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Topic"</label>
                            <input type="text" class="hc-input hc-textarea--code" prop:value=jget_str(&a,"topic") on:input=move |ev| aset("topic", json!(event_target_value(&ev))) />
                            <label class="field-label">"Payload"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="2" prop:value=jget_str(&a,"payload") on:input=move |ev| aset("payload", json!(event_target_value(&ev))) />
                        </div>
                    }.into_any(),

                    "call_service" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"URL"</label>
                            <input type="text" class="hc-input" prop:value=jget_str(&a,"url") on:input=move |ev| aset("url", json!(event_target_value(&ev))) />
                            <label class="field-label">"Method"</label>
                            <select class="hc-select" on:change=move |ev| aset("method", json!(event_target_value(&ev)))>
                                {["GET","POST","PUT","PATCH","DELETE"].map(|m| view! { <option value=m selected=a["method"].as_str()==Some(m)>{m}</option> }).collect_view()}
                            </select>
                        </div>
                    }.into_any(),

                    "set_variable" | "set_hub_variable" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Variable name"</label>
                            <input type="text" class="hc-input" prop:value=jget_str(&a,"name") on:input=move |ev| aset("name", json!(event_target_value(&ev))) />
                            <label class="field-label">"Value (JSON)"</label>
                            <input type="text" class="hc-input" prop:value=jget_opt_str(&a,"value") on:input=move |ev| aset_opt("value", &event_target_value(&ev)) />
                        </div>
                    }.into_any(),

                    "run_rule_actions" | "pause_rule" | "resume_rule" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Rule ID (UUID)"</label>
                            <input type="text" class="hc-input hc-textarea--code" prop:value=jget_str(&a,"rule_id") on:input=move |ev| aset("rule_id", json!(event_target_value(&ev))) />
                        </div>
                    }.into_any(),

                    "set_private_boolean" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Name"</label>
                            <input type="text" class="hc-input" prop:value=jget_str(&a,"name") on:input=move |ev| aset("name", json!(event_target_value(&ev))) />
                            <label class="field-label">"Value"</label>
                            <select class="hc-select" on:change=move |ev| aset("value", json!(event_target_value(&ev)=="true"))>
                                <option value="true" selected=a["value"].as_bool()!=Some(false)>"True"</option>
                                <option value="false" selected=a["value"].as_bool()==Some(false)>"False"</option>
                            </select>
                        </div>
                    }.into_any(),

                    "fade_device" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device"</label>
                            <DeviceSelect value=jget_str(&a, "device_id")
                                on_select=Callback::new(move |id: String| aset("device_id", json!(id))) />
                            <DeviceStateBuilder rule=rule path=path index=index />
                            <label class="field-label">"Duration (seconds)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem"
                                prop:value=a["duration_secs"].as_u64().unwrap_or(30).to_string()
                                on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<u64>() { aset("duration_secs", json!(n)); } } />
                        </div>
                    }.into_any(),

                    "capture_device_state" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Snapshot key"</label>
                            <input type="text" class="hc-input" prop:value=jget_str(&a,"key") on:input=move |ev| aset("key", json!(event_target_value(&ev))) />
                            <label class="field-label">"Device IDs (comma-separated)"</label>
                            <input type="text" class="hc-input"
                                prop:value=a["device_ids"].as_array().map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default()
                                on:input=move |ev| {
                                    let ids: Vec<Value> = event_target_value(&ev).split(',').map(|s| json!(s.trim())).filter(|v| v.as_str()!=Some("")).collect();
                                    aset("device_ids", json!(ids));
                                } />
                        </div>
                    }.into_any(),

                    "restore_device_state" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Snapshot key"</label>
                            <input type="text" class="hc-input" prop:value=jget_str(&a,"key") on:input=move |ev| aset("key", json!(event_target_value(&ev))) />
                        </div>
                    }.into_any(),

                    "wait_for_expression" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Rhai expression"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="3" prop:value=jget_str(&a,"expression") on:input=move |ev| aset("expression", json!(event_target_value(&ev))) />
                        </div>
                    }.into_any(),

                    "cancel_delays" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Cancel key (blank = all)"</label>
                            <input type="text" class="hc-input" prop:value=jget_opt_str(&a,"key") on:input=move |ev| aset_opt("key", &event_target_value(&ev)) />
                        </div>
                    }.into_any(),

                    "stop_rule_chain" | "exit_rule" | "cancel_rule_timers" | "wait_for_event" => view! {
                        <div class="trigger-fields">
                            <p class="msg-muted" style="font-size:0.85rem">"No configurable fields."</p>
                        </div>
                    }.into_any(),

                    // Block actions + anything else → JSON fallback
                    _ => view! {
                        <div class="trigger-fields">
                            <JsonBlock rule=rule path_prefix=path index=index rows=8 />
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
    #[prop(default = 6)] rows: u32,
) -> impl IntoView {
    let json_err: RwSignal<Option<String>> = RwSignal::new(None);
    let text: RwSignal<String> = RwSignal::new(
        serde_json::to_string_pretty(&rule.get_untracked()[path_prefix][index]).unwrap_or_default()
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
                            rule.update(|v| { v[path_prefix][index] = parsed; });
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
    rule: RwSignal<Value>,
) -> impl IntoView {
    let devices = use_context::<RwSignal<Vec<DeviceState>>>().unwrap_or(RwSignal::new(vec![]));

    // Read primary device_id + additional device_ids from trigger
    let get_all_ids = move || -> Vec<String> {
        let trigger = rule.get()["trigger"].clone();
        let primary = trigger["device_id"].as_str().unwrap_or("").to_string();
        let mut ids = trigger["device_ids"].as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect::<Vec<_>>())
            .unwrap_or_default();
        // When device_ids is non-empty, primary is included in the set.
        // When device_ids is empty, just the primary.
        if ids.is_empty() && !primary.is_empty() {
            ids.push(primary);
        } else if !ids.is_empty() && !primary.is_empty() && !ids.contains(&primary) {
            ids.insert(0, primary);
        }
        ids
    };

    let set_ids = move |ids: Vec<String>| {
        rule.update(|v| {
            if ids.len() <= 1 {
                // Single device: use device_id only, clear device_ids.
                v["trigger"]["device_id"] = json!(ids.first().cloned().unwrap_or_default());
                if let Some(obj) = v["trigger"].as_object_mut() { obj.remove("device_ids"); }
            } else {
                // Multiple: device_id = first, device_ids = all.
                v["trigger"]["device_id"] = json!(ids[0]);
                v["trigger"]["device_ids"] = json!(ids);
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
) -> impl IntoView {
    let devices = use_context::<RwSignal<Vec<DeviceState>>>().unwrap_or(RwSignal::new(vec![]));

    let state_key = move || {
        if rule.get_untracked()[path][index]["type"].as_str() == Some("fade_device") { "target" } else { "state" }
    };

    view! {
        <div class="state-builder">
            {move || {
                let device_id = rule.get()[path][index]["device_id"].as_str().unwrap_or("").to_string();
                let sk = state_key();
                let state = rule.get()[path][index][sk].clone();

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
                            let sk = state_key();
                            rule.update(|v| { v[path][index][sk] = new_state; });
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
                                            let sk = state_key();
                                            rule.update(|v| { v[path][index][sk]["brightness_pct"] = json!(n); });
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
                                            let sk = state_key();
                                            rule.update(|v| { v[path][index][sk]["color_temp"] = json!(n); });
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
                                            let sk = state_key();
                                            rule.update(|v| { v[path][index][sk]["position"] = json!(n); });
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
                                            let sk = state_key();
                                            rule.update(|v| { v[path][index][sk]["volume"] = json!(n); });
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
                                            let sk = state_key();
                                            rule.update(|v| { v[path][index][sk]["bass"] = json!(n); });
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
                                            let sk = state_key();
                                            rule.update(|v| { v[path][index][sk]["treble"] = json!(n); });
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
                                    on:click=move |_| { let sk = state_key(); rule.update(|v| { v[path][index][sk]["muted"] = json!(true); }); }
                                >"Muted"</button>
                                <button class:active=state["muted"].as_bool() == Some(false)
                                    on:click=move |_| { let sk = state_key(); rule.update(|v| { v[path][index][sk]["muted"] = json!(false); }); }
                                >"Unmuted"</button>
                            </div>
                        </div>
                    }.into_any(),

                    "set_shuffle" => view! {
                        <div class="control-row">
                            <span class="control-label">"Shuffle"</span>
                            <div class="toggle-group">
                                <button class:active=state["shuffle"].as_bool() == Some(true)
                                    on:click=move |_| { let sk = state_key(); rule.update(|v| { v[path][index][sk]["shuffle"] = json!(true); }); }
                                >"On"</button>
                                <button class:active=state["shuffle"].as_bool() == Some(false)
                                    on:click=move |_| { let sk = state_key(); rule.update(|v| { v[path][index][sk]["shuffle"] = json!(false); }); }
                                >"Off"</button>
                            </div>
                        </div>
                    }.into_any(),

                    "set_loudness" => view! {
                        <div class="control-row">
                            <span class="control-label">"Loudness"</span>
                            <div class="toggle-group">
                                <button class:active=state["loudness"].as_bool() == Some(true)
                                    on:click=move |_| { let sk = state_key(); rule.update(|v| { v[path][index][sk]["loudness"] = json!(true); }); }
                                >"On"</button>
                                <button class:active=state["loudness"].as_bool() == Some(false)
                                    on:click=move |_| { let sk = state_key(); rule.update(|v| { v[path][index][sk]["loudness"] = json!(false); }); }
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
                                        let sk = state_key();
                                        rule.update(|v| { v[path][index][sk]["favorite"] = json!(event_target_value(&ev)); });
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
                                        let sk = state_key();
                                        rule.update(|v| { v[path][index][sk]["playlist"] = json!(event_target_value(&ev)); });
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

                    // Simple commands with no extra controls
                    "on_true" | "on_false" | "lock" | "unlock"
                    | "play" | "pause" | "stop" | "next" | "prev" => {
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

// ── SceneSelect ──────────────────────────────────────────────────────────────

#[component]
fn SceneSelect(value: String, on_select: Callback<String>) -> impl IntoView {
    let scenes = use_context::<RwSignal<Vec<Scene>>>().unwrap_or(RwSignal::new(vec![]));

    view! {
        <select class="hc-select"
            on:change=move |ev| on_select.run(event_target_value(&ev))
        >
            <option value="" selected=value.is_empty()>"— Select scene —"</option>
            {move || {
                let current = value.clone();
                let ss = scenes.get();
                let has_current = current.is_empty() || ss.iter().any(|s| s.id == current);
                let orphan = if !has_current {
                    Some(view! { <option value=current.clone() selected=true>{format!("{current} (unknown)")}</option> })
                } else { None };
                let options = ss.into_iter().map(|s| {
                    let sel = s.id == current;
                    view! { <option value=s.id.clone() selected=sel>{s.name.clone()}</option> }
                }).collect_view();
                view! { {orphan} {options} }
            }}
        </select>
    }
}

// ── CheckboxField ────────────────────────────────────────────────────────────

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
