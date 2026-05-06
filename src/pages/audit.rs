//! Audit log viewer — admin-only page.
//!
//! Layout: compact filter bar, day-grouped list of expandable rows.
//! Denied/error events are color-coded; successes stay quiet. Detail JSON
//! is hidden by default and revealed with a click on the row.

use crate::api::{fetch_audit, AuditFilter};
use crate::auth::use_auth;
use chrono::{DateTime, Datelike, Utc};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::Value;

const PAGE_SIZE: u32 = 100;

#[component]
pub fn AuditPage() -> impl IntoView {
    let auth = use_auth();

    // Filter state.
    let actor_type = RwSignal::new(String::new());
    let event_type = RwSignal::new(String::new());
    let target_kind = RwSignal::new(String::new());
    let target_id = RwSignal::new(String::new());
    let result_filter = RwSignal::new(String::new());
    let from = RwSignal::new(String::new());
    let to_field = RwSignal::new(String::new());
    let offset = RwSignal::new(0u32);

    // Data state.
    let rows: RwSignal<Vec<Value>> = RwSignal::new(Vec::new());
    let loading = RwSignal::new(false);
    let error: RwSignal<Option<String>> = RwSignal::new(None);
    let total_returned = RwSignal::new(0u32);

    let load = move || {
        let token = auth.token_str().unwrap_or_default();
        let filter = AuditFilter {
            actor_id: None,
            actor_type: none_if_blank(&actor_type.get()),
            event_type: none_if_blank(&event_type.get()),
            target_kind: none_if_blank(&target_kind.get()),
            target_id: none_if_blank(&target_id.get()),
            result: none_if_blank(&result_filter.get()),
            from: none_if_blank(&from.get()).map(|s| to_rfc3339(&s)),
            to: none_if_blank(&to_field.get()).map(|s| to_rfc3339(&s)),
            limit: PAGE_SIZE,
            offset: offset.get(),
        };
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match fetch_audit(&token, &filter).await {
                Ok(list) => {
                    total_returned.set(list.len() as u32);
                    rows.set(list);
                }
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    };

    Effect::new(move |_| {
        load();
    });

    let do_search = {
        let load = load.clone();
        move |_| {
            offset.set(0);
            load();
        }
    };
    let reset_filters = {
        let load = load.clone();
        move |_| {
            actor_type.set(String::new());
            event_type.set(String::new());
            target_kind.set(String::new());
            target_id.set(String::new());
            result_filter.set(String::new());
            from.set(String::new());
            to_field.set(String::new());
            offset.set(0);
            load();
        }
    };
    let prev_page = {
        let load = load.clone();
        move |_| {
            let cur = offset.get();
            if cur > 0 {
                offset.set(cur.saturating_sub(PAGE_SIZE));
                load();
            }
        }
    };
    let next_page = {
        let load = load.clone();
        move |_| {
            if total_returned.get() >= PAGE_SIZE {
                offset.set(offset.get() + PAGE_SIZE);
                load();
            }
        }
    };

    view! {
        <section class="audit-view">
            <header class="audit-header">
                <div class="audit-title">
                    <i class="ph ph-list-checks audit-title-icon"></i>
                    <div>
                        <h1>"Audit log"</h1>
                        <p class="audit-subtitle">
                            "Who did what, when. Admin-scoped. Retention controlled by "
                            <code>"[auth].audit_retention_days"</code>"."
                        </p>
                    </div>
                </div>
            </header>

            <div class="audit-filter-card">
                <div class="audit-filter-row">
                    <div class="audit-field">
                        <i class="ph ph-user audit-field-icon"></i>
                        <select
                            class="audit-input"
                            prop:value=move || actor_type.get()
                            on:change=move |ev| actor_type.set(event_target_value(&ev))
                        >
                            <option value="">"Any actor"</option>
                            <option value="user">"User"</option>
                            <option value="api_key">"API key"</option>
                            <option value="local_admin">"Local admin"</option>
                            <option value="ip_whitelist">"IP whitelist"</option>
                            <option value="anonymous">"Anonymous"</option>
                            <option value="system">"System"</option>
                        </select>
                    </div>

                    <div class="audit-field audit-field--wide">
                        <i class="ph ph-lightning audit-field-icon"></i>
                        <input
                            type="text"
                            class="audit-input"
                            placeholder="Event — e.g. auth.login, api_key.created"
                            prop:value=move || event_type.get()
                            on:input=move |ev| event_type.set(event_target_value(&ev))
                        />
                    </div>

                    <div class="audit-field">
                        <i class="ph ph-tag audit-field-icon"></i>
                        <input
                            type="text"
                            class="audit-input"
                            placeholder="Target kind"
                            prop:value=move || target_kind.get()
                            on:input=move |ev| target_kind.set(event_target_value(&ev))
                        />
                    </div>

                    <div class="audit-field audit-field--wide">
                        <i class="ph ph-fingerprint audit-field-icon"></i>
                        <input
                            type="text"
                            class="audit-input"
                            placeholder="Target ID"
                            prop:value=move || target_id.get()
                            on:input=move |ev| target_id.set(event_target_value(&ev))
                        />
                    </div>

                    <div class="audit-field">
                        <i class="ph ph-check-circle audit-field-icon"></i>
                        <select
                            class="audit-input"
                            prop:value=move || result_filter.get()
                            on:change=move |ev| result_filter.set(event_target_value(&ev))
                        >
                            <option value="">"Any result"</option>
                            <option value="success">"Success"</option>
                            <option value="denied">"Denied"</option>
                            <option value="error">"Error"</option>
                        </select>
                    </div>
                </div>

                <div class="audit-filter-row audit-filter-row--sub">
                    <div class="audit-field">
                        <span class="audit-field-label">"From"</span>
                        <input
                            type="datetime-local"
                            class="audit-input"
                            prop:value=move || from.get()
                            on:input=move |ev| from.set(event_target_value(&ev))
                        />
                    </div>
                    <div class="audit-field">
                        <span class="audit-field-label">"To"</span>
                        <input
                            type="datetime-local"
                            class="audit-input"
                            prop:value=move || to_field.get()
                            on:input=move |ev| to_field.set(event_target_value(&ev))
                        />
                    </div>

                    <div class="audit-filter-actions">
                        <button
                            class="hc-btn hc-btn--sm hc-btn--primary"
                            on:click=do_search
                            disabled=move || loading.get()
                        >
                            <i class="ph ph-magnifying-glass" style="font-size:16px"></i>
                            "Search"
                        </button>
                        <button
                            class="hc-btn hc-btn--sm hc-btn--outline"
                            on:click=reset_filters
                        >
                            "Reset"
                        </button>
                    </div>
                </div>
            </div>

            {move || error.get().map(|e| view! {
                <div class="hc-alert hc-alert--error">{e}</div>
            })}

            <div class="audit-stream">
                {move || {
                    if loading.get() && rows.get().is_empty() {
                        view! {
                            <div class="audit-empty">
                                <i class="ph ph-hourglass audit-empty-icon"></i>
                                <p>"Loading…"</p>
                            </div>
                        }.into_any()
                    } else if rows.get().is_empty() {
                        view! {
                            <div class="audit-empty">
                                <i class="ph ph-magnifying-glass-minus audit-empty-icon"></i>
                                <p>"No matching events."</p>
                                <p class="audit-empty-hint">
                                    "Try widening the date range or clearing a filter."
                                </p>
                            </div>
                        }.into_any()
                    } else {
                        let grouped = group_by_day(rows.get());
                        view! {
                            <>
                                {grouped.into_iter().map(|(label, day_rows)| view! {
                                    <div class="audit-day">
                                        <span class="audit-day-label">{label}</span>
                                        <span class="audit-day-count">{format!("{} events", day_rows.len())}</span>
                                    </div>
                                    {day_rows.into_iter().map(render_entry).collect_view()}
                                }).collect_view()}
                            </>
                        }.into_any()
                    }
                }}
            </div>

            <footer class="audit-pager">
                <span class="audit-pager-range">
                    {move || {
                        if total_returned.get() == 0 {
                            String::new()
                        } else {
                            format!(
                                "{}–{}",
                                offset.get() + 1,
                                offset.get() + total_returned.get()
                            )
                        }
                    }}
                </span>
                <div class="audit-pager-buttons">
                    <button
                        class="hc-btn hc-btn--sm hc-btn--outline"
                        on:click=prev_page
                        disabled=move || offset.get() == 0 || loading.get()
                    >
                        <i class="ph ph-caret-left" style="font-size:16px"></i>
                        "Prev"
                    </button>
                    <button
                        class="hc-btn hc-btn--sm hc-btn--outline"
                        on:click=next_page
                        disabled=move || total_returned.get() < PAGE_SIZE || loading.get()
                    >
                        "Next"
                        <i class="ph ph-caret-right" style="font-size:16px"></i>
                    </button>
                </div>
            </footer>
        </section>
    }
}

// ─── Rendering helpers ───────────────────────────────────────────────────────

fn render_entry(r: Value) -> impl IntoView {
    let ts_raw = r
        .get("ts")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let parsed = DateTime::parse_from_rfc3339(&ts_raw).ok();
    let ts_local = parsed
        .map(|d| crate::tz::fmt_time(&d.with_timezone(&Utc)))
        .unwrap_or_default();

    let result = r
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let event = r
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let actor = r
        .get("actor_label")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let actor_type = r
        .get("actor_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let target_kind = r
        .get("target_kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let target_id = r
        .get("target_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let detail = r.get("detail").cloned().unwrap_or(Value::Null);
    let detail_pretty = if detail.is_null()
        || (detail.is_object() && detail.as_object().map(|m| m.is_empty()).unwrap_or(false))
    {
        None
    } else {
        Some(serde_json::to_string_pretty(&detail).unwrap_or_default())
    };

    let has_target = !target_kind.is_empty() || !target_id.is_empty();
    let entry_class = format!("audit-entry audit-entry--{}", result);
    let pill_class = format!("audit-pill audit-pill--{}", result);
    let actor_icon = actor_icon_for(&actor_type);

    view! {
        <details class=entry_class>
            <summary class="audit-entry-summary">
                <span class="audit-entry-time">{ts_local}</span>
                <span class=pill_class title=result.clone()>{result.clone()}</span>
                <span class="audit-entry-event">{event}</span>
                <span class="audit-entry-actor">
                    <i class={format!("ph ph-{} audit-entry-actor-icon", actor_icon)}></i>
                    {actor}
                </span>
                {has_target.then(|| view! {
                    <span class="audit-chip">
                        {(!target_kind.is_empty()).then(|| view! {
                            <span class="audit-chip-kind">{target_kind.clone()}</span>
                        })}
                        {(!target_id.is_empty()).then(|| view! {
                            <span class="audit-chip-id">{target_id.clone()}</span>
                        })}
                    </span>
                })}
                {detail_pretty.is_some().then(|| view! {
                    <i class="ph ph-caret-down audit-entry-chev"></i>
                })}
            </summary>
            {detail_pretty.map(|p| view! {
                <div class="audit-entry-detail">
                    <pre>{p}</pre>
                </div>
            })}
        </details>
    }
}

/// Returns Phosphor icon names (slot into "ph ph-{name}" by the view).
fn actor_icon_for(kind: &str) -> &'static str {
    match kind {
        "user" => "user",
        "api_key" => "key",
        "local_admin" => "shield",
        "ip_whitelist" => "globe",
        "anonymous" => "question",
        "system" => "gear",
        _ => "question",
    }
}

/// Group rows by their local-timezone day, returning (label, rows) pairs in
/// server-returned order (already ts DESC). Labels: "Today", "Yesterday",
/// otherwise "Apr 22, 2026".
fn group_by_day(rows: Vec<Value>) -> Vec<(String, Vec<Value>)> {
    if rows.is_empty() {
        return Vec::new();
    }
    let today = crate::tz::today();
    let mut out: Vec<(String, Vec<Value>)> = Vec::new();
    let mut current: Option<String> = None;
    for r in rows {
        let ts_raw = r.get("ts").and_then(|v| v.as_str()).unwrap_or("");
        let day_label = DateTime::parse_from_rfc3339(ts_raw)
            .ok()
            .map(|d| crate::tz::local_date(&d.with_timezone(&Utc)))
            .map(|d| {
                if d == today {
                    "Today".to_string()
                } else if d == today.pred_opt().unwrap_or(today) {
                    "Yesterday".to_string()
                } else {
                    format!("{} {}, {}", month_short(d.month()), d.day(), d.year())
                }
            })
            .unwrap_or_else(|| "Unknown date".to_string());
        if current.as_ref() != Some(&day_label) {
            current = Some(day_label.clone());
            out.push((day_label, Vec::new()));
        }
        out.last_mut().unwrap().1.push(r);
    }
    out
}

fn month_short(m: u32) -> &'static str {
    [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][(m as usize).saturating_sub(1).min(11)]
}

fn none_if_blank(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Normalise a `datetime-local` value (`2026-04-22T14:30`) to RFC3339. Assume
/// UTC for lack of a better signal.
fn to_rfc3339(s: &str) -> String {
    let _ = Utc::now(); // touch to pin import
    if s.contains('Z') || s.contains('+') {
        s.to_string()
    } else if s.len() == 16 {
        format!("{s}:00Z")
    } else {
        format!("{s}Z")
    }
}
