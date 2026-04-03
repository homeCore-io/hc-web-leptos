//! Rule editor pages — create a new rule or edit an existing one.
//!
//! Working state model:
//!   Scalar rule fields are individual `RwSignal<T>` for fine-grained reactivity.
//!   `trigger`, `conditions`, and `actions` are JSON signals. Each condition/action
//!   row is its own `RwSignal<Value>` inside a `RwSignal<Vec<...>>` for row-level
//!   isolation.  Structured sub-editors (TriggerEditor, ConditionList, ActionList)
//!   will replace the JSON textareas in subsequent steps.

use crate::api::{clone_rule, create_rule, delete_rule, fetch_rule, rule_fire_history, test_rule, update_rule};
use crate::auth::use_auth;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_params_map};
use serde_json::{json, Value};

// ── Working state ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct RuleState {
    name:          RwSignal<String>,
    enabled:       RwSignal<bool>,
    priority:      RwSignal<i32>,
    tags:          RwSignal<Vec<String>>,
    cooldown_secs: RwSignal<String>,
    trigger_label: RwSignal<String>,
    run_mode:      RwSignal<String>,

    trigger:    RwSignal<Value>,
    conditions: RwSignal<Vec<RwSignal<Value>>>,
    actions:    RwSignal<Vec<RwSignal<Value>>>,

    required_expression: RwSignal<String>,
    cancel_on_false:     RwSignal<bool>,
    trigger_condition:   RwSignal<String>,
    log_events:          RwSignal<bool>,
    log_triggers:        RwSignal<bool>,
    log_actions:         RwSignal<bool>,
    variables:           RwSignal<String>,
}

impl RuleState {
    fn new_empty() -> Self {
        Self {
            name:                RwSignal::new(String::new()),
            enabled:             RwSignal::new(true),
            priority:            RwSignal::new(0),
            tags:                RwSignal::new(vec![]),
            cooldown_secs:       RwSignal::new(String::new()),
            trigger_label:       RwSignal::new(String::new()),
            run_mode:            RwSignal::new("parallel".to_string()),
            trigger:             RwSignal::new(json!({"type": "manual_trigger"})),
            conditions:          RwSignal::new(vec![]),
            actions:             RwSignal::new(vec![]),
            required_expression: RwSignal::new(String::new()),
            cancel_on_false:     RwSignal::new(false),
            trigger_condition:   RwSignal::new(String::new()),
            log_events:          RwSignal::new(false),
            log_triggers:        RwSignal::new(false),
            log_actions:         RwSignal::new(false),
            variables:           RwSignal::new("{}".to_string()),
        }
    }

    fn load_from(&self, rule: &Value) {
        self.name.set(rule["name"].as_str().unwrap_or("").to_string());
        self.enabled.set(rule["enabled"].as_bool().unwrap_or(true));
        self.priority.set(rule["priority"].as_i64().unwrap_or(0) as i32);
        self.tags.set(
            rule["tags"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
        );
        self.cooldown_secs.set(
            rule["cooldown_secs"].as_u64().map(|n| n.to_string()).unwrap_or_default(),
        );
        self.trigger_label.set(rule["trigger_label"].as_str().unwrap_or("").to_string());
        self.run_mode.set(
            rule["run_mode"]["type"].as_str().unwrap_or("parallel").to_string(),
        );
        if !rule["trigger"].is_null() {
            self.trigger.set(rule["trigger"].clone());
        }
        self.conditions.set(
            rule["conditions"]
                .as_array()
                .map(|a| a.iter().map(|v| RwSignal::new(v.clone())).collect())
                .unwrap_or_default(),
        );
        self.actions.set(
            rule["actions"]
                .as_array()
                .map(|a| a.iter().map(|v| RwSignal::new(v.clone())).collect())
                .unwrap_or_default(),
        );
        self.required_expression.set(
            rule["required_expression"].as_str().unwrap_or("").to_string(),
        );
        self.cancel_on_false.set(rule["cancel_on_false"].as_bool().unwrap_or(false));
        self.trigger_condition.set(
            rule["trigger_condition"].as_str().unwrap_or("").to_string(),
        );
        self.log_events.set(rule["log_events"].as_bool().unwrap_or(false));
        self.log_triggers.set(rule["log_triggers"].as_bool().unwrap_or(false));
        self.log_actions.set(rule["log_actions"].as_bool().unwrap_or(false));
        self.variables.set(
            rule["variables"]
                .as_object()
                .filter(|m| !m.is_empty())
                .map(|m| serde_json::to_string_pretty(&Value::Object(m.clone())).unwrap_or_default())
                .unwrap_or_else(|| "{}".to_string()),
        );
    }

    fn to_body(&self) -> Result<Value, String> {
        let name = self.name.get_untracked();
        if name.trim().is_empty() {
            return Err("Rule name is required.".to_string());
        }

        let conditions: Vec<Value> = self
            .conditions.get_untracked().iter().map(|s| s.get_untracked()).collect();
        let actions: Vec<Value> = self
            .actions.get_untracked().iter().map(|s| s.get_untracked()).collect();

        let variables: Value = serde_json::from_str(&self.variables.get_untracked())
            .unwrap_or(Value::Object(Default::default()));

        let cooldown: Option<u64> = {
            let raw = self.cooldown_secs.get_untracked();
            raw.trim().parse::<u64>().ok()
        };

        let run_mode = match self.run_mode.get_untracked().as_str() {
            "single"  => json!({"type": "single"}),
            "restart" => json!({"type": "restart"}),
            "queued"  => json!({"type": "queued", "max_queue": 10}),
            _         => json!({"type": "parallel"}),
        };

        let mut body = json!({
            "name":            name.trim(),
            "enabled":         self.enabled.get_untracked(),
            "priority":        self.priority.get_untracked(),
            "tags":            self.tags.get_untracked(),
            "trigger":         self.trigger.get_untracked(),
            "conditions":      conditions,
            "actions":         actions,
            "run_mode":        run_mode,
            "log_events":      self.log_events.get_untracked(),
            "log_triggers":    self.log_triggers.get_untracked(),
            "log_actions":     self.log_actions.get_untracked(),
            "cancel_on_false": self.cancel_on_false.get_untracked(),
        });

        if let Some(n) = cooldown {
            body["cooldown_secs"] = json!(n);
        }
        let tl = self.trigger_label.get_untracked();
        if !tl.trim().is_empty() { body["trigger_label"] = json!(tl.trim()); }
        let re = self.required_expression.get_untracked();
        if !re.trim().is_empty() { body["required_expression"] = json!(re.trim()); }
        let tc = self.trigger_condition.get_untracked();
        if !tc.trim().is_empty() { body["trigger_condition"] = json!(tc.trim()); }
        if !variables.as_object().map(|m| m.is_empty()).unwrap_or(true) {
            body["variables"] = variables;
        }

        Ok(body)
    }
}

// ── Route entry points ────────────────────────────────────────────────────────

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

// ── Editor ────────────────────────────────────────────────────────────────────

#[component]
fn RuleEditorPage(id: Option<Signal<String>>) -> impl IntoView {
    let auth    = use_auth();
    let is_new  = id.is_none();
    let state   = RuleState::new_empty();
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

    // ── Load (edit mode) ──────────────────────────────────────────────────────
    if let Some(id_sig) = id {
        Effect::new(move |_| {
            let token = match auth.token.get() { Some(t) => t, None => return };
            let rule_id = id_sig.get();
            if rule_id.is_empty() { return; }
            spawn_local(async move {
                match fetch_rule(&token, &rule_id).await {
                    Ok(rule) => { state.load_from(&rule); loading.set(false); }
                    Err(e)   => { save_err.set(Some(format!("Load failed: {e}"))); loading.set(false); }
                }
            });
        });
    }

    // ── Tag commit ────────────────────────────────────────────────────────────
    let commit_tag = move || {
        let raw = tag_input.get_untracked().trim().to_string();
        if raw.is_empty() { return; }
        state.tags.update(|tags| { if !tags.contains(&raw) { tags.push(raw); } });
        tag_input.set(String::new());
    };

    // ── View ──────────────────────────────────────────────────────────────────

    // navigate is cloned once per use so each on:click closure captures its own.
    let navigate = use_navigate();

    view! {
        <div class="rule-editor">

            // ── Heading ───────────────────────────────────────────────────────
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

            // ── Status banners ────────────────────────────────────────────────
            {move || save_err.get().map(|e| view! { <p class="msg-error">{e}</p> })}
            {move || save_ok.get().then(|| view! { <p class="msg-ok">"Saved."</p> })}
            {move || loading.get().then(|| view! { <p class="msg-muted">"Loading…"</p> })}

            // ── Editor (hidden while loading) ─────────────────────────────────
            <Show when=move || !loading.get()>
                <div class="rule-editor-cols">

                    // ── Left panel ─────────────────────────────────────────────
                    <div class="rule-editor-left">

                        // ── Metadata ──────────────────────────────────────────
                        <section class="detail-card">
                            <h3 class="detail-card-title">"Rule"</h3>

                            <label class="field-label">"Name"</label>
                            <input
                                type="text"
                                class="hc-input"
                                placeholder="Rule name"
                                prop:value=move || state.name.get()
                                on:input=move |ev| state.name.set(event_target_value(&ev))
                            />

                            <div class="rule-meta-row">
                                <CheckboxField label="Enabled" value=state.enabled />

                                <label class="field-label" style="margin:0">"Priority"</label>
                                <input
                                    type="number"
                                    class="hc-input hc-input--sm"
                                    style="width:5rem"
                                    prop:value=move || state.priority.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(n) = event_target_value(&ev).parse::<i32>() {
                                            state.priority.set(n);
                                        }
                                    }
                                />
                            </div>

                            // ── Tags ──────────────────────────────────────────
                            <label class="field-label">"Tags"</label>
                            <div class="tag-input-row">
                                {move || state.tags.get().into_iter().enumerate().map(|(i, tag)| {
                                    view! {
                                        <span class="tag-chip">
                                            {tag}
                                            <button
                                                class="tag-chip-remove"
                                                on:click=move |_| state.tags.update(|t| { t.remove(i); })
                                            >"×"</button>
                                        </span>
                                    }
                                }).collect_view()}
                                <input
                                    type="text"
                                    class="hc-input hc-input--sm tag-chip-input"
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

                            // ── Run mode ──────────────────────────────────────
                            <label class="field-label">"Run mode"</label>
                            <select
                                class="hc-select"
                                on:change=move |ev| state.run_mode.set(event_target_value(&ev))
                            >
                                {[
                                    ("parallel", "Parallel (default)"),
                                    ("single",   "Single — skip if running"),
                                    ("restart",  "Restart — cancel and restart"),
                                    ("queued",   "Queued"),
                                ].map(|(v, label)| view! {
                                    <option value=v selected=move || state.run_mode.get() == v>
                                        {label}
                                    </option>
                                }).collect_view()}
                            </select>

                            // ── Cooldown ──────────────────────────────────────
                            <label class="field-label">"Cooldown (seconds)"</label>
                            <input
                                type="number"
                                class="hc-input hc-input--sm"
                                style="width:8rem"
                                placeholder="None"
                                prop:value=move || state.cooldown_secs.get()
                                on:input=move |ev| state.cooldown_secs.set(event_target_value(&ev))
                            />
                        </section>

                        // ── Trigger ───────────────────────────────────────────
                        <section class="detail-card">
                            <h3 class="detail-card-title">"Trigger"</h3>
                            <TriggerEditor value=state.trigger />
                        </section>

                        // ── Advanced (collapsible) ────────────────────────────
                        <section class="detail-card">
                            <button
                                class="rule-advanced-toggle"
                                on:click=move |_| advanced_open.update(|v| *v = !*v)
                            >
                                {move || if advanced_open.get() { "▾ Advanced" } else { "▸ Advanced" }}
                            </button>

                            <Show when=move || advanced_open.get()>
                                <div class="rule-advanced-body">
                                    <label class="field-label">"Trigger label"</label>
                                    <input
                                        type="text"
                                        class="hc-input"
                                        placeholder="e.g. motion_hallway"
                                        prop:value=move || state.trigger_label.get()
                                        on:input=move |ev| state.trigger_label.set(event_target_value(&ev))
                                    />

                                    <label class="field-label">"Required expression (Rhai)"</label>
                                    <textarea
                                        class="hc-textarea hc-textarea--code"
                                        rows="3"
                                        placeholder="e.g. mode_is(\"mode_night\")"
                                        prop:value=move || state.required_expression.get()
                                        on:input=move |ev| state.required_expression.set(event_target_value(&ev))
                                    />

                                    <CheckboxField
                                        label="Cancel pending delays when required expression is false"
                                        value=state.cancel_on_false
                                    />

                                    <label class="field-label">"Trigger condition (Rhai)"</label>
                                    <textarea
                                        class="hc-textarea hc-textarea--code"
                                        rows="3"
                                        placeholder="Per-event condition expression"
                                        prop:value=move || state.trigger_condition.get()
                                        on:input=move |ev| state.trigger_condition.set(event_target_value(&ev))
                                    />

                                    <label class="field-label">"Variables (JSON object)"</label>
                                    <textarea
                                        class="hc-textarea hc-textarea--code"
                                        rows="4"
                                        prop:value=move || state.variables.get()
                                        on:input=move |ev| state.variables.set(event_target_value(&ev))
                                    />

                                    <div class="rule-logging-row">
                                        <span class="field-label" style="margin:0">"Logging:"</span>
                                        <CheckboxField label="Events"   value=state.log_events   />
                                        <CheckboxField label="Triggers" value=state.log_triggers />
                                        <CheckboxField label="Actions"  value=state.log_actions  />
                                    </div>
                                </div>
                            </Show>
                        </section>
                    </div>

                    // ── Right panel ────────────────────────────────────────────
                    <div class="rule-editor-right">

                        // ── Conditions ────────────────────────────────────────
                        <section class="detail-card">
                            <div class="rule-section-header">
                                <h3 class="detail-card-title">"Conditions"</h3>
                                <button
                                    class="hc-btn hc-btn--sm hc-btn--outline"
                                    on:click=move |_| state.conditions.update(|list| {
                                        list.push(RwSignal::new(json!({
                                            "type": "device_state",
                                            "device_id": "",
                                            "attribute": "on",
                                            "op": "eq",
                                            "value": true
                                        })));
                                    })
                                >"+ Add"</button>
                            </div>

                            {move || {
                                let conds = state.conditions.get();
                                if conds.is_empty() {
                                    view! {
                                        <p class="msg-muted" style="font-size:0.85rem">
                                            "No conditions — rule fires unconditionally."
                                        </p>
                                    }.into_any()
                                } else {
                                    let _len = conds.len();
                                    conds.into_iter().enumerate().map(|(i, sig)| {
                                        view! {
                                            <div class="json-row">
                                                <div class="json-row-controls">
                                                    <span class="json-row-index">{i + 1}</span>
                                                    <button
                                                        class="hc-btn hc-btn--sm hc-btn--outline"
                                                        title="Move up"
                                                        disabled=move || i == 0
                                                        on:click=move |_| state.conditions.update(|l| {
                                                            if i > 0 { l.swap(i - 1, i); }
                                                        })
                                                    >
                                                        <span class="material-icons" style="font-size:14px">"arrow_upward"</span>
                                                    </button>
                                                    <button
                                                        class="hc-btn hc-btn--sm hc-btn--outline"
                                                        title="Move down"
                                                        disabled=i + 1 >= len
                                                        on:click=move |_| state.conditions.update(|l| {
                                                            if i + 1 < l.len() { l.swap(i, i + 1); }
                                                        })
                                                    >
                                                        <span class="material-icons" style="font-size:14px">"arrow_downward"</span>
                                                    </button>
                                                    <button
                                                        class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline"
                                                        title="Remove"
                                                        on:click=move |_| state.conditions.update(|l| { l.remove(i); })
                                                    >
                                                        <span class="material-icons" style="font-size:14px">"close"</span>
                                                    </button>
                                                </div>
                                                <ConditionEditor value=sig />
                                            </div>
                                        }
                                    }).collect_view().into_any()
                                }
                            }}
                        </section>

                        // ── Actions ───────────────────────────────────────────
                        <section class="detail-card">
                            <div class="rule-section-header">
                                <h3 class="detail-card-title">"Actions"</h3>
                                <button
                                    class="hc-btn hc-btn--sm hc-btn--outline"
                                    on:click=move |_| state.actions.update(|list| {
                                        list.push(RwSignal::new(json!({
                                            "type": "log_message",
                                            "message": "",
                                            "enabled": true
                                        })));
                                    })
                                >"+ Add"</button>
                            </div>

                            {move || {
                                let acts = state.actions.get();
                                if acts.is_empty() {
                                    view! {
                                        <p class="msg-muted" style="font-size:0.85rem">"No actions."</p>
                                    }.into_any()
                                } else {
                                    let _len = acts.len();
                                    acts.into_iter().enumerate().map(|(i, sig)| {
                                        let enabled = sig.get_untracked()["enabled"].as_bool().unwrap_or(true);
                                        view! {
                                            <div class="json-row" class:json-row--disabled=!enabled>
                                                <div class="json-row-controls">
                                                    <span class="json-row-index">{i + 1}</span>
                                                    <button
                                                        class="hc-btn hc-btn--sm hc-btn--outline"
                                                        title="Move up"
                                                        disabled=move || i == 0
                                                        on:click=move |_| state.actions.update(|l| {
                                                            if i > 0 { l.swap(i - 1, i); }
                                                        })
                                                    >
                                                        <span class="material-icons" style="font-size:14px">"arrow_upward"</span>
                                                    </button>
                                                    <button
                                                        class="hc-btn hc-btn--sm hc-btn--outline"
                                                        title="Move down"
                                                        disabled=i + 1 >= len
                                                        on:click=move |_| state.actions.update(|l| {
                                                            if i + 1 < l.len() { l.swap(i, i + 1); }
                                                        })
                                                    >
                                                        <span class="material-icons" style="font-size:14px">"arrow_downward"</span>
                                                    </button>
                                                    <button
                                                        class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline"
                                                        title="Remove"
                                                        on:click=move |_| state.actions.update(|l| { l.remove(i); })
                                                    >
                                                        <span class="material-icons" style="font-size:14px">"close"</span>
                                                    </button>
                                                </div>
                                                <ActionEditor value=sig />
                                            </div>
                                        }
                                    }).collect_view().into_any()
                                }
                            }}
                        </section>
                    </div>
                </div>

                // ── Action bar ────────────────────────────────────────────────
                <div class="rule-action-bar">
                    {
                        // Each button gets its own navigate clone so no ownership conflicts.
                        let nav_save   = navigate.clone();
                        let nav_cancel = navigate.clone();
                        let nav_clone  = navigate.clone();
                        let nav_delete = navigate.clone();

                        view! {
                            // Save
                            <button
                                class="hc-btn hc-btn--primary"
                                disabled=move || saving.get()
                                on:click=move |_| {
                                    let token = match auth.token.get_untracked() {
                                        Some(t) => t, None => return,
                                    };
                                    let body = match state.to_body() {
                                        Ok(b) => b,
                                        Err(e) => { save_err.set(Some(e)); return; }
                                    };
                                    save_err.set(None);
                                    save_ok.set(false);
                                    saving.set(true);
                                    let nav = nav_save.clone();
                                    let rule_id = id.map(|s| s.get_untracked()).unwrap_or_default();
                                    spawn_local(async move {
                                        let result = if rule_id.is_empty() {
                                            create_rule(&token, &body).await
                                        } else {
                                            update_rule(&token, &rule_id, &body).await
                                        };
                                        match result {
                                            Ok(saved) => {
                                                if rule_id.is_empty() {
                                                    let new_id = saved["id"].as_str().unwrap_or("").to_string();
                                                    if !new_id.is_empty() {
                                                        nav(&format!("/rules/{new_id}"), Default::default());
                                                    }
                                                } else {
                                                    state.load_from(&saved);
                                                    save_ok.set(true);
                                                }
                                            }
                                            Err(e) => save_err.set(Some(e)),
                                        }
                                        saving.set(false);
                                    });
                                }
                            >
                                {move || if saving.get() { "Saving…" } else { "Save" }}
                            </button>

                            // Cancel
                            <button
                                class="hc-btn hc-btn--outline"
                                disabled=move || saving.get()
                                on:click=move |_| nav_cancel("/rules", Default::default())
                            >"Cancel"</button>

                            // Clone + Delete (edit mode only)
                            <Show when=move || !is_new>
                                {
                                    let nav_clone_btn = nav_clone.clone();
                                    let nav_delete_btn = nav_delete.clone();
                                    view! {
                                <button
                                    class="hc-btn hc-btn--outline"
                                    disabled=move || saving.get()
                                    title="Clone this rule"
                                    on:click=move |_| {
                                        let token = match auth.token.get_untracked() {
                                            Some(t) => t, None => return,
                                        };
                                        let rule_id = id.map(|s| s.get_untracked()).unwrap_or_default();
                                        if rule_id.is_empty() { return; }
                                        let nav = nav_clone_btn.clone();
                                        saving.set(true);
                                        spawn_local(async move {
                                            match clone_rule(&token, &rule_id).await {
                                                Ok(new_rule) => {
                                                    let new_id = new_rule["id"].as_str().unwrap_or("").to_string();
                                                    if !new_id.is_empty() {
                                                        nav(&format!("/rules/{new_id}"), Default::default());
                                                    }
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
                                            <button
                                                class="hc-btn hc-btn--sm hc-btn--danger"
                                                on:click=move |_| {
                                                    let token = match auth.token.get_untracked() {
                                                        Some(t) => t, None => return,
                                                    };
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
                                            <button
                                                class="hc-btn hc-btn--sm hc-btn--outline"
                                                on:click=move |_| confirm_delete.set(false)
                                            >"Cancel"</button>
                                        </span>
                                    }.into_any()
                                } else {
                                    view! {
                                        <button
                                            class="hc-btn hc-btn--outline hc-btn--danger-outline"
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
                            <button
                                class="hc-btn hc-btn--sm hc-btn--outline"
                                disabled=move || test_loading.get()
                                on:click=move |_| {
                                    let token = match auth.token.get_untracked() {
                                        Some(t) => t, None => return,
                                    };
                                    let rule_id = id.map(|s| s.get_untracked()).unwrap_or_default();
                                    if rule_id.is_empty() { return; }
                                    test_loading.set(true);
                                    test_err.set(None);
                                    test_result.set(None);
                                    spawn_local(async move {
                                        match test_rule(&token, &rule_id).await {
                                            Ok(r) => test_result.set(Some(r)),
                                            Err(e) => test_err.set(Some(e)),
                                        }
                                        test_loading.set(false);
                                    });
                                }
                            >
                                {move || if test_loading.get() { "Running…" } else { "Run Test" }}
                            </button>
                        </div>
                        <p class="msg-muted" style="font-size:0.78rem">
                            "Evaluates conditions and shows which actions would fire, without executing them."
                        </p>
                        {move || test_err.get().map(|e| view! { <p class="msg-error">{e}</p> })}
                        {move || test_result.get().map(|r| {
                            let pretty = serde_json::to_string_pretty(&r).unwrap_or_default();
                            view! {
                                <pre class="test-result-pre">{pretty}</pre>
                            }
                        })}
                    </section>
                </Show>

                // ── Fire History panel (edit mode only) ──────────────────────
                <Show when=move || !is_new>
                    <section class="detail-card">
                        <div class="rule-section-header">
                            <h3 class="detail-card-title">"Fire History"</h3>
                            <button
                                class="hc-btn hc-btn--sm hc-btn--outline"
                                disabled=move || history_loading.get()
                                on:click=move |_| {
                                    let token = match auth.token.get_untracked() {
                                        Some(t) => t, None => return,
                                    };
                                    let rule_id = id.map(|s| s.get_untracked()).unwrap_or_default();
                                    if rule_id.is_empty() { return; }
                                    history_loading.set(true);
                                    history_err.set(None);
                                    history_data.set(None);
                                    history_open.set(true);
                                    spawn_local(async move {
                                        match rule_fire_history(&token, &rule_id).await {
                                            Ok(h) => history_data.set(Some(h)),
                                            Err(e) => history_err.set(Some(e)),
                                        }
                                        history_loading.set(false);
                                    });
                                }
                            >
                                {move || if history_loading.get() { "Loading…" } else { "Load History" }}
                            </button>
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
                                                let ts = entry["timestamp"].as_str().unwrap_or("").to_string();
                                                let result = entry["result"].as_str().unwrap_or("ok").to_string();
                                                let actions_run = entry["actions_executed"].as_u64().unwrap_or(0);
                                                let elapsed = entry["elapsed_ms"].as_u64().unwrap_or(0);
                                                let is_err = result != "ok" && result != "success";
                                                view! {
                                                    <div class="history-entry" class:history-entry--error=is_err>
                                                        <span class="history-ts">{ts}</span>
                                                        <span class="history-result">{result}</span>
                                                        <span class="history-detail">
                                                            {format!("{actions_run} actions, {elapsed}ms")}
                                                        </span>
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

// ── TriggerEditor ─────────────────────────────────────────────────────────────

/// Returns a default JSON skeleton for a given trigger type string.
fn default_trigger_json(t: &str) -> Value {
    match t {
        "device_state_changed" => json!({
            "type": "device_state_changed",
            "device_id": ""
        }),
        "device_availability_changed" => json!({
            "type": "device_availability_changed",
            "device_id": ""
        }),
        "button_event" => json!({
            "type": "button_event",
            "device_id": "",
            "event": "pushed"
        }),
        "numeric_threshold" => json!({
            "type": "numeric_threshold",
            "device_id": "",
            "attribute": "",
            "op": "crosses_above",
            "value": 0.0
        }),
        "time_of_day" => json!({
            "type": "time_of_day",
            "time": "08:00:00",
            "days": ["Mon","Tue","Wed","Thu","Fri","Sat","Sun"]
        }),
        "sun_event" => json!({
            "type": "sun_event",
            "event": "sunset",
            "offset_minutes": 0
        }),
        "cron" => json!({
            "type": "cron",
            "expression": "0 0 8 * * *"
        }),
        "periodic" => json!({
            "type": "periodic",
            "every_n": 15,
            "unit": "minutes"
        }),
        "calendar_event" => json!({
            "type": "calendar_event",
            "offset_minutes": 0
        }),
        "custom_event" => json!({
            "type": "custom_event",
            "event_type": ""
        }),
        "system_started" => json!({"type": "system_started"}),
        "hub_variable_changed" => json!({"type": "hub_variable_changed"}),
        "mode_changed" => json!({"type": "mode_changed"}),
        "webhook_received" => json!({
            "type": "webhook_received",
            "path": "/hooks/"
        }),
        "mqtt_message" => json!({
            "type": "mqtt_message",
            "topic_pattern": "homecore/devices/+/state"
        }),
        _ => json!({"type": "manual_trigger"}),
    }
}

/// Structured trigger editor. Reads/writes `value: RwSignal<Value>`.
/// When the trigger type changes the entire JSON skeleton is replaced.
#[component]
fn TriggerEditor(value: RwSignal<Value>) -> impl IntoView {
    // ── helpers that update individual JSON fields in the trigger ────────────
    let set_str = move |key: &'static str, val: String| {
        value.update(|v| { v[key] = json!(val); });
    };
    let set_u64 = move |key: &'static str, raw: String| {
        value.update(|v| {
            if raw.trim().is_empty() {
                if let Some(obj) = v.as_object_mut() { obj.remove(key); }
            } else if let Ok(n) = raw.trim().parse::<u64>() {
                v[key] = json!(n);
            }
        });
    };
    let set_i64 = move |key: &'static str, raw: String| {
        value.update(|v| {
            if let Ok(n) = raw.trim().parse::<i64>() { v[key] = json!(n); }
        });
    };
    let set_f64 = move |key: &'static str, raw: String| {
        value.update(|v| {
            if let Ok(n) = raw.trim().parse::<f64>() { v[key] = json!(n); }
        });
    };
    // Set an optional JSON field from a text input. Empty string → removes field.
    let set_opt_json = move |key: &'static str, raw: String| {
        value.update(|v| {
            if raw.trim().is_empty() {
                if let Some(obj) = v.as_object_mut() { obj.remove(key); }
            } else {
                // try JSON parse first, fall back to string
                let parsed = serde_json::from_str::<Value>(raw.trim())
                    .unwrap_or_else(|_| json!(raw.trim()));
                v[key] = parsed;
            }
        });
    };
    let set_opt_str = move |key: &'static str, raw: String| {
        value.update(|v| {
            if raw.trim().is_empty() {
                if let Some(obj) = v.as_object_mut() { obj.remove(key); }
            } else {
                v[key] = json!(raw.trim());
            }
        });
    };

    view! {
        <div class="trigger-editor">

            // ── Type selector ─────────────────────────────────────────────────
            <label class="field-label">"Trigger type"</label>
            <select
                class="hc-select"
                on:change=move |ev| {
                    let t = event_target_value(&ev);
                    value.set(default_trigger_json(&t));
                }
            >
                <optgroup label="Device">
                    {["device_state_changed","device_availability_changed","button_event","numeric_threshold"]
                        .map(|t| {
                            let label = match t {
                                "device_state_changed"         => "Device state changed",
                                "device_availability_changed"  => "Device availability changed",
                                "button_event"                 => "Button event",
                                "numeric_threshold"            => "Numeric threshold",
                                _                              => t,
                            };
                            view! { <option value=t selected=move || value.get()["type"].as_str().unwrap_or("") == t>{label}</option> }
                        }).collect_view()
                    }
                </optgroup>
                <optgroup label="Time">
                    {["time_of_day","sun_event","cron","periodic","calendar_event"]
                        .map(|t| {
                            let label = match t {
                                "time_of_day"    => "Time of day",
                                "sun_event"      => "Sun event (sunrise/sunset)",
                                "cron"           => "Cron schedule",
                                "periodic"       => "Periodic (every N …)",
                                "calendar_event" => "Calendar event",
                                _                => t,
                            };
                            view! { <option value=t selected=move || value.get()["type"].as_str().unwrap_or("") == t>{label}</option> }
                        }).collect_view()
                    }
                </optgroup>
                <optgroup label="Event">
                    {["custom_event","system_started","hub_variable_changed","mode_changed","webhook_received","mqtt_message"]
                        .map(|t| {
                            let label = match t {
                                "custom_event"         => "Custom event",
                                "system_started"       => "System started",
                                "hub_variable_changed" => "Hub variable changed",
                                "mode_changed"         => "Mode changed",
                                "webhook_received"     => "Webhook received",
                                "mqtt_message"         => "MQTT message",
                                _                      => t,
                            };
                            view! { <option value=t selected=move || value.get()["type"].as_str().unwrap_or("") == t>{label}</option> }
                        }).collect_view()
                    }
                </optgroup>
                <optgroup label="Manual">
                    <option value="manual_trigger" selected=move || value.get()["type"].as_str().unwrap_or("") == "manual_trigger">"Manual trigger"</option>
                </optgroup>
            </select>

            // ── Type-specific fields ──────────────────────────────────────────
            {move || {
                let t = value.get()["type"].as_str().unwrap_or("").to_string();
                match t.as_str() {

                    // ── device_state_changed ──────────────────────────────────
                    "device_state_changed" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device ID"</label>
                            <input type="text" class="hc-input" placeholder="e.g. light.living_room"
                                prop:value=move || value.get()["device_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("device_id", event_target_value(&ev))
                            />

                            <label class="field-label">"Attribute (blank = any change)"</label>
                            <input type="text" class="hc-input" placeholder="e.g. on, brightness"
                                prop:value=move || value.get()["attribute"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("attribute", event_target_value(&ev))
                            />

                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"To (JSON value, blank = any)"</label>
                                    <input type="text" class="hc-input" placeholder=r#"e.g. true, 100, "on""#
                                        prop:value=move || {
                                            { let v = &value.get()["to"]; if v.is_null() { String::new() } else { v.to_string() } }
                                        }
                                        on:input=move |ev| set_opt_json("to", event_target_value(&ev))
                                    />
                                </div>
                                <div>
                                    <label class="field-label">"From (JSON value, blank = any)"</label>
                                    <input type="text" class="hc-input" placeholder=r#"e.g. false"#
                                        prop:value=move || {
                                            { let v = &value.get()["from"]; if v.is_null() { String::new() } else { v.to_string() } }
                                        }
                                        on:input=move |ev| set_opt_json("from", event_target_value(&ev))
                                    />
                                </div>
                            </div>

                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"Not-to (JSON, blank = ignore)"</label>
                                    <input type="text" class="hc-input" placeholder="exclude this target value"
                                        prop:value=move || {
                                            { let v = &value.get()["not_to"]; if v.is_null() { String::new() } else { v.to_string() } }
                                        }
                                        on:input=move |ev| set_opt_json("not_to", event_target_value(&ev))
                                    />
                                </div>
                                <div>
                                    <label class="field-label">"Not-from (JSON, blank = ignore)"</label>
                                    <input type="text" class="hc-input" placeholder="exclude this prior value"
                                        prop:value=move || {
                                            { let v = &value.get()["not_from"]; if v.is_null() { String::new() } else { v.to_string() } }
                                        }
                                        on:input=move |ev| set_opt_json("not_from", event_target_value(&ev))
                                    />
                                </div>
                            </div>

                            <label class="field-label">"Hold duration (seconds, blank = immediate)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="None"
                                prop:value=move || value.get()["for_duration_secs"].as_u64().map(|n| n.to_string()).unwrap_or_default()
                                on:input=move |ev| set_u64("for_duration_secs", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    // ── device_availability_changed ───────────────────────────
                    "device_availability_changed" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device ID"</label>
                            <input type="text" class="hc-input" placeholder="e.g. sensor.door"
                                prop:value=move || value.get()["device_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("device_id", event_target_value(&ev))
                            />

                            <label class="field-label">"Direction"</label>
                            <select class="hc-select"
                                on:change=move |ev| {
                                    let raw = event_target_value(&ev);
                                    value.update(|v| {
                                        match raw.as_str() {
                                            "online"  => v["to"] = json!(true),
                                            "offline" => v["to"] = json!(false),
                                            _         => { if let Some(obj) = v.as_object_mut() { obj.remove("to"); } }
                                        }
                                    });
                                }
                            >
                                <option value="any"    selected=move || value.get().get("to").map(|v| v.is_null()).unwrap_or(true)>"Any change"</option>
                                <option value="online" selected=move || value.get()["to"].as_bool() == Some(true)>"Goes online"</option>
                                <option value="offline" selected=move || value.get()["to"].as_bool() == Some(false)>"Goes offline"</option>
                            </select>

                            <label class="field-label">"Hold duration (seconds, blank = immediate)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="None"
                                prop:value=move || value.get()["for_duration_secs"].as_u64().map(|n| n.to_string()).unwrap_or_default()
                                on:input=move |ev| set_u64("for_duration_secs", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    // ── button_event ──────────────────────────────────────────
                    "button_event" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device ID"</label>
                            <input type="text" class="hc-input" placeholder="e.g. button.kitchen"
                                prop:value=move || value.get()["device_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("device_id", event_target_value(&ev))
                            />

                            <label class="field-label">"Event type"</label>
                            <select class="hc-select"
                                on:change=move |ev| set_str("event", event_target_value(&ev))
                            >
                                {["pushed","held","double_tapped","released"].map(|e| view! {
                                    <option value=e selected=move || value.get()["event"].as_str().unwrap_or("pushed") == e>
                                        {match e { "pushed" => "Pushed", "held" => "Held", "double_tapped" => "Double-tapped", _ => "Released" }}
                                    </option>
                                }).collect_view()}
                            </select>

                            <label class="field-label">"Button number (blank = any)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:6rem" placeholder="Any"
                                prop:value=move || value.get()["button_number"].as_u64().map(|n| n.to_string()).unwrap_or_default()
                                on:input=move |ev| set_u64("button_number", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    // ── numeric_threshold ─────────────────────────────────────
                    "numeric_threshold" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device ID"</label>
                            <input type="text" class="hc-input" placeholder="e.g. sensor.temp"
                                prop:value=move || value.get()["device_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("device_id", event_target_value(&ev))
                            />

                            <label class="field-label">"Attribute"</label>
                            <input type="text" class="hc-input" placeholder="e.g. temperature"
                                prop:value=move || value.get()["attribute"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("attribute", event_target_value(&ev))
                            />

                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"Operator"</label>
                                    <select class="hc-select"
                                        on:change=move |ev| set_str("op", event_target_value(&ev))
                                    >
                                        {[
                                            ("crosses_above", "Crosses above"),
                                            ("crosses_below", "Crosses below"),
                                            ("above",         "Is above (each change)"),
                                            ("below",         "Is below (each change)"),
                                        ].map(|(v, label)| view! {
                                            <option value=v selected=move || value.get()["op"].as_str().unwrap_or("crosses_above") == v>
                                                {label}
                                            </option>
                                        }).collect_view()}
                                    </select>
                                </div>
                                <div>
                                    <label class="field-label">"Threshold value"</label>
                                    <input type="number" class="hc-input" placeholder="0"
                                        prop:value=move || value.get()["value"].as_f64().unwrap_or(0.0).to_string()
                                        on:input=move |ev| set_f64("value", event_target_value(&ev))
                                    />
                                </div>
                            </div>

                            <label class="field-label">"Hold duration (seconds, blank = immediate)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="None"
                                prop:value=move || value.get()["for_duration_secs"].as_u64().map(|n| n.to_string()).unwrap_or_default()
                                on:input=move |ev| set_u64("for_duration_secs", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    // ── time_of_day ───────────────────────────────────────────
                    "time_of_day" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Time (HH:MM)"</label>
                            <input type="time" class="hc-input hc-input--sm" style="width:10rem"
                                prop:value=move || {
                                    // strip seconds if present: "08:00:00" → "08:00"
                                    let raw = value.get()["time"].as_str().unwrap_or("08:00:00").to_string();
                                    raw.get(..5).unwrap_or(&raw).to_string()
                                }
                                on:input=move |ev| {
                                    let hm = event_target_value(&ev); // "HH:MM"
                                    value.update(|v| { v["time"] = json!(format!("{hm}:00")); });
                                }
                            />

                            <label class="field-label">"Days"</label>
                            <div class="trigger-day-row">
                                {["Mon","Tue","Wed","Thu","Fri","Sat","Sun"].map(|day| {
                                    view! {
                                        <label class="day-chip">
                                            <input
                                                type="checkbox"
                                                prop:checked=move || {
                                                    value.get()["days"].as_array()
                                                        .map(|a| a.iter().any(|d| d.as_str() == Some(day)))
                                                        .unwrap_or(false)
                                                }
                                                on:change=move |ev| {
                                                    use wasm_bindgen::JsCast;
                                                    let checked = ev.target()
                                                        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                        .map(|el| el.checked())
                                                        .unwrap_or(false);
                                                    value.update(|v| {
                                                        let days = v["days"].as_array_mut()
                                                            .expect("days is always array");
                                                        if checked {
                                                            if !days.iter().any(|d| d.as_str() == Some(day)) {
                                                                days.push(json!(day));
                                                            }
                                                        } else {
                                                            days.retain(|d| d.as_str() != Some(day));
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

                    // ── sun_event ─────────────────────────────────────────────
                    "sun_event" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Sun event"</label>
                            <select class="hc-select"
                                on:change=move |ev| set_str("event", event_target_value(&ev))
                            >
                                {[
                                    ("sunrise",    "Sunrise"),
                                    ("sunset",     "Sunset"),
                                    ("solar_noon", "Solar noon"),
                                    ("civil_dawn", "Civil dawn"),
                                    ("civil_dusk", "Civil dusk"),
                                ].map(|(v, label)| view! {
                                    <option value=v selected=move || value.get()["event"].as_str().unwrap_or("sunset") == v>
                                        {label}
                                    </option>
                                }).collect_view()}
                            </select>

                            <label class="field-label">"Offset (minutes, negative = before)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem"
                                prop:value=move || value.get()["offset_minutes"].as_i64().unwrap_or(0).to_string()
                                on:input=move |ev| set_i64("offset_minutes", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    // ── cron ──────────────────────────────────────────────────
                    "cron" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Cron expression (6-field: sec min hour dom month dow)"</label>
                            <input type="text" class="hc-input hc-textarea--code" placeholder="0 0 8 * * *"
                                prop:value=move || value.get()["expression"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("expression", event_target_value(&ev))
                            />
                            <p class="msg-muted" style="font-size:0.78rem; margin:0.2rem 0 0">
                                "Example: " <code>"0 30 7 * * Mon-Fri"</code> " = 7:30 AM on weekdays"
                            </p>
                        </div>
                    }.into_any(),

                    // ── periodic ──────────────────────────────────────────────
                    "periodic" => view! {
                        <div class="trigger-fields">
                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"Every N"</label>
                                    <input type="number" class="hc-input" placeholder="15" min="1"
                                        prop:value=move || value.get()["every_n"].as_u64().unwrap_or(15).to_string()
                                        on:input=move |ev| {
                                            if let Ok(n) = event_target_value(&ev).trim().parse::<u64>() {
                                                value.update(|v| { v["every_n"] = json!(n); });
                                            }
                                        }
                                    />
                                </div>
                                <div>
                                    <label class="field-label">"Unit"</label>
                                    <select class="hc-select"
                                        on:change=move |ev| set_str("unit", event_target_value(&ev))
                                    >
                                        {["minutes","hours","days","weeks"].map(|u| view! {
                                            <option value=u selected=move || value.get()["unit"].as_str().unwrap_or("minutes") == u>
                                                {match u { "minutes" => "Minutes", "hours" => "Hours", "days" => "Days", _ => "Weeks" }}
                                            </option>
                                        }).collect_view()}
                                    </select>
                                </div>
                            </div>
                        </div>
                    }.into_any(),

                    // ── calendar_event ────────────────────────────────────────
                    "calendar_event" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Calendar ID (blank = any calendar)"</label>
                            <input type="text" class="hc-input" placeholder="e.g. us_holidays"
                                prop:value=move || value.get()["calendar_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("calendar_id", event_target_value(&ev))
                            />

                            <label class="field-label">"Title contains (blank = any event)"</label>
                            <input type="text" class="hc-input" placeholder="e.g. Holiday"
                                prop:value=move || value.get()["title_contains"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("title_contains", event_target_value(&ev))
                            />

                            <label class="field-label">"Offset (minutes, negative = before event)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem"
                                prop:value=move || value.get()["offset_minutes"].as_i64().unwrap_or(0).to_string()
                                on:input=move |ev| set_i64("offset_minutes", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    // ── custom_event ──────────────────────────────────────────
                    "custom_event" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Event type"</label>
                            <input type="text" class="hc-input" placeholder="e.g. motion_detected"
                                prop:value=move || value.get()["event_type"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("event_type", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    // ── hub_variable_changed ──────────────────────────────────
                    "hub_variable_changed" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Variable name (blank = any variable)"</label>
                            <input type="text" class="hc-input" placeholder="e.g. alarm_state"
                                prop:value=move || value.get()["name"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("name", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    // ── mode_changed ──────────────────────────────────────────
                    "mode_changed" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Mode ID (blank = any mode)"</label>
                            <input type="text" class="hc-input" placeholder="e.g. mode_night"
                                prop:value=move || value.get()["mode_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("mode_id", event_target_value(&ev))
                            />

                            <label class="field-label">"Direction"</label>
                            <select class="hc-select"
                                on:change=move |ev| {
                                    let raw = event_target_value(&ev);
                                    value.update(|v| {
                                        match raw.as_str() {
                                            "on"  => v["to"] = json!(true),
                                            "off" => v["to"] = json!(false),
                                            _     => { if let Some(obj) = v.as_object_mut() { obj.remove("to"); } }
                                        }
                                    });
                                }
                            >
                                <option value="any" selected=move || !value.get().get("to").is_some_and(|v| !v.is_null())>"Any change"</option>
                                <option value="on"  selected=move || value.get()["to"].as_bool() == Some(true)>"Turns on"</option>
                                <option value="off" selected=move || value.get()["to"].as_bool() == Some(false)>"Turns off"</option>
                            </select>
                        </div>
                    }.into_any(),

                    // ── webhook_received ──────────────────────────────────────
                    "webhook_received" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Webhook path"</label>
                            <input type="text" class="hc-input" placeholder="e.g. /hooks/doorbell"
                                prop:value=move || value.get()["path"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("path", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    // ── mqtt_message ──────────────────────────────────────────
                    "mqtt_message" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Topic pattern"</label>
                            <input type="text" class="hc-input hc-textarea--code" placeholder="homecore/devices/+/state"
                                prop:value=move || value.get()["topic_pattern"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("topic_pattern", event_target_value(&ev))
                            />

                            <label class="field-label">"Exact payload match (blank = any payload)"</label>
                            <input type="text" class="hc-input" placeholder="e.g. ON"
                                prop:value=move || value.get()["payload"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("payload", event_target_value(&ev))
                            />

                            <label class="field-label">"JSON value path (blank = no extraction)"</label>
                            <input type="text" class="hc-input hc-textarea--code" placeholder=r#"e.g. /temperature"#
                                prop:value=move || value.get()["value_path"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("value_path", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    // ── manual_trigger / system_started / fallback ────────────
                    _ => view! {
                        <div class="trigger-fields">
                            <p class="msg-muted" style="font-size:0.85rem">
                                {if t == "system_started" {
                                    "Fires once when the rule engine finishes starting up."
                                } else {
                                    "This trigger has no configurable fields."
                                }}
                            </p>
                        </div>
                    }.into_any(),
                }
            }}
        </div>
    }
}

// ── ConditionEditor ───────────────────────────────────────────────────────────

fn default_condition_json(t: &str) -> Value {
    match t {
        "device_state" => json!({
            "type": "device_state", "device_id": "", "attribute": "on", "op": "eq", "value": true
        }),
        "time_window" => json!({
            "type": "time_window", "start": "08:00:00", "end": "22:00:00"
        }),
        "script_expression" => json!({"type": "script_expression", "script": ""}),
        "time_elapsed" => json!({
            "type": "time_elapsed", "device_id": "", "attribute": "", "duration_secs": 60
        }),
        "device_last_change" => json!({"type": "device_last_change", "device_id": ""}),
        "private_boolean_is" => json!({"type": "private_boolean_is", "name": "", "value": true}),
        "hub_variable" => json!({
            "type": "hub_variable", "name": "", "op": "eq", "value": ""
        }),
        "mode_is" => json!({"type": "mode_is", "mode_id": "", "on": true}),
        // Nested types fall back to JSON
        _ => json!({"type": t}),
    }
}

/// Structured condition form. For nested types (not/and/or/xor) falls back to JSON textarea.
#[component]
fn ConditionEditor(value: RwSignal<Value>) -> impl IntoView {
    let set_str = move |key: &'static str, val: String| {
        value.update(|v| { v[key] = json!(val); });
    };
    let set_opt_str = move |key: &'static str, raw: String| {
        value.update(|v| {
            if raw.trim().is_empty() {
                if let Some(obj) = v.as_object_mut() { obj.remove(key); }
            } else {
                v[key] = json!(raw.trim());
            }
        });
    };
    let set_opt_json = move |key: &'static str, raw: String| {
        value.update(|v| {
            if raw.trim().is_empty() {
                if let Some(obj) = v.as_object_mut() { obj.remove(key); }
            } else {
                let parsed = serde_json::from_str::<Value>(raw.trim())
                    .unwrap_or_else(|_| json!(raw.trim()));
                v[key] = parsed;
            }
        });
    };
    let set_u64 = move |key: &'static str, raw: String| {
        value.update(|v| {
            if let Ok(n) = raw.trim().parse::<u64>() { v[key] = json!(n); }
        });
    };
    let set_bool = move |key: &'static str, raw: String| {
        value.update(|v| {
            v[key] = json!(raw == "true");
        });
    };

    // Condition types that get nested editors (JSON fallback)
    let _is_nested = move || {
        matches!(
            value.get()["type"].as_str().unwrap_or(""),
            "not" | "and" | "or" | "xor"
        )
    };

    view! {
        <div class="condition-editor">
            <label class="field-label">"Condition type"</label>
            <select
                class="hc-select"
                on:change=move |ev| {
                    let t = event_target_value(&ev);
                    value.set(default_condition_json(&t));
                }
            >
                {[
                    ("device_state",      "Device state"),
                    ("time_window",       "Time window"),
                    ("script_expression", "Script expression (Rhai)"),
                    ("time_elapsed",      "Time elapsed"),
                    ("device_last_change","Device last change"),
                    ("private_boolean_is","Private boolean"),
                    ("hub_variable",      "Hub variable"),
                    ("mode_is",           "Mode is on/off"),
                    ("not",               "NOT (negate)"),
                    ("and",               "AND (all of)"),
                    ("or",                "OR (any of)"),
                    ("xor",               "XOR (exactly one)"),
                ].map(|(v, label)| view! {
                    <option value=v selected=move || value.get()["type"].as_str().unwrap_or("") == v>{label}</option>
                }).collect_view()}
            </select>

            {move || {
                let t = value.get()["type"].as_str().unwrap_or("").to_string();
                match t.as_str() {
                    "device_state" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device ID"</label>
                            <input type="text" class="hc-input" placeholder="e.g. light.living_room"
                                prop:value=move || value.get()["device_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("device_id", event_target_value(&ev))
                            />
                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"Attribute"</label>
                                    <input type="text" class="hc-input" placeholder="e.g. on"
                                        prop:value=move || value.get()["attribute"].as_str().unwrap_or("").to_string()
                                        on:input=move |ev| set_str("attribute", event_target_value(&ev))
                                    />
                                </div>
                                <div>
                                    <label class="field-label">"Operator"</label>
                                    <select class="hc-select"
                                        on:change=move |ev| set_str("op", event_target_value(&ev))
                                    >
                                        {[("eq","="),("ne","≠"),("gt",">"),("gte","≥"),("lt","<"),("lte","≤")]
                                            .map(|(v, label)| view! {
                                                <option value=v selected=move || value.get()["op"].as_str().unwrap_or("eq") == v>{label}</option>
                                            }).collect_view()}
                                    </select>
                                </div>
                            </div>
                            <label class="field-label">"Value (JSON)"</label>
                            <input type="text" class="hc-input" placeholder=r#"e.g. true, 100, "on""#
                                prop:value=move || {
                                    let v = &value.get()["value"];
                                    if v.is_null() { String::new() } else { v.to_string() }
                                }
                                on:input=move |ev| set_opt_json("value", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "time_window" => view! {
                        <div class="trigger-fields">
                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"Start (HH:MM)"</label>
                                    <input type="time" class="hc-input hc-input--sm"
                                        prop:value=move || value.get()["start"].as_str().unwrap_or("08:00:00").get(..5).unwrap_or("08:00").to_string()
                                        on:input=move |ev| {
                                            let hm = event_target_value(&ev);
                                            value.update(|v| { v["start"] = json!(format!("{hm}:00")); });
                                        }
                                    />
                                </div>
                                <div>
                                    <label class="field-label">"End (HH:MM)"</label>
                                    <input type="time" class="hc-input hc-input--sm"
                                        prop:value=move || value.get()["end"].as_str().unwrap_or("22:00:00").get(..5).unwrap_or("22:00").to_string()
                                        on:input=move |ev| {
                                            let hm = event_target_value(&ev);
                                            value.update(|v| { v["end"] = json!(format!("{hm}:00")); });
                                        }
                                    />
                                </div>
                            </div>
                        </div>
                    }.into_any(),

                    "script_expression" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Rhai expression (must return bool)"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="4"
                                placeholder=r#"e.g. device_attr("sensor_1", "temperature") > 75.0"#
                                prop:value=move || value.get()["script"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("script", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "time_elapsed" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device ID"</label>
                            <input type="text" class="hc-input" placeholder="e.g. sensor.door"
                                prop:value=move || value.get()["device_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("device_id", event_target_value(&ev))
                            />
                            <label class="field-label">"Attribute"</label>
                            <input type="text" class="hc-input" placeholder="e.g. open"
                                prop:value=move || value.get()["attribute"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("attribute", event_target_value(&ev))
                            />
                            <label class="field-label">"Duration (seconds)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem"
                                prop:value=move || value.get()["duration_secs"].as_u64().unwrap_or(60).to_string()
                                on:input=move |ev| set_u64("duration_secs", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "device_last_change" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device ID"</label>
                            <input type="text" class="hc-input"
                                prop:value=move || value.get()["device_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("device_id", event_target_value(&ev))
                            />
                            <label class="field-label">"Change kind (blank = any)"</label>
                            <input type="text" class="hc-input" placeholder="e.g. mqtt, api, rule"
                                prop:value=move || value.get()["kind"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("kind", event_target_value(&ev))
                            />
                            <label class="field-label">"Source (blank = any)"</label>
                            <input type="text" class="hc-input"
                                prop:value=move || value.get()["source"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("source", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "private_boolean_is" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Boolean name"</label>
                            <input type="text" class="hc-input" placeholder="e.g. motion_active"
                                prop:value=move || value.get()["name"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("name", event_target_value(&ev))
                            />
                            <label class="field-label">"Expected value"</label>
                            <select class="hc-select"
                                on:change=move |ev| set_bool("value", event_target_value(&ev))
                            >
                                <option value="true"  selected=move || value.get()["value"].as_bool() != Some(false)>"True"</option>
                                <option value="false" selected=move || value.get()["value"].as_bool() == Some(false)>"False"</option>
                            </select>
                        </div>
                    }.into_any(),

                    "hub_variable" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Variable name"</label>
                            <input type="text" class="hc-input" placeholder="e.g. alarm_state"
                                prop:value=move || value.get()["name"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("name", event_target_value(&ev))
                            />
                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"Operator"</label>
                                    <select class="hc-select"
                                        on:change=move |ev| set_str("op", event_target_value(&ev))
                                    >
                                        {[("eq","="),("ne","≠"),("gt",">"),("gte","≥"),("lt","<"),("lte","≤")]
                                            .map(|(v, label)| view! {
                                                <option value=v selected=move || value.get()["op"].as_str().unwrap_or("eq") == v>{label}</option>
                                            }).collect_view()}
                                    </select>
                                </div>
                                <div>
                                    <label class="field-label">"Value (JSON)"</label>
                                    <input type="text" class="hc-input" placeholder=r#"e.g. "armed", 42"#
                                        prop:value=move || {
                                            let v = &value.get()["value"];
                                            if v.is_null() { String::new() } else { v.to_string() }
                                        }
                                        on:input=move |ev| set_opt_json("value", event_target_value(&ev))
                                    />
                                </div>
                            </div>
                        </div>
                    }.into_any(),

                    "mode_is" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Mode ID"</label>
                            <input type="text" class="hc-input" placeholder="e.g. mode_night"
                                prop:value=move || value.get()["mode_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("mode_id", event_target_value(&ev))
                            />
                            <label class="field-label">"Expected state"</label>
                            <select class="hc-select"
                                on:change=move |ev| set_bool("on", event_target_value(&ev))
                            >
                                <option value="true"  selected=move || value.get()["on"].as_bool() != Some(false)>"On"</option>
                                <option value="false" selected=move || value.get()["on"].as_bool() == Some(false)>"Off"</option>
                            </select>
                        </div>
                    }.into_any(),

                    // Nested types (not/and/or/xor) → JSON fallback
                    _ => view! {
                        <div class="trigger-fields">
                            <p class="msg-muted" style="font-size:0.78rem">"Nested condition — edit as JSON:"</p>
                            <JsonTextarea value=value label="" rows=6 />
                        </div>
                    }.into_any(),
                }
            }}
        </div>
    }
}

// ── ActionEditor ─────────────────────────────────────────────────────────────

/// All action types that have structured forms (Phase 1).
const STRUCTURED_ACTIONS: &[(&str, &str)] = &[
    ("set_device_state",   "Set device state"),
    ("publish_mqtt",       "Publish MQTT"),
    ("call_service",       "Call HTTP service"),
    ("fire_event",         "Fire event"),
    ("run_script",         "Run script (Rhai)"),
    ("notify",             "Notify"),
    ("delay",              "Delay"),
    ("set_variable",       "Set variable"),
    ("set_hub_variable",   "Set hub variable"),
    ("set_mode",           "Set mode"),
    ("run_rule_actions",   "Run rule actions"),
    ("pause_rule",         "Pause rule"),
    ("resume_rule",        "Resume rule"),
    ("cancel_delays",      "Cancel delays"),
    ("cancel_rule_timers", "Cancel rule timers"),
    ("set_private_boolean","Set private boolean"),
    ("log_message",        "Log message"),
    ("comment",            "Comment"),
    ("stop_rule_chain",    "Stop rule chain"),
    ("exit_rule",          "Exit rule"),
    ("wait_for_event",     "Wait for event"),
    ("wait_for_expression","Wait for expression"),
    ("capture_device_state","Capture device state"),
    ("restore_device_state","Restore device state"),
    ("fade_device",        "Fade device"),
];

/// Action types that use JSON fallback (nested block actions).
const BLOCK_ACTIONS: &[(&str, &str)] = &[
    ("parallel",                "Parallel (block)"),
    ("conditional",             "Conditional (block)"),
    ("repeat_until",            "Repeat until (block)"),
    ("repeat_while",            "Repeat while (block)"),
    ("repeat_count",            "Repeat count (block)"),
    ("ping_host",               "Ping host (block)"),
    ("set_device_state_per_mode","Set state per mode (block)"),
    ("delay_per_mode",          "Delay per mode (block)"),
    ("activate_scene_per_mode", "Scene per mode (block)"),
];

fn default_action_json(t: &str) -> Value {
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
        // Block types
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
    // Preserve enabled flag
    base["enabled"] = json!(true);
    base
}

#[component]
fn ActionEditor(value: RwSignal<Value>) -> impl IntoView {
    let set_str = move |key: &'static str, val: String| {
        value.update(|v| { v[key] = json!(val); });
    };
    let set_opt_str = move |key: &'static str, raw: String| {
        value.update(|v| {
            if raw.trim().is_empty() {
                if let Some(obj) = v.as_object_mut() { obj.remove(key); }
            } else {
                v[key] = json!(raw.trim());
            }
        });
    };
    let set_u64 = move |key: &'static str, raw: String| {
        value.update(|v| {
            if let Ok(n) = raw.trim().parse::<u64>() { v[key] = json!(n); }
        });
    };
    let set_opt_u64 = move |key: &'static str, raw: String| {
        value.update(|v| {
            if raw.trim().is_empty() {
                if let Some(obj) = v.as_object_mut() { obj.remove(key); }
            } else if let Ok(n) = raw.trim().parse::<u64>() {
                v[key] = json!(n);
            }
        });
    };
    let set_bool = move |key: &'static str, raw: String| {
        value.update(|v| { v[key] = json!(raw == "true"); });
    };

    // Determine if this is a block type that needs JSON fallback
    let _is_block = move || {
        BLOCK_ACTIONS.iter().any(|(k, _)| value.get()["type"].as_str() == Some(k))
    };

    view! {
        <div class="action-editor">
            // ── Enabled toggle + type selector ────────────────────────────────
            <div class="action-header-row">
                <label class="rule-meta-inline">
                    <input type="checkbox"
                        prop:checked=move || value.get()["enabled"].as_bool().unwrap_or(true)
                        on:change=move |ev| {
                            use wasm_bindgen::JsCast;
                            let checked = ev.target()
                                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                .map(|el| el.checked())
                                .unwrap_or(true);
                            value.update(|v| { v["enabled"] = json!(checked); });
                        }
                    />
                    " Enabled"
                </label>
            </div>

            <label class="field-label">"Action type"</label>
            <select class="hc-select"
                on:change=move |ev| {
                    let t = event_target_value(&ev);
                    value.set(default_action_json(&t));
                }
            >
                <optgroup label="Device">
                    {["set_device_state","fade_device","capture_device_state","restore_device_state"]
                        .map(|t| {
                            let label = STRUCTURED_ACTIONS.iter().find(|(k,_)| *k == t).map(|(_,l)| *l).unwrap_or(t);
                            view! { <option value=t selected=move || value.get()["type"].as_str() == Some(t)>{label}</option> }
                        }).collect_view()
                    }
                </optgroup>
                <optgroup label="Communication">
                    {["publish_mqtt","call_service","fire_event","notify","log_message","comment"]
                        .map(|t| {
                            let label = STRUCTURED_ACTIONS.iter().find(|(k,_)| *k == t).map(|(_,l)| *l).unwrap_or(t);
                            view! { <option value=t selected=move || value.get()["type"].as_str() == Some(t)>{label}</option> }
                        }).collect_view()
                    }
                </optgroup>
                <optgroup label="Script & Variables">
                    {["run_script","set_variable","set_hub_variable","set_private_boolean"]
                        .map(|t| {
                            let label = STRUCTURED_ACTIONS.iter().find(|(k,_)| *k == t).map(|(_,l)| *l).unwrap_or(t);
                            view! { <option value=t selected=move || value.get()["type"].as_str() == Some(t)>{label}</option> }
                        }).collect_view()
                    }
                </optgroup>
                <optgroup label="Timing & Flow">
                    {["delay","wait_for_event","wait_for_expression","stop_rule_chain","exit_rule"]
                        .map(|t| {
                            let label = STRUCTURED_ACTIONS.iter().find(|(k,_)| *k == t).map(|(_,l)| *l).unwrap_or(t);
                            view! { <option value=t selected=move || value.get()["type"].as_str() == Some(t)>{label}</option> }
                        }).collect_view()
                    }
                </optgroup>
                <optgroup label="Mode & Rule Control">
                    {["set_mode","run_rule_actions","pause_rule","resume_rule","cancel_delays","cancel_rule_timers"]
                        .map(|t| {
                            let label = STRUCTURED_ACTIONS.iter().find(|(k,_)| *k == t).map(|(_,l)| *l).unwrap_or(t);
                            view! { <option value=t selected=move || value.get()["type"].as_str() == Some(t)>{label}</option> }
                        }).collect_view()
                    }
                </optgroup>
                <optgroup label="Block actions (JSON)">
                    {BLOCK_ACTIONS.iter().map(|(t, label)| view! {
                        <option value=*t selected=move || value.get()["type"].as_str() == Some(t)>{*label}</option>
                    }).collect_view()}
                </optgroup>
            </select>

            // ── Type-specific fields ──────────────────────────────────────────
            {move || {
                let t = value.get()["type"].as_str().unwrap_or("").to_string();
                match t.as_str() {

                    "set_device_state" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device ID"</label>
                            <input type="text" class="hc-input" placeholder="e.g. light.living_room"
                                prop:value=move || value.get()["device_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("device_id", event_target_value(&ev))
                            />
                            <label class="field-label">"State (JSON)"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="3"
                                placeholder=r#"{"on": true, "brightness": 200}"#
                                prop:value=move || serde_json::to_string_pretty(&value.get()["state"]).unwrap_or_default()
                                on:input=move |ev| {
                                    if let Ok(parsed) = serde_json::from_str::<Value>(&event_target_value(&ev)) {
                                        value.update(|v| { v["state"] = parsed; });
                                    }
                                }
                            />
                        </div>
                    }.into_any(),

                    "publish_mqtt" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Topic"</label>
                            <input type="text" class="hc-input hc-textarea--code" placeholder="homecore/devices/my_device/cmd"
                                prop:value=move || value.get()["topic"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("topic", event_target_value(&ev))
                            />
                            <label class="field-label">"Payload"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="2"
                                prop:value=move || value.get()["payload"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("payload", event_target_value(&ev))
                            />
                            <label class="rule-meta-inline">
                                <input type="checkbox"
                                    prop:checked=move || value.get()["retain"].as_bool().unwrap_or(false)
                                    on:change=move |ev| {
                                        use wasm_bindgen::JsCast;
                                        let checked = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).map(|el| el.checked()).unwrap_or(false);
                                        value.update(|v| { v["retain"] = json!(checked); });
                                    }
                                />
                                " Retain"
                            </label>
                        </div>
                    }.into_any(),

                    "call_service" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"URL"</label>
                            <input type="text" class="hc-input" placeholder="https://..."
                                prop:value=move || value.get()["url"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("url", event_target_value(&ev))
                            />
                            <label class="field-label">"HTTP method"</label>
                            <select class="hc-select" on:change=move |ev| set_str("method", event_target_value(&ev))>
                                {["GET","POST","PUT","PATCH","DELETE"].map(|m| view! {
                                    <option value=m selected=move || value.get()["method"].as_str().unwrap_or("POST") == m>{m}</option>
                                }).collect_view()}
                            </select>
                            <label class="field-label">"Body (JSON, optional)"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="3"
                                prop:value=move || serde_json::to_string_pretty(&value.get()["body"]).unwrap_or_default()
                                on:input=move |ev| {
                                    if let Ok(parsed) = serde_json::from_str::<Value>(&event_target_value(&ev)) {
                                        value.update(|v| { v["body"] = parsed; });
                                    }
                                }
                            />
                            <label class="field-label">"Timeout (ms, blank = default)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem"
                                prop:value=move || value.get()["timeout_ms"].as_u64().map(|n| n.to_string()).unwrap_or_default()
                                on:input=move |ev| set_opt_u64("timeout_ms", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "fire_event" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Event type"</label>
                            <input type="text" class="hc-input" placeholder="e.g. motion_detected"
                                prop:value=move || value.get()["event_type"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("event_type", event_target_value(&ev))
                            />
                            <label class="field-label">"Payload (JSON)"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="2"
                                prop:value=move || serde_json::to_string_pretty(&value.get()["payload"]).unwrap_or_default()
                                on:input=move |ev| {
                                    if let Ok(parsed) = serde_json::from_str::<Value>(&event_target_value(&ev)) {
                                        value.update(|v| { v["payload"] = parsed; });
                                    }
                                }
                            />
                        </div>
                    }.into_any(),

                    "run_script" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Rhai script"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="6"
                                placeholder="// Rhai script body"
                                prop:value=move || value.get()["script"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("script", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "notify" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Channel"</label>
                            <input type="text" class="hc-input" placeholder="e.g. telegram, pushover, all"
                                prop:value=move || value.get()["channel"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("channel", event_target_value(&ev))
                            />
                            <label class="field-label">"Message"</label>
                            <textarea class="hc-textarea" rows="2"
                                prop:value=move || value.get()["message"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("message", event_target_value(&ev))
                            />
                            <label class="field-label">"Title (optional)"</label>
                            <input type="text" class="hc-input"
                                prop:value=move || value.get()["title"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("title", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "delay" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Duration (seconds)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem"
                                prop:value=move || value.get()["duration_secs"].as_u64().unwrap_or(5).to_string()
                                on:input=move |ev| set_u64("duration_secs", event_target_value(&ev))
                            />
                            <label class="rule-meta-inline">
                                <input type="checkbox"
                                    prop:checked=move || value.get()["cancelable"].as_bool().unwrap_or(false)
                                    on:change=move |ev| {
                                        use wasm_bindgen::JsCast;
                                        let checked = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).map(|el| el.checked()).unwrap_or(false);
                                        value.update(|v| { v["cancelable"] = json!(checked); });
                                    }
                                />
                                " Cancelable"
                            </label>
                            <label class="field-label">"Cancel key (optional)"</label>
                            <input type="text" class="hc-input" placeholder="blank = auto-generated"
                                prop:value=move || value.get()["cancel_key"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("cancel_key", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "set_variable" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Variable name"</label>
                            <input type="text" class="hc-input"
                                prop:value=move || value.get()["name"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("name", event_target_value(&ev))
                            />
                            <label class="field-label">"Value (JSON)"</label>
                            <input type="text" class="hc-input"
                                prop:value=move || { let v = &value.get()["value"]; if v.is_null() { String::new() } else { v.to_string() } }
                                on:input=move |ev| {
                                    let raw = event_target_value(&ev);
                                    let parsed = serde_json::from_str::<Value>(raw.trim()).unwrap_or_else(|_| json!(raw.trim()));
                                    value.update(|v| { v["value"] = parsed; });
                                }
                            />
                            <label class="field-label">"Operation"</label>
                            <select class="hc-select" on:change=move |ev| {
                                let raw = event_target_value(&ev);
                                value.update(|v| {
                                    if raw == "set" { if let Some(obj) = v.as_object_mut() { obj.remove("op"); } }
                                    else { v["op"] = json!(raw); }
                                });
                            }>
                                {[("set","Set (replace)"),("add","Add"),("subtract","Subtract"),("multiply","Multiply"),("divide","Divide"),("toggle","Toggle")]
                                    .map(|(v, label)| view! {
                                        <option value=v selected=move || value.get()["op"].as_str().unwrap_or("set") == v>{label}</option>
                                    }).collect_view()}
                            </select>
                        </div>
                    }.into_any(),

                    "set_hub_variable" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Variable name"</label>
                            <input type="text" class="hc-input" placeholder="e.g. alarm_state"
                                prop:value=move || value.get()["name"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("name", event_target_value(&ev))
                            />
                            <label class="field-label">"Value (JSON)"</label>
                            <input type="text" class="hc-input"
                                prop:value=move || { let v = &value.get()["value"]; if v.is_null() { String::new() } else { v.to_string() } }
                                on:input=move |ev| {
                                    let raw = event_target_value(&ev);
                                    let parsed = serde_json::from_str::<Value>(raw.trim()).unwrap_or_else(|_| json!(raw.trim()));
                                    value.update(|v| { v["value"] = parsed; });
                                }
                            />
                        </div>
                    }.into_any(),

                    "set_mode" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Mode ID"</label>
                            <input type="text" class="hc-input" placeholder="e.g. mode_away"
                                prop:value=move || value.get()["mode_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("mode_id", event_target_value(&ev))
                            />
                            <label class="field-label">"Command"</label>
                            <select class="hc-select" on:change=move |ev| set_str("command", event_target_value(&ev))>
                                {[("on","On"),("off","Off"),("toggle","Toggle")].map(|(v, label)| view! {
                                    <option value=v selected=move || value.get()["command"].as_str().unwrap_or("on") == v>{label}</option>
                                }).collect_view()}
                            </select>
                        </div>
                    }.into_any(),

                    "run_rule_actions" | "pause_rule" | "resume_rule" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Rule ID (UUID)"</label>
                            <input type="text" class="hc-input hc-textarea--code" placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
                                prop:value=move || value.get()["rule_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("rule_id", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "cancel_delays" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Cancel key (blank = all cancellable delays)"</label>
                            <input type="text" class="hc-input"
                                prop:value=move || value.get()["key"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("key", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "cancel_rule_timers" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Rule ID (blank = current rule)"</label>
                            <input type="text" class="hc-input hc-textarea--code"
                                prop:value=move || value.get()["rule_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("rule_id", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "set_private_boolean" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Boolean name"</label>
                            <input type="text" class="hc-input"
                                prop:value=move || value.get()["name"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("name", event_target_value(&ev))
                            />
                            <label class="field-label">"Value"</label>
                            <select class="hc-select" on:change=move |ev| set_bool("value", event_target_value(&ev))>
                                <option value="true"  selected=move || value.get()["value"].as_bool() != Some(false)>"True"</option>
                                <option value="false" selected=move || value.get()["value"].as_bool() == Some(false)>"False"</option>
                            </select>
                        </div>
                    }.into_any(),

                    "log_message" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Message"</label>
                            <textarea class="hc-textarea" rows="2"
                                prop:value=move || value.get()["message"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("message", event_target_value(&ev))
                            />
                            <label class="field-label">"Level"</label>
                            <select class="hc-select" on:change=move |ev| set_opt_str("level", event_target_value(&ev))>
                                {[("","Default (info)"),("trace","Trace"),("debug","Debug"),("info","Info"),("warn","Warn"),("error","Error")]
                                    .map(|(v, label)| view! {
                                        <option value=v selected=move || value.get()["level"].as_str().unwrap_or("") == v>{label}</option>
                                    }).collect_view()}
                            </select>
                        </div>
                    }.into_any(),

                    "comment" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Comment text"</label>
                            <textarea class="hc-textarea" rows="2"
                                prop:value=move || value.get()["text"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("text", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "wait_for_event" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Event type (blank = device event)"</label>
                            <input type="text" class="hc-input"
                                prop:value=move || value.get()["event_type"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("event_type", event_target_value(&ev))
                            />
                            <label class="field-label">"Device ID (blank = custom event)"</label>
                            <input type="text" class="hc-input"
                                prop:value=move || value.get()["device_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("device_id", event_target_value(&ev))
                            />
                            <label class="field-label">"Attribute (optional)"</label>
                            <input type="text" class="hc-input"
                                prop:value=move || value.get()["attribute"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_opt_str("attribute", event_target_value(&ev))
                            />
                            <label class="field-label">"Timeout (ms, blank = no timeout)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem"
                                prop:value=move || value.get()["timeout_ms"].as_u64().map(|n| n.to_string()).unwrap_or_default()
                                on:input=move |ev| set_opt_u64("timeout_ms", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "wait_for_expression" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Rhai expression"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="3"
                                prop:value=move || value.get()["expression"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("expression", event_target_value(&ev))
                            />
                            <label class="field-label">"Timeout (ms, blank = no timeout)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem"
                                prop:value=move || value.get()["timeout_ms"].as_u64().map(|n| n.to_string()).unwrap_or_default()
                                on:input=move |ev| set_opt_u64("timeout_ms", event_target_value(&ev))
                            />
                            <label class="field-label">"Poll interval (ms, blank = 500)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:8rem"
                                prop:value=move || value.get()["poll_interval_ms"].as_u64().map(|n| n.to_string()).unwrap_or_default()
                                on:input=move |ev| set_opt_u64("poll_interval_ms", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "capture_device_state" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Snapshot key"</label>
                            <input type="text" class="hc-input" placeholder="e.g. pre_movie"
                                prop:value=move || value.get()["key"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("key", event_target_value(&ev))
                            />
                            <label class="field-label">"Device IDs (comma-separated)"</label>
                            <input type="text" class="hc-input" placeholder="light_living, light_hall"
                                prop:value=move || {
                                    value.get()["device_ids"].as_array()
                                        .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
                                        .unwrap_or_default()
                                }
                                on:input=move |ev| {
                                    let raw = event_target_value(&ev);
                                    let ids: Vec<Value> = raw.split(',').map(|s| json!(s.trim())).filter(|v| v.as_str() != Some("")).collect();
                                    value.update(|v| { v["device_ids"] = json!(ids); });
                                }
                            />
                        </div>
                    }.into_any(),

                    "restore_device_state" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Snapshot key"</label>
                            <input type="text" class="hc-input" placeholder="e.g. pre_movie"
                                prop:value=move || value.get()["key"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("key", event_target_value(&ev))
                            />
                        </div>
                    }.into_any(),

                    "fade_device" => view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Device ID"</label>
                            <input type="text" class="hc-input"
                                prop:value=move || value.get()["device_id"].as_str().unwrap_or("").to_string()
                                on:input=move |ev| set_str("device_id", event_target_value(&ev))
                            />
                            <label class="field-label">"Target state (JSON)"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="2"
                                placeholder=r#"{"on": true, "brightness": 255}"#
                                prop:value=move || serde_json::to_string_pretty(&value.get()["target"]).unwrap_or_default()
                                on:input=move |ev| {
                                    if let Ok(parsed) = serde_json::from_str::<Value>(&event_target_value(&ev)) {
                                        value.update(|v| { v["target"] = parsed; });
                                    }
                                }
                            />
                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"Duration (seconds)"</label>
                                    <input type="number" class="hc-input hc-input--sm"
                                        prop:value=move || value.get()["duration_secs"].as_u64().unwrap_or(30).to_string()
                                        on:input=move |ev| set_u64("duration_secs", event_target_value(&ev))
                                    />
                                </div>
                                <div>
                                    <label class="field-label">"Steps (blank = 1/sec)"</label>
                                    <input type="number" class="hc-input hc-input--sm"
                                        prop:value=move || value.get()["steps"].as_u64().map(|n| n.to_string()).unwrap_or_default()
                                        on:input=move |ev| set_opt_u64("steps", event_target_value(&ev))
                                    />
                                </div>
                            </div>
                        </div>
                    }.into_any(),

                    "stop_rule_chain" | "exit_rule" => view! {
                        <div class="trigger-fields">
                            <p class="msg-muted" style="font-size:0.85rem">
                                {if t == "stop_rule_chain" { "Stops evaluation of lower-priority rules." } else { "Halts remaining actions in this rule." }}
                            </p>
                        </div>
                    }.into_any(),

                    // Block actions → JSON fallback
                    _ => view! {
                        <div class="trigger-fields">
                            <p class="msg-muted" style="font-size:0.78rem">"Block action — edit as JSON:"</p>
                            <JsonTextarea value=value label="" rows=8 />
                        </div>
                    }.into_any(),
                }
            }}
        </div>
    }
}

// ── JsonTextarea ──────────────────────────────────────────────────────────────

#[component]
fn JsonTextarea(
    value: RwSignal<Value>,
    label: &'static str,
    #[prop(default = 6)] rows: u32,
) -> impl IntoView {
    let text: RwSignal<String> =
        RwSignal::new(serde_json::to_string_pretty(&value.get_untracked()).unwrap_or_default());
    let json_err: RwSignal<Option<String>> = RwSignal::new(None);

    view! {
        <div class="json-editor">
            {(!label.is_empty()).then(|| view! { <label class="field-label">{label}</label> })}
            <textarea
                class="hc-textarea hc-textarea--code"
                rows=rows
                prop:value=move || text.get()
                on:input=move |ev| {
                    let raw = event_target_value(&ev);
                    text.set(raw.clone());
                    match serde_json::from_str::<Value>(&raw) {
                        Ok(v)  => { value.set(v); json_err.set(None); }
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

// ── CheckboxField ─────────────────────────────────────────────────────────────

#[component]
fn CheckboxField(label: &'static str, value: RwSignal<bool>) -> impl IntoView {
    view! {
        <label class="rule-meta-inline">
            <input
                type="checkbox"
                prop:checked=move || value.get()
                on:change=move |ev| {
                    use wasm_bindgen::JsCast;
                    let checked = ev.target()
                        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                        .map(|el| el.checked())
                        .unwrap_or(false);
                    value.set(checked);
                }
            />
            " "{label}
        </label>
    }
}
