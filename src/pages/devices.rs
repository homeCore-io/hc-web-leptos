//! Devices page — responsive list with live WebSocket updates.
//!
//! WS updates arrive via the shared `WsContext` (see `ws.rs`).  This page:
//!  - Opens no WebSocket of its own.
//!  - Seeds the shared device HashMap via the initial REST fetch.
//!  - Derives `sorted_filtered` purely from data signals — no timer tick.
//!  - Passes `timer_tick` only to `DeviceListRow` for the time-display cell.
//!  - Uses `<For>` (keyed by device_id) so only changed rows re-render on WS events.

use crate::api::{fetch_devices, set_device_state};
use crate::auth::use_auth;
use crate::models::*;
use crate::ws::{WsStatus, use_ws};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_shadcn_ui::{Button, ButtonVariant, Input};
use std::collections::HashSet;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Name,
    Area,
    Status,
    Type,
    LastSeen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Availability {
    All,
    Online,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Density {
    Comfortable,
    Compact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandIntent {
    Toggle(bool),
    Play,
    Pause,
}

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

const PAGE_SIZES: &[usize] = &[25, 50, 100];

struct Prefs {
    density: Density,
    show_media: bool,
    page_size: usize,
    search: String,
    availability: Availability,
    area_filter: String,
    type_filter: String,
    plugin_filter: String,
    sort_by: SortKey,
    sort_dir: SortDir,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            density: Density::Comfortable,
            show_media: false,
            page_size: 25,
            search: String::new(),
            availability: Availability::All,
            area_filter: "all".to_string(),
            type_filter: "all".to_string(),
            plugin_filter: "all".to_string(),
            sort_by: SortKey::Name,
            sort_dir: SortDir::Asc,
        }
    }
}

fn load_prefs() -> Prefs {
    let raw = match ls_get(PREFS_KEY) {
        Some(s) => s,
        None => return Prefs::default(),
    };
    let v: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Prefs::default(),
    };
    Prefs {
        density: if v["density"] == "compact" { Density::Compact } else { Density::Comfortable },
        show_media: v["show_media"].as_bool().unwrap_or(false),
        page_size: v["page_size"]
            .as_u64()
            .map(|n| n as usize)
            .filter(|n| PAGE_SIZES.contains(n))
            .unwrap_or(25),
        search: v["search"].as_str().unwrap_or("").to_string(),
        availability: match v["availability"].as_str() {
            Some("online") => Availability::Online,
            Some("offline") => Availability::Offline,
            _ => Availability::All,
        },
        area_filter: v["area_filter"].as_str().unwrap_or("all").to_string(),
        type_filter: v["type_filter"].as_str().unwrap_or("all").to_string(),
        plugin_filter: v["plugin_filter"].as_str().unwrap_or("all").to_string(),
        sort_by: match v["sort_by"].as_str() {
            Some("area") => SortKey::Area,
            Some("status") => SortKey::Status,
            Some("type") => SortKey::Type,
            Some("last_seen") => SortKey::LastSeen,
            _ => SortKey::Name,
        },
        sort_dir: if v["sort_dir"] == "desc" { SortDir::Desc } else { SortDir::Asc },
    }
}

fn save_prefs(
    density: Density,
    show_media: bool,
    page_size: usize,
    search: &str,
    availability: Availability,
    area_filter: &str,
    type_filter: &str,
    plugin_filter: &str,
    sort_by: SortKey,
    sort_dir: SortDir,
) {
    let value = serde_json::json!({
        "density": if density == Density::Compact { "compact" } else { "comfortable" },
        "show_media": show_media,
        "page_size": page_size,
        "search": search,
        "availability": match availability {
            Availability::Online => "online",
            Availability::Offline => "offline",
            Availability::All => "all",
        },
        "area_filter": area_filter,
        "type_filter": type_filter,
        "plugin_filter": plugin_filter,
        "sort_by": match sort_by {
            SortKey::Name => "name",
            SortKey::Area => "area",
            SortKey::Status => "status",
            SortKey::Type => "type",
            SortKey::LastSeen => "last_seen",
        },
        "sort_dir": if sort_dir == SortDir::Desc { "desc" } else { "asc" },
    });
    ls_set(PREFS_KEY, &value.to_string());
}

#[component]
pub fn DevicesPage() -> impl IntoView {
    let auth = use_auth();
    let ws = use_ws();

    let loading = RwSignal::new(true);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);
    let busy_id = RwSignal::new(Option::<String>::None);

    let prefs = load_prefs();
    let search = RwSignal::new(prefs.search);
    let availability = RwSignal::new(prefs.availability);
    let area_filter = RwSignal::new(prefs.area_filter);
    let type_filter = RwSignal::new(prefs.type_filter);
    let plugin_filter = RwSignal::new(prefs.plugin_filter);

    let sort_by = RwSignal::new(prefs.sort_by);
    let sort_dir = RwSignal::new(prefs.sort_dir);

    let density = RwSignal::new(prefs.density);
    let show_media = RwSignal::new(prefs.show_media);
    let filter_open = RwSignal::new(false);
    let page = RwSignal::new(0usize);
    let page_size = RwSignal::new(prefs.page_size);

    // Timer tick — used ONLY for relative timestamp display inside DeviceListRow.
    // NOT tracked by sorted_filtered so the filter/sort memo doesn't recompute
    // every second.
    let timer_tick = RwSignal::new(0u64);

    Effect::new(move |_| {
        let callback = Closure::<dyn FnMut()>::new(move || {
            timer_tick.update(|t| *t += 1);
        });
        let handle = web_sys::window().and_then(|window| {
            window
                .set_interval_with_callback_and_timeout_and_arguments_0(
                    callback.as_ref().unchecked_ref(),
                    1000,
                )
                .ok()
        });
        callback.forget();
        on_cleanup(move || {
            if let (Some(window), Some(handle)) = (web_sys::window(), handle) {
                window.clear_interval_with_handle(handle);
            }
        });
    });

    Effect::new(move |_| {
        save_prefs(
            density.get(),
            show_media.get(),
            page_size.get(),
            &search.get(),
            availability.get(),
            &area_filter.get(),
            &type_filter.get(),
            &plugin_filter.get(),
            sort_by.get(),
            sort_dir.get(),
        );
    });

    // Reset to page 0 whenever filters, sort order, or page size change.
    // Skip on first run (prev is None) so the page isn't cleared at mount.
    Effect::new(move |prev: Option<()>| {
        let _ = (
            search.get(), availability.get(), area_filter.get(),
            type_filter.get(), plugin_filter.get(),
            sort_by.get(), sort_dir.get(), page_size.get(),
        );
        if prev.is_some() {
            page.set(0);
        }
    });

    // Initial REST fetch — seeds the shared device map.
    // WS events keep it live from here on.
    Effect::new(move |_| {
        let token = auth.token_str().unwrap_or_default();
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match fetch_devices(&token).await {
                Ok(list) => {
                    ws.devices.update(|m| {
                        for d in list {
                            m.insert(d.device_id.clone(), d);
                        }
                    });
                }
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    });

    // ── Derived filter option lists ───────────────────────────────────────────
    // These read ws.devices and recompute whenever the device map changes.

    let area_options: Memo<Vec<String>> = Memo::new(move |_| {
        let mut areas: Vec<String> = ws
            .devices
            .get()
            .values()
            .filter(|d| !is_scene_like(d))
            .filter_map(|d| d.area.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        areas.sort_by_key(|a| display_area_name(a));
        areas
    });

    let type_options: Memo<Vec<String>> = Memo::new(move |_| {
        let mut types: Vec<String> = ws
            .devices
            .get()
            .values()
            .filter(|d| !is_scene_like(d))
            .map(presentation_device_type_label)
            .map(str::to_string)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        types.sort();
        types
    });

    let plugin_options: Memo<Vec<String>> = Memo::new(move |_| {
        let mut plugins: Vec<String> = ws
            .devices
            .get()
            .values()
            .filter(|d| !is_scene_like(d))
            .map(|d| d.plugin_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        plugins.sort();
        plugins
    });

    let active_filter_summary: Memo<Vec<String>> = Memo::new(move |_| {
        let mut summary = Vec::new();
        if !search.get().trim().is_empty() {
            summary.push(format!("Search: {}", search.get().trim()));
        }
        match availability.get() {
            Availability::Online => summary.push("Online only".into()),
            Availability::Offline => summary.push("Offline only".into()),
            Availability::All => {}
        }
        if area_filter.get() != "all" {
            summary.push(format!("Area: {}", display_area_name(&area_filter.get())));
        }
        if type_filter.get() != "all" {
            summary.push(format!("Type: {}", type_filter.get()));
        }
        if plugin_filter.get() != "all" {
            summary.push(format!("Plugin: {}", plugin_filter.get()));
        }
        summary
    });

    // sorted_filtered does NOT subscribe to timer_tick — recomputes only when
    // the device map or filter/sort signals change.
    let sorted_filtered: Memo<Vec<DeviceState>> = Memo::new(move |_| {
        let all = ws.devices.get();
        let q = search.get().trim().to_lowercase();
        let avail = availability.get();
        let area = area_filter.get();
        let type_f = type_filter.get();
        let plugin_f = plugin_filter.get();
        let sb = sort_by.get();
        let sd = sort_dir.get();

        let mut result: Vec<DeviceState> = all
            .into_values()
            .filter(|d| !is_scene_like(d))
            .filter(|d| match avail {
                Availability::All => true,
                Availability::Online => d.available,
                Availability::Offline => !d.available,
            })
            .filter(|d| area == "all" || d.area.as_deref().unwrap_or("Unassigned") == area)
            .filter(|d| type_f == "all" || presentation_device_type_label(d) == type_f)
            .filter(|d| plugin_f == "all" || d.plugin_id == plugin_f)
            .filter(|d| {
                if q.is_empty() {
                    return true;
                }
                let haystack = format!(
                    "{} {} {} {} {} {} {} {}",
                    display_name(d),
                    d.device_id,
                    d.canonical_name.as_deref().unwrap_or(""),
                    d.area.as_deref().unwrap_or(""),
                    display_area_value(d.area.as_deref()),
                    presentation_device_type_label(d),
                    d.plugin_id,
                    status_text(d),
                )
                .to_lowercase();
                haystack.contains(&q)
            })
            .collect();

        result.sort_by(|a, b| {
            let cmp = match sb {
                SortKey::Name => crate::models::sort_key_str(display_name(a))
                    .cmp(&crate::models::sort_key_str(display_name(b))),
                SortKey::Area => {
                    crate::models::sort_key_str(&display_area_value(a.area.as_deref())).cmp(
                        &crate::models::sort_key_str(&display_area_value(b.area.as_deref())),
                    )
                }
                SortKey::Status => crate::models::sort_key_str(&status_text(a))
                    .cmp(&crate::models::sort_key_str(&status_text(b))),
                SortKey::Type => crate::models::sort_key_str(presentation_device_type_label(a))
                    .cmp(&crate::models::sort_key_str(
                        presentation_device_type_label(b),
                    )),
                SortKey::LastSeen => last_change_time(a).cmp(&last_change_time(b)),
            };
            if sd == SortDir::Desc {
                cmp.reverse()
            } else {
                cmp
            }
        });

        result
    });

    // ── Pagination ────────────────────────────────────────────────────────────

    let total_filtered: Signal<usize> =
        Signal::derive(move || sorted_filtered.get().len());

    let total_pages: Memo<usize> = Memo::new(move |_| {
        let n = total_filtered.get();
        let s = page_size.get();
        if n == 0 { 1 } else { n.div_ceil(s) }
    });

    // Clamp page index so stale values don't produce an empty view after
    // filters narrow the result set.
    let page_clamped: Memo<usize> = Memo::new(move |_| {
        page.get().min(total_pages.get().saturating_sub(1))
    });

    let paged_devices: Memo<Vec<DeviceState>> = Memo::new(move |_| {
        let all = sorted_filtered.get();
        let p = page_clamped.get();
        let s = page_size.get();
        all.into_iter().skip(p * s).take(s).collect()
    });

    view! {
        <div class="page">
            <div class="heading">
                <div>
                    <h1>"Devices"</h1>
                    <p>"Responsive inventory list with filters, live updates, and inline controls."</p>
                </div>
            </div>

            // WS status banner
            {move || {
                let status = ws.status.get();
                (status != WsStatus::Live).then(|| {
                    let msg = match status {
                        WsStatus::Connecting => "Connecting to live updates…",
                        WsStatus::Disconnected => "Live updates lost — reconnecting…",
                        WsStatus::Live => unreachable!(),
                    };
                    view! { <p class="msg-warning">{msg}</p> }
                })
            }}

            <DeviceFiltersPanel
                search=search
                availability=availability
                area_filter=area_filter
                type_filter=type_filter
                plugin_filter=plugin_filter
                sort_by=sort_by
                sort_dir=sort_dir
                density=density
                show_media=show_media
                filter_open=filter_open
                area_options=area_options
                type_options=type_options
                plugin_options=plugin_options
                active_filter_summary=active_filter_summary
                result_count=Signal::derive(move || sorted_filtered.get().len())
            />

            {move || error.get().map(|e| view! { <p class="msg-error">{e}</p> })}
            {move || notice.get().map(|n| view! { <p class="msg-notice">{n}</p> })}

            <DevicesListSection
                devices=paged_devices
                loading=loading
                has_source_devices=Signal::derive(move || !ws.devices.get().is_empty())
                density=density
                show_media=show_media
                timer_tick=timer_tick
                busy_id=busy_id
                error=error
                notice=notice
                auth_token=auth.token
                page=page
                page_clamped=page_clamped
                page_size=page_size
                total_pages=total_pages
                total_filtered=total_filtered
            />
        </div>
    }
}

#[component]
fn DeviceFiltersPanel(
    search: RwSignal<String>,
    availability: RwSignal<Availability>,
    area_filter: RwSignal<String>,
    type_filter: RwSignal<String>,
    plugin_filter: RwSignal<String>,
    sort_by: RwSignal<SortKey>,
    sort_dir: RwSignal<SortDir>,
    density: RwSignal<Density>,
    show_media: RwSignal<bool>,
    filter_open: RwSignal<bool>,
    area_options: Memo<Vec<String>>,
    type_options: Memo<Vec<String>>,
    plugin_options: Memo<Vec<String>>,
    active_filter_summary: Memo<Vec<String>>,
    result_count: Signal<usize>,
) -> impl IntoView {
    view! {
        <div class="filter-panel panel">
            <div class="filter-bar">
                <Input
                    value=Signal::derive(move || search.get())
                    on_change=Callback::new(move |value| search.set(value))
                    input_type="search"
                    placeholder="Search name, area, type, plugin, status…"
                />
                <Button
                    variant=ButtonVariant::Secondary
                    on_click=Callback::new(move |_| filter_open.update(|v| *v = !*v))
                >
                    <span class="material-icons" style="font-size:16px;vertical-align:middle">
                        {move || if filter_open.get() { "expand_less" } else { "tune" }}
                    </span>
                    {move || if filter_open.get() { " Less" } else { " Filters" }}
                </Button>
            </div>

            <div class="filter-summary">
                <div class="filter-summary-count">
                    <strong>{result_count}</strong>
                    <span>" devices"</span>
                </div>
                <div class="filter-summary-chips">
                    {move || {
                        let chips = active_filter_summary.get();
                        if chips.is_empty() {
                            view! { <span class="summary-chip muted">"No active filters"</span> }.into_any()
                        } else {
                            chips.into_iter().map(|chip| view! {
                                <span class="summary-chip">{chip}</span>
                            }).collect_view().into_any()
                        }
                    }}
                </div>
            </div>

            {move || filter_open.get().then(|| view! {
                <div class="filter-body">
                    <div class="toolbar-row toolbar-grid">
                        <select
                            on:change=move |ev| {
                                let val = event_target_value(&ev);
                                availability.set(match val.as_str() {
                                    "online" => Availability::Online,
                                    "offline" => Availability::Offline,
                                    _ => Availability::All,
                                });
                            }
                        >
                            <option value="all" selected=move || availability.get() == Availability::All>
                                "All devices"
                            </option>
                            <option value="online" selected=move || availability.get() == Availability::Online>
                                "Online only"
                            </option>
                            <option value="offline" selected=move || availability.get() == Availability::Offline>
                                "Offline only"
                            </option>
                        </select>

                        <select on:change=move |ev| area_filter.set(event_target_value(&ev))>
                            <option value="all">"All areas"</option>
                            <For
                                each=move || area_options.get()
                                key=|a| a.clone()
                                children=|area| view! {
                                    <option value=area.clone()>{display_area_name(&area)}</option>
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

                    <div class="toolbar-row toolbar-grid toolbar-actions">
                        <label>
                            "Sort "
                            <select on:change=move |ev| {
                                let value = event_target_value(&ev);
                                sort_by.set(match value.as_str() {
                                    "area" => SortKey::Area,
                                    "status" => SortKey::Status,
                                    "type" => SortKey::Type,
                                    "last_seen" => SortKey::LastSeen,
                                    _ => SortKey::Name,
                                });
                            }>
                                <option value="name" selected=move || sort_by.get() == SortKey::Name>"Name"</option>
                                <option value="area" selected=move || sort_by.get() == SortKey::Area>"Area"</option>
                                <option value="status" selected=move || sort_by.get() == SortKey::Status>"Status"</option>
                                <option value="type" selected=move || sort_by.get() == SortKey::Type>"Type"</option>
                                <option value="last_seen" selected=move || sort_by.get() == SortKey::LastSeen>"Last changed"</option>
                            </select>
                        </label>

                        <label>
                            "Direction "
                            <select on:change=move |ev| {
                                sort_dir.set(if event_target_value(&ev) == "desc" {
                                    SortDir::Desc
                                } else {
                                    SortDir::Asc
                                });
                            }>
                                <option value="asc" selected=move || sort_dir.get() == SortDir::Asc>"Ascending"</option>
                                <option value="desc" selected=move || sort_dir.get() == SortDir::Desc>"Descending"</option>
                            </select>
                        </label>

                        <label>
                            "Density "
                            <select on:change=move |ev| {
                                density.set(if event_target_value(&ev) == "compact" {
                                    Density::Compact
                                } else {
                                    Density::Comfortable
                                });
                            }>
                                <option value="comfortable" selected=move || density.get() == Density::Comfortable>
                                    "Comfortable"
                                </option>
                                <option value="compact" selected=move || density.get() == Density::Compact>
                                    "Compact"
                                </option>
                            </select>
                        </label>

                        <label class="inline-check">
                            <input
                                type="checkbox"
                                prop:checked=move || show_media.get()
                                on:change=move |ev| {
                                    let cb = ev.target()
                                        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok());
                                    if let Some(cb) = cb {
                                        show_media.set(cb.checked());
                                    }
                                }
                            />
                            "Show media details"
                        </label>

                        <Button
                            variant=ButtonVariant::Outline
                            on_click=Callback::new(move |_| {
                                search.set(String::new());
                                availability.set(Availability::All);
                                area_filter.set("all".to_string());
                                type_filter.set("all".to_string());
                                plugin_filter.set("all".to_string());
                                sort_by.set(SortKey::Name);
                                sort_dir.set(SortDir::Asc);
                                density.set(Density::Comfortable);
                                show_media.set(false);
                            })
                        >
                            "Reset view"
                        </Button>
                    </div>
                </div>
            })}
        </div>
    }
}

#[component]
fn DevicesListSection(
    devices: Memo<Vec<DeviceState>>,
    loading: RwSignal<bool>,
    has_source_devices: Signal<bool>,
    density: RwSignal<Density>,
    show_media: RwSignal<bool>,
    timer_tick: RwSignal<u64>,
    busy_id: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
    notice: RwSignal<Option<String>>,
    auth_token: RwSignal<Option<String>>,
    page: RwSignal<usize>,
    page_clamped: Memo<usize>,
    page_size: RwSignal<usize>,
    total_pages: Memo<usize>,
    total_filtered: Signal<usize>,
) -> impl IntoView {
    view! {
        <div class="device-list panel" class:compact=move || density.get() == Density::Compact>
            {move || {
                let is_loading = loading.get();
                if is_loading && !has_source_devices.get() {
                    return view! { <p class="device-list-empty">"Loading devices…"</p> }.into_any();
                }
                if total_filtered.get() == 0 {
                    return view! { <p class="device-list-empty">"No devices match the current filters."</p> }.into_any();
                }

                view! {
                    <Pagination
                        page=page
                        page_clamped=page_clamped
                        page_size=page_size
                        total_pages=total_pages
                        total_filtered=total_filtered
                    />
                    <div class="device-list-header">
                        <span>"Device"</span>
                        <span>"State"</span>
                        <span>"Last changed"</span>
                        <span class="device-list-header-action">"Action"</span>
                    </div>
                    <div class="device-list-body">
                        // Keyed list: only rows whose device data changed are re-rendered.
                        <For
                            each=move || devices.get()
                            key=|d| d.device_id.clone()
                            children=move |device| view! {
                                <DeviceListRow
                                    device=device
                                    show_media=show_media
                                    timer_tick=timer_tick
                                    busy_id=busy_id
                                    error=error
                                    notice=notice
                                    auth_token=auth_token
                                />
                            }
                        />
                    </div>
                    <Pagination
                        page=page
                        page_clamped=page_clamped
                        page_size=page_size
                        total_pages=total_pages
                        total_filtered=total_filtered
                    />
                }.into_any()
            }}
        </div>
    }
}

#[component]
fn Pagination(
    page: RwSignal<usize>,
    page_clamped: Memo<usize>,
    page_size: RwSignal<usize>,
    total_pages: Memo<usize>,
    total_filtered: Signal<usize>,
) -> impl IntoView {
    view! {
        <div class="pagination">
            <span class="pagination-info">
                {move || {
                    let p = page_clamped.get();
                    let s = page_size.get();
                    let total = total_filtered.get();
                    if total == 0 {
                        "No results".to_string()
                    } else {
                        let start = p * s + 1;
                        let end = ((p + 1) * s).min(total);
                        format!("{start}–{end} of {total}")
                    }
                }}
            </span>
            <div class="pagination-controls">
                <select
                    on:change=move |ev| {
                        if let Ok(v) = event_target_value(&ev).parse::<usize>() {
                            page_size.set(v);
                        }
                    }
                >
                    {PAGE_SIZES.iter().map(|&s| view! {
                        <option value=s.to_string() selected=move || page_size.get() == s>
                            {format!("{s} / page")}
                        </option>
                    }).collect_view()}
                </select>
                <button
                    class="pag-btn"
                    disabled={move || page_clamped.get() == 0}
                    on:click=move |_| { let cur = page.get(); page.set(cur.saturating_sub(1)); }
                >
                    "‹ Prev"
                </button>
                <span class="pag-pages">
                    {move || format!("{} / {}", page_clamped.get() + 1, total_pages.get())}
                </span>
                <button
                    class="pag-btn"
                    disabled={move || page_clamped.get() + 1 >= total_pages.get()}
                    on:click=move |_| { let cur = page.get(); page.set(cur + 1); }
                >
                    "Next ›"
                </button>
            </div>
        </div>
    }
}

#[component]
fn DeviceListRow(
    device: DeviceState,
    show_media: RwSignal<bool>,
    /// Shared 1-second tick — used ONLY for relative-time display.
    /// Does NOT cause the rest of the row to re-render.
    timer_tick: RwSignal<u64>,
    busy_id: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
    notice: RwSignal<Option<String>>,
    auth_token: RwSignal<Option<String>>,
) -> impl IntoView {
    let ws = use_ws();

    // Capture the data we need from `device` at creation time.
    // The <For> parent re-creates this row whenever the device's data changes,
    // so these values are always fresh.
    let id = device.device_id.clone();
    let name = display_name(&device).to_string();
    let tone = status_tone(&device);
    let icon = status_icon_name(&device);
    let status = status_text(&device);
    let available = device.available;
    let area = display_area_value(device.area.as_deref());
    let device_type = presentation_device_type_label(&device).to_string();
    let plugin = device.plugin_id.clone();
    let media_summary = if is_media_player(&device) {
        media_summary(&device)
    } else {
        None
    };
    // Copy the timestamp out of the reference before `device` is moved into
    // DevicePrimaryAction.  The reactive closure captures the owned Copy value.
    let change_time: Option<chrono::DateTime<chrono::Utc>> =
        last_change_time(&device).copied();
    let change_text = change_summary(&device);
    let abs_time = format_abs(change_time.as_ref());

    let on_cmd =
        move |did: String, body: serde_json::Value, label: String, intent: CommandIntent| {
            let token = auth_token.get().unwrap_or_default();
            busy_id.set(Some(did.clone()));
            error.set(None);
            notice.set(None);
            spawn_local(async move {
                match set_device_state(&token, &did, &body).await {
                    Ok(_) => {
                        // Optimistic patch — update shared device map directly.
                        ws.devices.update(|m| {
                            if let Some(d) = m.get_mut(&did) {
                                match intent {
                                    CommandIntent::Toggle(on) => {
                                        d.attributes.insert("on".into(), serde_json::json!(on));
                                    }
                                    CommandIntent::Play => {
                                        d.attributes.insert(
                                            "state".into(),
                                            serde_json::json!("playing"),
                                        );
                                    }
                                    CommandIntent::Pause => {
                                        d.attributes.insert(
                                            "state".into(),
                                            serde_json::json!("paused"),
                                        );
                                    }
                                }
                                let now = chrono::Utc::now();
                                d.last_seen = Some(now);
                                d.last_change = Some(DeviceChange {
                                    changed_at: now,
                                    kind: DeviceChangeKind::Homecore,
                                    source: Some("api".into()),
                                    actor_id: None,
                                    actor_name: None,
                                    correlation_id: None,
                                });
                            }
                        });
                        notice.set(Some(format!("{label} sent")));
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy_id.set(None);
            });
        };

    let on_row_click = {
        let id = id.clone();
        move |_: web_sys::MouseEvent| {
            if let Some(win) = web_sys::window() {
                let _ = win.location().set_href(&format!("/devices/{id}"));
            }
        }
    };

    view! {
        <div class="device-row" class:offline=!available on:click=on_row_click>
            <div class="device-row-main">
                <div class="device-row-title">
                    <span class=format!("status-badge {}", tone.css_class()) title=status.clone()>
                        <span class="material-icons" style="font-size:18px">{icon}</span>
                    </span>
                    <div class="device-row-title-text">
                        <div class="device-row-name-line">
                            <a
                                class="cell-link"
                                href=format!("/devices/{}", id)
                                on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
                            >
                                {name.clone()}
                            </a>
                            <span class:chip-online=available class:chip-offline=!available>
                                {if available { "Online" } else { "Offline" }}
                            </span>
                        </div>
                        <div class="device-row-meta">
                            <span class="device-row-area">{area}</span>
                            <span class="device-row-type">{device_type}</span>
                            <span class="device-row-plugin">{plugin}</span>
                        </div>
                    </div>
                </div>
            </div>

            <div class="device-row-state">
                <span class="device-row-label">"State"</span>
                <div class="cell-primary">
                    <span>{status.clone()}</span>
                    {move || {
                        if show_media.get() {
                            media_summary.clone().map(|summary| view! {
                                <span class="cell-subtle">{summary}</span>
                            })
                        } else {
                            None
                        }
                    }}
                </div>
            </div>

            <div class="device-row-seen">
                <span class="device-row-label">"Last changed"</span>
                <div class="cell-primary">
                    // Reactive relative time — only this span re-renders on each tick.
                    <span>{move || { let _ = timer_tick.get(); format_relative(change_time.as_ref()) }}</span>
                    <span class="cell-subtle">{change_text.clone()}</span>
                    <span class="cell-subtle">{abs_time.clone()}</span>
                </div>
            </div>

            <div class="device-row-actions">
                <DevicePrimaryAction
                    device=device
                    device_id=id
                    busy_id=busy_id
                    on_cmd=on_cmd
                />
            </div>
        </div>
    }
}

#[component]
fn DevicePrimaryAction<F>(
    device: DeviceState,
    device_id: String,
    busy_id: RwSignal<Option<String>>,
    on_cmd: F,
) -> impl IntoView
where
    F: Fn(String, serde_json::Value, String, CommandIntent) + Clone + 'static,
{
    let did = device_id.clone();

    if supports_inline_toggle(&device) {
        let is_on = bool_attr(device.attributes.get("on")) == Some(true);
        let label = if is_on { "Turn off" } else { "Turn on" };
        let body = serde_json::json!({ "on": !is_on });
        let click_id = did.clone();
        view! {
            <button
                class="secondary device-action-control"
                disabled=move || busy_id.get().as_deref() == Some(&did)
                on:click=move |ev: web_sys::MouseEvent| {
                    ev.stop_propagation();
                    on_cmd.clone()(
                        click_id.clone(),
                        body.clone(),
                        label.to_string(),
                        CommandIntent::Toggle(!is_on),
                    );
                }
            >
                {label}
            </button>
        }
        .into_any()
    } else if is_media_player(&device) {
        let playback = playback_state(&device);
        if playback == "playing" && supports_action(&device, "pause") {
            let click_id = did.clone();
            view! {
                <button
                    class="secondary device-action-control"
                    disabled=move || busy_id.get().as_deref() == Some(&did)
                    on:click=move |ev: web_sys::MouseEvent| {
                        ev.stop_propagation();
                        on_cmd.clone()(
                            click_id.clone(),
                            serde_json::json!({"action":"pause"}),
                            "Pause".into(),
                            CommandIntent::Pause,
                        );
                    }
                >
                    "Pause"
                </button>
            }
            .into_any()
        } else if supports_action(&device, "play") {
            let click_id = did.clone();
            view! {
                <button
                    class="secondary device-action-control"
                    disabled=move || busy_id.get().as_deref() == Some(&did)
                    on:click=move |ev: web_sys::MouseEvent| {
                        ev.stop_propagation();
                        on_cmd.clone()(
                            click_id.clone(),
                            serde_json::json!({"action":"play"}),
                            "Play".into(),
                            CommandIntent::Play,
                        );
                    }
                >
                    "Play"
                </button>
            }
            .into_any()
        } else {
            view! {
                <a
                    class="secondary-link device-action-control"
                    href=format!("/devices/{}", device_id)
                    on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
                >
                    "Details"
                </a>
            }
            .into_any()
        }
    } else {
        view! {
            <a
                class="secondary-link device-action-control"
                href=format!("/devices/{}", device_id)
                on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
            >
                "Details"
            </a>
        }
        .into_any()
    }
}
