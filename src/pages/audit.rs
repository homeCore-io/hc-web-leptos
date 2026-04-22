//! Audit log viewer — admin-only page.
//!
//! Filter by actor, event type, target, result, and date range. Paginated.
//! Uses the server's GET /api/v1/audit endpoint under the hood.

use crate::api::{fetch_audit, AuditFilter};
use crate::auth::use_auth;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::Value;

const PAGE_SIZE: u32 = 100;

#[component]
pub fn AuditPage() -> impl IntoView {
    let auth = use_auth();

    // Filter inputs.
    let actor_type = RwSignal::new(String::new());
    let event_type = RwSignal::new(String::new());
    let target_kind = RwSignal::new(String::new());
    let target_id = RwSignal::new(String::new());
    let result_filter = RwSignal::new(String::new());
    let from = RwSignal::new(String::new());
    let to_field = RwSignal::new(String::new());
    let offset = RwSignal::new(0u32);

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

    // Load on mount.
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
            offset.set(cur.saturating_sub(PAGE_SIZE));
            load();
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
        <section class="page">
            <div class="page-header">
                <h1>"Audit Log"</h1>
                <p class="page-subtitle">
                    "Who did what, when. Admin-scoped. Retention is controlled by "
                    <code>"[auth].audit_retention_days"</code>" (default 365)."
                </p>
            </div>

            <div class="filter-bar">
                <label>
                    "Actor type"
                    <select
                        prop:value=move || actor_type.get()
                        on:change=move |ev| actor_type.set(event_target_value(&ev))
                    >
                        <option value="">"(any)"</option>
                        <option value="user">"user"</option>
                        <option value="api_key">"api_key"</option>
                        <option value="local_admin">"local_admin"</option>
                        <option value="ip_whitelist">"ip_whitelist"</option>
                        <option value="anonymous">"anonymous"</option>
                        <option value="system">"system"</option>
                    </select>
                </label>
                <label>
                    "Event"
                    <input
                        type="text"
                        placeholder="auth.login, api_key.created, …"
                        prop:value=move || event_type.get()
                        on:input=move |ev| event_type.set(event_target_value(&ev))
                    />
                </label>
                <label>
                    "Target kind"
                    <input
                        type="text"
                        placeholder="user, api_key, rule, device"
                        prop:value=move || target_kind.get()
                        on:input=move |ev| target_kind.set(event_target_value(&ev))
                    />
                </label>
                <label>
                    "Target ID"
                    <input
                        type="text"
                        prop:value=move || target_id.get()
                        on:input=move |ev| target_id.set(event_target_value(&ev))
                    />
                </label>
                <label>
                    "Result"
                    <select
                        prop:value=move || result_filter.get()
                        on:change=move |ev| result_filter.set(event_target_value(&ev))
                    >
                        <option value="">"(any)"</option>
                        <option value="success">"success"</option>
                        <option value="denied">"denied"</option>
                        <option value="error">"error"</option>
                    </select>
                </label>
                <label>
                    "From"
                    <input
                        type="datetime-local"
                        prop:value=move || from.get()
                        on:input=move |ev| from.set(event_target_value(&ev))
                    />
                </label>
                <label>
                    "To"
                    <input
                        type="datetime-local"
                        prop:value=move || to_field.get()
                        on:input=move |ev| to_field.set(event_target_value(&ev))
                    />
                </label>

                <div class="filter-bar-actions">
                    <button class="hc-btn hc-btn--sm" on:click=do_search disabled=move || loading.get()>"Search"</button>
                    <button class="hc-btn hc-btn--sm hc-btn--outline" on:click=reset_filters>"Reset"</button>
                </div>
            </div>

            {move || error.get().map(|e| view! {
                <div class="hc-alert hc-alert--error">{e}</div>
            })}

            <div class="audit-table-wrap">
                <table class="audit-table">
                    <thead>
                        <tr>
                            <th>"Timestamp"</th>
                            <th>"Result"</th>
                            <th>"Event"</th>
                            <th>"Actor"</th>
                            <th>"Target"</th>
                            <th>"Detail"</th>
                        </tr>
                    </thead>
                    <tbody>
                        {move || {
                            if rows.get().is_empty() {
                                let msg = if loading.get() { "Loading…" } else { "(no matching events)" };
                                view! {
                                    <tr><td colspan="6" class="audit-empty">{msg}</td></tr>
                                }.into_any()
                            } else {
                                view! {
                                    <>
                                        {rows.get().into_iter().map(|r| {
                                            let ts = r.get("ts").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let result = r.get("result").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let event = r.get("event_type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let actor = r.get("actor_label").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let target = format!(
                                                "{} {}",
                                                r.get("target_kind").and_then(|v| v.as_str()).unwrap_or(""),
                                                r.get("target_id").and_then(|v| v.as_str()).unwrap_or("")
                                            );
                                            let detail = r.get("detail").map(|d| d.to_string()).unwrap_or_default();
                                            let ts_short = ts.splitn(2, '.').next().unwrap_or(&ts).replace('T', " ");
                                            let result_cls = format!("audit-pill audit-pill--{}", result);
                                            view! {
                                                <tr>
                                                    <td class="audit-ts">{ts_short}</td>
                                                    <td><span class=result_cls>{result}</span></td>
                                                    <td class="audit-event">{event}</td>
                                                    <td class="audit-actor">{actor}</td>
                                                    <td class="audit-target">{target}</td>
                                                    <td class="audit-detail"><code>{detail}</code></td>
                                                </tr>
                                            }
                                        }).collect_view()}
                                    </>
                                }.into_any()
                            }
                        }}
                    </tbody>
                </table>
            </div>

            <div class="page-pager">
                <button
                    class="hc-btn hc-btn--sm"
                    on:click=prev_page
                    disabled=move || offset.get() == 0 || loading.get()
                >
                    "← Prev"
                </button>
                <span class="glue-meta">
                    {move || format!(
                        "showing rows {}–{}",
                        offset.get() + 1,
                        offset.get() + total_returned.get()
                    )}
                </span>
                <button
                    class="hc-btn hc-btn--sm"
                    on:click=next_page
                    disabled=move || total_returned.get() < PAGE_SIZE || loading.get()
                >
                    "Next →"
                </button>
            </div>
        </section>
    }
}

fn none_if_blank(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Convert a `datetime-local` input value (e.g. `2026-04-22T14:30`) to an
/// RFC3339 timestamp. The server parses both; we add `:00Z` as a suffix so
/// it's unambiguous if the browser omitted seconds.
fn to_rfc3339(s: &str) -> String {
    if s.contains('Z') || s.contains('+') {
        s.to_string()
    } else if s.len() == 16 {
        format!("{s}:00Z")
    } else {
        format!("{s}Z")
    }
}
