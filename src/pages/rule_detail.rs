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

use crate::pages::shared::{ErrorBanner, use_toast};
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
use hc_types::device::DeviceChangeKind as HcDeviceChangeKind;
use hc_types::rule::{
    Action, ButtonEventType, CompareOp, Condition, LogLevel, ModeCommand,
    ModeDelayEntry, ModeSceneEntry, ModeStateEntry,
    PeriodicUnit, RuleAction, SunEventType, ThresholdOp, VariableOp,
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


// ── Bridge: merge typed metadata with sub-editor Value data for save ────────

/// Build a Rule for saving.
/// Render a Utc timestamp as a short relative age suitable for the
/// sticky save-status indicator. Granularity: seconds for the first
/// minute, minutes for the first hour, otherwise hours.
fn format_relative_age(ts: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let secs = (now - ts).num_seconds();
    if secs < 5 {
        "saved just now".into()
    } else if secs < 60 {
        format!("saved {secs}s ago")
    } else if secs < 3600 {
        format!("saved {}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("saved {}h ago", secs / 3600)
    } else {
        format!("saved {}d ago", secs / 86_400)
    }
}

fn build_save_rule(rule: RwSignal<Rule>) -> Rule {
    rule.get_untracked()
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
    let saved_rule: RwSignal<Option<Rule>> = RwSignal::new(None);
    let is_dirty = Memo::new(move |_| {
        saved_rule.get().map_or(false, |s| s != rule.get())
    });
    let loading = RwSignal::new(!is_new);
    let saving  = RwSignal::new(false);
    let save_err: RwSignal<Option<String>> = RwSignal::new(None);
    // Timestamp of the last successful save — drives the "saved Xs ago"
    // text in the sticky action bar.
    let last_saved_at: RwSignal<Option<chrono::DateTime<chrono::Utc>>> = RwSignal::new(None);
    let toast = use_toast();
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
                        saved_rule.set(Some(r.clone()));
                        rule.set(r);
                        loading.set(false);
                    }
                    Err(e) => { save_err.set(Some(format!("Load failed: {e}"))); loading.set(false); }
                }
            });
        }
    });

    // ── Unsaved changes: beforeunload warning ─────────────────────────────────
    Effect::new(move |_| {
        use wasm_bindgen::prelude::*;
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            if is_dirty.get_untracked() {
                ev.prevent_default();
            }
        });
        if let Some(window) = web_sys::window() {
            let _ = window.add_event_listener_with_callback("beforeunload", cb.as_ref().unchecked_ref());
        }
        let cb_ref = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
        cb.forget();
        on_cleanup(move || {
            if let Some(window) = web_sys::window() {
                let _ = window.remove_event_listener_with_callback("beforeunload", &cb_ref);
            }
        });
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
                        {move || is_dirty.get().then(|| view! {
                            <span class="unsaved-badge">" (unsaved)"</span>
                        })}
                    </h2>
                </div>
            </div>

            // ── Status banners ───────────────────────────────────────────────
            <ErrorBanner error=save_err />
            {move || loading.get().then(|| view! { <p class="msg-muted">"Loading…"</p> })}

            // ── Editor (hidden while loading) ────────────────────────────────
            <Show when=move || !loading.get()>

                // ── Rule header card: Identity (top) + Behavior (bottom) ─────
                // Identity is just name + enabled + tags — feels like the
                // document's title block. Behavior groups priority / run-mode /
                // cooldown into a denser, mono-numerals block below a hairline.
                // Action buttons (Save/Cancel/Clone/Delete) live in a sticky
                // bottom bar — see further down — so they're always reachable
                // without duplicating them up here.
                <section class="detail-card">
                    // ── Identity ─────────────────────────────────────────────
                    <div class="rule-identity">
                        <input type="text" class="hc-input rule-name-input" placeholder="Rule name"
                            prop:value=move || rule.get().name.clone()
                            on:input=move |ev| {
                                let v = event_target_value(&ev);
                                rule.update(|r| r.name = v);
                            }
                        />
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
                    </div>

                    // ── Behavior ─────────────────────────────────────────────
                    <span class="rule-behavior-kicker">"behavior"</span>
                    <div class="rule-behavior">
                        <div>
                            <label class="field-label">"Priority"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:100%"
                                prop:value=move || rule.get().priority.to_string()
                                on:input=move |ev| {
                                    if let Ok(n) = event_target_value(&ev).parse::<i32>() {
                                        rule.update(|r| r.priority = n);
                                    }
                                }
                            />
                        </div>
                        <div>
                            <label class="field-label">"Run mode"</label>
                            <select class="hc-select" style="width:100%"
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
                                {[("parallel","Parallel"),("single","Single"),("restart","Restart"),("queued","Queued")]
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
                        </div>
                        <div>
                            <label class="field-label">"Cooldown (sec)"</label>
                            <input type="number" class="hc-input hc-input--sm" style="width:100%" placeholder="None"
                                prop:value=move || rule.get().cooldown_secs.map(|n| n.to_string()).unwrap_or_default()
                                on:input=move |ev| {
                                    let raw = event_target_value(&ev);
                                    rule.update(|r| {
                                        r.cooldown_secs = raw.trim().parse::<u64>().ok();
                                    });
                                }
                            />
                        </div>
                        {move || matches!(rule.get().run_mode, RunMode::Queued { .. }).then(|| {
                            let mq = match rule.get().run_mode { RunMode::Queued { max_queue } => max_queue, _ => 10 };
                            view! {
                                <div>
                                    <label class="field-label">"Max queue"</label>
                                    <input type="number" class="hc-input hc-input--sm" style="width:100%" min="1" prop:value=mq.to_string()
                                        on:input=move |ev| {
                                            if let Ok(n) = event_target_value(&ev).parse::<usize>() {
                                                rule.update(|r| { if let RunMode::Queued { ref mut max_queue } = r.run_mode { *max_queue = n.max(1); } });
                                            }
                                        } />
                                </div>
                            }
                        })}
                        // Reactive helper text describing the chosen run mode.
                        <div class="rule-behavior-helper">
                            {move || match rule.get().run_mode {
                                RunMode::Parallel => "Each fire spawns its own task; previous runs continue uninterrupted.",
                                RunMode::Single   => "If a previous run is still in progress, this fire is skipped.",
                                RunMode::Restart  => "If a previous run is still in progress, it's cancelled and a new run starts.",
                                RunMode::Queued { .. } => "Fires queue up; one runs at a time. Drops if the queue is full.",
                            }}
                        </div>
                    </div>
                </section>

                // ── Rule flow: Trigger → Conditions → Actions ───────────────
                // Wrapped in .hc-rule-flow so the connector hairline draws
                // through all three steps. Each step gets a kicker + numbered
                // marker on the left margin.
                <div class="hc-rule-flow">

                    // ── Step 1 · Trigger ─────────────────────────────────────
                    <section class="hc-step">
                        <div class="hc-step__marker"><i class="ph ph-clock"></i></div>
                        <div class="hc-step__head">
                            <span class="hc-step__kicker">"step 1 · when"</span>
                            <span class="hc-step__title">"Trigger"</span>
                        </div>
                        <TriggerEditor rule=rule />
                    </section>

                    // ── Step 2 · Conditions ──────────────────────────────────
                    <section class="hc-step">
                        <div class="hc-step__marker"><i class="ph ph-funnel"></i></div>
                        <div class="hc-step__head">
                            <span class="hc-step__kicker">"step 2 · if"</span>
                            <span class="hc-step__title">"Conditions"</span>
                        </div>
                        <ConditionList rule=rule />
                        <button class="hc-rule-add-slot"
                            on:click=move |_| rule.update(|r| {
                                r.conditions.push(default_condition_typed("device_state"));
                            })
                        >
                            <i class="ph ph-plus"></i>
                            "add condition"
                        </button>
                    </section>

                    // ── Step 3 · Actions ─────────────────────────────────────
                    <section class="hc-step">
                        <div class="hc-step__marker"><i class="ph ph-arrow-right"></i></div>
                        <div class="hc-step__head">
                            <span class="hc-step__kicker">"step 3 · then"</span>
                            <span class="hc-step__title">"Actions"</span>
                        </div>
                        <ActionList rule=rule />
                        <button class="hc-rule-add-slot"
                            on:click=move |_| rule.update(|r| {
                                r.actions.push(RuleAction {
                                    enabled: true,
                                    action: default_action_typed("log_message"),
                                });
                            })
                        >
                            <i class="ph ph-plus"></i>
                            "add action"
                        </button>
                    </section>
                </div>

                // ── Advanced (collapsible) ───────────────────────────────────
                <section class="detail-card">
                    <button class="rule-advanced-toggle"
                        on:click=move |_| advanced_open.update(|v| *v = !*v)
                    >
                        <i class=move || if advanced_open.get() {
                            "ph ph-caret-down"
                        } else {
                            "ph ph-caret-right"
                        } style="font-size:14px"></i>
                        " advanced"
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

                // ── Sticky action bar ────────────────────────────────────────
                // Always reachable while editing long rules. Save status
                // ("just now", "2 min ago") sits in the right-aligned slot
                // so users get persistent confirmation that their last save
                // landed without a dismissable banner.
                <div class="rule-action-bar rule-action-bar--sticky">
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
                                    let typed = build_save_rule(rule);
                                    if typed.name.trim().is_empty() { save_err.set(Some("Rule name is required.".into())); return; }
                                    save_err.set(None); saving.set(true);
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
                                                    saved_rule.set(Some(saved.clone()));
                                                    rule.set(saved);
                                                    last_saved_at.set(Some(chrono::Utc::now()));
                                                    toast.success("Saved successfully");
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
                                    <i class="ph ph-copy" style="font-size:15px;vertical-align:middle"></i>
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

                            // Save status — right-aligned, persistent. States:
                            //   saving  → "saving…"
                            //   dirty   → "unsaved changes"
                            //   saved   → "saved Xs ago" (relative)
                            //   none    → "" (nothing yet)
                            <span class=move || {
                                if saving.get() {
                                    "rule-save-status rule-save-status--saving"
                                } else if is_dirty.get() {
                                    "rule-save-status"
                                } else if last_saved_at.get().is_some() {
                                    "rule-save-status rule-save-status--saved"
                                } else {
                                    "rule-save-status"
                                }
                            }>
                                {move || {
                                    if saving.get() {
                                        "saving…".to_string()
                                    } else if is_dirty.get() {
                                        "unsaved changes".to_string()
                                    } else if let Some(ts) = last_saved_at.get() {
                                        format_relative_age(ts)
                                    } else {
                                        String::new()
                                    }
                                }}
                            </span>
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
                        <ErrorBanner error=test_err />
                        {move || test_result.get().map(|r| {
                            let would_fire = r["would_fire"].as_bool().unwrap_or(false);
                            let conditions = r["conditions"].as_array().cloned().unwrap_or_default();
                            let action_count = r["actions"].as_array().map(|a| a.len()).unwrap_or(0);
                            let overall_class = if would_fire { "test-overall--pass" } else { "test-overall--fail" };
                            let overall_text = if would_fire { "Rule WOULD fire" } else { "Rule would NOT fire" };
                            let overall_icon = if would_fire { "check-circle" } else { "x-circle" };
                            let pretty = serde_json::to_string_pretty(&r).unwrap_or_default();
                            let show_raw = RwSignal::new(false);

                            view! {
                                <div class=format!("test-overall {overall_class}")>
                                    <i class={move || format!("ph ph-{}", overall_icon)} style="font-size:18px; vertical-align:middle"></i>
                                    " " {overall_text}
                                    {if would_fire { format!(" — {action_count} action(s)") } else { String::new() }}
                                </div>
                                <div class="test-conditions">
                                    {conditions.into_iter().enumerate().map(|(i, cond)| {
                                        let passed = cond["passed"].as_bool().unwrap_or(false);
                                        let badge_class = if passed { "test-badge test-badge--pass" } else { "test-badge test-badge--fail" };
                                        let icon = if passed { "check" } else { "x" };
                                        let cond_type = cond["condition"].as_object()
                                            .and_then(|o| o.keys().next().cloned())
                                            .unwrap_or_else(|| "unknown".to_string());
                                        let reason = cond["reason"].as_str().unwrap_or("").to_string();
                                        let actual = cond.get("actual").filter(|v| !v.is_null()).map(|v| v.to_string()).unwrap_or_default();
                                        let expected = cond.get("expected").filter(|v| !v.is_null()).map(|v| v.to_string()).unwrap_or_default();

                                        view! {
                                            <div class=badge_class>
                                                <i class={format!("ph ph-{}", icon)} style="font-size:14px"></i>
                                                <span class="test-badge-label">{format!("{}. {}", i + 1, cond_type)}</span>
                                                {(!reason.is_empty()).then(|| view! { <span class="test-badge-reason">{format!(" — {reason}")}</span> })}
                                                {(!actual.is_empty() && !expected.is_empty()).then(|| view! {
                                                    <span class="test-badge-detail">{format!(" (actual: {actual}, expected: {expected})")}</span>
                                                })}
                                            </div>
                                        }
                                    }).collect_view()}
                                </div>
                                <button class="hc-btn hc-btn--sm hc-btn--outline" style="margin-top:0.5rem"
                                    on:click=move |_| show_raw.update(|v| *v = !*v)
                                >{move || if show_raw.get() { "Hide raw JSON" } else { "Show raw JSON" }}</button>
                                <Show when=move || show_raw.get()>
                                    <pre class="test-result-pre">{pretty.clone()}</pre>
                                </Show>
                            }
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
                            <ErrorBanner error=history_err />
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
        Trigger::DeviceBatteryLow { .. } => "device_battery_low",
        Trigger::DeviceBatteryRecovered { .. } => "device_battery_recovered",
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
        "device_battery_low" => Trigger::DeviceBatteryLow { device_id: None },
        "device_battery_recovered" => Trigger::DeviceBatteryRecovered { device_id: None },
        _ => Trigger::ManualTrigger,
    }
}

#[component]
fn TriggerEditor(rule: RwSignal<Rule>) -> impl IntoView {
    let tg = move || rule.get().trigger.clone();

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
                    {[("device_state_changed","Device state changed"),("device_availability_changed","Device availability changed"),("button_event","Button event"),("numeric_threshold","Numeric threshold"),("device_battery_low","Battery low"),("device_battery_recovered","Battery recovered")]
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
                    Trigger::DeviceStateChanged { ref device_id, ref attribute, ref to, ref from, ref not_to, ref not_from, ref for_duration_secs, ref change_kind, ref change_source, .. } => {
                        let did = device_id.clone();
                        let attr = attribute.clone().unwrap_or_default();
                        let to_val = to.clone().map(|v| serde_json::to_value(&v).unwrap_or_default()).unwrap_or(Value::Null);
                        let from_val = from.clone().map(|v| serde_json::to_value(&v).unwrap_or_default()).unwrap_or(Value::Null);
                        let not_to_val = not_to.clone().map(|v| serde_json::to_value(&v).unwrap_or_default()).unwrap_or(Value::Null);
                        let not_from_val = not_from.clone().map(|v| serde_json::to_value(&v).unwrap_or_default()).unwrap_or(Value::Null);
                        let dur = for_duration_secs.map(|n| n.to_string()).unwrap_or_default();
                        let kind_key = match change_kind {
                            None => "any",
                            Some(HcDeviceChangeKind::Homecore) => "homecore",
                            Some(HcDeviceChangeKind::Physical) => "physical",
                            Some(HcDeviceChangeKind::External) => "external",
                            Some(HcDeviceChangeKind::Unknown) => "unknown",
                        };
                        let src = change_source.clone().unwrap_or_default();
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
                                <div class="trigger-row-2">
                                    <AttrValueSelect device_id=did.clone() attribute=attr.clone() value=not_to_val label="Not to (optional)"
                                        on_select=Callback::new(move |raw: String| rule.update(|r| {
                                            if let Trigger::DeviceStateChanged { ref mut not_to, .. } = r.trigger {
                                                *not_to = if raw.trim().is_empty() { None } else {
                                                    Some(serde_json::from_str(&raw).unwrap_or_else(|_| json!(raw)))
                                                };
                                            }
                                        })) />
                                    <AttrValueSelect device_id=did.clone() attribute=attr.clone() value=not_from_val label="Not from (optional)"
                                        on_select=Callback::new(move |raw: String| rule.update(|r| {
                                            if let Trigger::DeviceStateChanged { ref mut not_from, .. } = r.trigger {
                                                *not_from = if raw.trim().is_empty() { None } else {
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
                                <div class="trigger-row-2">
                                    <div>
                                        <label class="field-label">"Change origin (blank = any)"</label>
                                        <select class="hc-select" on:change=move |ev| {
                                            let raw = event_target_value(&ev);
                                            let new_kind = match raw.as_str() {
                                                "homecore" => Some(HcDeviceChangeKind::Homecore),
                                                "physical" => Some(HcDeviceChangeKind::Physical),
                                                "external" => Some(HcDeviceChangeKind::External),
                                                "unknown" => Some(HcDeviceChangeKind::Unknown),
                                                _ => None,
                                            };
                                            rule.update(|r| {
                                                if let Trigger::DeviceStateChanged { ref mut change_kind, .. } = r.trigger {
                                                    *change_kind = new_kind;
                                                }
                                            });
                                        }>
                                            {[("any","Any"),("homecore","HomeCore"),("physical","Physical"),("external","Plugin / External"),("unknown","Unknown")]
                                                .map(|(v,l)| view! { <option value=v selected=kind_key==v>{l}</option> }).collect_view()}
                                        </select>
                                    </div>
                                    <div>
                                        <label class="field-label">"Change source (exact match, blank = any)"</label>
                                        <input type="text" class="hc-input" placeholder="e.g. zwave, lutron" prop:value=src
                                            on:input=move |ev| {
                                                let v = event_target_value(&ev);
                                                rule.update(|r| {
                                                    if let Trigger::DeviceStateChanged { ref mut change_source, .. } = r.trigger {
                                                        *change_source = if v.is_empty() { None } else { Some(v) };
                                                    }
                                                });
                                            } />
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    },

                    Trigger::DeviceAvailabilityChanged { ref device_id, ref to, ref for_duration_secs } => {
                        let did = device_id.clone();
                        let to_val = *to;
                        let dur = for_duration_secs.map(|n| n.to_string()).unwrap_or_default();
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
                                <label class="field-label">"Hold duration (seconds, blank = immediate)"</label>
                                <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="None" prop:value=dur
                                    on:input=move |ev| {
                                        let raw = event_target_value(&ev);
                                        rule.update(|r| {
                                            if let Trigger::DeviceAvailabilityChanged { ref mut for_duration_secs, .. } = r.trigger {
                                                *for_duration_secs = raw.trim().parse::<u64>().ok();
                                            }
                                        });
                                    } />
                            </div>
                        }.into_any()
                    },

                    Trigger::ButtonEvent { ref device_id, ref event, ref button_number, .. } => {
                        let did = device_id.clone();
                        let evt = format!("{:?}", event);
                        let btn_str = button_number.map(|n| n.to_string()).unwrap_or_default();
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Device"</label>
                                <DeviceSelect value=did on_select=Callback::new(move |id: String| rule.update(|r| {
                                    if let Trigger::ButtonEvent { ref mut device_id, .. } = r.trigger { *device_id = id; }
                                })) />
                                <div class="trigger-row-2">
                                    <div>
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
                                    <div>
                                        <label class="field-label">"Button # (blank = any)"</label>
                                        <input type="number" class="hc-input hc-input--sm" min="1" max="32"
                                            placeholder="Any"
                                            prop:value=btn_str
                                            on:input=move |ev| {
                                                let raw = event_target_value(&ev);
                                                rule.update(|r| {
                                                    if let Trigger::ButtonEvent { ref mut button_number, .. } = r.trigger {
                                                        *button_number = raw.trim().parse::<u32>().ok();
                                                    }
                                                });
                                            } />
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    },

                    Trigger::NumericThreshold { ref device_id, ref attribute, ref op, value, ref for_duration_secs } => {
                        let did = device_id.clone();
                        let attr = attribute.clone();
                        let op_str = format!("{:?}", op);
                        let val = value;
                        let dur = for_duration_secs.map(|n| n.to_string()).unwrap_or_default();
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
                                <label class="field-label">"Hold duration (seconds, blank = immediate)"</label>
                                <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="None" prop:value=dur
                                    on:input=move |ev| {
                                        let raw = event_target_value(&ev);
                                        rule.update(|r| {
                                            if let Trigger::NumericThreshold { ref mut for_duration_secs, .. } = r.trigger {
                                                *for_duration_secs = raw.trim().parse::<u64>().ok();
                                            }
                                        });
                                    } />
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

                    Trigger::MqttMessage { ref topic_pattern, ref payload, ref value_path, ref value_op, ref value_cmp } => {
                        let tp = topic_pattern.clone();
                        let pl = payload.clone().unwrap_or_default();
                        let vp = value_path.clone().unwrap_or_default();
                        let op_str = value_op.map(|o| format!("{:?}", o)).unwrap_or_default();
                        let cmp_str = value_cmp.as_ref().map(|v| {
                            if v.is_string() { v.as_str().unwrap_or("").to_string() } else { v.to_string() }
                        }).unwrap_or_default();
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
                                <label class="field-label">"JSON value path (e.g. /temperature)"</label>
                                <input type="text" class="hc-input hc-textarea--code" placeholder="/field/subfield" prop:value=vp
                                    on:input=move |ev| { let v = event_target_value(&ev); rule.update(|r| {
                                        if let Trigger::MqttMessage { ref mut value_path, .. } = r.trigger { *value_path = if v.is_empty() { None } else { Some(v) }; }
                                    }); } />
                                <div class="trigger-row-2">
                                    <div>
                                        <label class="field-label">"Value operator"</label>
                                        <select class="hc-select" on:change=move |ev| {
                                            let raw = event_target_value(&ev);
                                            let new_op = match raw.as_str() {
                                                "Eq" => Some(CompareOp::Eq),
                                                "Ne" => Some(CompareOp::Ne),
                                                "Gt" => Some(CompareOp::Gt),
                                                "Gte" => Some(CompareOp::Gte),
                                                "Lt" => Some(CompareOp::Lt),
                                                "Lte" => Some(CompareOp::Lte),
                                                _ => None,
                                            };
                                            rule.update(|r| {
                                                if let Trigger::MqttMessage { ref mut value_op, .. } = r.trigger { *value_op = new_op; }
                                            });
                                        }>
                                            {[("","—"),("Eq","="),("Ne","≠"),("Gt",">"),("Gte","≥"),("Lt","<"),("Lte","≤")]
                                                .map(|(v,l)| view! { <option value=v selected=op_str==v>{l}</option> }).collect_view()}
                                        </select>
                                    </div>
                                    <div>
                                        <label class="field-label">"Expected value (JSON)"</label>
                                        <input type="text" class="hc-input" placeholder="e.g. 80 or \"on\"" prop:value=cmp_str
                                            on:input=move |ev| {
                                                let raw = event_target_value(&ev);
                                                rule.update(|r| {
                                                    if let Trigger::MqttMessage { ref mut value_cmp, .. } = r.trigger {
                                                        *value_cmp = if raw.is_empty() { None } else {
                                                            Some(serde_json::from_str(&raw).unwrap_or_else(|_| json!(raw)))
                                                        };
                                                    }
                                                });
                                            } />
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    },

                    Trigger::SystemStarted => view! {
                        <div class="trigger-fields">
                            <p class="msg-muted" style="font-size:0.85rem">"Fires once when the rule engine starts."</p>
                        </div>
                    }.into_any(),

                    Trigger::DeviceBatteryLow { ref device_id } => {
                        let did = device_id.clone().unwrap_or_default();
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Device (blank = any battery-powered device)"</label>
                                <DeviceSelect value=did on_select=Callback::new(move |id: String| rule.update(|r| {
                                    if let Trigger::DeviceBatteryLow { ref mut device_id } = r.trigger {
                                        *device_id = if id.is_empty() { None } else { Some(id) };
                                    }
                                })) />
                                <p class="msg-muted" style="font-size:0.85rem">
                                    "Fires once when the device's battery crosses the low threshold (configured in homecore.toml). Hysteresis prevents flapping; the trigger fires again only after the battery recovers and drops back below threshold."
                                </p>
                            </div>
                        }.into_any()
                    },

                    Trigger::DeviceBatteryRecovered { ref device_id } => {
                        let did = device_id.clone().unwrap_or_default();
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Device (blank = any battery-powered device)"</label>
                                <DeviceSelect value=did on_select=Callback::new(move |id: String| rule.update(|r| {
                                    if let Trigger::DeviceBatteryRecovered { ref mut device_id } = r.trigger {
                                        *device_id = if id.is_empty() { None } else { Some(id) };
                                    }
                                })) />
                                <p class="msg-muted" style="font-size:0.85rem">
                                    "Fires once when a previously-low device's battery climbs back above the recover band."
                                </p>
                            </div>
                        }.into_any()
                    },

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
        Condition::CalendarActive { .. } => "calendar_active",
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
        "calendar_active" => Condition::CalendarActive { calendar_id: None, title_contains: None },
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
                    let n = i + 1;
                    view! {
                        // AND divider between rows (skipped before the first).
                        {(!is_first).then(|| view! {
                            <div class="hc-rule-divider">"and"</div>
                        })}
                        <div class="json-row">
                            <span class="json-row__num">{n.to_string()}</span>
                            <div class="json-row-controls">
                                <button class="hc-btn hc-btn--sm hc-btn--outline" title="Move up" disabled=is_first
                                    on:click=move |_| rule.update(|r| { if i > 0 { r.conditions.swap(i - 1, i); } })
                                ><i class="ph ph-arrow-up" style="font-size:14px"></i></button>
                                <button class="hc-btn hc-btn--sm hc-btn--outline" title="Move down" disabled=is_last
                                    on:click=move |_| rule.update(|r| { if i + 1 < r.conditions.len() { r.conditions.swap(i, i + 1); } })
                                ><i class="ph ph-arrow-down" style="font-size:14px"></i></button>
                                <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove"
                                    on:click=move |_| rule.update(|r| { r.conditions.remove(i); })
                                ><i class="ph ph-x" style="font-size:14px"></i></button>
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
                  ("hub_variable","Hub variable"),("mode_is","Mode is on/off"),("calendar_active","Calendar active"),
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

                    Condition::DeviceLastChange { ref device_id, ref source, ref kind, ref actor_id, ref actor_name } => {
                        let did = device_id.clone();
                        let src = source.clone().unwrap_or_default();
                        let aid = actor_id.clone().unwrap_or_default();
                        let aname = actor_name.clone().unwrap_or_default();
                        let kind_key = match kind {
                            None => "any",
                            Some(HcDeviceChangeKind::Homecore) => "homecore",
                            Some(HcDeviceChangeKind::Physical) => "physical",
                            Some(HcDeviceChangeKind::External) => "external",
                            Some(HcDeviceChangeKind::Unknown) => "unknown",
                        };
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Device"</label>
                                <DeviceSelect value=did on_select=Callback::new(move |id: String| {
                                    let mut c = get.get_untracked(); if let Condition::DeviceLastChange { ref mut device_id, .. } = c { *device_id = id; } set.run(c);
                                }) />
                                <div class="trigger-row-2">
                                    <div>
                                        <label class="field-label">"Change origin (blank = any)"</label>
                                        <select class="hc-select" on:change=move |ev| {
                                            let raw = event_target_value(&ev);
                                            let new_kind = match raw.as_str() {
                                                "homecore" => Some(HcDeviceChangeKind::Homecore),
                                                "physical" => Some(HcDeviceChangeKind::Physical),
                                                "external" => Some(HcDeviceChangeKind::External),
                                                "unknown" => Some(HcDeviceChangeKind::Unknown),
                                                _ => None,
                                            };
                                            let mut c = get.get_untracked();
                                            if let Condition::DeviceLastChange { ref mut kind, .. } = c { *kind = new_kind; }
                                            set.run(c);
                                        }>
                                            {[("any","Any"),("homecore","HomeCore"),("physical","Physical"),("external","Plugin / External"),("unknown","Unknown")]
                                                .map(|(v,l)| view! { <option value=v selected=kind_key==v>{l}</option> }).collect_view()}
                                        </select>
                                    </div>
                                    <div>
                                        <label class="field-label">"Source (blank = any)"</label>
                                        <input type="text" class="hc-input" placeholder="e.g. zwave" prop:value=src
                                            on:input=move |ev| { let v = event_target_value(&ev);
                                                let mut c = get.get_untracked(); if let Condition::DeviceLastChange { ref mut source, .. } = c { *source = if v.is_empty() { None } else { Some(v) }; } set.run(c);
                                            } />
                                    </div>
                                </div>
                                <div class="trigger-row-2">
                                    <div>
                                        <label class="field-label">"Actor ID (blank = any)"</label>
                                        <input type="text" class="hc-input" placeholder="e.g. user:john" prop:value=aid
                                            on:input=move |ev| { let v = event_target_value(&ev);
                                                let mut c = get.get_untracked(); if let Condition::DeviceLastChange { ref mut actor_id, .. } = c { *actor_id = if v.is_empty() { None } else { Some(v) }; } set.run(c);
                                            } />
                                    </div>
                                    <div>
                                        <label class="field-label">"Actor name (blank = any)"</label>
                                        <input type="text" class="hc-input" prop:value=aname
                                            on:input=move |ev| { let v = event_target_value(&ev);
                                                let mut c = get.get_untracked(); if let Condition::DeviceLastChange { ref mut actor_name, .. } = c { *actor_name = if v.is_empty() { None } else { Some(v) }; } set.run(c);
                                            } />
                                    </div>
                                </div>
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

                    Condition::CalendarActive { ref calendar_id, ref title_contains } => {
                        let cid = calendar_id.clone().unwrap_or_default();
                        let tc = title_contains.clone().unwrap_or_default();
                        view! {
                            <div class="trigger-fields">
                                <label class="field-label">"Calendar ID (optional)"</label>
                                <input class="input" type="text" prop:value=cid placeholder="e.g. us_holidays"
                                    on:input=move |ev| {
                                        let mut c = get.get_untracked();
                                        if let Condition::CalendarActive { ref mut calendar_id, .. } = c {
                                            let v = event_target_value(&ev);
                                            *calendar_id = if v.is_empty() { None } else { Some(v) };
                                        }
                                        set.run(c);
                                    } />
                                <label class="field-label">"Title contains (optional)"</label>
                                <input class="input" type="text" prop:value=tc placeholder="e.g. Holiday"
                                    on:input=move |ev| {
                                        let mut c = get.get_untracked();
                                        if let Condition::CalendarActive { ref mut title_contains, .. } = c {
                                            let v = event_target_value(&ev);
                                            *title_contains = if v.is_empty() { None } else { Some(v) };
                                        }
                                        set.run(c);
                                    } />
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
                                                ><i class="ph ph-arrow-up" style="font-size:14px"></i></button>
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
                                                ><i class="ph ph-arrow-down" style="font-size:14px"></i></button>
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
                                                ><i class="ph ph-x" style="font-size:14px"></i></button>
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
                                        let _vk = vk.clone();
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
                    Condition::Not { condition: _ } => {
                        view! {
                            <div class="cond-group cond-group--not">
                                <p class="cond-group-label">"NOT — inverts the result"</p>
                                <div class="json-row">
                                    <div class="json-row-controls">
                                        <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Reset inner"
                                            on:click=move |_| {
                                                set.run(Condition::Not { condition: Box::new(default_condition_typed("device_state")) });
                                            }
                                        ><i class="ph ph-x" style="font-size:14px"></i></button>
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

// ── Typed Action Editor ──────────────────────────────────────────────────────

fn action_variant_key(a: &Action) -> &'static str {
    match a {
        Action::SetDeviceState { .. } => "set_device_state",
        Action::PublishMqtt { .. } => "publish_mqtt",
        Action::CallService { .. } => "call_service",
        Action::FireEvent { .. } => "fire_event",
        Action::RunScript { .. } => "run_script",
        Action::Notify { .. } => "notify",
        Action::Delay { .. } => "delay",
        Action::Parallel { .. } => "parallel",
        Action::RepeatUntil { .. } => "repeat_until",
        Action::RepeatWhile { .. } => "repeat_while",
        Action::RepeatCount { .. } => "repeat_count",
        Action::Conditional { .. } => "conditional",
        Action::StopRuleChain => "stop_rule_chain",
        Action::ExitRule => "exit_rule",
        Action::Comment { .. } => "comment",
        Action::WaitForEvent { .. } => "wait_for_event",
        Action::WaitForExpression { .. } => "wait_for_expression",
        Action::SetVariable { .. } => "set_variable",
        Action::RunRuleActions { .. } => "run_rule_actions",
        Action::PauseRule { .. } => "pause_rule",
        Action::ResumeRule { .. } => "resume_rule",
        Action::CancelDelays { .. } => "cancel_delays",
        Action::CancelRuleTimers { .. } => "cancel_rule_timers",
        Action::SetPrivateBoolean { .. } => "set_private_boolean",
        Action::LogMessage { .. } => "log_message",
        Action::SetDeviceStatePerMode { .. } => "set_device_state_per_mode",
        Action::PingHost { .. } => "ping_host",
        Action::CaptureDeviceState { .. } => "capture_device_state",
        Action::RestoreDeviceState { .. } => "restore_device_state",
        Action::DelayPerMode { .. } => "delay_per_mode",
        Action::SetHubVariable { .. } => "set_hub_variable",
        Action::ActivateScenePerMode { .. } => "activate_scene_per_mode",
        Action::FadeDevice { .. } => "fade_device",
        Action::SetMode { .. } => "set_mode",
    }
}

fn action_category_typed(a: &Action) -> &'static str {
    match a {
        Action::SetDeviceState { .. } | Action::FadeDevice { .. } | Action::CaptureDeviceState { .. } | Action::RestoreDeviceState { .. } => "device",
        Action::Conditional { .. } => "conditional",
        Action::Notify { .. } => "notify",
        Action::SetMode { .. } => "mode",
        Action::Delay { .. } | Action::WaitForEvent { .. } | Action::WaitForExpression { .. } => "timing",
        Action::RunScript { .. } => "script",
        Action::RunRuleActions { .. } | Action::PauseRule { .. } | Action::ResumeRule { .. } | Action::CancelDelays { .. } | Action::CancelRuleTimers { .. } => "rule_ctrl",
        _ => "more",
    }
}

fn default_action_typed(key: &str) -> Action {
    match key {
        "set_device_state" => Action::SetDeviceState { device_id: String::new(), state: json!({}), track_event_value: false },
        "publish_mqtt" => Action::PublishMqtt { topic: String::new(), payload: String::new(), retain: false },
        "call_service" => Action::CallService { url: String::new(), method: "POST".into(), body: json!({}), timeout_ms: None, retries: None, response_event: None },
        "fire_event" => Action::FireEvent { event_type: String::new(), payload: json!({}) },
        "run_script" => Action::RunScript { script: String::new() },
        "notify" => Action::Notify { channel: String::new(), message: String::new(), title: None },
        "delay" => Action::Delay { duration_secs: 5, cancelable: false, cancel_key: None },
        "set_variable" => Action::SetVariable { name: String::new(), value: json!(""), op: None },
        "set_hub_variable" => Action::SetHubVariable { name: String::new(), value: json!(""), op: None },
        "set_mode" => Action::SetMode { mode_id: String::new(), command: ModeCommand::On },
        "run_rule_actions" => Action::RunRuleActions { rule_id: Uuid::nil() },
        "pause_rule" => Action::PauseRule { rule_id: Uuid::nil() },
        "resume_rule" => Action::ResumeRule { rule_id: Uuid::nil() },
        "cancel_delays" => Action::CancelDelays { key: None },
        "cancel_rule_timers" => Action::CancelRuleTimers { rule_id: None },
        "set_private_boolean" => Action::SetPrivateBoolean { name: String::new(), value: true },
        "log_message" => Action::LogMessage { message: String::new(), level: None },
        "comment" => Action::Comment { text: String::new() },
        "stop_rule_chain" => Action::StopRuleChain,
        "exit_rule" => Action::ExitRule,
        "wait_for_event" => Action::WaitForEvent { event_type: None, device_id: None, attribute: None, timeout_ms: None },
        "wait_for_expression" => Action::WaitForExpression { expression: String::new(), poll_interval_ms: None, timeout_ms: None, hold_duration_ms: None },
        "capture_device_state" => Action::CaptureDeviceState { key: String::new(), device_ids: vec![] },
        "restore_device_state" => Action::RestoreDeviceState { key: String::new() },
        "fade_device" => Action::FadeDevice { device_id: String::new(), target: json!({}), duration_secs: 30, steps: None },
        "parallel" => Action::Parallel { actions: vec![] },
        "conditional" => Action::Conditional { condition: String::new(), then_actions: vec![], else_if: vec![], else_actions: vec![] },
        "repeat_until" => Action::RepeatUntil { condition: String::new(), actions: vec![], max_iterations: Some(10), interval_ms: None },
        "repeat_while" => Action::RepeatWhile { condition: String::new(), actions: vec![], max_iterations: Some(10), interval_ms: None },
        "repeat_count" => Action::RepeatCount { count: 3, actions: vec![], interval_ms: None },
        "ping_host" => Action::PingHost { host: String::new(), count: None, timeout_ms: None, then_actions: vec![], else_actions: vec![], response_event: None },
        _ => Action::LogMessage { message: String::new(), level: None },
    }
}

/// Top-level action list — renders `rule.actions` (Vec<RuleAction>).
#[component]
fn ActionList(rule: RwSignal<Rule>) -> impl IntoView {
    view! {
        {move || {
            let actions = rule.get().actions.clone();
            if actions.is_empty() {
                view! { <p class="msg-muted" style="font-size:0.85rem">"No actions — rule will not do anything when triggered."</p> }.into_any()
            } else {
                let total = actions.len();
                actions.into_iter().enumerate().map(|(i, ra)| {
                    let is_first = i == 0;
                    let is_last = i + 1 >= total;
                    let disabled_class = if !ra.enabled { " json-row--disabled" } else { "" };
                    let n = i + 1;
                    view! {
                        // THEN divider between actions (skipped before the first).
                        {(!is_first).then(|| view! {
                            <div class="hc-rule-divider">"then"</div>
                        })}
                        <div class=format!("json-row{disabled_class}")>
                            <span class="json-row__num">{n.to_string()}</span>
                            <div class="json-row-controls">
                                <button class="hc-btn hc-btn--sm hc-btn--outline" title="Move up" disabled=is_first
                                    on:click=move |_| rule.update(|r| { if i > 0 { r.actions.swap(i - 1, i); } })
                                ><i class="ph ph-arrow-up" style="font-size:14px"></i></button>
                                <button class="hc-btn hc-btn--sm hc-btn--outline" title="Move down" disabled=is_last
                                    on:click=move |_| rule.update(|r| { if i + 1 < r.actions.len() { r.actions.swap(i, i + 1); } })
                                ><i class="ph ph-arrow-down" style="font-size:14px"></i></button>
                                <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove"
                                    on:click=move |_| rule.update(|r| { r.actions.remove(i); })
                                ><i class="ph ph-x" style="font-size:14px"></i></button>
                            </div>
                            <TypedRuleActionEditor
                                get=Signal::derive(move || rule.get().actions.get(i).cloned().unwrap_or(RuleAction { enabled: true, action: default_action_typed("log_message") }))
                                set=Callback::new(move |ra: RuleAction| rule.update(|r| { if i < r.actions.len() { r.actions[i] = ra; } }))
                            />
                        </div>
                    }
                }).collect_view().into_any()
            }
        }}
    }
}

/// Wraps TypedActionEditor with an enabled toggle (for top-level RuleAction).
#[component]
fn TypedRuleActionEditor(
    get: Signal<RuleAction>,
    set: Callback<RuleAction>,
) -> impl IntoView {
    view! {
        <div class="action-editor">
            <div class="action-header-row">
                <label class="rule-meta-inline">
                    <input type="checkbox"
                        prop:checked=move || get.get().enabled
                        on:change=move |ev| {
                            use wasm_bindgen::JsCast;
                            let checked = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).map(|el| el.checked()).unwrap_or(true);
                            let mut ra = get.get_untracked(); ra.enabled = checked; set.run(ra);
                        }
                    />
                    " Enabled"
                </label>
            </div>
            <TypedActionEditor
                get=Signal::derive(move || get.get().action.clone())
                set=Callback::new(move |a: Action| { let mut ra = get.get_untracked(); ra.action = a; set.run(ra); })
            />
        </div>
    }
}

/// Typed action editor — takes get/set callbacks, works at any nesting depth.
/// Handles all action types including recursive ones (Parallel, Conditional, Repeat).
#[component]
fn TypedActionEditor(
    get: Signal<Action>,
    set: Callback<Action>,
) -> impl IntoView {
    let category_for = move || action_category_typed(&get.get());

    view! {
        // ── Category selector ────────────────────────────────────────
        <div class="action-cat-row">
            {ACTION_CATEGORIES.iter().map(|(key, label, icon)| {
                view! {
                    <button class="action-cat-btn"
                        class:action-cat-btn--active=move || category_for() == *key
                        on:click=move |_| {
                            if category_for() == *key { return; }
                            let def = category_default(key);
                            set.run(default_action_typed(def));
                        }
                    >
                        <i class={format!("ph ph-{}", icon)} style="font-size:16px"></i>
                        <span class="action-cat-label">{*label}</span>
                    </button>
                }
            }).collect_view()}
        </div>

        // ── Action-specific editor ───────────────────────────────────
        {move || {
            let a = get.get();
            let cat = action_category_typed(&a);

            match cat {
                "device" => {
                    let vk = action_variant_key(&a);
                    view! {
                        <div class="trigger-fields">
                            <select class="hc-select" on:change=move |ev| set.run(default_action_typed(&event_target_value(&ev)))>
                                {[("set_device_state","Command device"),("fade_device","Fade device"),("capture_device_state","Capture state"),("restore_device_state","Restore state")]
                                    .map(|(v,l)| view! { <option value=v selected=vk==v>{l}</option> }).collect_view()}
                            </select>
                            {match &a {
                                Action::SetDeviceState { device_id, .. } | Action::FadeDevice { device_id, .. } => {
                                    let did = device_id.clone();
                                    let is_fade = matches!(&a, Action::FadeDevice { .. });
                                    let dur = if let Action::FadeDevice { duration_secs, .. } = &a { *duration_secs } else { 30 };
                                    let steps_str = if let Action::FadeDevice { steps, .. } = &a {
                                        steps.map(|n| n.to_string()).unwrap_or_default()
                                    } else { String::new() };
                                    let track = if let Action::SetDeviceState { track_event_value, .. } = &a { *track_event_value } else { false };
                                    view! {
                                        <label class="field-label">"Device"</label>
                                        <DeviceSelect value=did on_select=Callback::new(move |id: String| {
                                            let mut a = get.get_untracked();
                                            match &mut a {
                                                Action::SetDeviceState { ref mut device_id, .. } | Action::FadeDevice { ref mut device_id, .. } => *device_id = id,
                                                _ => {}
                                            }
                                            set.run(a);
                                        }) />
                                        <TypedDeviceStateBuilder get=get set=set />
                                        {(!is_fade).then(|| view! {
                                            <label class="field-label" style="display:flex;align-items:center;gap:.4rem;margin-top:.4rem">
                                                <input type="checkbox" prop:checked=track
                                                    on:change=move |ev| {
                                                        let mut a = get.get_untracked();
                                                        if let Action::SetDeviceState { ref mut track_event_value, .. } = a {
                                                            *track_event_value = event_target_checked(&ev);
                                                        }
                                                        set.run(a);
                                                    } />
                                                "Track event value (mirror trigger's new value)"
                                            </label>
                                        })}
                                        {is_fade.then(|| view! {
                                            <label class="field-label">"Duration (seconds)"</label>
                                            <input type="number" class="hc-input hc-input--sm" style="width:8rem" prop:value=dur.to_string()
                                                on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<u64>() {
                                                    let mut a = get.get_untracked();
                                                    if let Action::FadeDevice { ref mut duration_secs, .. } = a { *duration_secs = n; }
                                                    set.run(a);
                                                }} />
                                            <label class="field-label">"Steps (optional, 2–100)"</label>
                                            <input type="number" class="hc-input hc-input--sm" style="width:8rem" min="2" max="100"
                                                placeholder="auto" prop:value=steps_str
                                                on:input=move |ev| {
                                                    let v = event_target_value(&ev);
                                                    let mut a = get.get_untracked();
                                                    if let Action::FadeDevice { ref mut steps, .. } = a {
                                                        *steps = if v.is_empty() { None } else { v.parse::<u32>().ok() };
                                                    }
                                                    set.run(a);
                                                } />
                                        })}
                                    }.into_any()
                                },
                                Action::CaptureDeviceState { key, device_ids } => {
                                    let k = key.clone();
                                    let ids_str = device_ids.join(", ");
                                    view! {
                                        <label class="field-label">"Snapshot key"</label>
                                        <input type="text" class="hc-input" prop:value=k
                                            on:input=move |ev| { let mut a = get.get_untracked(); if let Action::CaptureDeviceState { ref mut key, .. } = a { *key = event_target_value(&ev); } set.run(a); } />
                                        <label class="field-label">"Devices (comma-separated)"</label>
                                        <input type="text" class="hc-input" prop:value=ids_str
                                            on:input=move |ev| {
                                                let ids: Vec<String> = event_target_value(&ev).split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                                                let mut a = get.get_untracked(); if let Action::CaptureDeviceState { ref mut device_ids, .. } = a { *device_ids = ids; } set.run(a);
                                            } />
                                    }.into_any()
                                },
                                Action::RestoreDeviceState { key } => {
                                    let k = key.clone();
                                    view! {
                                        <label class="field-label">"Snapshot key"</label>
                                        <input type="text" class="hc-input" prop:value=k
                                            on:input=move |ev| { let mut a = get.get_untracked(); if let Action::RestoreDeviceState { ref mut key } = a { *key = event_target_value(&ev); } set.run(a); } />
                                    }.into_any()
                                },
                                _ => view! { <span /> }.into_any(),
                            }}
                        </div>
                    }.into_any()
                },

                "conditional" => {
                    let cond = if let Action::Conditional { condition, .. } = &a { condition.clone() } else { String::new() };
                    let else_if_count = if let Action::Conditional { else_if, .. } = &a { else_if.len() } else { 0 };
                    view! {
                        <div class="trigger-fields">
                            <div class="cond-branch cond-branch--if">
                                <span class="cond-branch-label">"IF"</span>
                                <label class="field-label">"Condition (Rhai expression)"</label>
                                <textarea class="hc-textarea hc-textarea--code" rows="2" prop:value=cond
                                    on:input=move |ev| { let mut a = get.get_untracked(); if let Action::Conditional { ref mut condition, .. } = a { *condition = event_target_value(&ev); } set.run(a); } />
                                <label class="field-label">"THEN actions:"</label>
                                <TypedNestedActionList
                                    get_actions=Signal::derive(move || match &get.get() { Action::Conditional { then_actions, .. } => then_actions.clone(), _ => vec![] })
                                    set_actions=Callback::new(move |acts: Vec<Action>| { let mut a = get.get_untracked(); if let Action::Conditional { ref mut then_actions, .. } = a { *then_actions = acts; } set.run(a); })
                                />
                            </div>

                            // ELSE IF branches
                            {(0..else_if_count).map(|bi| {
                                let branch_cond = if let Action::Conditional { else_if, .. } = &a {
                                    else_if.get(bi).map(|b| b.condition.clone()).unwrap_or_default()
                                } else { String::new() };
                                view! {
                                    <div class="cond-branch cond-branch--elseif">
                                        <div class="cond-branch-header">
                                            <span class="cond-branch-label">"ELSE IF"</span>
                                            <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove branch"
                                                on:click=move |_| {
                                                    let mut a = get.get_untracked();
                                                    if let Action::Conditional { ref mut else_if, .. } = a { if bi < else_if.len() { else_if.remove(bi); } }
                                                    set.run(a);
                                                }
                                            ><i class="ph ph-x" style="font-size:14px"></i></button>
                                        </div>
                                        <label class="field-label">"Condition"</label>
                                        <textarea class="hc-textarea hc-textarea--code" rows="2" prop:value=branch_cond
                                            on:input=move |ev| {
                                                let mut a = get.get_untracked();
                                                if let Action::Conditional { ref mut else_if, .. } = a {
                                                    if let Some(b) = else_if.get_mut(bi) { b.condition = event_target_value(&ev); }
                                                }
                                                set.run(a);
                                            } />
                                        <label class="field-label">"THEN actions:"</label>
                                        <TypedNestedActionList
                                            get_actions=Signal::derive(move || match &get.get() {
                                                Action::Conditional { else_if, .. } => else_if.get(bi).map(|b| b.actions.clone()).unwrap_or_default(),
                                                _ => vec![],
                                            })
                                            set_actions=Callback::new(move |acts: Vec<Action>| {
                                                let mut a = get.get_untracked();
                                                if let Action::Conditional { ref mut else_if, .. } = a {
                                                    if let Some(b) = else_if.get_mut(bi) { b.actions = acts; }
                                                }
                                                set.run(a);
                                            })
                                        />
                                    </div>
                                }
                            }).collect_view()}

                            <button class="hc-btn hc-btn--sm hc-btn--outline" style="margin:0.25rem 0"
                                on:click=move |_| {
                                    let mut a = get.get_untracked();
                                    if let Action::Conditional { ref mut else_if, .. } = a {
                                        else_if.push(hc_types::rule::ConditionalBranch { condition: String::new(), actions: vec![] });
                                    }
                                    set.run(a);
                                }
                            >"+ Add ELSE IF"</button>

                            <div class="cond-branch cond-branch--else">
                                <span class="cond-branch-label">"ELSE"</span>
                                <TypedNestedActionList
                                    get_actions=Signal::derive(move || match &get.get() { Action::Conditional { else_actions, .. } => else_actions.clone(), _ => vec![] })
                                    set_actions=Callback::new(move |acts: Vec<Action>| { let mut a = get.get_untracked(); if let Action::Conditional { ref mut else_actions, .. } = a { *else_actions = acts; } set.run(a); })
                                />
                            </div>
                        </div>
                    }.into_any()
                },

                "notify" => {
                    let ch = if let Action::Notify { channel, .. } = &a { channel.clone() } else { String::new() };
                    let ti = if let Action::Notify { title, .. } = &a { title.clone().unwrap_or_default() } else { String::new() };
                    let msg = if let Action::Notify { message, .. } = &a { message.clone() } else { String::new() };
                    view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Channel"</label>
                            <select class="hc-select" on:change=move |ev| { let mut a = get.get_untracked(); if let Action::Notify { ref mut channel, .. } = a { *channel = event_target_value(&ev); } set.run(a); }>
                                {[("all","All channels"),("telegram","Telegram"),("pushover","Pushover"),("email","Email")]
                                    .map(|(v,l)| view! { <option value=v selected=ch==v>{l}</option> }).collect_view()}
                            </select>
                            <label class="field-label">"Title (optional)"</label>
                            <input type="text" class="hc-input" prop:value=ti
                                on:input=move |ev| { let v = event_target_value(&ev); let mut a = get.get_untracked(); if let Action::Notify { ref mut title, .. } = a { *title = if v.is_empty() { None } else { Some(v) }; } set.run(a); } />
                            <label class="field-label">"Message"</label>
                            <textarea class="hc-textarea" rows="2" prop:value=msg
                                on:input=move |ev| { let mut a = get.get_untracked(); if let Action::Notify { ref mut message, .. } = a { *message = event_target_value(&ev); } set.run(a); } />
                        </div>
                    }.into_any()
                },

                "mode" => {
                    let mid = if let Action::SetMode { mode_id, .. } = &a { mode_id.clone() } else { String::new() };
                    let cmd = if let Action::SetMode { command, .. } = &a { format!("{:?}", command) } else { "On".into() };
                    view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Mode"</label>
                            <ModeSelect value=mid on_select=Callback::new(move |id: String| {
                                let mut a = get.get_untracked(); if let Action::SetMode { ref mut mode_id, .. } = a { *mode_id = id; } set.run(a);
                            }) />
                            <label class="field-label">"Command"</label>
                            <div class="toggle-group">
                                <button class:active=cmd=="On" on:click=move |_| { let mut a = get.get_untracked(); if let Action::SetMode { ref mut command, .. } = a { *command = ModeCommand::On; } set.run(a); }>"On"</button>
                                <button class:active=cmd=="Off" on:click=move |_| { let mut a = get.get_untracked(); if let Action::SetMode { ref mut command, .. } = a { *command = ModeCommand::Off; } set.run(a); }>"Off"</button>
                                <button class:active=cmd=="Toggle" on:click=move |_| { let mut a = get.get_untracked(); if let Action::SetMode { ref mut command, .. } = a { *command = ModeCommand::Toggle; } set.run(a); }>"Toggle"</button>
                            </div>
                        </div>
                    }.into_any()
                },

                "timing" => {
                    let vk = action_variant_key(&a);
                    view! {
                        <div class="trigger-fields">
                            <select class="hc-select" on:change=move |ev| set.run(default_action_typed(&event_target_value(&ev)))>
                                {[("delay","Delay"),("wait_for_event","Wait for event"),("wait_for_expression","Wait for expression")]
                                    .map(|(v,l)| view! { <option value=v selected=vk==v>{l}</option> }).collect_view()}
                            </select>
                            {match &a {
                                Action::Delay { duration_secs, cancelable, cancel_key } => {
                                    let dur = *duration_secs;
                                    let canc = *cancelable;
                                    let ck = cancel_key.clone().unwrap_or_default();
                                    view! {
                                        <div class="control-row">
                                            <span class="control-label">"Duration"</span>
                                            <div class="state-slider-row">
                                                <input type="range" class="state-slider" min="1" max="300" step="1" prop:value=dur.to_string()
                                                    on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<u64>() {
                                                        let mut a = get.get_untracked(); if let Action::Delay { ref mut duration_secs, .. } = a { *duration_secs = n; } set.run(a);
                                                    }} />
                                                <span class="state-slider-val">{format!("{dur}s")}</span>
                                            </div>
                                        </div>
                                        <div class="control-row">
                                            <span class="control-label">"Cancelable"</span>
                                            <div class="toggle-group">
                                                <button class:active=canc on:click=move |_| { let mut a = get.get_untracked(); if let Action::Delay { ref mut cancelable, .. } = a { *cancelable = true; } set.run(a); }>"Yes"</button>
                                                <button class:active=!canc on:click=move |_| { let mut a = get.get_untracked(); if let Action::Delay { ref mut cancelable, .. } = a { *cancelable = false; } set.run(a); }>"No"</button>
                                            </div>
                                        </div>
                                        {canc.then(|| view! {
                                            <label class="field-label">"Cancel key (optional, groups cancelable delays)"</label>
                                            <input type="text" class="hc-input" placeholder="e.g. off_timer" prop:value=ck
                                                on:input=move |ev| {
                                                    let v = event_target_value(&ev);
                                                    let mut a = get.get_untracked();
                                                    if let Action::Delay { ref mut cancel_key, .. } = a {
                                                        *cancel_key = if v.is_empty() { None } else { Some(v) };
                                                    }
                                                    set.run(a);
                                                } />
                                        })}
                                    }.into_any()
                                },
                                Action::WaitForExpression { expression, timeout_ms, poll_interval_ms, hold_duration_ms } => {
                                    let expr = expression.clone();
                                    let tms = timeout_ms.map(|n| n.to_string()).unwrap_or_default();
                                    let pms = poll_interval_ms.map(|n| n.to_string()).unwrap_or_default();
                                    let hms = hold_duration_ms.map(|n| n.to_string()).unwrap_or_default();
                                    view! {
                                        <label class="field-label">"Rhai expression"</label>
                                        <textarea class="hc-textarea hc-textarea--code" rows="3" prop:value=expr
                                            on:input=move |ev| { let mut a = get.get_untracked(); if let Action::WaitForExpression { ref mut expression, .. } = a { *expression = event_target_value(&ev); } set.run(a); } />
                                        <label class="field-label">"Timeout (ms, blank = none)"</label>
                                        <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="none" prop:value=tms
                                            on:input=move |ev| { let mut a = get.get_untracked(); if let Action::WaitForExpression { ref mut timeout_ms, .. } = a { *timeout_ms = event_target_value(&ev).parse::<u64>().ok(); } set.run(a); } />
                                        <label class="field-label">"Poll interval (ms, blank = default)"</label>
                                        <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="default" prop:value=pms
                                            on:input=move |ev| {
                                                let v = event_target_value(&ev);
                                                let mut a = get.get_untracked();
                                                if let Action::WaitForExpression { ref mut poll_interval_ms, .. } = a {
                                                    *poll_interval_ms = if v.is_empty() { None } else { v.parse::<u64>().ok() };
                                                }
                                                set.run(a);
                                            } />
                                        <label class="field-label">"Hold duration (ms, must stay true for)"</label>
                                        <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="none" prop:value=hms
                                            on:input=move |ev| {
                                                let v = event_target_value(&ev);
                                                let mut a = get.get_untracked();
                                                if let Action::WaitForExpression { ref mut hold_duration_ms, .. } = a {
                                                    *hold_duration_ms = if v.is_empty() { None } else { v.parse::<u64>().ok() };
                                                }
                                                set.run(a);
                                            } />
                                    }.into_any()
                                },
                                Action::WaitForEvent { device_id, attribute, event_type, timeout_ms } => {
                                    let did = device_id.clone().unwrap_or_default();
                                    let attr = attribute.clone().unwrap_or_default();
                                    let et = event_type.clone().unwrap_or_default();
                                    let tms = timeout_ms.map(|n| n.to_string()).unwrap_or_default();
                                    view! {
                                        <label class="field-label">"Event type (blank = any)"</label>
                                        <input type="text" class="hc-input" placeholder="e.g. custom_event_name" prop:value=et
                                            on:input=move |ev| { let v = event_target_value(&ev); let mut a = get.get_untracked();
                                                if let Action::WaitForEvent { ref mut event_type, .. } = a { *event_type = if v.is_empty() { None } else { Some(v) }; } set.run(a); } />
                                        <label class="field-label">"Device (blank = any)"</label>
                                        <DeviceSelect value=did on_select=Callback::new(move |id: String| {
                                            let mut a = get.get_untracked();
                                            if let Action::WaitForEvent { ref mut device_id, .. } = a { *device_id = if id.is_empty() { None } else { Some(id) }; }
                                            set.run(a);
                                        }) />
                                        <label class="field-label">"Attribute (blank = any)"</label>
                                        <AttributeSelect device_id={device_id.clone().unwrap_or_default()} value=attr
                                            on_select=Callback::new(move |a_str: String| {
                                                let mut a = get.get_untracked();
                                                if let Action::WaitForEvent { ref mut attribute, .. } = a { *attribute = if a_str.is_empty() { None } else { Some(a_str) }; }
                                                set.run(a);
                                            }) />
                                        <label class="field-label">"Timeout (ms, blank = no timeout)"</label>
                                        <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="none" prop:value=tms
                                            on:input=move |ev| { let mut a = get.get_untracked();
                                                if let Action::WaitForEvent { ref mut timeout_ms, .. } = a { *timeout_ms = event_target_value(&ev).parse::<u64>().ok(); }
                                                set.run(a); } />
                                    }.into_any()
                                },
                                _ => view! { <span /> }.into_any(),
                            }}
                        </div>
                    }.into_any()
                },

                "script" => {
                    let s = if let Action::RunScript { script } = &a { script.clone() } else { String::new() };
                    view! {
                        <div class="trigger-fields">
                            <label class="field-label">"Rhai script"</label>
                            <textarea class="hc-textarea hc-textarea--code" rows="6" prop:value=s
                                on:input=move |ev| { let mut a = get.get_untracked(); if let Action::RunScript { ref mut script } = a { *script = event_target_value(&ev); } set.run(a); } />
                        </div>
                    }.into_any()
                },

                "rule_ctrl" => {
                    let vk = action_variant_key(&a);
                    view! {
                        <div class="trigger-fields">
                            <select class="hc-select" on:change=move |ev| set.run(default_action_typed(&event_target_value(&ev)))>
                                {[("run_rule_actions","Run rule actions"),("pause_rule","Pause rule"),("resume_rule","Resume rule"),
                                  ("cancel_delays","Cancel delays"),("cancel_rule_timers","Cancel rule timers")]
                                    .map(|(v,l)| view! { <option value=v selected=vk==v>{l}</option> }).collect_view()}
                            </select>
                            {match &a {
                                Action::RunRuleActions { rule_id } | Action::PauseRule { rule_id } | Action::ResumeRule { rule_id } => {
                                    let rid = rule_id.to_string();
                                    view! {
                                        <label class="field-label">"Rule"</label>
                                        <RuleSelect value=rid on_select=Callback::new(move |id: String| {
                                            if let Ok(uid) = id.parse::<Uuid>() {
                                                let mut a = get.get_untracked();
                                                match &mut a {
                                                    Action::RunRuleActions { ref mut rule_id } | Action::PauseRule { ref mut rule_id } | Action::ResumeRule { ref mut rule_id } => *rule_id = uid,
                                                    _ => {}
                                                }
                                                set.run(a);
                                            }
                                        }) />
                                    }.into_any()
                                },
                                Action::CancelDelays { key } => {
                                    let k = key.clone().unwrap_or_default();
                                    view! {
                                        <label class="field-label">"Cancel key (blank = all)"</label>
                                        <input type="text" class="hc-input" prop:value=k
                                            on:input=move |ev| { let v = event_target_value(&ev); let mut a = get.get_untracked(); if let Action::CancelDelays { ref mut key } = a { *key = if v.is_empty() { None } else { Some(v) }; } set.run(a); } />
                                    }.into_any()
                                },
                                Action::CancelRuleTimers { rule_id } => {
                                    let rid = rule_id.map(|u| u.to_string()).unwrap_or_default();
                                    view! {
                                        <label class="field-label">"Target rule"</label>
                                        <RuleSelect value=rid on_select=Callback::new(move |id: String| {
                                            let mut a = get.get_untracked();
                                            if let Action::CancelRuleTimers { ref mut rule_id } = a {
                                                *rule_id = if id.is_empty() { None } else { Uuid::parse_str(&id).ok() };
                                            }
                                            set.run(a);
                                        }) />
                                        <p class="msg-muted" style="font-size:0.85rem">"Blank = cancel this rule's own timers."</p>
                                    }.into_any()
                                },
                                _ => view! { <span /> }.into_any(),
                            }}
                        </div>
                    }.into_any()
                },

                // ── MORE (remaining action types) ─────────────────────
                _ => {
                    let vk = action_variant_key(&a);
                    view! {
                        <div class="trigger-fields">
                            <select class="hc-select" on:change=move |ev| set.run(default_action_typed(&event_target_value(&ev)))>
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
                                    .map(|(v,l)| view! { <option value=v selected=vk==v>{l}</option> }).collect_view()}
                            </select>
                            {match &a {
                                Action::LogMessage { message, level } => {
                                    let msg = message.clone();
                                    let lvl = level.as_ref().map(|l| format!("{:?}", l)).unwrap_or_default();
                                    view! {
                                        <label class="field-label">"Level"</label>
                                        <select class="hc-select" on:change=move |ev| {
                                            let raw = event_target_value(&ev);
                                            let mut a = get.get_untracked();
                                            if let Action::LogMessage { ref mut level, .. } = a {
                                                *level = match raw.as_str() { "Debug" => Some(LogLevel::Debug), "Warn" => Some(LogLevel::Warn), "Error" => Some(LogLevel::Error), _ => None };
                                            }
                                            set.run(a);
                                        }>
                                            {[("","Info (default)"),("Debug","Debug"),("Warn","Warning"),("Error","Error")]
                                                .map(|(v,l)| view! { <option value=v selected=lvl==v>{l}</option> }).collect_view()}
                                        </select>
                                        <label class="field-label">"Message"</label>
                                        <textarea class="hc-textarea" rows="2" prop:value=msg
                                            on:input=move |ev| { let mut a = get.get_untracked(); if let Action::LogMessage { ref mut message, .. } = a { *message = event_target_value(&ev); } set.run(a); } />
                                    }.into_any()
                                },
                                Action::Comment { text } => {
                                    let t = text.clone();
                                    view! {
                                        <textarea class="hc-textarea" rows="2" placeholder="Comment text" prop:value=t
                                            on:input=move |ev| { let mut a = get.get_untracked(); if let Action::Comment { ref mut text } = a { *text = event_target_value(&ev); } set.run(a); } />
                                    }.into_any()
                                },
                                Action::FireEvent { event_type, payload } => {
                                    let et = event_type.clone();
                                    let pl = serde_json::to_string_pretty(payload).unwrap_or_default();
                                    view! {
                                        <label class="field-label">"Event type"</label>
                                        <input type="text" class="hc-input" prop:value=et
                                            on:input=move |ev| { let mut a = get.get_untracked(); if let Action::FireEvent { ref mut event_type, .. } = a { *event_type = event_target_value(&ev); } set.run(a); } />
                                        <label class="field-label">"Payload (JSON)"</label>
                                        <textarea class="hc-textarea hc-textarea--code" rows="2" prop:value=pl
                                            on:input=move |ev| { if let Ok(p) = serde_json::from_str::<Value>(&event_target_value(&ev)) {
                                                let mut a = get.get_untracked(); if let Action::FireEvent { ref mut payload, .. } = a { *payload = p; } set.run(a);
                                            }} />
                                    }.into_any()
                                },
                                Action::CallService { url, method, body, timeout_ms, retries, response_event } => {
                                    let u = url.clone();
                                    let m = method.clone();
                                    let b = serde_json::to_string_pretty(body).unwrap_or_default();
                                    let tms = timeout_ms.map(|n| n.to_string()).unwrap_or_default();
                                    let ret = retries.map(|n| n.to_string()).unwrap_or_default();
                                    let resp = response_event.clone().unwrap_or_default();
                                    view! {
                                        <label class="field-label">"URL"</label>
                                        <input type="text" class="hc-input" prop:value=u
                                            on:input=move |ev| { let mut a = get.get_untracked(); if let Action::CallService { ref mut url, .. } = a { *url = event_target_value(&ev); } set.run(a); } />
                                        <label class="field-label">"Method"</label>
                                        <select class="hc-select" on:change=move |ev| {
                                            let mut a = get.get_untracked(); if let Action::CallService { ref mut method, .. } = a { *method = event_target_value(&ev); } set.run(a);
                                        }>
                                            {["GET","POST","PUT","PATCH","DELETE"].map(|v| view! { <option value=v selected=m==v>{v}</option> }).collect_view()}
                                        </select>
                                        <label class="field-label">"Body (JSON, optional)"</label>
                                        <textarea class="hc-textarea hc-textarea--code" rows="3" prop:value=b
                                            on:input=move |ev| { if let Ok(p) = serde_json::from_str::<Value>(&event_target_value(&ev)) {
                                                let mut a = get.get_untracked(); if let Action::CallService { ref mut body, .. } = a { *body = p; } set.run(a);
                                            }} />
                                        <div class="trigger-row-2">
                                            <div>
                                                <label class="field-label">"Timeout (ms)"</label>
                                                <input type="number" class="hc-input hc-input--sm" placeholder="none" prop:value=tms
                                                    on:input=move |ev| { let mut a = get.get_untracked();
                                                        if let Action::CallService { ref mut timeout_ms, .. } = a { *timeout_ms = event_target_value(&ev).parse::<u64>().ok(); }
                                                        set.run(a); } />
                                            </div>
                                            <div>
                                                <label class="field-label">"Retries"</label>
                                                <input type="number" class="hc-input hc-input--sm" placeholder="0" prop:value=ret
                                                    on:input=move |ev| { let mut a = get.get_untracked();
                                                        if let Action::CallService { ref mut retries, .. } = a { *retries = event_target_value(&ev).parse::<u32>().ok(); }
                                                        set.run(a); } />
                                            </div>
                                        </div>
                                        <label class="field-label">"Response event (fires on completion, blank = none)"</label>
                                        <input type="text" class="hc-input" placeholder="e.g. http_response" prop:value=resp
                                            on:input=move |ev| {
                                                let v = event_target_value(&ev);
                                                let mut a = get.get_untracked();
                                                if let Action::CallService { ref mut response_event, .. } = a {
                                                    *response_event = if v.is_empty() { None } else { Some(v) };
                                                }
                                                set.run(a);
                                            } />
                                    }.into_any()
                                },
                                Action::PublishMqtt { topic, payload, retain } => {
                                    let tp = topic.clone();
                                    let pl = payload.clone();
                                    let ret = *retain;
                                    view! {
                                        <label class="field-label">"Topic"</label>
                                        <input type="text" class="hc-input hc-textarea--code" prop:value=tp
                                            on:input=move |ev| { let mut a = get.get_untracked(); if let Action::PublishMqtt { ref mut topic, .. } = a { *topic = event_target_value(&ev); } set.run(a); } />
                                        <label class="field-label">"Payload"</label>
                                        <textarea class="hc-textarea hc-textarea--code" rows="2" prop:value=pl
                                            on:input=move |ev| { let mut a = get.get_untracked(); if let Action::PublishMqtt { ref mut payload, .. } = a { *payload = event_target_value(&ev); } set.run(a); } />
                                        <label class="field-label" style="display:flex;align-items:center;gap:.4rem;margin-top:.4rem">
                                            <input type="checkbox" prop:checked=ret
                                                on:change=move |ev| {
                                                    let mut a = get.get_untracked();
                                                    if let Action::PublishMqtt { ref mut retain, .. } = a { *retain = event_target_checked(&ev); }
                                                    set.run(a);
                                                } />
                                            "Retain (broker keeps last value for new subscribers)"
                                        </label>
                                    }.into_any()
                                },
                                Action::SetVariable { name, value, op } | Action::SetHubVariable { name, value, op } => {
                                    let n = name.clone();
                                    let val = if value.is_string() { value.as_str().unwrap_or("").to_string() } else { value.to_string() };
                                    let op_key = match op {
                                        None => "set",
                                        Some(VariableOp::Set) => "set",
                                        Some(VariableOp::Add) => "add",
                                        Some(VariableOp::Subtract) => "subtract",
                                        Some(VariableOp::Multiply) => "multiply",
                                        Some(VariableOp::Divide) => "divide",
                                        Some(VariableOp::Toggle) => "toggle",
                                    };
                                    let is_toggle = matches!(op, Some(VariableOp::Toggle));
                                    view! {
                                        <label class="field-label">"Variable name"</label>
                                        <input type="text" class="hc-input" prop:value=n
                                            on:input=move |ev| { let mut a = get.get_untracked();
                                                match &mut a { Action::SetVariable { ref mut name, .. } | Action::SetHubVariable { ref mut name, .. } => *name = event_target_value(&ev), _ => {} }
                                                set.run(a); } />
                                        <label class="field-label">"Operation"</label>
                                        <select class="hc-select" on:change=move |ev| {
                                            let new_op = match event_target_value(&ev).as_str() {
                                                "add" => Some(VariableOp::Add),
                                                "subtract" => Some(VariableOp::Subtract),
                                                "multiply" => Some(VariableOp::Multiply),
                                                "divide" => Some(VariableOp::Divide),
                                                "toggle" => Some(VariableOp::Toggle),
                                                _ => None,
                                            };
                                            let mut a = get.get_untracked();
                                            match &mut a {
                                                Action::SetVariable { ref mut op, .. } | Action::SetHubVariable { ref mut op, .. } => *op = new_op,
                                                _ => {}
                                            }
                                            set.run(a);
                                        }>
                                            {[("set","Set (replace)"),("add","Add"),("subtract","Subtract"),("multiply","Multiply"),("divide","Divide"),("toggle","Toggle (boolean)")]
                                                .map(|(v,l)| view! { <option value=v selected=op_key==v>{l}</option> }).collect_view()}
                                        </select>
                                        {(!is_toggle).then(|| view! {
                                            <label class="field-label">"Value (JSON)"</label>
                                            <input type="text" class="hc-input" prop:value=val
                                                on:input=move |ev| { let raw = event_target_value(&ev); let v = serde_json::from_str(&raw).unwrap_or_else(|_| json!(raw));
                                                    let mut a = get.get_untracked();
                                                    match &mut a { Action::SetVariable { ref mut value, .. } | Action::SetHubVariable { ref mut value, .. } => *value = v, _ => {} }
                                                    set.run(a); } />
                                        })}
                                    }.into_any()
                                },
                                Action::SetPrivateBoolean { name, value } => {
                                    let n = name.clone(); let v = *value;
                                    view! {
                                        <label class="field-label">"Name"</label>
                                        <input type="text" class="hc-input" prop:value=n
                                            on:input=move |ev| { let mut a = get.get_untracked(); if let Action::SetPrivateBoolean { ref mut name, .. } = a { *name = event_target_value(&ev); } set.run(a); } />
                                        <div class="toggle-group">
                                            <button class:active=v on:click=move |_| { let mut a = get.get_untracked(); if let Action::SetPrivateBoolean { ref mut value, .. } = a { *value = true; } set.run(a); }>"True"</button>
                                            <button class:active=!v on:click=move |_| { let mut a = get.get_untracked(); if let Action::SetPrivateBoolean { ref mut value, .. } = a { *value = false; } set.run(a); }>"False"</button>
                                        </div>
                                    }.into_any()
                                },
                                Action::StopRuleChain | Action::ExitRule => {
                                    let msg = if matches!(&a, Action::StopRuleChain) { "Stops lower-priority rules." } else { "Halts remaining actions." };
                                    view! { <p class="msg-muted" style="font-size:0.85rem">{msg}</p> }.into_any()
                                },
                                Action::Parallel { .. } => {
                                    view! {
                                        <TypedNestedActionList
                                            get_actions=Signal::derive(move || match &get.get() { Action::Parallel { actions } => actions.clone(), _ => vec![] })
                                            set_actions=Callback::new(move |acts: Vec<Action>| { let mut a = get.get_untracked(); if let Action::Parallel { ref mut actions } = a { *actions = acts; } set.run(a); })
                                        />
                                    }.into_any()
                                },
                                Action::RepeatCount { count, interval_ms, .. } => {
                                    let c = *count;
                                    let ims = interval_ms.map(|n| n.to_string()).unwrap_or_default();
                                    view! {
                                        <label class="field-label">"Count"</label>
                                        <input type="number" class="hc-input hc-input--sm" min="1" prop:value=c.to_string()
                                            on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<u32>() {
                                                let mut a = get.get_untracked(); if let Action::RepeatCount { ref mut count, .. } = a { *count = n; } set.run(a);
                                            }} />
                                        <label class="field-label">"Interval between iterations (ms, blank = none)"</label>
                                        <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="none" prop:value=ims
                                            on:input=move |ev| {
                                                let v = event_target_value(&ev);
                                                let mut a = get.get_untracked();
                                                if let Action::RepeatCount { ref mut interval_ms, .. } = a {
                                                    *interval_ms = if v.is_empty() { None } else { v.parse::<u64>().ok() };
                                                }
                                                set.run(a);
                                            } />
                                        <TypedNestedActionList
                                            get_actions=Signal::derive(move || match &get.get() { Action::RepeatCount { actions, .. } => actions.clone(), _ => vec![] })
                                            set_actions=Callback::new(move |acts: Vec<Action>| { let mut a = get.get_untracked(); if let Action::RepeatCount { ref mut actions, .. } = a { *actions = acts; } set.run(a); })
                                        />
                                    }.into_any()
                                },
                                Action::RepeatUntil { condition, max_iterations, interval_ms, .. } | Action::RepeatWhile { condition, max_iterations, interval_ms, .. } => {
                                    let cond = condition.clone();
                                    let mi = max_iterations.map(|n| n.to_string()).unwrap_or_default();
                                    let ims = interval_ms.map(|n| n.to_string()).unwrap_or_default();
                                    let _is_until = matches!(&a, Action::RepeatUntil { .. });
                                    view! {
                                        <label class="field-label">"Condition (Rhai)"</label>
                                        <textarea class="hc-textarea hc-textarea--code" rows="2" prop:value=cond
                                            on:input=move |ev| { let mut a = get.get_untracked();
                                                match &mut a { Action::RepeatUntil { ref mut condition, .. } | Action::RepeatWhile { ref mut condition, .. } => *condition = event_target_value(&ev), _ => {} }
                                                set.run(a); } />
                                        <label class="field-label">"Max iterations"</label>
                                        <input type="number" class="hc-input hc-input--sm" placeholder="unlimited" prop:value=mi
                                            on:input=move |ev| { let mut a = get.get_untracked();
                                                let v = event_target_value(&ev).parse::<u32>().ok();
                                                match &mut a { Action::RepeatUntil { ref mut max_iterations, .. } | Action::RepeatWhile { ref mut max_iterations, .. } => *max_iterations = v, _ => {} }
                                                set.run(a); } />
                                        <label class="field-label">"Interval between iterations (ms, blank = none)"</label>
                                        <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="none" prop:value=ims
                                            on:input=move |ev| {
                                                let v = event_target_value(&ev);
                                                let parsed = if v.is_empty() { None } else { v.parse::<u64>().ok() };
                                                let mut a = get.get_untracked();
                                                match &mut a {
                                                    Action::RepeatUntil { ref mut interval_ms, .. } | Action::RepeatWhile { ref mut interval_ms, .. } => *interval_ms = parsed,
                                                    _ => {}
                                                }
                                                set.run(a);
                                            } />
                                        <TypedNestedActionList
                                            get_actions=Signal::derive(move || match &get.get() { Action::RepeatUntil { actions, .. } | Action::RepeatWhile { actions, .. } => actions.clone(), _ => vec![] })
                                            set_actions=Callback::new(move |acts: Vec<Action>| { let mut a = get.get_untracked();
                                                match &mut a { Action::RepeatUntil { ref mut actions, .. } | Action::RepeatWhile { ref mut actions, .. } => *actions = acts, _ => {} }
                                                set.run(a); })
                                        />
                                    }.into_any()
                                },
                                Action::PingHost { host, count, timeout_ms, response_event, .. } => {
                                    let h = host.clone();
                                    let cnt = count.map(|n| n.to_string()).unwrap_or_default();
                                    let tms = timeout_ms.map(|n| n.to_string()).unwrap_or_default();
                                    let resp = response_event.clone().unwrap_or_default();
                                    view! {
                                        <label class="field-label">"Host"</label>
                                        <input type="text" class="hc-input" placeholder="e.g. 192.168.1.1" prop:value=h
                                            on:input=move |ev| { let mut a = get.get_untracked(); if let Action::PingHost { ref mut host, .. } = a { *host = event_target_value(&ev); } set.run(a); } />
                                        <div class="trigger-row-2">
                                            <div>
                                                <label class="field-label">"Echo count (default 1)"</label>
                                                <input type="number" class="hc-input hc-input--sm" min="1" placeholder="1" prop:value=cnt
                                                    on:input=move |ev| {
                                                        let v = event_target_value(&ev);
                                                        let parsed = if v.is_empty() { None } else { v.parse::<u32>().ok() };
                                                        let mut a = get.get_untracked();
                                                        if let Action::PingHost { ref mut count, .. } = a { *count = parsed; }
                                                        set.run(a);
                                                    } />
                                            </div>
                                            <div>
                                                <label class="field-label">"Timeout (ms, default 3000)"</label>
                                                <input type="number" class="hc-input hc-input--sm" placeholder="3000" prop:value=tms
                                                    on:input=move |ev| {
                                                        let v = event_target_value(&ev);
                                                        let parsed = if v.is_empty() { None } else { v.parse::<u64>().ok() };
                                                        let mut a = get.get_untracked();
                                                        if let Action::PingHost { ref mut timeout_ms, .. } = a { *timeout_ms = parsed; }
                                                        set.run(a);
                                                    } />
                                            </div>
                                        </div>
                                        <label class="field-label">"Response event (blank = none)"</label>
                                        <input type="text" class="hc-input" placeholder="e.g. router_ping" prop:value=resp
                                            on:input=move |ev| {
                                                let v = event_target_value(&ev);
                                                let mut a = get.get_untracked();
                                                if let Action::PingHost { ref mut response_event, .. } = a {
                                                    *response_event = if v.is_empty() { None } else { Some(v) };
                                                }
                                                set.run(a);
                                            } />
                                        <div class="cond-branch cond-branch--if">
                                            <span class="cond-branch-label">"Reachable"</span>
                                            <TypedNestedActionList
                                                get_actions=Signal::derive(move || match &get.get() { Action::PingHost { then_actions, .. } => then_actions.clone(), _ => vec![] })
                                                set_actions=Callback::new(move |acts: Vec<Action>| { let mut a = get.get_untracked(); if let Action::PingHost { ref mut then_actions, .. } = a { *then_actions = acts; } set.run(a); })
                                            />
                                        </div>
                                        <div class="cond-branch cond-branch--else">
                                            <span class="cond-branch-label">"Unreachable"</span>
                                            <TypedNestedActionList
                                                get_actions=Signal::derive(move || match &get.get() { Action::PingHost { else_actions, .. } => else_actions.clone(), _ => vec![] })
                                                set_actions=Callback::new(move |acts: Vec<Action>| { let mut a = get.get_untracked(); if let Action::PingHost { ref mut else_actions, .. } = a { *else_actions = acts; } set.run(a); })
                                            />
                                        </div>
                                    }.into_any()
                                },
                                Action::SetDeviceStatePerMode { device_id, modes, default_state } => {
                                    let did = device_id.clone();
                                    let mode_count = modes.len();
                                    let def_str = default_state.as_ref().map(|v| serde_json::to_string_pretty(v).unwrap_or_default()).unwrap_or_default();
                                    view! {
                                        <label class="field-label">"Device"</label>
                                        <DeviceSelect value=did on_select=Callback::new(move |id: String| {
                                            let mut a = get.get_untracked(); if let Action::SetDeviceStatePerMode { ref mut device_id, .. } = a { *device_id = id; } set.run(a);
                                        }) />
                                        <label class="field-label">"Modes"</label>
                                        {(0..mode_count).map(|mi| {
                                            let mode_id = modes[mi].mode.clone();
                                            let state_str = serde_json::to_string_pretty(&modes[mi].state).unwrap_or_default();
                                            view! {
                                                <div class="json-row">
                                                    <div class="json-row-controls">
                                                        <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove"
                                                            on:click=move |_| { let mut a = get.get_untracked();
                                                                if let Action::SetDeviceStatePerMode { ref mut modes, .. } = a { if mi < modes.len() { modes.remove(mi); } } set.run(a);
                                                            }
                                                        ><i class="ph ph-x" style="font-size:14px"></i></button>
                                                    </div>
                                                    <ModeSelect value=mode_id on_select=Callback::new(move |id: String| {
                                                        let mut a = get.get_untracked();
                                                        if let Action::SetDeviceStatePerMode { ref mut modes, .. } = a { if let Some(m) = modes.get_mut(mi) { m.mode = id; } } set.run(a);
                                                    }) />
                                                    <textarea class="hc-textarea hc-textarea--code" rows="2" prop:value=state_str
                                                        on:input=move |ev| { if let Ok(v) = serde_json::from_str::<Value>(&event_target_value(&ev)) {
                                                            let mut a = get.get_untracked();
                                                            if let Action::SetDeviceStatePerMode { ref mut modes, .. } = a { if let Some(m) = modes.get_mut(mi) { m.state = v; } } set.run(a);
                                                        }} />
                                                </div>
                                            }
                                        }).collect_view()}
                                        <button class="hc-btn hc-btn--sm hc-btn--outline" style="margin-top:0.25rem"
                                            on:click=move |_| { let mut a = get.get_untracked();
                                                if let Action::SetDeviceStatePerMode { ref mut modes, .. } = a { modes.push(ModeStateEntry { mode: String::new(), state: json!({}) }); } set.run(a);
                                            }
                                        >"+ Add mode"</button>
                                        <label class="field-label">"Default state (JSON, blank = none)"</label>
                                        <textarea class="hc-textarea hc-textarea--code" rows="2" prop:value=def_str
                                            on:input=move |ev| { let raw = event_target_value(&ev);
                                                let mut a = get.get_untracked();
                                                if let Action::SetDeviceStatePerMode { ref mut default_state, .. } = a {
                                                    *default_state = if raw.trim().is_empty() { None } else { serde_json::from_str(&raw).ok() };
                                                } set.run(a);
                                            } />
                                    }.into_any()
                                },

                                Action::DelayPerMode { modes, default_secs } => {
                                    let mode_count = modes.len();
                                    let def = default_secs.map(|n| n.to_string()).unwrap_or_default();
                                    view! {
                                        <label class="field-label">"Modes"</label>
                                        {(0..mode_count).map(|mi| {
                                            let mode_id = modes[mi].mode.clone();
                                            let dur = modes[mi].duration_secs;
                                            view! {
                                                <div class="json-row">
                                                    <div class="json-row-controls">
                                                        <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove"
                                                            on:click=move |_| { let mut a = get.get_untracked();
                                                                if let Action::DelayPerMode { ref mut modes, .. } = a { if mi < modes.len() { modes.remove(mi); } } set.run(a);
                                                            }
                                                        ><i class="ph ph-x" style="font-size:14px"></i></button>
                                                    </div>
                                                    <div class="trigger-row-2">
                                                        <ModeSelect value=mode_id on_select=Callback::new(move |id: String| {
                                                            let mut a = get.get_untracked();
                                                            if let Action::DelayPerMode { ref mut modes, .. } = a { if let Some(m) = modes.get_mut(mi) { m.mode = id; } } set.run(a);
                                                        }) />
                                                        <div>
                                                            <label class="field-label">"Seconds"</label>
                                                            <input type="number" class="hc-input hc-input--sm" prop:value=dur.to_string()
                                                                on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<u64>() {
                                                                    let mut a = get.get_untracked();
                                                                    if let Action::DelayPerMode { ref mut modes, .. } = a { if let Some(m) = modes.get_mut(mi) { m.duration_secs = n; } } set.run(a);
                                                                }} />
                                                        </div>
                                                    </div>
                                                </div>
                                            }
                                        }).collect_view()}
                                        <button class="hc-btn hc-btn--sm hc-btn--outline" style="margin-top:0.25rem"
                                            on:click=move |_| { let mut a = get.get_untracked();
                                                if let Action::DelayPerMode { ref mut modes, .. } = a { modes.push(ModeDelayEntry { mode: String::new(), duration_secs: 60 }); } set.run(a);
                                            }
                                        >"+ Add mode"</button>
                                        <label class="field-label">"Default (seconds, blank = skip)"</label>
                                        <input type="number" class="hc-input hc-input--sm" style="width:8rem" placeholder="none" prop:value=def
                                            on:input=move |ev| { let mut a = get.get_untracked();
                                                if let Action::DelayPerMode { ref mut default_secs, .. } = a { *default_secs = event_target_value(&ev).parse::<u64>().ok(); } set.run(a);
                                            } />
                                    }.into_any()
                                },

                                Action::ActivateScenePerMode { modes, default_scene_id } => {
                                    let scenes_ctx = use_context::<RwSignal<Vec<Scene>>>().unwrap_or(RwSignal::new(vec![]));
                                    let mode_count = modes.len();
                                    let def_id = default_scene_id.map(|id| id.to_string()).unwrap_or_default();
                                    view! {
                                        <label class="field-label">"Modes"</label>
                                        {(0..mode_count).map(|mi| {
                                            let mode_id = modes[mi].mode.clone();
                                            let scene_id = modes[mi].scene_id.to_string();
                                            view! {
                                                <div class="json-row">
                                                    <div class="json-row-controls">
                                                        <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove"
                                                            on:click=move |_| { let mut a = get.get_untracked();
                                                                if let Action::ActivateScenePerMode { ref mut modes, .. } = a { if mi < modes.len() { modes.remove(mi); } } set.run(a);
                                                            }
                                                        ><i class="ph ph-x" style="font-size:14px"></i></button>
                                                    </div>
                                                    <div class="trigger-row-2">
                                                        <ModeSelect value=mode_id on_select=Callback::new(move |id: String| {
                                                            let mut a = get.get_untracked();
                                                            if let Action::ActivateScenePerMode { ref mut modes, .. } = a { if let Some(m) = modes.get_mut(mi) { m.mode = id; } } set.run(a);
                                                        }) />
                                                        <div>
                                                            <label class="field-label">"Scene"</label>
                                                            <select class="hc-select" on:change=move |ev| {
                                                                if let Ok(uid) = event_target_value(&ev).parse::<Uuid>() {
                                                                    let mut a = get.get_untracked();
                                                                    if let Action::ActivateScenePerMode { ref mut modes, .. } = a { if let Some(m) = modes.get_mut(mi) { m.scene_id = uid; } } set.run(a);
                                                                }
                                                            }>
                                                                <option value="" selected=scene_id.is_empty()>"— Select —"</option>
                                                                {move || scenes_ctx.get().iter().map(|s| {
                                                                    let sel = s.id == scene_id;
                                                                    let sid = s.id.clone();
                                                                    let sname = s.name.clone();
                                                                    view! { <option value=sid selected=sel>{sname}</option> }
                                                                }).collect_view()}
                                                            </select>
                                                        </div>
                                                    </div>
                                                </div>
                                            }
                                        }).collect_view()}
                                        <button class="hc-btn hc-btn--sm hc-btn--outline" style="margin-top:0.25rem"
                                            on:click=move |_| { let mut a = get.get_untracked();
                                                if let Action::ActivateScenePerMode { ref mut modes, .. } = a { modes.push(ModeSceneEntry { mode: String::new(), scene_id: Uuid::nil() }); } set.run(a);
                                            }
                                        >"+ Add mode"</button>
                                        <label class="field-label">"Default scene (blank = none)"</label>
                                        <select class="hc-select" on:change=move |ev| {
                                            let raw = event_target_value(&ev);
                                            let mut a = get.get_untracked();
                                            if let Action::ActivateScenePerMode { ref mut default_scene_id, .. } = a {
                                                *default_scene_id = if raw.is_empty() { None } else { raw.parse::<Uuid>().ok() };
                                            }
                                            set.run(a);
                                        }>
                                            <option value="" selected=def_id.is_empty()>"— None —"</option>
                                            {move || scenes_ctx.get().iter().map(|s| {
                                                let sel = s.id == def_id;
                                                let sid = s.id.clone();
                                                let sname = s.name.clone();
                                                view! { <option value=sid selected=sel>{sname}</option> }
                                            }).collect_view()}
                                        </select>
                                    }.into_any()
                                },

                                // Fallback: JSON editor
                                _ => {
                                    let json_str = serde_json::to_string_pretty(&a).unwrap_or_default();
                                    view! {
                                        <textarea class="hc-textarea hc-textarea--code" rows="6" prop:value=json_str
                                            on:input=move |ev| {
                                                if let Ok(parsed) = serde_json::from_str::<Action>(&event_target_value(&ev)) { set.run(parsed); }
                                            } />
                                    }.into_any()
                                },
                            }}
                        </div>
                    }.into_any()
                },
            }
        }}
    }
}

/// Renders a nested list of plain `Action`s (not `RuleAction` — no enabled toggle).
/// Used inside Parallel, Conditional, RepeatUntil, etc.
#[component]
fn TypedNestedActionList(
    get_actions: Signal<Vec<Action>>,
    set_actions: Callback<Vec<Action>>,
) -> impl IntoView {
    view! {
        <div class="nested-action-list">
            {move || {
                let actions = get_actions.get();
                if actions.is_empty() {
                    view! { <p class="msg-muted" style="font-size:0.78rem">"No actions."</p> }.into_any()
                } else {
                    actions.into_iter().enumerate().map(|(i, _)| {
                        view! {
                            <div class="json-row nested-action-row">
                                <div class="json-row-controls">
                                    <span class="json-row-index">{i + 1}</span>
                                    <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline" title="Remove"
                                        on:click=move |_| {
                                            let mut acts = get_actions.get_untracked();
                                            acts.remove(i);
                                            set_actions.run(acts);
                                        }
                                    ><i class="ph ph-x" style="font-size:14px"></i></button>
                                </div>
                                <TypedActionEditor
                                    get=Signal::derive(move || get_actions.get().get(i).cloned().unwrap_or_else(|| default_action_typed("log_message")))
                                    set=Callback::new(move |a: Action| {
                                        let mut acts = get_actions.get_untracked();
                                        if i < acts.len() { acts[i] = a; }
                                        set_actions.run(acts);
                                    })
                                />
                            </div>
                        }
                    }).collect_view().into_any()
                }
            }}
            <button class="hc-btn hc-btn--sm hc-btn--outline" style="margin-top:0.25rem"
                on:click=move |_| {
                    let mut acts = get_actions.get_untracked();
                    acts.push(default_action_typed("log_message"));
                    set_actions.run(acts);
                }
            >"+ Add action"</button>
        </div>
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

// Icon names are Phosphor identifiers (slot into "ph ph-{name}" by the view).
const ACTION_CATEGORIES: &[(&str, &str, &str)] = &[
    ("device",      "Control device",  "devices"),
    ("conditional", "IF / ELSE",       "git-branch"),
    ("notify",      "Notify",          "bell"),
    ("mode",        "Set mode",        "sliders-horizontal"),
    ("timing",      "Delay / Wait",    "clock"),
    ("script",      "Script",          "code"),
    ("rule_ctrl",   "Rule control",    "robot"),
    ("more",        "More…",           "dots-three"),
];

// ── TypedDeviceStateBuilder ──────────────────────────────────────────────────
// Renders command dropdown + controls based on device capabilities.

fn device_commands(d: &DeviceState) -> Vec<(&'static str, &'static str)> {
    let mut cmds = Vec::new();
    let has = |k: &str| d.attributes.contains_key(k);
    let has_f = |k: &str| d.attributes.get(k).and_then(|v| v.as_f64()).is_some();
    if is_timer_device(d) {
        cmds.push(("timer_start","Start timer")); cmds.push(("timer_cancel","Cancel timer"));
        cmds.push(("timer_pause","Pause timer")); cmds.push(("timer_resume","Resume timer"));
        cmds.push(("timer_restart","Restart timer"));
        return cmds;
    }
    if is_scene_like(d) { cmds.push(("activate","Activate scene")); return cmds; }
    if has("on") { cmds.push(("on_true","Turn on")); cmds.push(("on_false","Turn off")); }
    if has_f("brightness_pct") { cmds.push(("brightness_pct","Set brightness")); }
    if has_f("color_temp") { cmds.push(("color_temp","Set color temperature")); }
    if has_f("position") { cmds.push(("position","Set position")); }
    if has("locked") { cmds.push(("lock","Lock")); cmds.push(("unlock","Unlock")); }
    if is_media_player(d) {
        cmds.push(("play","Play")); cmds.push(("pause","Pause")); cmds.push(("stop","Stop"));
        cmds.push(("next","Next track")); cmds.push(("prev","Previous track"));
        if has_f("volume") { cmds.push(("set_volume","Set volume")); }
        if has("muted") { cmds.push(("set_mute","Set mute")); }
        if has("shuffle") { cmds.push(("set_shuffle","Set shuffle")); }
        if !media_available_favorites(d).is_empty() { cmds.push(("play_favorite","Play favorite")); }
        if !media_available_playlists(d).is_empty() { cmds.push(("play_playlist","Play playlist")); }
    }
    cmds
}

fn detect_command(state: &Value) -> String {
    let obj = match state.as_object() { Some(o) => o, None => return String::new() };
    if let Some(cmd) = obj.get("command").and_then(|v| v.as_str()) {
        return match cmd { "start"=>"timer_start", "cancel"=>"timer_cancel", "pause"=>"timer_pause", "resume"=>"timer_resume", "restart"=>"timer_restart", _ => cmd }.to_string();
    }
    if obj.get("activate").and_then(|v| v.as_bool()) == Some(true) { return "activate".to_string(); }
    if let Some(act) = obj.get("action").and_then(|v| v.as_str()) {
        return match act { "play"=>"play", "pause"=>"pause", "stop"=>"stop", "next"=>"next", "previous"=>"prev", "set_volume"=>"set_volume", "set_mute"=>"set_mute", "set_shuffle"=>"set_shuffle", "play_favorite"=>"play_favorite", "play_playlist"=>"play_playlist", _ => act }.to_string();
    }
    if let Some(v) = obj.get("on") { return if v.as_bool()==Some(true) { "on_true" } else { "on_false" }.to_string(); }
    if obj.contains_key("locked") { return if obj["locked"].as_bool()==Some(true) { "lock" } else { "unlock" }.to_string(); }
    if obj.contains_key("brightness_pct") { return "brightness_pct".to_string(); }
    if obj.contains_key("color_temp") { return "color_temp".to_string(); }
    if obj.contains_key("position") { return "position".to_string(); }
    String::new()
}

fn command_to_state(cmd: &str, d: &DeviceState) -> Value {
    match cmd {
        "timer_start" => json!({"command":"start","duration_secs":300}), "timer_cancel" => json!({"command":"cancel"}),
        "timer_pause" => json!({"command":"pause"}), "timer_resume" => json!({"command":"resume"}), "timer_restart" => json!({"command":"restart"}),
        "activate" => json!({"activate":true}), "on_true" => json!({"on":true}), "on_false" => json!({"on":false}),
        "brightness_pct" => json!({"brightness_pct": d.attributes.get("brightness_pct").and_then(|v| v.as_i64()).unwrap_or(50)}),
        "color_temp" => json!({"color_temp": d.attributes.get("color_temp").and_then(|v| v.as_i64()).unwrap_or(2700)}),
        "position" => json!({"position": d.attributes.get("position").and_then(|v| v.as_i64()).unwrap_or(50)}),
        "lock" => json!({"locked":true}), "unlock" => json!({"locked":false}),
        "play" => json!({"action":"play"}), "pause" => json!({"action":"pause"}), "stop" => json!({"action":"stop"}),
        "next" => json!({"action":"next"}), "prev" => json!({"action":"previous"}),
        "set_volume" => json!({"action":"set_volume","volume": d.attributes.get("volume").and_then(|v| v.as_i64()).unwrap_or(20)}),
        "set_mute" => json!({"action":"set_mute","muted":false}), "set_shuffle" => json!({"action":"set_shuffle","shuffle":false}),
        "play_favorite" => json!({"action":"play_favorite","favorite":""}), "play_playlist" => json!({"action":"play_playlist","playlist":""}),
        _ => json!({}),
    }
}

/// Typed device state builder — renders command dropdown + controls.
/// Reads device capabilities from context and reads/writes the state JSON
/// inside a SetDeviceState or FadeDevice action.
///
/// Falls back to a raw-JSON editor when the device's capabilities don't
/// match any known command shape (e.g. plugin-specific commands like
/// Lutron `set_led`, `press_button`). Operators can also explicitly
/// toggle into JSON mode for any device when the typed editor can't
/// express what they need.
#[component]
fn TypedDeviceStateBuilder(
    get: Signal<Action>,
    set: Callback<Action>,
) -> impl IntoView {
    let devices = use_context::<RwSignal<Vec<DeviceState>>>().unwrap_or(RwSignal::new(vec![]));

    // Persistent UI state — kept at component scope so it survives re-renders
    // of the inner closure when the action signal updates.
    let edit_as_json: RwSignal<bool> = RwSignal::new(false);
    let raw_text: RwSignal<String> = RwSignal::new(String::new());
    let raw_error: RwSignal<Option<String>> = RwSignal::new(None);
    let last_synced: RwSignal<String> = RwSignal::new(String::new());

    // Read the device_id and state from the action
    let get_state_info = move || -> (String, Value) {
        match &get.get() {
            Action::SetDeviceState { device_id, state, .. } => (device_id.clone(), state.clone()),
            Action::FadeDevice { device_id, target, .. } => (device_id.clone(), target.clone()),
            _ => (String::new(), json!({})),
        }
    };

    // Write state back to the action
    let set_state = move |new_state: Value| {
        let mut a = get.get_untracked();
        match &mut a {
            Action::SetDeviceState { ref mut state, .. } => *state = new_state,
            Action::FadeDevice { ref mut target, .. } => *target = new_state,
            _ => {}
        }
        set.run(a);
    };

    // Sync raw_text from the action's state when the action changes from
    // outside (e.g. typed editor wrote a new command). We compare a canonical
    // pretty-printed form so user keystrokes don't trigger a self-loop.
    Effect::new(move |_| {
        let (_, state) = get_state_info();
        let canon = serde_json::to_string_pretty(&state).unwrap_or_default();
        if canon != last_synced.get_untracked() {
            raw_text.set(canon.clone());
            last_synced.set(canon);
            raw_error.set(None);
        }
    });

    view! {
        <div class="state-builder">
            {move || {
                let (device_id, state) = get_state_info();
                if device_id.is_empty() {
                    return view! { <p class="msg-muted" style="font-size:0.85rem">"Select a device first."</p> }.into_any();
                }
                let dev = devices.get().into_iter().find(|d| d.device_id == device_id);
                let d = match dev {
                    Some(d) => d,
                    None => return view! { <p class="msg-muted" style="font-size:0.85rem">"Device not found."</p> }.into_any(),
                };
                let cmds = device_commands(&d);
                let no_typed = cmds.is_empty();
                // Default to JSON mode when nothing typed is available.
                if no_typed && !edit_as_json.get_untracked() {
                    edit_as_json.set(true);
                }
                let json_mode = edit_as_json.get();

                // Always-available raw-JSON editor view. Used as fallback when
                // no typed commands match and as opt-in for plugin-specific
                // commands the typed editor can't express.
                let raw_editor = move || {
                    let banner = if no_typed {
                        view! {
                            <p class="msg-muted" style="font-size:0.82rem; margin: 0 0 0.4rem 0;">
                                "This device has no recognised typed commands. Send arbitrary command JSON the device understands — see the plugin's README for the supported shape (e.g. "
                                <code>{"{\"set_led\":{\"button\":1,\"state\":1}}"}</code>
                                ")."
                            </p>
                        }.into_any()
                    } else {
                        view! { <span /> }.into_any()
                    };
                    view! {
                        {banner}
                        <label class="field-label">"Command JSON"</label>
                        <textarea
                            class="hc-input"
                            style="width:100%; min-height: 7rem; font-family: ui-monospace, monospace; font-size: 0.85rem;"
                            prop:value=move || raw_text.get()
                            on:input=move |ev| {
                                let txt = event_target_value(&ev);
                                raw_text.set(txt.clone());
                                let trimmed = txt.trim();
                                let parsed: Result<Value, _> = if trimmed.is_empty() {
                                    Ok(json!({}))
                                } else {
                                    serde_json::from_str::<Value>(trimmed)
                                };
                                match parsed {
                                    Ok(v) => {
                                        raw_error.set(None);
                                        // Mark this as the last synced value so
                                        // the Effect doesn't bounce it back.
                                        last_synced.set(
                                            serde_json::to_string_pretty(&v).unwrap_or_default()
                                        );
                                        set_state(v);
                                    }
                                    Err(e) => raw_error.set(Some(e.to_string())),
                                }
                            }
                        />
                        {move || raw_error.get().map(|e| view! {
                            <p class="msg-error" style="font-size:0.78rem; margin-top:0.25rem; color: var(--hc-danger);">
                                {format!("JSON parse error: {e}")}
                            </p>
                        })}
                    }
                };

                if json_mode {
                    let toggle = if no_typed {
                        view! { <span /> }.into_any()
                    } else {
                        view! {
                            <button
                                type="button"
                                class="hc-btn hc-btn--sm hc-btn--outline"
                                style="margin-bottom: 0.4rem;"
                                on:click=move |_| edit_as_json.set(false)
                            >
                                "← Use typed editor"
                            </button>
                        }.into_any()
                    };
                    return view! {
                        <div>
                            {toggle}
                            {raw_editor()}
                        </div>
                    }.into_any();
                }

                let current_cmd = detect_command(&state);
                let d_for_change = d.clone();

                // Command selector
                let cmd_view = view! {
                    <label class="field-label">"Command"</label>
                    <select class="hc-select" on:change=move |ev| {
                        let cmd = event_target_value(&ev);
                        set_state(command_to_state(&cmd, &d_for_change));
                    }>
                        <option value="" disabled=true selected=current_cmd.is_empty()>"— Select command —"</option>
                        {cmds.iter().map(|(k, label)| {
                            let sel = *k == current_cmd;
                            view! { <option value=*k selected=sel>{*label}</option> }
                        }).collect_view()}
                    </select>
                };

                // Command-specific controls
                let control = match current_cmd.as_str() {
                    "brightness_pct" => {
                        let val = state["brightness_pct"].as_i64().unwrap_or(50);
                        view! {
                            <div class="control-row">
                                <span class="control-label">"Brightness"</span>
                                <div class="state-slider-row">
                                    <input type="range" class="state-slider" min="0" max="100" step="1" prop:value=val.to_string()
                                        on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<i64>() { set_state(json!({"brightness_pct": n})); }} />
                                    <span class="state-slider-val">{format!("{val}%")}</span>
                                </div>
                            </div>
                        }.into_any()
                    },
                    "color_temp" => {
                        let val = state["color_temp"].as_i64().unwrap_or(2700);
                        view! {
                            <div class="control-row">
                                <span class="control-label">"Color Temp"</span>
                                <div class="state-slider-row">
                                    <input type="range" class="state-slider" min="2000" max="6500" step="100" prop:value=val.to_string()
                                        on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<i64>() { set_state(json!({"color_temp": n})); }} />
                                    <span class="state-slider-val">{format!("{val}K")}</span>
                                </div>
                            </div>
                        }.into_any()
                    },
                    "position" => {
                        let val = state["position"].as_i64().unwrap_or(50);
                        view! {
                            <div class="control-row">
                                <span class="control-label">"Position"</span>
                                <div class="state-slider-row">
                                    <input type="range" class="state-slider" min="0" max="100" step="1" prop:value=val.to_string()
                                        on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<i64>() { set_state(json!({"position": n})); }} />
                                    <span class="state-slider-val">{format!("{val}%")}</span>
                                </div>
                            </div>
                        }.into_any()
                    },
                    "set_volume" => {
                        let val = state["volume"].as_i64().unwrap_or(20);
                        view! {
                            <div class="control-row">
                                <span class="control-label">"Volume"</span>
                                <div class="state-slider-row">
                                    <input type="range" class="state-slider" min="0" max="100" step="1" prop:value=val.to_string()
                                        on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<i64>() { set_state(json!({"action":"set_volume","volume": n})); }} />
                                    <span class="state-slider-val">{format!("{val}%")}</span>
                                </div>
                            </div>
                        }.into_any()
                    },
                    "timer_start" => {
                        let dur = state["duration_secs"].as_u64().unwrap_or(300);
                        let label = state["label"].as_str().unwrap_or("").to_string();
                        view! {
                            <div class="trigger-row-2">
                                <div>
                                    <label class="field-label">"Duration (seconds)"</label>
                                    <input type="number" class="hc-input hc-input--sm" prop:value=dur.to_string()
                                        on:input=move |ev| { if let Ok(n) = event_target_value(&ev).parse::<u64>() {
                                            let mut s = json!({"command":"start","duration_secs": n});
                                            let l = get.get_untracked();
                                            if let Action::SetDeviceState { state: ref st, .. } = l {
                                                if let Some(lbl) = st.get("label").and_then(|v| v.as_str()) { s["label"] = json!(lbl); }
                                            }
                                            set_state(s);
                                        }} />
                                </div>
                                <div>
                                    <label class="field-label">"Label (optional)"</label>
                                    <input type="text" class="hc-input hc-input--sm" prop:value=label
                                        on:input=move |ev| {
                                            let lbl = event_target_value(&ev);
                                            let d = get.get_untracked();
                                            let dur = if let Action::SetDeviceState { ref state, .. } = d { state["duration_secs"].as_u64().unwrap_or(300) } else { 300 };
                                            let mut s = json!({"command":"start","duration_secs": dur});
                                            if !lbl.is_empty() { s["label"] = json!(lbl); }
                                            set_state(s);
                                        } />
                                </div>
                            </div>
                        }.into_any()
                    },
                    "play_favorite" => {
                        let current = state["favorite"].as_str().unwrap_or("").to_string();
                        let favs = media_available_favorites(&d);
                        view! {
                            <label class="field-label">"Favorite"</label>
                            <select class="hc-select" on:change=move |ev| set_state(json!({"action":"play_favorite","favorite": event_target_value(&ev)}))>
                                <option value="" selected=current.is_empty()>"— Select —"</option>
                                {favs.iter().map(|f| { let sel = *f == current; view! { <option value=f.clone() selected=sel>{f.clone()}</option> }}).collect_view()}
                            </select>
                        }.into_any()
                    },
                    "play_playlist" => {
                        let current = state["playlist"].as_str().unwrap_or("").to_string();
                        let pls = media_available_playlists(&d);
                        view! {
                            <label class="field-label">"Playlist"</label>
                            <select class="hc-select" on:change=move |ev| set_state(json!({"action":"play_playlist","playlist": event_target_value(&ev)}))>
                                <option value="" selected=current.is_empty()>"— Select —"</option>
                                {pls.iter().map(|p| { let sel = *p == current; view! { <option value=p.clone() selected=sel>{p.clone()}</option> }}).collect_view()}
                            </select>
                        }.into_any()
                    },
                    _ => view! { <span /> }.into_any(),
                };

                let switch_to_json = view! {
                    <div style="margin-top: 0.5rem;">
                        <button
                            type="button"
                            class="hc-btn hc-btn--sm hc-btn--outline"
                            on:click=move |_| edit_as_json.set(true)
                        >
                            "Edit as JSON…"
                        </button>
                    </div>
                };

                view! { {cmd_view} {control} {switch_to_json} }.into_any()
            }}
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
