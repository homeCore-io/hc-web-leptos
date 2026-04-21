//! ThermostatCard — dedicated view for `device_type = "thermostat"` rendered
//! inside the DeviceDetailPage. Includes live status, setpoint/mode/hysteresis
//! controls, a collapsible configuration section, diagnostics, and a history
//! chart with time-range selection.

use crate::api::{fetch_device_history_range, send_plugin_command, set_device_state};
use crate::auth::use_auth;
use crate::models::{thermostat_temperature_unit, DeviceState, HistoryEntry};
use crate::ws::use_ws;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::rc::Rc;

#[component]
pub fn ThermostatCard(device: DeviceState) -> impl IntoView {
    let auth = use_auth();
    let ws = use_ws();
    let device_id = device.device_id.clone();

    let history: RwSignal<Vec<HistoryEntry>> = RwSignal::new(Vec::new());
    let history_range_hours: RwSignal<u32> = RwSignal::new(24);
    let history_loading = RwSignal::new(false);
    let busy = RwSignal::new(false);

    // Command helper — publishes to the device cmd topic via /devices/:id/state.
    // Wrapped in Rc so it can be cloned into many on-click / on-change handlers.
    let device_id_cmd = device_id.clone();
    let auth_for_cmd = auth.clone();
    let send_cmd: Rc<dyn Fn(Value)> = Rc::new(move |cmd: Value| {
        let Some(token) = auth_for_cmd.token.get_untracked() else { return };
        let id = device_id_cmd.clone();
        busy.set(true);
        spawn_local(async move {
            let _ = set_device_state(&token, &id, &cmd).await;
            busy.set(false);
        });
    });

    // Delete confirmation state + handler.
    let confirm_delete = RwSignal::new(false);
    let delete_busy = RwSignal::new(false);
    let plugin_id = device.plugin_id.clone();
    let therm_id = device_id
        .strip_prefix("thermostat_")
        .unwrap_or(&device_id)
        .to_string();
    let auth_for_del = auth.clone();
    let nav = use_navigate();
    let do_delete = {
        let plugin_id = plugin_id.clone();
        let therm_id = therm_id.clone();
        let nav = nav.clone();
        move || {
            let Some(token) = auth_for_del.token.get_untracked() else { return };
            let pid = plugin_id.clone();
            let tid = therm_id.clone();
            let nav = nav.clone();
            delete_busy.set(true);
            spawn_local(async move {
                let _ = send_plugin_command(&token, &pid, "remove_thermostat", json!({"id": tid}))
                    .await;
                delete_busy.set(false);
                nav("/devices", Default::default());
            });
        }
    };

    // History fetch effect — refires on range change.
    let device_id_hist = device_id.clone();
    Effect::new(move |_| {
        let Some(token) = auth.token.get() else { return };
        let hours = history_range_hours.get();
        let did = device_id_hist.clone();
        let now_ms = js_sys::Date::now();
        let from_ms = now_ms - (hours as f64 * 3_600_000.0);
        let from_iso = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(from_ms))
            .to_iso_string()
            .as_string()
            .unwrap_or_default();
        history_loading.set(true);
        spawn_local(async move {
            if let Ok(h) =
                fetch_device_history_range(&token, &did, Some(&from_iso), None, None, 5000).await
            {
                history.set(h);
            }
            history_loading.set(false);
        });
    });

    // All of the below are captured from the current device attributes. Since
    // the device signal is updated on every ws push, the view re-renders.
    let attrs = device.attributes.clone();

    let sp = attrs.get("setpoint").and_then(|v| v.as_f64()).unwrap_or(70.0);
    let hyst = attrs.get("hysteresis").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let mode = attrs
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("off")
        .to_string();
    let call = attrs
        .get("call_for")
        .and_then(|v| v.as_str())
        .unwrap_or("idle")
        .to_string();
    let act_on = attrs
        .get("actuator_state")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let temp = attrs.get("current_temperature").and_then(|v| v.as_f64());
    let pending = attrs
        .get("pending_call")
        .and_then(|v| v.as_str())
        .map(String::from);
    let lockout_until = attrs
        .get("lockout_until")
        .and_then(|v| v.as_str())
        .map(String::from);
    let actuator_id = attrs
        .get("actuator_device_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let min_on = attrs
        .get("min_on_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let min_off = attrs
        .get("min_off_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let sensor_ids: Vec<String> = attrs
        .get("sensor_ids")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let sensor_attr = attrs
        .get("sensor_attribute")
        .and_then(|v| v.as_str())
        .unwrap_or("temperature")
        .to_string();
    let aggregation = attrs
        .get("aggregation")
        .and_then(|v| v.as_str())
        .unwrap_or("average")
        .to_string();
    let actuator_error = attrs.get("actuator_last_error").and_then(|v| {
        v.as_object().map(|o| {
            let ts = o.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
            let msg = o.get("message").and_then(|v| v.as_str()).unwrap_or("");
            (ts.to_string(), msg.to_string())
        })
    });

    // Diagnostics.
    let devmap = ws.devices.get();
    let mut diagnostics: Vec<String> = Vec::new();
    if sensor_ids.is_empty() {
        diagnostics.push("No temperature sensors configured.".into());
    }
    let missing_sensors: Vec<&String> = sensor_ids
        .iter()
        .filter(|id| !devmap.contains_key(*id))
        .collect();
    if !missing_sensors.is_empty() {
        diagnostics.push(format!(
            "Sensor(s) not found: {}",
            missing_sensors
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if actuator_id.is_empty() {
        diagnostics.push("No actuator device configured — commands will be dropped.".into());
    } else if !devmap.contains_key(&actuator_id) {
        diagnostics.push(format!("Actuator `{actuator_id}` not found."));
    }
    if call == "stale" {
        diagnostics.push("All sensor readings unavailable.".into());
    }
    if let Some((_, msg)) = &actuator_error {
        diagnostics.push(format!("Last actuator publish failed: {msg}"));
    }

    let temp_unit = thermostat_temperature_unit(&device, &devmap);
    let fmt_temp = |t: f64| -> String {
        match temp_unit {
            Some(unit) => format!("{t:.1} {unit}"),
            None => format!("{t:.1}°"),
        }
    };
    let fmt_deadband = |d: f64| -> String {
        match temp_unit {
            Some(unit) => format!("±{d:.1} {unit}"),
            None => format!("±{d:.1}°"),
        }
    };
    let temp_str = temp.map(fmt_temp).unwrap_or_else(|| "—".to_string());
    let setpoint_str = fmt_temp(sp);
    let deadband_str = fmt_deadband(hyst / 2.0);
    let pill_class = match call.as_str() {
        "heat" => "pill-heat",
        "cool" => "pill-cool",
        "stale" => "pill-stale",
        _ => "pill-idle",
    };

    let lockout_remaining_secs = move || -> Option<u64> {
        let ts = lockout_until.as_deref()?;
        let until_ms = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(ts)).get_time();
        if !until_ms.is_finite() {
            return None;
        }
        let now_ms = js_sys::Date::now();
        let remaining = ((until_ms - now_ms) / 1000.0).round();
        if remaining > 0.0 {
            Some(remaining as u64)
        } else {
            None
        }
    };

    let sp_minus = {
        let send_cmd = send_cmd.clone();
        move |_| send_cmd(json!({"command":"set_setpoint","value": sp - 0.5}))
    };
    let sp_plus = {
        let send_cmd = send_cmd.clone();
        move |_| send_cmd(json!({"command":"set_setpoint","value": sp + 0.5}))
    };
    let set_heat = {
        let send_cmd = send_cmd.clone();
        move |_| send_cmd(json!({"command":"set_mode","value":"heat"}))
    };
    let set_cool = {
        let send_cmd = send_cmd.clone();
        move |_| send_cmd(json!({"command":"set_mode","value":"cool"}))
    };
    let set_off = {
        let send_cmd = send_cmd.clone();
        move |_| send_cmd(json!({"command":"set_mode","value":"off"}))
    };
    let mode_heat = mode == "heat";
    let mode_cool = mode == "cool";
    let mode_off = mode == "off";

    view! {
        <section class="detail-card thermostat-card">
            {(!diagnostics.is_empty()).then(|| view! {
                <div class="thermostat-diagnostics">
                    <strong>"Attention:"</strong>
                    <ul>
                        {diagnostics.iter().map(|d| view! { <li>{d.clone()}</li> }).collect_view()}
                    </ul>
                </div>
            })}

            <div class="thermostat-readout">
                <div class="thermostat-temp">{temp_str}</div>
                <div class="thermostat-status">
                    <span class=format!("pill {pill_class}")>{call.clone()}</span>
                    {pending.clone().map(|p| view! {
                        <span class="pill pill-pending">{format!("pending {p}")}</span>
                    })}
                    <span class="glue-meta">
                        {format!(" actuator: {}", if act_on { "ON" } else { "off" })}
                    </span>
                </div>
                <div class="thermostat-setpoint">
                    "target " <strong>{setpoint_str.clone()}</strong>
                    <span class="glue-meta">{format!(" (deadband {deadband_str})")}</span>
                </div>
            </div>

            <div class="thermostat-controls">
                <div class="glue-ctrl-row">
                    <span class="field-label">"Setpoint"</span>
                    <div class="glue-ctrl-btns">
                        <button class="hc-btn hc-btn--sm" disabled=move || busy.get()
                            on:click=sp_minus>"−"</button>
                        <span class="glue-ctrl-value">{setpoint_str.clone()}</span>
                        <button class="hc-btn hc-btn--sm" disabled=move || busy.get()
                            on:click=sp_plus>"+"</button>
                    </div>
                </div>

                <div class="glue-ctrl-row">
                    <span class="field-label">"Mode"</span>
                    <div class="toggle-group">
                        <button class:active=mode_heat disabled=move || busy.get()
                            on:click=set_heat>"Heat"</button>
                        <button class:active=mode_cool disabled=move || busy.get()
                            on:click=set_cool>"Cool"</button>
                        <button class:active=mode_off disabled=move || busy.get()
                            on:click=set_off>"Off"</button>
                    </div>
                </div>

                <div class="glue-ctrl-row">
                    <span class="field-label">"Hysteresis"</span>
                    <input type="range" class="state-slider"
                        min="0" max="5" step="0.5"
                        prop:value=hyst.to_string()
                        on:change={
                            let send_cmd = send_cmd.clone();
                            move |ev| {
                                if let Ok(n) = event_target_value(&ev).parse::<f64>() {
                                    send_cmd(json!({"command":"set_hysteresis","value": n}));
                                }
                            }
                        }
                    />
                    <span class="glue-ctrl-value">{deadband_str.clone()}</span>
                </div>
            </div>

            <div class="thermostat-footer glue-meta">
                {if actuator_id.is_empty() {
                    "No actuator configured".to_string()
                } else {
                    format!("Actuator: {actuator_id}")
                }}
                {(min_on > 0 || min_off > 0).then(|| {
                    format!(" · min-on {}s / min-off {}s", min_on, min_off)
                })}
                {move || lockout_remaining_secs().map(|s| {
                    let m = s / 60;
                    let sec = s % 60;
                    view! {
                        <span class="thermostat-lockout">
                            {format!(" · lockout {m}:{sec:02} remaining")}
                        </span>
                    }
                })}
            </div>

            // Delete action
            <div style="display:flex; justify-content:flex-end; gap:0.5rem; margin-top:0.5rem">
                {move || if confirm_delete.get() {
                    view! {
                        <span class="glue-meta">"Remove thermostat? "</span>
                        <button class="hc-btn hc-btn--sm hc-btn--danger"
                            disabled=move || delete_busy.get()
                            on:click={
                                let do_delete = do_delete.clone();
                                move |_| do_delete()
                            }
                        >{move || if delete_busy.get() { "Removing…" } else { "Yes, remove" }}</button>
                        <button class="hc-btn hc-btn--sm hc-btn--outline"
                            on:click=move |_| confirm_delete.set(false)
                        >"Cancel"</button>
                    }.into_any()
                } else {
                    view! {
                        <button class="hc-btn hc-btn--sm hc-btn--outline hc-btn--danger-outline"
                            on:click=move |_| confirm_delete.set(true)
                        >"Remove thermostat"</button>
                    }.into_any()
                }}
            </div>

            <details class="thermostat-config-section">
                <summary>"Configuration"</summary>

                <label class="field-label" style="margin-top:0.5rem">"Sensors"</label>
                {
                    let sa = sensor_attr.clone();
                    let sa_for_cb = sa.clone();
                    let current_ids = sensor_ids.clone();
                    let candidates = sensor_candidates(&devmap, &sa);
                    view! {
                        <div class="thermostat-sensor-list">
                            {candidates.into_iter().map(|(id, label)| {
                                let checked = current_ids.contains(&id);
                                let current_ids_cb = current_ids.clone();
                                let id_cb = id.clone();
                                let sa_cb = sa_for_cb.clone();
                                let send_cmd_cb = send_cmd.clone();
                                view! {
                                    <label class="thermostat-check-row">
                                        <input type="checkbox" prop:checked=checked
                                            on:change=move |ev| {
                                                let on = event_target_checked(&ev);
                                                let mut ids = current_ids_cb.clone();
                                                if on {
                                                    if !ids.contains(&id_cb) { ids.push(id_cb.clone()); }
                                                } else {
                                                    ids.retain(|s| s != &id_cb);
                                                }
                                                send_cmd_cb(json!({
                                                    "command":"set_sensors",
                                                    "sensor_ids": ids,
                                                    "attribute": sa_cb,
                                                }));
                                            }
                                        />
                                        <span>{label}</span>
                                    </label>
                                }
                            }).collect_view()}
                        </div>
                    }
                }

                <label class="field-label" style="margin-top:0.5rem">"Sensor Attribute"</label>
                <input type="text" class="hc-input"
                    prop:value=sensor_attr.clone()
                    on:change={
                        let ids = sensor_ids.clone();
                        let send_cmd = send_cmd.clone();
                        move |ev| {
                            let v = event_target_value(&ev);
                            send_cmd(json!({
                                "command":"set_sensors",
                                "sensor_ids": ids,
                                "attribute": v,
                            }));
                        }
                    }
                />

                <label class="field-label" style="margin-top:0.5rem">"Aggregation"</label>
                <div class="toggle-group">
                    {["average","min","max"].iter().map(|m| {
                        let active = aggregation == *m;
                        let m_str = m.to_string();
                        let send_cmd_cb = send_cmd.clone();
                        view! {
                            <button class:active=active disabled=move || busy.get()
                                on:click=move |_| send_cmd_cb(json!({"command":"set_aggregation","value": m_str}))
                            >{*m}</button>
                        }
                    }).collect_view()}
                </div>

                <label class="field-label" style="margin-top:0.5rem">"Actuator"</label>
                {
                    let act_candidates = actuator_candidates(&devmap);
                    let aid_for_cmp = actuator_id.clone();
                    let send_cmd_cb = send_cmd.clone();
                    view! {
                        <select class="hc-select"
                            on:change=move |ev| {
                                let v = event_target_value(&ev);
                                send_cmd_cb(json!({"command":"set_actuator","device_id": v}));
                            }
                        >
                            <option value="" selected=move || actuator_id.is_empty()>"— none —"</option>
                            {act_candidates.into_iter().map(|(id, label)| {
                                let selected = id == aid_for_cmp;
                                view! { <option value=id.clone() selected=selected>{label}</option> }
                            }).collect_view()}
                        </select>
                    }
                }

                <div class="thermostat-wizard-row" style="margin-top:0.5rem">
                    <div style="flex:1">
                        <label class="field-label">"Min on (sec)"</label>
                        <input type="number" class="hc-input" min="0"
                            prop:value=min_on.to_string()
                            on:change={
                                let send_cmd = send_cmd.clone();
                                move |ev| {
                                    if let Ok(v) = event_target_value(&ev).parse::<u64>() {
                                        send_cmd(json!({
                                            "command":"set_short_cycle",
                                            "min_on_secs": v,
                                            "min_off_secs": min_off,
                                        }));
                                    }
                                }
                            }
                        />
                    </div>
                    <div style="flex:1">
                        <label class="field-label">"Min off (sec)"</label>
                        <input type="number" class="hc-input" min="0"
                            prop:value=min_off.to_string()
                            on:change={
                                let send_cmd = send_cmd.clone();
                                move |ev| {
                                    if let Ok(v) = event_target_value(&ev).parse::<u64>() {
                                        send_cmd(json!({
                                            "command":"set_short_cycle",
                                            "min_on_secs": min_on,
                                            "min_off_secs": v,
                                        }));
                                    }
                                }
                            }
                        />
                    </div>
                </div>
            </details>

            <div class="thermostat-chart-header">
                <div class="toggle-group">
                    {[("1h", 1u32), ("6h", 6), ("24h", 24), ("7d", 168)].iter().map(|(label, hrs)| {
                        let hrs = *hrs;
                        let label = *label;
                        view! {
                            <button
                                class:active=move || history_range_hours.get() == hrs
                                on:click=move |_| history_range_hours.set(hrs)
                            >{label}</button>
                        }
                    }).collect_view()}
                </div>
                {move || history_loading.get().then(|| view! {
                    <span class="glue-meta" style="margin-left:0.5rem">"Loading…"</span>
                })}
            </div>
            {move || {
                let h = history.get();
                let hours = history_range_hours.get();
                if h.is_empty() {
                    view! { <p class="glue-meta" style="margin-top:0.5rem">"No history in this range."</p> }.into_any()
                } else {
                    render_thermostat_chart(&h, hours).into_any()
                }
            }}
        </section>
    }
}

/// Devices with a numeric reading at `attr`; skip thermostats (can't self-feed).
fn sensor_candidates(
    devices: &HashMap<String, DeviceState>,
    attr: &str,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = devices
        .values()
        .filter(|d| d.device_type.as_deref() != Some("thermostat"))
        .filter(|d| d.attributes.get(attr).and_then(|v| v.as_f64()).is_some())
        .map(|d| {
            let label = format!(
                "{} ({})  — {}",
                d.name,
                d.device_id,
                d.attributes
                    .get(attr)
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            );
            (d.device_id.clone(), label)
        })
        .collect();
    out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    out
}

/// Devices with an `on` boolean attribute — switches, lights, relays.
fn actuator_candidates(devices: &HashMap<String, DeviceState>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = devices
        .values()
        .filter(|d| d.device_type.as_deref() != Some("thermostat"))
        .filter(|d| d.attributes.get("on").and_then(|v| v.as_bool()).is_some())
        .map(|d| {
            let label = format!("{} ({})", d.name, d.device_id);
            (d.device_id.clone(), label)
        })
        .collect();
    out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    out
}

fn render_thermostat_chart(history: &[HistoryEntry], window_hours: u32) -> impl IntoView {
    const W: f64 = 600.0;
    const H: f64 = 180.0;
    const PAD_L: f64 = 36.0;
    const PAD_R: f64 = 8.0;
    const PAD_T: f64 = 8.0;
    const PAD_B: f64 = 22.0;
    let chart_w = W - PAD_L - PAD_R;
    let chart_h = H - PAD_T - PAD_B;

    let now_ms = js_sys::Date::now();
    let t_min = now_ms - (window_hours as f64 * 3_600_000.0);
    let t_max = now_ms;
    let t_span = (t_max - t_min).max(1.0);

    let mut temps: Vec<(f64, f64)> = Vec::new();
    let mut setpoints: Vec<(f64, f64)> = Vec::new();
    let mut actuator_segments: Vec<(f64, f64, bool)> = Vec::new();
    let mut last_act: Option<(f64, bool)> = None;

    for h in history {
        let ms = h.recorded_at.timestamp_millis() as f64;
        if ms < t_min {
            continue;
        }
        match h.attribute.as_str() {
            "current_temperature" => {
                if let Some(f) = h.value.as_f64() {
                    temps.push((ms, f));
                }
            }
            "setpoint" => {
                if let Some(f) = h.value.as_f64() {
                    setpoints.push((ms, f));
                }
            }
            "actuator_state" => {
                if let Some(b) = h.value.as_bool() {
                    if let Some((start, on)) = last_act {
                        actuator_segments.push((start, ms, on));
                    }
                    last_act = Some((ms, b));
                }
            }
            _ => {}
        }
    }
    if temps.is_empty() {
        return view! { <p class="glue-meta" style="margin-top:0.5rem">"No temperature samples in this range."</p> }.into_any();
    }
    if let Some((start, on)) = last_act {
        actuator_segments.push((start, now_ms, on));
    }
    actuator_segments = actuator_segments
        .into_iter()
        .filter(|(_, e, _)| *e >= t_min)
        .map(|(s, e, on)| (s.max(t_min), e, on))
        .collect();

    let (mut v_min, mut v_max) = temps
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (_, v)| {
            (lo.min(*v), hi.max(*v))
        });
    for (_, v) in &setpoints {
        v_min = v_min.min(*v);
        v_max = v_max.max(*v);
    }
    if (v_max - v_min).abs() < 1.0 {
        v_max = v_min + 2.0;
    }
    let pad = (v_max - v_min) * 0.1;
    v_min -= pad;
    v_max += pad;
    let v_span = v_max - v_min;

    let tx = move |ms: f64| PAD_L + ((ms - t_min) / t_span) * chart_w;
    let ty = move |v: f64| PAD_T + chart_h - ((v - v_min) / v_span) * chart_h;

    let temp_path = temps
        .iter()
        .enumerate()
        .map(|(i, (ms, v))| {
            let prefix = if i == 0 { "M" } else { "L" };
            format!("{prefix}{:.1},{:.1}", tx(*ms), ty(*v))
        })
        .collect::<Vec<_>>()
        .join(" ");

    let setpoint_path = if setpoints.is_empty() {
        String::new()
    } else {
        setpoints
            .iter()
            .enumerate()
            .map(|(i, (ms, v))| {
                let prefix = if i == 0 { "M" } else { "L" };
                format!("{prefix}{:.1},{:.1}", tx(*ms), ty(*v))
            })
            .collect::<Vec<_>>()
            .join(" ")
    };

    let rects = actuator_segments
        .iter()
        .filter(|(_, _, on)| *on)
        .map(|(s, e, _)| {
            let x = tx(*s);
            let w = (tx(*e) - x).max(1.0);
            view! {
                <rect class="chart-actuator" x=format!("{x:.1}") y=format!("{:.1}", PAD_T)
                    width=format!("{w:.1}") height=format!("{:.1}", chart_h) />
            }
        })
        .collect_view();

    let y_min_label = format!("{v_min:.1}°");
    let y_max_label = format!("{v_max:.1}°");

    let tick_count = 5;
    let x_ticks: Vec<(f64, String)> = (0..tick_count)
        .map(|i| {
            let frac = i as f64 / (tick_count as f64 - 1.0);
            let ms = t_min + t_span * frac;
            (PAD_L + frac * chart_w, format_chart_time(ms, window_hours))
        })
        .collect();
    let range_label = match window_hours {
        1 => "Last hour".to_string(),
        h if h < 24 => format!("Last {h}h"),
        168 => "Last 7d".to_string(),
        h => format!("Last {}h", h),
    };

    view! {
        <div style="margin-top:0.75rem">
            <div class="glue-meta" style="margin-bottom:0.2rem">{range_label}</div>
            <svg class="thermostat-chart" viewBox=format!("0 0 {W} {H}") preserveAspectRatio="none">
                {rects}
                <line class="chart-axis" x1=format!("{PAD_L}") y1=format!("{PAD_T}")
                    x2=format!("{PAD_L}") y2=format!("{}", PAD_T + chart_h) />
                <line class="chart-axis" x1=format!("{PAD_L}") y1=format!("{}", PAD_T + chart_h)
                    x2=format!("{}", W - PAD_R) y2=format!("{}", PAD_T + chart_h) />
                <text class="chart-label" x="4" y=format!("{}", PAD_T + 9.0)>{y_max_label}</text>
                <text class="chart-label" x="4" y=format!("{}", PAD_T + chart_h)>{y_min_label}</text>
                {x_ticks.into_iter().map(|(x, label)| {
                    let align = if x < PAD_L + 10.0 {
                        "start"
                    } else if x > W - PAD_R - 10.0 {
                        "end"
                    } else {
                        "middle"
                    };
                    view! {
                        <text class="chart-label" x=format!("{x:.1}")
                            y=format!("{}", PAD_T + chart_h + 14.0)
                            text-anchor=align>{label}</text>
                    }
                }).collect_view()}
                {(!setpoint_path.is_empty()).then(|| view! { <path class="chart-setpoint" d=setpoint_path /> })}
                <path class="chart-temp" d=temp_path />
            </svg>
        </div>
    }.into_any()
}

fn format_chart_time(ms: f64, window_hours: u32) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms));
    if window_hours >= 168 {
        let m = date.get_month() + 1;
        let d = date.get_date();
        format!("{m}/{d}")
    } else if window_hours >= 24 {
        let weekday = match date.get_day() {
            0 => "Sun",
            1 => "Mon",
            2 => "Tue",
            3 => "Wed",
            4 => "Thu",
            5 => "Fri",
            _ => "Sat",
        };
        format!("{weekday} {:02}", date.get_hours())
    } else {
        format!("{:02}:{:02}", date.get_hours(), date.get_minutes())
    }
}
