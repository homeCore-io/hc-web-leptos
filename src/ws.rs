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
//! The WS URL includes `&type=device_state_changed,device_availability_changed`
//! so the server only forwards the two event types the client uses.  All other
//! event types (`rule_fired`, `scene_activated`, …) are dropped server-side.
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
use std::collections::HashMap;
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
}

impl WsContext {
    pub fn new() -> Self {
        Self {
            devices: RwSignal::new(HashMap::new()),
            scene_activations: RwSignal::new(HashMap::new()),
            status: RwSignal::new(WsStatus::Connecting),
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
        w.set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            delay,
        )
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
    let reconnect_trigger: RwSignal<u32> = RwSignal::new(0);

    Effect::new(move |_| {
        let attempt = reconnect_trigger.get();

        let token = match auth_token.get() {
            Some(t) => t,
            None => return,
        };

        ctx.status.set(WsStatus::Connecting);

        // Append server-side event-type filter — reduces fanout to only what
        // the client actually handles.
        let base = events_ws_url(&token);
        let url =
            format!("{base}&type=device_state_changed,device_availability_changed,scene_activated");

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
                let event: WsEvent = match serde_json::from_str(&text) {
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

        let on_err = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            ctx.status.set(WsStatus::Disconnected);
            schedule_reconnect(reconnect_trigger, attempt);
        });
        ws.set_onerror(Some(on_err.as_ref().unchecked_ref()));
        on_err.forget();

        let on_close = Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |_| {
            ctx.status.set(WsStatus::Disconnected);
            schedule_reconnect(reconnect_trigger, attempt);
        });
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        on_close.forget();

        // Cleanup: close socket before Effect re-runs (token change or retry).
        on_cleanup(move || {
            let _ = ws.close();
        });
    });
}
