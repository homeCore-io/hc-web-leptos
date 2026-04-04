//! Events page — unified activity timeline merging events + logs.
//!
//! Architecture:
//!   - REST `GET /events` seeds the initial event history
//!   - WS `/events/stream` provides live event updates
//!   - WS `/logs/stream?history=N` provides log history + live log updates
//!   - Both streams merge into a single `RwSignal<Vec<ActivityEntry>>`
//!   - 500-entry buffer cap, newest first
//!   - Pause/resume: when paused, incoming entries buffer silently

use crate::api::fetch_events;
use crate::auth::{events_ws_url, logs_ws_url, use_auth};
use crate::ws::use_ws;
use crate::pages::shared::{
    FilterToggleButton, MultiSelectDropdown, ResetFiltersButton, SearchField,
    SortDir, SortDirToggle,
};
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
    kind: String,          // event type or log target
    severity: String,      // "info", "warn", "error", "debug", "trace"
    summary: String,
    device_id: Option<String>,
    rule_id: Option<String>,
    raw: Value,
}

fn normalize_event(seq: u64, ev: &Value) -> ActivityEntry {
    let t = ev["type"].as_str().unwrap_or("unknown");
    let severity = match t {
        "action_failed" | "system_alert" => "error",
        "rule_evaluation_failed" => "warn",
        _ => "info",
    };
    // Summary uses {DEVICE_ID} placeholders — resolved at render time.
    let did = ev["device_id"].as_str().unwrap_or("");
    let summary = match t {
        "device_state_changed" => {
            let changed = ev["changed"].as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            format!("{did}: {changed}")
        }
        "device_availability_changed" => {
            let avail = if ev["available"].as_bool() == Some(true) { "online" } else { "offline" };
            format!("{did} → {avail}")
        }
        "rule_fired" => {
            let name = ev["rule_name"].as_str().unwrap_or("");
            let ms = ev["elapsed_ms"].as_u64().map(|n| format!(" ({n}ms)")).unwrap_or_default();
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
            let name = ev["mode_name"].as_str().unwrap_or(ev["mode_id"].as_str().unwrap_or(""));
            let on = if ev["on"].as_bool() == Some(true) { "on" } else { "off" };
            format!("Mode: {name} → {on}")
        }
        "device_command_sent" => format!("Command → {did}"),
        "timer_state_changed" => {
            let tid = ev["timer_id"].as_str().unwrap_or("");
            let state = ev["state"].as_str().unwrap_or("");
            format!("Timer {tid}: {state}")
        }
        "plugin_registered" => format!("Plugin registered: {}", ev["plugin_id"].as_str().unwrap_or("")),
        "plugin_offline" => format!("Plugin offline: {}", ev["plugin_id"].as_str().unwrap_or("")),
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
        "warn"  => "activity-sev--warn",
        "debug" | "trace" => "activity-sev--debug",
        _ => "activity-sev--info",
    }
}

fn source_class(src: &str) -> &'static str {
    match src {
        "event" => "activity-src--event",
        "log"   => "activity-src--log",
        _ => "",
    }
}

fn format_time(ts: &str) -> String {
    // Parse UTC timestamp and convert to local time via JS Date.
    if let Some(_window) = web_sys::window() {
        let js_date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(ts));
        if js_date.get_time().is_finite() {
            let h = js_date.get_hours();
            let m = js_date.get_minutes();
            let s = js_date.get_seconds();
            return format!("{h:02}:{m:02}:{s:02}");
        }
    }
    // Fallback: extract from raw string
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
    let filter_open = RwSignal::new(false);
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
                if buf.len() > MAX_ENTRIES { buf.remove(0); }
            });
        } else {
            entries.update(|list| {
                list.insert(0, entry);
                if list.len() > MAX_ENTRIES { list.pop(); }
            });
        }
    };

    // ── Resume: flush buffer ─────────────────────────────────────────────────
    let resume = move || {
        paused.set(false);
        let buf = pause_buffer.get_untracked();
        if !buf.is_empty() {
            entries.update(|list| {
                for e in buf { list.insert(0, e); }
                list.truncate(MAX_ENTRIES);
            });
            pause_buffer.set(vec![]);
        }
    };

    // Device name lookup — uses the shared WsContext device map.
    let ws_devices = ws.devices;

    // ── Load initial events + connect WS ─────────────────────────────────────
    Effect::new(move |_| {
        let token = match auth.token.get() { Some(t) => t, None => return };

        // Fetch event history via REST
        {
            let token = token.clone();
            spawn_local(async move {
                if let Ok(data) = fetch_events(&token, 200).await {
                    let events: Vec<ActivityEntry> = data.iter().enumerate()
                        .map(|(i, ev)| {
                            let seq = ev["seq"].as_u64().unwrap_or(i as u64);
                            normalize_event(seq, &ev["event"])
                        })
                        .collect();
                    entries.update(|list| {
                        for e in events { list.push(e); }
                        list.truncate(MAX_ENTRIES);
                    });
                }
            });
        }

        // Connect events WS
        {
            let token = token.clone();
            let url = events_ws_url(&token);
            let event_counter: RwSignal<u64> = RwSignal::new(100_000);
            if let Ok(ws) = web_sys::WebSocket::new(&url) {
                let on_open = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                    ws_status.set("live");
                });
                ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
                on_open.forget();

                let on_msg = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
                    let text = match ev.data().as_string() { Some(s) => s, None => return };
                    let parsed: Value = match serde_json::from_str(&text) { Ok(v) => v, Err(_) => return };
                    let seq = event_counter.get_untracked();
                    event_counter.set(seq + 1);
                    add_entry(normalize_event(seq, &parsed));
                });
                ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
                on_msg.forget();

                let on_err = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                    ws_status.set("disconnected");
                });
                ws.set_onerror(Some(on_err.as_ref().unchecked_ref()));
                on_err.forget();

                on_cleanup(move || { let _ = ws.close(); });
            }
        }

        // Connect logs WS (with history replay)
        {
            let url = logs_ws_url(&token, 200);
            if let Ok(ws) = web_sys::WebSocket::new(&url) {
                let on_msg = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
                    let text = match ev.data().as_string() { Some(s) => s, None => return };
                    let parsed: Value = match serde_json::from_str(&text) { Ok(v) => v, Err(_) => return };
                    let c = log_counter.get_untracked();
                    log_counter.set(c + 1);
                    add_entry(normalize_log(c, &parsed));
                });
                ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
                on_msg.forget();

                on_cleanup(move || { let _ = ws.close(); });
            }
        }
    });

    // ── Dynamic filter options ───────────────────────────────────────────────
    let source_options: Signal<Vec<(String, String)>> = Signal::derive(move || {
        vec![("event".into(), "Events".into()), ("log".into(), "Logs".into())]
    });
    let severity_options: Signal<Vec<(String, String)>> = Signal::derive(move || {
        vec![
            ("error".into(), "Error".into()), ("warn".into(), "Warning".into()),
            ("info".into(), "Info".into()), ("debug".into(), "Debug".into()),
        ]
    });
    let type_options: Signal<Vec<(String, String)>> = Signal::derive(move || {
        let mut types: Vec<String> = entries.get().iter()
            .map(|e| e.kind.clone())
            .collect::<HashSet<_>>()
            .into_iter().collect();
        types.sort();
        types.into_iter().map(|t| {
            let label = t.replace('_', " ");
            (t, label)
        }).collect()
    });

    // ── Filtered list ────────────────────────────────────────────────────────
    let filtered = Memo::new(move |_| {
        let q = search.get().to_lowercase();
        let src = source_filter.get();
        let sev = severity_filter.get();
        let typ = type_filter.get();
        let dir = sort_dir.get();

        let mut list: Vec<ActivityEntry> = entries.get().into_iter()
            .filter(|e| {
                if !q.is_empty() && !e.summary.to_lowercase().contains(&q) && !e.kind.to_lowercase().contains(&q) {
                    return false;
                }
                if !src.is_empty() && !src.contains(e.source) { return false; }
                if !sev.is_empty() && !sev.contains(&e.severity) { return false; }
                if !typ.is_empty() && !typ.contains(&e.kind) { return false; }
                true
            })
            .collect();

        if dir == SortDir::Asc { list.reverse(); }
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
                    <FilterToggleButton filter_open />
                </div>

                {move || filter_open.get().then(|| view! {
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
                })}
            </div>

            // ── Timeline ─────────────────────────────────────────────────────
            <div class="activity-timeline">
                {move || {
                    let list = filtered.get();
                    if list.is_empty() {
                        view! {
                            <p class="msg-muted" style="padding:1rem">"No activity entries."</p>
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
                            let raw_summary = entry.summary.clone();
                            let raw = entry.raw.clone();

                            // Resolve device IDs → names at render time (device map is now populated)
                            let summary = {
                                let devs = ws_devices.get();
                                let mut s = raw_summary;
                                for (did, dev) in devs.iter() {
                                    if s.contains(did.as_str()) {
                                        s = s.replace(did.as_str(), &dev.name);
                                    }
                                }
                                s
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
