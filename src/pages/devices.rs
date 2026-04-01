//! Devices page — sortable/filterable table with live WebSocket updates.
//!
//! ## Leptos reactivity notes
//!
//! - `devices: RwSignal<Vec<DeviceState>>` is the live source of truth.
//!   Updated by the initial REST fetch AND by WebSocket `DeviceStateChanged`
//!   / `DeviceAvailabilityChanged` events.
//!
//! - `sorted_filtered: Memo<Vec<DeviceState>>` is a *lazy* derived computation.
//!   It only re-runs when one of the signals it reads changes (search, filters,
//!   sort, or devices).  Changing density or visible_columns does NOT trigger it.
//!
//! - The `<For>` component keys rows by `device_id`.  When a WebSocket update
//!   changes a single device, only that row re-renders — not the whole table.
//!
//! - Column prefs are persisted to localStorage so they survive page reloads.

use crate::api::{fetch_devices, set_device_state, StreamEvent};
use crate::auth::{events_ws_url, use_auth};
use crate::models::*;
use leptos::prelude::*;
use leptos::task::spawn_local;
use thaw::{Button, Input, InputType};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

// ── Column enum ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Col {
    StatusIcon,
    Name,
    Area,
    DeviceType,
    StatusText,
    MediaInfo,
    LastSeen,
    Control,
    CanonicalName,
    Plugin,
}

impl Col {
    fn all() -> &'static [Col] {
        &[
            Col::StatusIcon, Col::Name,   Col::Area,          Col::DeviceType,
            Col::StatusText, Col::MediaInfo, Col::LastSeen,   Col::Control,
            Col::CanonicalName, Col::Plugin,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            Col::StatusIcon    => "Status icon",
            Col::Name          => "Name",
            Col::Area          => "Area",
            Col::DeviceType    => "Type",
            Col::StatusText    => "Status",
            Col::MediaInfo     => "Media info",
            Col::LastSeen      => "Last changed",
            Col::Control       => "Control",
            Col::CanonicalName => "Canonical name",
            Col::Plugin        => "Plugin",
        }
    }

    fn sort_key(self) -> Option<SortKey> {
        match self {
            Col::Name                         => Some(SortKey::Name),
            Col::Area                         => Some(SortKey::Area),
            Col::StatusIcon | Col::StatusText => Some(SortKey::Status),
            Col::DeviceType                   => Some(SortKey::Type),
            Col::LastSeen                     => Some(SortKey::LastSeen),
            _                                 => None,
        }
    }
}

fn default_columns() -> Vec<Col> {
    vec![
        Col::StatusIcon, Col::Name, Col::Area, Col::DeviceType,
        Col::StatusText, Col::LastSeen, Col::Control,
    ]
}

// ── Sort / filter types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortKey { Name, Area, Status, Type, LastSeen }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortDir { Asc, Desc }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Availability { All, Online, Offline }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Density { Comfortable, Compact }

// ── localStorage keys ─────────────────────────────────────────────────────────

const PREFS_KEY: &str = "hc-leptos:devices:prefs";

fn ls_get(key: &str) -> Option<String> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(key).ok().flatten())
}

fn ls_set(key: &str, val: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(key, val);
    }
}

fn load_prefs() -> (Vec<Col>, Density) {
    let raw = match ls_get(PREFS_KEY) {
        Some(s) => s,
        None    => return (default_columns(), Density::Comfortable),
    };
    let v: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v)  => v,
        Err(_) => return (default_columns(), Density::Comfortable),
    };
    let cols: Vec<Col> = v["columns"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|s| col_from_str(s.as_str()?))
                .collect()
        })
        .filter(|v: &Vec<Col>| !v.is_empty())
        .unwrap_or_else(default_columns);
    let density = if v["density"] == "compact" {
        Density::Compact
    } else {
        Density::Comfortable
    };
    (cols, density)
}

fn save_prefs(cols: &[Col], density: Density) {
    let col_strs: Vec<&str> = cols.iter().map(|c| col_to_str(*c)).collect();
    let v = serde_json::json!({
        "columns": col_strs,
        "density": if density == Density::Compact { "compact" } else { "comfortable" },
    });
    ls_set(PREFS_KEY, &v.to_string());
}

fn col_to_str(c: Col) -> &'static str {
    match c {
        Col::StatusIcon    => "status_icon",
        Col::Name          => "name",
        Col::Area          => "area",
        Col::DeviceType    => "device_type",
        Col::StatusText    => "status_text",
        Col::MediaInfo     => "media_info",
        Col::LastSeen      => "last_seen",
        Col::Control       => "control",
        Col::CanonicalName => "canonical_name",
        Col::Plugin        => "plugin",
    }
}

fn col_from_str(s: &str) -> Option<Col> {
    Some(match s {
        "status_icon"    => Col::StatusIcon,
        "name"           => Col::Name,
        "area"           => Col::Area,
        "device_type"    => Col::DeviceType,
        "status_text"    => Col::StatusText,
        "media_info"     => Col::MediaInfo,
        "last_seen"      => Col::LastSeen,
        "control"        => Col::Control,
        "canonical_name" => Col::CanonicalName,
        "plugin"         => Col::Plugin,
        _                => return None,
    })
}

// ── Main component ────────────────────────────────────────────────────────────

#[component]
pub fn DevicesPage() -> impl IntoView {
    let auth = use_auth();

    // ── State ─────────────────────────────────────────────────────────────────

    let devices: RwSignal<Vec<DeviceState>> = RwSignal::new(vec![]);
    let loading  = RwSignal::new(true);
    let error    = RwSignal::new(Option::<String>::None);
    let notice   = RwSignal::new(Option::<String>::None);
    let busy_id  = RwSignal::new(Option::<String>::None);

    // Filters
    let search       = RwSignal::new(String::new());
    let availability = RwSignal::new(Availability::All);
    let area_filter   = RwSignal::new("all".to_string());
    let type_filter   = RwSignal::new("all".to_string());
    let plugin_filter = RwSignal::new("all".to_string());

    // Sort
    let sort_by  = RwSignal::new(SortKey::Name);
    let sort_dir = RwSignal::new(SortDir::Asc);

    // Prefs (persisted)
    let (init_cols, init_density) = load_prefs();
    let visible_cols = RwSignal::new(init_cols);
    let density      = RwSignal::new(init_density);
    let show_media   = RwSignal::new(false);

    // Filter panel expanded
    let filter_open = RwSignal::new(false);

    // Column drag state
    let drag_col = RwSignal::new(Option::<Col>::None);

    // Column chooser open
    let col_menu_open = RwSignal::new(false);

    // ── Save prefs reactively ─────────────────────────────────────────────────

    Effect::new(move |_| {
        save_prefs(&visible_cols.get(), density.get());
    });

    // ── Initial device load ───────────────────────────────────────────────────

    let refresh = move || {
        let token = auth.token_str().unwrap_or_default();
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match fetch_devices(&token).await {
                Ok(list) => devices.set(list),
                Err(e)   => error.set(Some(e)),
            }
            loading.set(false);
        });
    };

    // Fetch on mount
    Effect::new(move |_| { refresh(); });

    // ── WebSocket live updates ────────────────────────────────────────────────
    //
    // Connects after the token is confirmed present.
    // When a DeviceStateChanged event arrives, only that device's attributes are
    // updated in the Vec — Leptos's <For key=device_id> then only re-renders
    // that one row.

    Effect::new(move |_| {
        let token = match auth.token_str() {
            Some(t) => t,
            None    => return,
        };

        let url = events_ws_url(&token);
        let ws  = match web_sys::WebSocket::new(&url) {
            Ok(ws) => ws,
            Err(_) => return,
        };

        // onmessage
        let on_msg = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
            move |ev: web_sys::MessageEvent| {
                let text = match ev.data().as_string() {
                    Some(s) => s,
                    None    => return,
                };
                let event: StreamEvent = match serde_json::from_str(&text) {
                    Ok(e)  => e,
                    Err(_) => return,
                };
                match event {
                    StreamEvent::DeviceStateChanged { device_id, current, .. } => {
                        devices.update(|list| {
                            if let Some(d) = list.iter_mut().find(|d| d.device_id == device_id) {
                                d.attributes = current;
                                d.last_seen  = Some(chrono::Utc::now());
                            }
                        });
                    }
                    StreamEvent::DeviceAvailabilityChanged { device_id, available } => {
                        devices.update(|list| {
                            if let Some(d) = list.iter_mut().find(|d| d.device_id == device_id) {
                                d.available = available;
                            }
                        });
                    }
                    StreamEvent::Other => {}
                }
            },
        );
        ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
        on_msg.forget();

        on_cleanup(move || { let _ = ws.close(); });
    });

    // ── Derived: area / type option lists ─────────────────────────────────────
    //
    // These Memos only recompute when `devices` changes.

    let area_options: Memo<Vec<String>> = Memo::new(move |_| {
        let mut areas: Vec<String> = devices
            .get()
            .iter()
            .filter(|d| !is_scene_like(d))
            .filter_map(|d| d.area.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        areas.sort();
        areas
    });

    let type_options: Memo<Vec<String>> = Memo::new(move |_| {
        let mut types: Vec<String> = devices
            .get()
            .iter()
            .filter(|d| !is_scene_like(d))
            .filter_map(|d| d.device_type.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        types.sort();
        types
    });

    let plugin_options: Memo<Vec<String>> = Memo::new(move |_| {
        let mut plugins: Vec<String> = devices
            .get()
            .iter()
            .filter(|d| !is_scene_like(d))
            .map(|d| d.plugin_id.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        plugins.sort();
        plugins
    });

    // ── Derived: filtered + sorted list ──────────────────────────────────────
    //
    // This Memo is the key Leptos demonstration.  It subscribes to exactly the
    // signals it reads.  Changing `density` or `visible_cols` does NOT cause
    // this to recompute — those are independent subscriptions in the render.

    let sorted_filtered: Memo<Vec<DeviceState>> = Memo::new(move |_| {
        let all    = devices.get();
        let q      = search.get().trim().to_lowercase();
        let avail    = availability.get();
        let area     = area_filter.get();
        let type_f   = type_filter.get();
        let plugin_f = plugin_filter.get();
        let sb       = sort_by.get();
        let sd     = sort_dir.get();

        let mut result: Vec<DeviceState> = all
            .into_iter()
            .filter(|d| !is_scene_like(d))
            .filter(|d| match avail {
                Availability::All     => true,
                Availability::Online  => d.available,
                Availability::Offline => !d.available,
            })
            .filter(|d| {
                area == "all"
                    || d.area.as_deref().unwrap_or("Unassigned") == area
            })
            .filter(|d| {
                type_f == "all"
                    || d.device_type.as_deref().unwrap_or("device") == type_f
            })
            .filter(|d| plugin_f == "all" || d.plugin_id == plugin_f)
            .filter(|d| {
                if q.is_empty() { return true; }
                let haystack = format!(
                    "{} {} {} {} {} {} {}",
                    display_name(d),
                    d.device_id,
                    d.canonical_name.as_deref().unwrap_or(""),
                    d.area.as_deref().unwrap_or(""),
                    d.device_type.as_deref().unwrap_or(""),
                    d.plugin_id,
                    status_text(d),
                ).to_lowercase();
                haystack.contains(&q)
            })
            .collect();

        result.sort_by(|a, b| {
            let cmp = match sb {
                SortKey::Name     => sort_key_str(display_name(a)).cmp(&sort_key_str(display_name(b))),
                SortKey::Area     => sort_key_str(a.area.as_deref().unwrap_or("Unassigned"))
                                        .cmp(&sort_key_str(b.area.as_deref().unwrap_or("Unassigned"))),
                SortKey::Status   => sort_key_str(&status_text(a)).cmp(&sort_key_str(&status_text(b))),
                SortKey::Type     => sort_key_str(a.device_type.as_deref().unwrap_or("device"))
                                        .cmp(&sort_key_str(b.device_type.as_deref().unwrap_or("device"))),
                SortKey::LastSeen => a.last_seen.cmp(&b.last_seen),
            };
            if sd == SortDir::Desc { cmp.reverse() } else { cmp }
        });

        result
    });

    // ── Sort column click handler ─────────────────────────────────────────────

    let toggle_sort = move |col: Col| {
        let Some(key) = col.sort_key() else { return };
        if sort_by.get() == key {
            sort_dir.update(|d| *d = if *d == SortDir::Asc { SortDir::Desc } else { SortDir::Asc });
        } else {
            sort_by.set(key);
            sort_dir.set(if key == SortKey::LastSeen { SortDir::Desc } else { SortDir::Asc });
        }
    };

    // ── Column drag-reorder ───────────────────────────────────────────────────

    let col_dragstart = move |col: Col| drag_col.set(Some(col));
    let col_drop      = move |target: Col| {
        let Some(from) = drag_col.get() else { return };
        if from == target { return; }
        visible_cols.update(|cols| {
            let fi = cols.iter().position(|&c| c == from);
            let ti = cols.iter().position(|&c| c == target);
            if let (Some(f), Some(t)) = (fi, ti) {
                let item = cols.remove(f);
                cols.insert(t, item);
            }
        });
        drag_col.set(None);
    };

    // ── View ──────────────────────────────────────────────────────────────────

    view! {
        <div class="page">

            // Heading row
            <div class="heading">
                <div>
                    <h1>"Devices"</h1>
                    <p>"Flat inventory with sortable columns, live updates, and inline controls."</p>
                </div>
                <Button
                    on_click=move |_| refresh()
                    disabled=Signal::derive(move || loading.get())
                    loading=Signal::derive(move || loading.get())
                >
                    "Refresh"
                </Button>
            </div>

            // Filter panel
            <div class="filter-panel panel">
                <div class="filter-bar">
                    <Input
                        value=search
                        input_type=InputType::Search
                        placeholder="Search name, area, type, plugin, status…"
                    />
                    <Button on_click=move |_| filter_open.update(|v| *v = !*v)>
                        <span class="material-icons" style="font-size:16px;vertical-align:middle">
                            {move || if filter_open.get() { "expand_less" } else { "tune" }}
                        </span>
                        {move || if filter_open.get() { " Less" } else { " Filters & columns" }}
                    </Button>
                </div>

                // Expandable filter body
                {move || filter_open.get().then(|| view! {
                    <div class="filter-body">
                        // Filter row 1
                        <div class="toolbar-row">
                            <select
                                on:change=move |ev| {
                                    let val = event_target_value(&ev);
                                    availability.set(match val.as_str() {
                                        "online"  => Availability::Online,
                                        "offline" => Availability::Offline,
                                        _         => Availability::All,
                                    });
                                }
                            >
                                <option value="all">"All devices"</option>
                                <option value="online">"Online only"</option>
                                <option value="offline">"Offline only"</option>
                            </select>

                            <select on:change=move |ev| area_filter.set(event_target_value(&ev))>
                                <option value="all">"All areas"</option>
                                <For
                                    each=move || area_options.get()
                                    key=|a| a.clone()
                                    children=|area| view! {
                                        <option value=area.clone()>{area.clone()}</option>
                                    }
                                />
                            </select>

                            <select on:change=move |ev| type_filter.set(event_target_value(&ev))>
                                <option value="all">"All types"</option>
                                <For
                                    each=move || type_options.get()
                                    key=|t| t.clone()
                                    children=|t| view! {
                                        <option value=t.clone()>{t.clone()}</option>
                                    }
                                />
                            </select>

                            <select on:change=move |ev| plugin_filter.set(event_target_value(&ev))>
                                <option value="all">"All plugins"</option>
                                <For
                                    each=move || plugin_options.get()
                                    key=|p| p.clone()
                                    children=|p| view! {
                                        <option value=p.clone()>{p.clone()}</option>
                                    }
                                />
                            </select>
                        </div>

                        // Filter row 2 — display prefs
                        <div class="toolbar-row">
                            <label>
                                "Density "
                                <select on:change=move |ev| {
                                    density.set(if event_target_value(&ev) == "compact" {
                                        Density::Compact
                                    } else {
                                        Density::Comfortable
                                    });
                                }>
                                    <option value="comfortable">"Comfortable"</option>
                                    <option value="compact">"Compact"</option>
                                </select>
                            </label>

                            <label class="inline-check">
                                <input type="checkbox"
                                    prop:checked=move || show_media.get()
                                    on:change=move |ev| {
                                        use wasm_bindgen::JsCast;
                                        let cb = ev.target()
                                            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok());
                                        if let Some(cb) = cb { show_media.set(cb.checked()); }
                                    }
                                />
                                "Show media details"
                            </label>

                            <Button
                                on_click=move |_: web_sys::MouseEvent| {
                                    visible_cols.set(default_columns());
                                    density.set(Density::Comfortable);
                                    show_media.set(false);
                                    sort_by.set(SortKey::Name);
                                    sort_dir.set(SortDir::Asc);
                                    plugin_filter.set("all".to_string());
                                    type_filter.set("all".to_string());
                                    area_filter.set("all".to_string());
                                    availability.set(Availability::All);
                                }
                            >
                                "Reset layout"
                            </Button>

                            // Column chooser
                            <ColumnChooser
                                all_cols=Col::all()
                                visible_cols=visible_cols
                                menu_open=col_menu_open
                            />
                        </div>
                    </div>
                })}
            </div>

            // Feedback
            {move || error.get().map(|e| view! { <p class="msg-error">{e}</p> })}
            {move || notice.get().map(|n| view! { <p class="msg-notice">{n}</p> })}

            // Table
            <div class="table-wrap panel">
                {move || {
                    let count = sorted_filtered.get().len();
                    let is_loading = loading.get();

                    if is_loading && devices.get().is_empty() {
                        return view! { <p style="padding:1.2rem">"Loading devices…"</p> }.into_any();
                    }
                    if count == 0 {
                        return view! { <p style="padding:1.2rem">"No devices match the current filters."</p> }.into_any();
                    }

                    view! {
                        <table class:compact=move || density.get() == Density::Compact>
                            <thead>
                                <tr>
                                    <For
                                        each=move || visible_cols.get()
                                        key=|&c| c as u8
                                        children=move |col| {
                                            let is_active = move || {
                                                col.sort_key().is_some_and(|k| k == sort_by.get())
                                            };
                                            view! {
                                                <th
                                                    class:dragging=move || drag_col.get() == Some(col)
                                                    draggable="true"
                                                    on:dragstart=move |_| col_dragstart(col)
                                                    on:dragover=move |ev| ev.prevent_default()
                                                    on:drop=move |_| col_drop(col)
                                                    on:dragend=move |_| drag_col.set(None)
                                                >
                                                    <button class="header-btn"
                                                        on:click=move |_| toggle_sort(col)>
                                                        <span class="drag-handle">"⋮⋮"</span>
                                                        {col.label()}
                                                        {move || is_active().then(|| view! {
                                                            <span class="sort-indicator">
                                                                {if sort_dir.get() == SortDir::Asc { "↑" } else { "↓" }}
                                                            </span>
                                                        })}
                                                    </button>
                                                </th>
                                            }
                                        }
                                    />
                                </tr>
                            </thead>
                            <tbody>
                                <For
                                    each=move || sorted_filtered.get()
                                    key=|d| d.device_id.clone()
                                    children=move |device| {
                                        view! {
                                            <DeviceRow
                                                device=device
                                                visible_cols=visible_cols
                                                show_media=show_media
                                                busy_id=busy_id
                                                error=error
                                                notice=notice
                                                devices=devices
                                                auth_token=auth.token
                                            />
                                        }
                                    }
                                />
                            </tbody>
                        </table>
                    }.into_any()
                }}
            </div>

        </div>
    }
}

// ── DeviceRow component ───────────────────────────────────────────────────────
//
// Receives a cloned DeviceState.  Because <For> keys by device_id, Leptos
// only re-renders a row when its specific device changes in the parent Vec.

#[component]
fn DeviceRow(
    device: DeviceState,
    visible_cols: RwSignal<Vec<Col>>,
    show_media: RwSignal<bool>,
    busy_id: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
    notice: RwSignal<Option<String>>,
    devices: RwSignal<Vec<DeviceState>>,
    auth_token: RwSignal<Option<String>>,
) -> impl IntoView {
    // Inline send command — avoids Callback<T> API version uncertainty
    let on_cmd = move |did: String, body: serde_json::Value, label: String| {
        let token = auth_token.get().unwrap_or_default();
        busy_id.set(Some(did.clone()));
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match set_device_state(&token, &did, &body).await {
                Ok(_)  => notice.set(Some(format!("{label} sent"))),
                Err(e) => error.set(Some(e)),
            }
            busy_id.set(None);
            let t2 = auth_token.get().unwrap_or_default();
            if let Ok(list) = fetch_devices(&t2).await {
                devices.set(list);
            }
        });
    };
    let id       = device.device_id.clone();
    let name     = display_name(&device).to_string();
    let tone     = status_tone(&device);
    let icon     = status_icon_name(&device);
    let s_text   = status_text(&device);
    let rel_time = format_relative(device.last_seen.as_ref());
    let abs_time = format_abs(device.last_seen.as_ref());

    let on_row_click = {
        let id = id.clone();
        move |_: web_sys::MouseEvent| {
            if let Some(win) = web_sys::window() {
                let _ = win.location().set_href(&format!("/devices/{}", id));
            }
        }
    };

    view! {
        <tr
            class:offline=!device.available
            on:click=on_row_click
        >
            <For
                each=move || visible_cols.get()
                key=|&c| c as u8
                children={
                    let device = device.clone();
                    let id = id.clone();
                    let s_text = s_text.clone();
                    let rel_time = rel_time.clone();
                    let abs_time = abs_time.clone();
                    move |col| {
                        let device = device.clone();
                        let id = id.clone();
                        let s_text = s_text.clone();
                        let rel_time = rel_time.clone();
                        let abs_time = abs_time.clone();

                        let cell = match col {
                            Col::StatusIcon => view! {
                                <td data-col="status_icon">
                                    <span class=format!("status-badge {}", tone.css_class())
                                          title=s_text.clone()>
                                        <span class="material-icons" style="font-size:18px">{icon}</span>
                                    </span>
                                </td>
                            }.into_any(),

                            Col::Name => view! {
                                <td data-col="name">
                                    <div class="cell-primary">
                                        <a class="cell-link"
                                           href=format!("/devices/{}", id)
                                           on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()>
                                            {name.clone()}
                                        </a>
                                        <span class="cell-subtle">{device.device_id.clone()}</span>
                                    </div>
                                </td>
                            }.into_any(),

                            Col::Area => view! {
                                <td data-col="area">
                                    {device.area.as_deref().unwrap_or("Unassigned").to_string()}
                                </td>
                            }.into_any(),

                            Col::DeviceType => view! {
                                <td data-col="device_type">
                                    {device.device_type.as_deref().unwrap_or("device").to_string()}
                                </td>
                            }.into_any(),

                            Col::StatusText => {
                                let ps = if is_media_player(&device) {
                                    let pb = playback_state(&device);
                                    if pb != s_text.to_lowercase() { Some(pb) } else { None }
                                } else { None };
                                view! {
                                    <td data-col="status_text">
                                        <div class="cell-primary">
                                            <span>{s_text.clone()}</span>
                                            {ps.map(|p| view! { <span class="cell-subtle">{p}</span> })}
                                        </div>
                                    </td>
                                }.into_any()
                            },

                            Col::MediaInfo => {
                                if is_media_player(&device) {
                                    let summary = media_summary(&device)
                                        .unwrap_or_else(|| "Nothing playing".to_string());
                                    let img_url = media_image_url(&device).map(str::to_string);
                                    let show = show_media;
                                    view! {
                                        <td data-col="media_info">
                                            <div class="cell-media">
                                                {move || (show.get() && img_url.is_some()).then(|| view! {
                                                    <img src=img_url.clone().unwrap_or_default() alt="" />
                                                })}
                                                <div class="cell-primary">
                                                    <span>{summary.clone()}</span>
                                                    {move || show.get().then(|| view! {
                                                        <span class="cell-subtle">{playback_state(&device)}</span>
                                                    })}
                                                </div>
                                            </div>
                                        </td>
                                    }.into_any()
                                } else {
                                    view! { <td data-col="media_info"><span class="cell-subtle">"—"</span></td> }.into_any()
                                }
                            },

                            Col::LastSeen => view! {
                                <td data-col="last_seen">
                                    <div class="cell-primary">
                                        <span>{rel_time.clone()}</span>
                                        <span class="cell-subtle">{abs_time.clone()}</span>
                                    </div>
                                </td>
                            }.into_any(),

                            Col::Control => {
                                let busy = busy_id;
                                let did  = id.clone();

                                if supports_inline_toggle(&device) {
                                    let is_on = bool_attr(device.attributes.get("on")) == Some(true);
                                    let label = if is_on { "Turn off" } else { "Turn on" };
                                    let body  = serde_json::json!({ "on": !is_on });
                                    // Two separate clones — one per closure
                                    let did_dis = did.clone();
                                    let did_clk = did.clone();
                                    view! {
                                        <td data-col="control">
                                            <div class="cell-controls">
                                                <button class="secondary"
                                                    disabled=move || busy.get().as_deref() == Some(&did_dis)
                                                    on:click=move |ev: web_sys::MouseEvent| {
                                                        ev.stop_propagation();
                                                        on_cmd(did_clk.clone(), body.clone(), label.to_string());
                                                    }>
                                                    {label}
                                                </button>
                                            </div>
                                        </td>
                                    }.into_any()
                                } else if is_media_player(&device) {
                                    let pb    = playback_state(&device);
                                    let busy2 = busy_id;
                                    let busy3 = busy_id;
                                    if pb == "playing" && supports_action(&device, "pause") {
                                        let did_dis = did.clone();
                                        let did_clk = did.clone();
                                        view! {
                                            <td data-col="control">
                                                <div class="cell-controls">
                                                    <button class="secondary"
                                                        disabled=move || busy2.get().as_deref() == Some(&did_dis)
                                                        on:click=move |ev: web_sys::MouseEvent| {
                                                            ev.stop_propagation();
                                                            on_cmd(did_clk.clone(), serde_json::json!({"action":"pause"}), "Pause".into());
                                                        }>
                                                        "Pause"
                                                    </button>
                                                </div>
                                            </td>
                                        }.into_any()
                                    } else if supports_action(&device, "play") {
                                        let did_dis = did.clone();
                                        let did_clk = did.clone();
                                        view! {
                                            <td data-col="control">
                                                <div class="cell-controls">
                                                    <button class="secondary"
                                                        disabled=move || busy3.get().as_deref() == Some(&did_dis)
                                                        on:click=move |ev: web_sys::MouseEvent| {
                                                            ev.stop_propagation();
                                                            on_cmd(did_clk.clone(), serde_json::json!({"action":"play"}), "Play".into());
                                                        }>
                                                        "Play"
                                                    </button>
                                                </div>
                                            </td>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <td data-col="control">
                                                <a class="secondary-link"
                                                   href=format!("/devices/{}", id)
                                                   on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()>
                                                    "Details"
                                                </a>
                                            </td>
                                        }.into_any()
                                    }
                                } else {
                                    view! {
                                        <td data-col="control">
                                            <a class="secondary-link"
                                               href=format!("/devices/{}", id)
                                               on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()>
                                                "Details"
                                            </a>
                                        </td>
                                    }.into_any()
                                }
                            },

                            Col::CanonicalName => view! {
                                <td data-col="canonical_name">
                                    {device.canonical_name.as_deref().unwrap_or("—").to_string()}
                                </td>
                            }.into_any(),

                            Col::Plugin => view! {
                                <td data-col="plugin">{device.plugin_id.clone()}</td>
                            }.into_any(),
                        };
                        cell
                    }
                }
            />
        </tr>
    }
}

// ── Column chooser ────────────────────────────────────────────────────────────

#[component]
fn ColumnChooser(
    all_cols: &'static [Col],
    visible_cols: RwSignal<Vec<Col>>,
    menu_open: RwSignal<bool>,
) -> impl IntoView {
    let toggle_col = move |col: Col| {
        visible_cols.update(|cols| {
            if let Some(pos) = cols.iter().position(|&c| c == col) {
                if cols.len() > 1 { cols.remove(pos); }
            } else {
                // Append in canonical order
                let canonical_pos = all_cols.iter().position(|&c| c == col).unwrap_or(cols.len());
                let insert_at = cols.iter()
                    .position(|c| all_cols.iter().position(|&x| x == *c).unwrap_or(usize::MAX) > canonical_pos)
                    .unwrap_or(cols.len());
                cols.insert(insert_at, col);
            }
        });
    };

    view! {
        <div class="col-chooser">
            <Button on_click=move |_| menu_open.update(|v| *v = !*v)>
                <span class="material-icons" style="font-size:16px;vertical-align:middle">"view_column"</span>
                " Columns"
            </Button>

            {move || menu_open.get().then(|| view! {
                <div class="col-chooser-menu">
                    {all_cols.iter().map(|&col| {
                        let is_on = move || visible_cols.get().contains(&col);
                        view! {
                            <label class="col-chooser-item">
                                <input type="checkbox"
                                    prop:checked=is_on
                                    on:change=move |_| toggle_col(col)
                                />
                                {col.label()}
                            </label>
                        }
                    }).collect_view()}
                </div>
            })}
        </div>
    }
}
