//! Shared WebSocket context — single connection for the lifetime of the session.
//!
//! Provides `WsContext` (injected via Leptos context) containing:
//!  - `devices: RwSignal<HashMap<String, DeviceState>>` — O(1) keyed device map
//!  - `status:  RwSignal<WsStatus>`                    — connection health
//!
//! ## Reconnect
//! On close/error a `setTimeout`-based exponential backoff schedules a retry
//! by incrementing `reconnect_trigger`, which causes the WS `Effect` to
//! re-run (`on_cleanup` closes the old socket first).
//! Delays: 1 s → 2 s → 5 s → 15 s (cap).
//!
//! ## Event-type filter
//! No server-side filter is applied — the shared WS forwards every event
//! type. Pages that only consume typed updates (devices, scenes, plugins)
//! match on `WsEvent` and ignore anything they don't care about; the
//! Activity page subscribes to `WsContext.latest_event` to receive the
//! raw stream. `PluginHeartbeat` events are dropped server-side unless
//! explicitly requested via `&type=`.
//!
//! ## Closure lifetime note
//! `on_open / on_msg / on_err / on_close` are `forget()`-ed each reconnect.
//! Each closure is small (≤ a few hundred bytes).  Reconnects are infrequent
//! (network disruptions only), so the cumulative overhead is negligible for a
//! home automation session.

use crate::auth::events_ws_url;
use crate::models::{DeviceChange, DeviceState};
use chrono::{DateTime, Utc};
use leptos::prelude::*;
use serde::Deserialize;
use serde_json::Value;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

// ── Connection status ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsStatus {
    Connecting,
    Live,
    Disconnected,
}

// ── Context ───────────────────────────────────────────────────────────────────

/// Shared WebSocket context.  Provide once in `NavShell`; consume anywhere via
/// `use_ws()`.
#[derive(Clone, Copy)]
pub struct WsContext {
    /// Live device map keyed by `device_id` — O(1) lookup on WS events.
    /// Pages seed it via the REST snapshot; WS events keep it current.
    pub devices: RwSignal<HashMap<String, DeviceState>>,
    pub scene_activations: RwSignal<HashMap<String, DateTime<Utc>>>,
    pub status: RwSignal<WsStatus>,
    /// Live plugin map keyed by `plugin_id` — updated by REST seed + WS events.
    pub plugins: RwSignal<HashMap<String, crate::models::PluginInfo>>,
    /// Latest raw event payload + monotonic seq. Set on every WS frame so the
    /// Activity page can subscribe instead of opening its own `/events/stream`.
    /// Pages that only care about typed updates (devices/scenes/plugins) keep
    /// reading the existing maps and never touch this signal — no spurious
    /// re-renders on unrelated event types.
    pub latest_event: RwSignal<Option<(u64, Value)>>,
}

impl WsContext {
    pub fn new() -> Self {
        Self {
            devices: RwSignal::new(HashMap::new()),
            scene_activations: RwSignal::new(HashMap::new()),
            status: RwSignal::new(WsStatus::Connecting),
            plugins: RwSignal::new(HashMap::new()),
            latest_event: RwSignal::new(None),
        }
    }
}

pub fn use_ws() -> WsContext {
    use_context::<WsContext>().expect("WsContext not in context — mount inside NavShell")
}

// ── Internal event subset ─────────────────────────────────────────────────────
//
// Only the two event types forwarded by the server (see URL filter below).

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WsEvent {
    DeviceStateChanged {
        device_id: String,
        current: HashMap<String, Value>,
        #[serde(default)]
        change: Option<DeviceChange>,
    },
    DeviceAvailabilityChanged {
        device_id: String,
        available: bool,
    },
    SceneActivated {
        timestamp: DateTime<Utc>,
        scene_id: String,
        #[allow(dead_code)]
        scene_name: String,
    },
    PluginRegistered {
        plugin_id: String,
    },
    PluginStatusChanged {
        plugin_id: String,
        status: String,
    },
    #[serde(other)]
    Other,
}

// ── Reconnect backoff ─────────────────────────────────────────────────────────

fn backoff_ms(attempt: u32) -> i32 {
    match attempt {
        0 => 1_000,
        1 => 2_000,
        2 => 5_000,
        _ => 15_000,
    }
}

/// Schedule a reconnect by incrementing `trigger` after `backoff_ms(attempt)`.
/// Uses a one-shot `setTimeout`; the closure is small and `forget()`-ed.
fn schedule_reconnect(trigger: RwSignal<u32>, attempt: u32) {
    let delay = backoff_ms(attempt);
    let cb = Closure::<dyn FnMut()>::new(move || {
        trigger.update(|n| *n = attempt.saturating_add(1));
    });
    let _ = web_sys::window().and_then(|w| {
        w.set_timeout_with_callback_and_timeout_and_arguments_0(cb.as_ref().unchecked_ref(), delay)
            .ok()
    });
    cb.forget();
}

// ── Lifecycle ─────────────────────────────────────────────────────────────────

/// Mount the WS Effect.  Call once inside `NavShell` after `provide_context`.
///
/// The Effect tracks `auth_token` (changes at login/logout) and an internal
/// `reconnect_trigger` (incremented on each retry).  `on_cleanup` closes the
/// socket before the Effect re-runs, so there is never more than one socket
/// open at a time.
pub fn mount_ws(ctx: WsContext, auth_token: RwSignal<Option<String>>) {
    // Re-seed the shared device + plugin maps from REST every time the WS
    // transitions to Live. Covers three cases:
    //   1. First connect after login — pages have data immediately.
    //   2. Reconnect after a disconnect (network blip, server restart).
    //      Any state changes that happened during the gap are reconciled.
    //   3. Reconnect after a tokio broadcast lag on the server. The lag
    //      doesn't disconnect the WS, but a periodic reseed via reconnect
    //      will eventually catch up; for the lag-without-disconnect case
    //      we accept that until the next real disconnect.
    //
    // To avoid clobbering a live WS event that happened to arrive between
    // the REST snapshot and the response, we only replace a device entry
    // when REST's `last_seen` is at least as new as the in-memory copy.
    // PluginInfo doesn't have the same race because plugin events are
    // rare; we just replace.
    Effect::new(move |_| {
        if ctx.status.get() != WsStatus::Live {
            return;
        }
        let token = match auth_token.get_untracked() {
            Some(t) => t,
            None => return,
        };
        let ws = ctx;
        leptos::task::spawn_local(async move {
            if let Ok(list) = crate::api::fetch_devices(&token).await {
                ws.devices.update(|m| {
                    for d in list {
                        let should_replace = m
                            .get(&d.device_id)
                            .map(|existing| d.last_seen >= existing.last_seen)
                            .unwrap_or(true);
                        if should_replace {
                            m.insert(d.device_id.clone(), d);
                        }
                    }
                });
            }
            if let Ok(list) = crate::api::fetch_plugins(&token).await {
                ws.plugins.update(|m| {
                    for p in list {
                        m.insert(p.plugin_id.clone(), p);
                    }
                });
            }
        });
    });

    let reconnect_trigger: RwSignal<u32> = RwSignal::new(0);
    // Monotonic seq for `latest_event`. Survives reconnects so subscribers
    // (Activity page) can keep dedup state across socket churn.
    let event_seq: RwSignal<u64> = RwSignal::new(0);

    Effect::new(move |_| {
        let attempt = reconnect_trigger.get();

        let token = match auth_token.get() {
            Some(t) => t,
            None => return,
        };

        ctx.status.set(WsStatus::Connecting);

        // No server-side type filter — the Activity page consumes the full
        // event stream via `ctx.latest_event`. Other pages match on `WsEvent`
        // and silently drop unrelated types. PluginHeartbeat is filtered
        // server-side unless explicitly requested.
        let url = events_ws_url(&token);

        let ws = match web_sys::WebSocket::new(&url) {
            Ok(ws) => ws,
            Err(_) => {
                ctx.status.set(WsStatus::Disconnected);
                schedule_reconnect(reconnect_trigger, attempt);
                return;
            }
        };

        // onopen ──────────────────────────────────────────────────────────────
        let on_open = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            ctx.status.set(WsStatus::Live);
        });
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        on_open.forget();

        // onmessage ───────────────────────────────────────────────────────────
        let on_msg =
            Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
                let text = match ev.data().as_string() {
                    Some(s) => s,
                    None => return,
                };
                // Parse once into a generic Value for the Activity-page
                // subscriber, then deserialize into the typed enum for the
                // device/scene/plugin dispatch below.
                let raw: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => return,
                };
                let seq = event_seq.get_untracked().wrapping_add(1);
                event_seq.set(seq);
                ctx.latest_event.set(Some((seq, raw.clone())));

                let event: WsEvent = match serde_json::from_value(raw) {
                    Ok(e) => e,
                    Err(_) => return,
                };
                match event {
                    WsEvent::DeviceStateChanged {
                        device_id,
                        current,
                        change,
                    } => {
                        ctx.devices.update(|m| {
                            if let Some(d) = m.get_mut(&device_id) {
                                d.attributes = current;
                                if let Some(ch) = change {
                                    d.last_seen = Some(ch.changed_at);
                                    d.last_change = Some(ch);
                                } else {
                                    d.last_seen = Some(chrono::Utc::now());
                                }
                            }
                        });
                    }
                    WsEvent::DeviceAvailabilityChanged {
                        device_id,
                        available,
                    } => {
                        ctx.devices.update(|m| {
                            if let Some(d) = m.get_mut(&device_id) {
                                d.available = available;
                            }
                        });
                    }
                    WsEvent::SceneActivated {
                        timestamp,
                        scene_id,
                        ..
                    } => {
                        ctx.scene_activations.update(|m| {
                            m.insert(scene_id, timestamp);
                        });
                    }
                    WsEvent::PluginRegistered { plugin_id } => {
                        ctx.plugins.update(|m| {
                            if let Some(p) = m.get_mut(&plugin_id) {
                                p.status = "active".into();
                            }
                        });
                    }
                    WsEvent::PluginStatusChanged { plugin_id, status } => {
                        ctx.plugins.update(|m| {
                            if let Some(p) = m.get_mut(&plugin_id) {
                                p.status = status;
                            }
                        });
                    }
                    WsEvent::Other => {}
                }
            });
        ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
        on_msg.forget();

        // onerror / onclose — schedule reconnect ──────────────────────────────
        //
        // IMPORTANT: these handlers must NOT write to any signal that is read
        // by THIS Effect.  `reconnect_trigger` is only ever *written* here (via
        // `schedule_reconnect`) and *read* once at the top of the Effect — the
        // feedback loop is broken because the `set_timeout` fires outside the
        // reactive scope.
        //
        // Network-error closes fire both `onerror` and `onclose`. The shared
        // `scheduled` flag ensures only the first one schedules a reconnect.

        let scheduled = Rc::new(Cell::new(false));

        let on_err = {
            let scheduled = scheduled.clone();
            Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                if scheduled.replace(true) {
                    return;
                }
                ctx.status.set(WsStatus::Disconnected);
                schedule_reconnect(reconnect_trigger, attempt);
            })
        };
        ws.set_onerror(Some(on_err.as_ref().unchecked_ref()));
        on_err.forget();

        let on_close = {
            let scheduled = scheduled.clone();
            Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |_| {
                if scheduled.replace(true) {
                    return;
                }
                ctx.status.set(WsStatus::Disconnected);
                schedule_reconnect(reconnect_trigger, attempt);
            })
        };
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        on_close.forget();

        // Cleanup: close socket before Effect re-runs (token change or retry).
        on_cleanup(move || {
            let _ = ws.close();
        });
    });
}
