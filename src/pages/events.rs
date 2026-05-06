//! Events page — unified activity timeline merging events + logs.
//!
//! Architecture:
//!   - REST `GET /events` seeds the initial event history
//!   - Live event updates flow via the shared `WsContext.latest_event`
//!     signal — the page no longer opens its own `/events/stream`
//!     connection (WS-3, 0.1.2). This avoids a redundant socket on top
//!     of the session-wide WS in `crate::ws::mount_ws`.
//!   - WS `/logs/stream?history=N` still opens its own connection
//!     (different endpoint, separate ring buffer of historical lines).
//!   - Both streams merge into a single `RwSignal<Vec<ActivityEntry>>`
//!   - 500-entry buffer cap, newest first
//!   - Pause/resume: when paused, incoming entries buffer silently

use crate::api::fetch_events;
use crate::auth::{logs_ws_url, use_auth};
use crate::pages::shared::{
    MultiSelectDropdown, ResetFiltersButton, SearchField, SortDir, SortDirToggle,
};
use crate::ws::use_ws;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::Value;
use std::collections::HashSet;
use wasm_bindgen::prelude::*;

const MAX_ENTRIES: usize = 500;

// ── Activity entry ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
struct ActivityEntry {
    id: String,
    timestamp: String,
    source: &'static str, // "event" or "log"
    kind: String,         // event type or log target
    severity: String,     // "info", "warn", "error", "debug", "trace"
    summary: String,
    device_id: Option<String>,
    rule_id: Option<String>,
    raw: Value,
}

fn normalize_event(seq: u64, ev: &Value) -> ActivityEntry {
    let t = ev["type"].as_str().unwrap_or("unknown");
    let severity = match t {
        "action_failed" | "system_alert" => "error",
        _ => "info",
    };
    // Use device_name from event if available, fall back to device_id.
    let did = ev["device_id"].as_str().unwrap_or("");
    let dn = ev["device_name"].as_str().unwrap_or(did);
    let summary = match t {
        "device_state_changed" => {
            let changed = ev["changed"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            format!("{dn}: {changed}")
        }
        "device_availability_changed" => {
            let avail = if ev["available"].as_bool() == Some(true) {
                "online"
            } else {
                "offline"
            };
            format!("{dn} → {avail}")
        }
        "rule_fired" => {
            let name = ev["rule_name"].as_str().unwrap_or("");
            let ms = ev["elapsed_ms"]
                .as_u64()
                .map(|n| format!(" ({n}ms)"))
                .unwrap_or_default();
            format!("Rule fired: {name}{ms}")
        }
        "rule_evaluation_failed" => {
            let name = ev["rule_name"].as_str().unwrap_or("");
            let reason = ev["reason"].as_str().unwrap_or("");
            format!("Rule skipped: {name} — {reason}")
        }
        "action_failed" => {
            let name = ev["rule_name"].as_str().unwrap_or("");
            let action = ev["action_type"].as_str().unwrap_or("");
            let err = ev["error"].as_str().unwrap_or("");
            format!("Action failed: {name}/{action} — {err}")
        }
        "scene_activated" => {
            let name = ev["scene_name"].as_str().unwrap_or("");
            format!("Scene: {name}")
        }
        "mode_changed" => {
            let name = ev["mode_name"]
                .as_str()
                .unwrap_or(ev["mode_id"].as_str().unwrap_or(""));
            let on = if ev["on"].as_bool() == Some(true) {
                "on"
            } else {
                "off"
            };
            format!("Mode: {name} → {on}")
        }
        "device_command_sent" => {
            let cmd_dn = ev["device_name"].as_str().unwrap_or(did);
            format!("Command → {cmd_dn}")
        }
        "timer_state_changed" => {
            let tid = ev["timer_id"].as_str().unwrap_or("");
            let tname = ev["timer_name"].as_str().unwrap_or(tid);
            let state = ev["state"].as_str().unwrap_or("");
            format!("Timer {tname}: {state}")
        }
        "plugin_registered" => format!(
            "Plugin registered: {}",
            ev["plugin_id"].as_str().unwrap_or("")
        ),
        "plugin_offline" => format!("Plugin offline: {}", ev["plugin_id"].as_str().unwrap_or("")),
        "plugin_heartbeat" => format!(
            "Plugin heartbeat: {}",
            ev["plugin_id"].as_str().unwrap_or("")
        ),
        "custom" => format!("Custom: {}", ev["event_type"].as_str().unwrap_or("")),
        "system_alert" => format!("Alert: {}", ev["message"].as_str().unwrap_or("")),
        _ => t.to_string(),
    };

    ActivityEntry {
        id: format!("e-{seq}"),
        timestamp: ev["timestamp"].as_str().unwrap_or("").to_string(),
        source: "event",
        kind: t.to_string(),
        severity: severity.to_string(),
        summary,
        device_id: ev["device_id"].as_str().map(str::to_string),
        rule_id: ev["rule_id"].as_str().map(str::to_string),
        raw: ev.clone(),
    }
}

fn normalize_log(counter: u64, log: &Value) -> ActivityEntry {
    let level = log["level"].as_str().unwrap_or("INFO").to_lowercase();
    let target = log["target"].as_str().unwrap_or("").to_string();
    let message = log["message"].as_str().unwrap_or("").to_string();

    ActivityEntry {
        id: format!("l-{counter}"),
        timestamp: log["timestamp"].as_str().unwrap_or("").to_string(),
        source: "log",
        kind: target,
        severity: level,
        summary: message,
        device_id: log["fields"]["device_id"].as_str().map(str::to_string),
        rule_id: log["fields"]["rule_id"].as_str().map(str::to_string),
        raw: log.clone(),
    }
}

// ── Severity helpers ─────────────────────────────────────────────────────────

fn severity_class(sev: &str) -> &'static str {
    match sev {
        "error" => "activity-sev--error",
        "warn" => "activity-sev--warn",
        "debug" | "trace" => "activity-sev--debug",
        _ => "activity-sev--info",
    }
}

fn source_class(src: &str) -> &'static str {
    match src {
        "event" => "activity-src--event",
        "log" => "activity-src--log",
        _ => "",
    }
}

fn format_time(ts: &str) -> String {
    // Parse server UTC timestamp and render HH:MM:SS in the configured
    // home zone (set at app boot from /system/status). Browser-local
    // would also be a reasonable choice, but for a home-automation
    // dashboard "what time was this *at home*" matches the operator's
    // mental model better than "what time was this on my phone."
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        return crate::tz::fmt_time(&dt.with_timezone(&chrono::Utc));
    }
    // Fallback: extract from raw string when the input isn't a valid
    // RFC-3339 timestamp (defensive — server should always emit valid
    // timestamps, but ignoring the error here would render an empty
    // cell which is worse than the raw string).
    if let Some(t_pos) = ts.find('T') {
        let time_part = &ts[t_pos + 1..];
        time_part.get(..8).unwrap_or(time_part).to_string()
    } else {
        ts.to_string()
    }
}

// ── Page ─────────────────────────────────────────────────────────────────────

#[component]
pub fn EventsPage() -> impl IntoView {
    let auth = use_auth();
    let ws = use_ws();

    // ── State ────────────────────────────────────────────────────────────────
    let entries: RwSignal<Vec<ActivityEntry>> = RwSignal::new(vec![]);
    let paused = RwSignal::new(false);
    let _auto_scroll = RwSignal::new(true);
    let search = RwSignal::new(String::new());
    let source_filter: RwSignal<HashSet<String>> = RwSignal::new(HashSet::new());
    let severity_filter: RwSignal<HashSet<String>> = RwSignal::new(HashSet::new());
    let type_filter: RwSignal<HashSet<String>> = RwSignal::new(HashSet::new());

    let sort_dir = RwSignal::new(SortDir::Desc);
    let ws_status: RwSignal<&'static str> = RwSignal::new("connecting");
    let log_counter: RwSignal<u64> = RwSignal::new(0);
    let selected_entry: RwSignal<Option<String>> = RwSignal::new(None);

    // Pause buffer — entries received while paused, flushed on resume.
    let pause_buffer: RwSignal<Vec<ActivityEntry>> = RwSignal::new(vec![]);

    // ── Add entry helper ─────────────────────────────────────────────────────
    let add_entry = move |entry: ActivityEntry| {
        if paused.get_untracked() {
            pause_buffer.update(|buf| {
                buf.push(entry);
                if buf.len() > MAX_ENTRIES {
                    buf.remove(0);
                }
            });
        } else {
            entries.update(|list| {
                list.insert(0, entry);
                if list.len() > MAX_ENTRIES {
                    list.pop();
                }
            });
        }
    };

    // ── Resume: flush buffer ─────────────────────────────────────────────────
    let resume = move || {
        paused.set(false);
        let buf = pause_buffer.get_untracked();
        if !buf.is_empty() {
            entries.update(|list| {
                for e in buf {
                    list.insert(0, e);
                }
                list.truncate(MAX_ENTRIES);
            });
            pause_buffer.set(vec![]);
        }
    };

    // Device name lookup — uses the shared WsContext device map.
    let ws_devices = ws.devices;

    // ── Load initial events via REST + connect logs WS ─────────────────────
    //
    // Live event updates come from the shared `WsContext.latest_event`
    // signal, populated by `crate::ws::mount_ws`. This page no longer
    // opens its own `/events/stream` socket (WS-3). The logs WS is still
    // opened here — different endpoint, separate ring of historical lines.
    Effect::new(move |_| {
        let token = match auth.token.get() {
            Some(t) => t,
            None => return,
        };

        // Fetch event history via REST
        {
            let token = token.clone();
            spawn_local(async move {
                if let Ok(data) = fetch_events(&token, 200).await {
                    let events: Vec<ActivityEntry> = data
                        .iter()
                        .enumerate()
                        .map(|(i, ev)| {
                            let seq = ev["seq"].as_u64().unwrap_or(i as u64);
                            normalize_event(seq, &ev["event"])
                        })
                        .collect();
                    entries.update(|list| {
                        for e in events {
                            list.push(e);
                        }
                        list.truncate(MAX_ENTRIES);
                    });
                }
            });
        }

        // Connect logs WS (with history replay) — unchanged.
        {
            let url = logs_ws_url(&token, 200);
            if let Ok(ws) = web_sys::WebSocket::new(&url) {
                let on_msg = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
                    move |ev: web_sys::MessageEvent| {
                        let text = match ev.data().as_string() {
                            Some(s) => s,
                            None => return,
                        };
                        let parsed: Value = match serde_json::from_str(&text) {
                            Ok(v) => v,
                            Err(_) => return,
                        };
                        let c = log_counter.get_untracked();
                        log_counter.set(c + 1);
                        add_entry(normalize_log(c, &parsed));
                    },
                );
                ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
                on_msg.forget();

                on_cleanup(move || {
                    let _ = ws.close();
                });
            }
        }
    });

    // Reactively follow the shared WS status so the page header reflects
    // it after the initial load.
    Effect::new(move |_| {
        ws_status.set(match ws.status.get() {
            crate::ws::WsStatus::Live => "live",
            crate::ws::WsStatus::Connecting => "connecting",
            crate::ws::WsStatus::Disconnected => "disconnected",
        });
    });

    // Subscribe to the shared event stream. Track the last seq we've
    // already rendered so we don't double-add when other signals on the
    // page trigger this Effect.
    let last_seq: RwSignal<u64> = RwSignal::new(0);
    Effect::new(move |_| {
        let Some((seq, ref raw)) = ws.latest_event.get() else {
            return;
        };
        if seq <= last_seq.get_untracked() {
            return;
        }
        last_seq.set(seq);
        add_entry(normalize_event(seq, raw));
    });

    // ── Dynamic filter options ───────────────────────────────────────────────
    let source_options: Signal<Vec<(String, String)>> = Signal::derive(move || {
        vec![
            ("event".into(), "Events".into()),
            ("log".into(), "Logs".into()),
        ]
    });
    let severity_options: Signal<Vec<(String, String)>> = Signal::derive(move || {
        vec![
            ("error".into(), "Error".into()),
            ("warn".into(), "Warning".into()),
            ("info".into(), "Info".into()),
            ("debug".into(), "Debug".into()),
        ]
    });
    let type_options: Signal<Vec<(String, String)>> = Signal::derive(move || {
        let mut types: Vec<String> = entries
            .get()
            .iter()
            .map(|e| e.kind.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        types.sort();
        types
            .into_iter()
            .map(|t| {
                let label = t.replace('_', " ");
                (t, label)
            })
            .collect()
    });

    // ── Filtered list ────────────────────────────────────────────────────────
    let filtered = Memo::new(move |_| {
        let q = search.get().to_lowercase();
        let src = source_filter.get();
        let sev = severity_filter.get();
        let typ = type_filter.get();
        let dir = sort_dir.get();

        let mut list: Vec<ActivityEntry> = entries
            .get()
            .into_iter()
            .filter(|e| {
                if !q.is_empty()
                    && !e.summary.to_lowercase().contains(&q)
                    && !e.kind.to_lowercase().contains(&q)
                {
                    return false;
                }
                if !src.is_empty() && !src.contains(e.source) {
                    return false;
                }
                if !sev.is_empty() && !sev.contains(&e.severity) {
                    return false;
                }
                if !typ.is_empty() && !typ.contains(&e.kind) {
                    return false;
                }
                true
            })
            .collect();

        if dir == SortDir::Asc {
            list.reverse();
        }
        list
    });

    view! {
        <div class="events-page">
            // ── Page heading ─────────────────────────────────────────────────
            <div class="page-heading">
                <div>
                    <h1>"Activity"</h1>
                    <p>
                        {move || {
                            let total = entries.get().len();
                            let f = filtered.get().len();
                            let status = ws_status.get();
                            let status_label = match status {
                                "live" => "Live",
                                "disconnected" => "Disconnected",
                                _ => "Connecting",
                            };
                            if f == total { format!("{total} entries · {status_label}") }
                            else { format!("{f} / {total} entries · {status_label}") }
                        }}
                    </p>
                </div>
                <div class="events-controls">
                    <button
                        class="hc-btn hc-btn--sm"
                        class:hc-btn--primary=move || !paused.get()
                        class:hc-btn--outline=move || paused.get()
                        on:click=move |_| {
                            if paused.get_untracked() { resume(); } else { paused.set(true); }
                        }
                    >
                        {move || if paused.get() {
                            format!("Resume ({})", pause_buffer.get().len())
                        } else {
                            "Pause".to_string()
                        }}
                    </button>
                    <button class="hc-btn hc-btn--sm hc-btn--outline"
                        on:click=move |_| entries.set(vec![])
                    >"Clear"</button>
                </div>
            </div>

            // ── Filter toolbar ───────────────────────────────────────────────
            <div class="filter-panel panel">
                <div class="filter-bar">
                    <SearchField search=search placeholder="Search activity…" />
                    <SortDirToggle sort_dir />
                </div>

                <div class="filter-body">
                    <div class="filter-multisel-row">
                        <MultiSelectDropdown
                            label="sources"
                            placeholder="All sources"
                            options=source_options
                            selected=source_filter
                        />
                        <MultiSelectDropdown
                            label="severity"
                            placeholder="All levels"
                            options=severity_options
                            selected=severity_filter
                        />
                        <MultiSelectDropdown
                            label="types"
                            placeholder="All types"
                            options=type_options
                            selected=type_filter
                        />
                        <ResetFiltersButton on_reset=Callback::new(move |_| {
                            search.set(String::new());
                            source_filter.set(HashSet::new());
                            severity_filter.set(HashSet::new());
                            type_filter.set(HashSet::new());
                        }) />
                    </div>
                </div>
            </div>

            // ── Timeline ─────────────────────────────────────────────────────
            <div class="activity-timeline">
                {move || {
                    let list = filtered.get();
                    // Subscribe to device map so names update when devices load
                    let devs = ws_devices.get();
                    if list.is_empty() {
                        view! {
                            <div class="hc-empty">
                                <i class="ph ph-pulse hc-empty__icon"></i>
                                <div class="hc-empty__title">"No activity"</div>
                                <p class="hc-empty__body">
                                    "Events and logs from devices, plugins, and the rule engine \
                                     stream here as they happen. Adjust filters or wait for \
                                     activity to flow in."
                                </p>
                            </div>
                        }.into_any()
                    } else {
                        list.into_iter().map(|entry| {
                            let id = entry.id.clone();
                            let id_for_click = id.clone();
                            let id_sel = id.clone();
                            let id_sel2 = id.clone();
                            let sev_cls = severity_class(&entry.severity);
                            let src_cls = source_class(entry.source);
                            let time_str = format_time(&entry.timestamp);
                            let kind_label = entry.kind.replace('_', " ");
                            let raw = entry.raw.clone();

                            // For log entries, resolve device IDs in the summary
                            // (events already have device_name from core).
                            let summary = if entry.source == "log" {
                                let mut s = entry.summary.clone();
                                for (did, dev) in devs.iter() {
                                    if s.contains(did.as_str()) {
                                        s = s.replace(did.as_str(), &dev.name);
                                    }
                                }
                                s
                            } else {
                                entry.summary.clone()
                            };
                            let severity = entry.severity.clone();
                            let source = entry.source;

                            view! {
                                <div class="activity-entry">
                                    <div
                                        class=format!("activity-row {sev_cls}")
                                        class:activity-row--selected=move || selected_entry.get().as_deref() == Some(&id_sel)
                                        on:click=move |_| {
                                            selected_entry.update(|sel| {
                                                if sel.as_deref() == Some(&id_for_click) { *sel = None; }
                                                else { *sel = Some(id_for_click.clone()); }
                                            });
                                        }
                                    >
                                        <span class="activity-time">{time_str}</span>
                                        <span class=format!("activity-source-badge {src_cls}")>{source}</span>
                                        <span class="activity-severity-badge">{severity}</span>
                                        <span class="activity-kind">{kind_label}</span>
                                        <span class="activity-summary">{summary}</span>
                                    </div>
                                    {move || (selected_entry.get().as_deref() == Some(&id_sel2)).then(|| {
                                        let pretty = serde_json::to_string_pretty(&raw).unwrap_or_default();
                                        view! {
                                            <div class="activity-detail">{pretty}</div>
                                        }
                                    })}
                                </div>
                            }
                        }).collect_view().into_any()
                    }
                }}
            </div>
        </div>
    }
}
