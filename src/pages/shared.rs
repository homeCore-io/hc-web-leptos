use crate::ws::WsStatus;
use gloo_timers::callback::Timeout;
use leptos::prelude::*;
use serde_json::{Map, Value};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardSize {
    Small,
    Medium,
    Large,
}

pub fn card_size_from_str(value: Option<&str>) -> CardSize {
    match value {
        Some("small") => CardSize::Small,
        Some("large") => CardSize::Large,
        _ => CardSize::Medium,
    }
}

pub fn card_size_to_str(value: CardSize) -> &'static str {
    match value {
        CardSize::Small => "small",
        CardSize::Medium => "medium",
        CardSize::Large => "large",
    }
}

pub fn card_size_canvas_class(value: CardSize) -> &'static str {
    match value {
        CardSize::Small => "cards-canvas cards-canvas--sm",
        CardSize::Medium => "cards-canvas cards-canvas--md",
        CardSize::Large => "cards-canvas cards-canvas--lg",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

pub fn sort_dir_from_str(value: Option<&str>) -> SortDir {
    if value == Some("desc") {
        SortDir::Desc
    } else {
        SortDir::Asc
    }
}

pub fn sort_dir_to_str(value: SortDir) -> &'static str {
    match value {
        SortDir::Asc => "asc",
        SortDir::Desc => "desc",
    }
}

pub fn toggle_sort_dir(value: &mut SortDir) {
    *value = match *value {
        SortDir::Asc => SortDir::Desc,
        SortDir::Desc => SortDir::Asc,
    };
}

pub fn ls_get(key: &str) -> Option<String> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(key).ok().flatten())
}

pub fn ls_set(key: &str, val: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(key, val);
    }
}

pub fn load_pref_json(key: &str) -> Option<Value> {
    let raw = ls_get(key)?;
    serde_json::from_str(&raw).ok()
}

pub fn json_str_set(v: &serde_json::Value, key: &str) -> HashSet<String> {
    v[key]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub fn set_to_json_array(s: &HashSet<String>) -> serde_json::Value {
    serde_json::Value::Array(
        s.iter()
            .map(|v| serde_json::Value::String(v.clone()))
            .collect(),
    )
}

#[derive(Debug, Clone)]
pub struct CommonCardPrefs<TSort> {
    pub card_size: CardSize,
    pub search: String,
    pub sort_by: TSort,
    pub sort_dir: SortDir,
}

pub fn load_common_card_prefs<TSort>(
    value: &Value,
    sort_from_str: impl FnOnce(Option<&str>) -> TSort,
) -> CommonCardPrefs<TSort> {
    CommonCardPrefs {
        card_size: card_size_from_str(value["card_size"].as_str()),
        search: value["search"].as_str().unwrap_or("").to_string(),
        sort_by: sort_from_str(value["sort_by"].as_str()),
        sort_dir: sort_dir_from_str(value["sort_dir"].as_str()),
    }
}

pub fn common_card_prefs_map<TSort>(
    prefs: &CommonCardPrefs<TSort>,
    sort_to_str: impl FnOnce(TSort) -> &'static str,
) -> Map<String, Value>
where
    TSort: Copy,
{
    let mut map = Map::new();
    map.insert(
        "card_size".to_string(),
        Value::String(card_size_to_str(prefs.card_size).to_string()),
    );
    map.insert("search".to_string(), Value::String(prefs.search.clone()));
    map.insert(
        "sort_by".to_string(),
        Value::String(sort_to_str(prefs.sort_by).to_string()),
    );
    map.insert(
        "sort_dir".to_string(),
        Value::String(sort_dir_to_str(prefs.sort_dir).to_string()),
    );
    map
}

#[component]
pub fn CardSizeSelect(card_size: RwSignal<CardSize>) -> impl IntoView {
    view! {
        <select
            on:change=move |ev| {
                card_size.set(match event_target_value(&ev).as_str() {
                    "small" => CardSize::Small,
                    "large" => CardSize::Large,
                    _ => CardSize::Medium,
                });
            }
        >
            <option value="small" selected={move || card_size.get() == CardSize::Small}>"Small"</option>
            <option value="medium" selected={move || card_size.get() == CardSize::Medium}>"Medium"</option>
            <option value="large" selected={move || card_size.get() == CardSize::Large}>"Large"</option>
        </select>
    }
}

#[component]
pub fn SearchField(search: RwSignal<String>, placeholder: &'static str) -> impl IntoView {
    // Internal signal updates immediately for responsive typing.
    // External signal is debounced by 250ms to avoid excessive re-filtering.
    let local = RwSignal::new(search.get_untracked());
    let pending = std::rc::Rc::new(std::cell::Cell::new(None::<Timeout>));

    view! {
        <input
            type="search"
            class="search-input"
            placeholder=placeholder
            prop:value=move || local.get()
            on:input={
                let pending = pending.clone();
                move |ev| {
                    let val = event_target_value(&ev);
                    local.set(val.clone());
                    // Cancel any pending timeout and schedule a new one.
                    pending.take();
                    let timeout = Timeout::new(250, move || {
                        search.set(val);
                    });
                    pending.set(Some(timeout));
                }
            }
        />
    }
}

#[component]
pub fn SortSelect(
    current_value: Signal<String>,
    #[prop(into)] options: Signal<Vec<(String, String)>>,
    on_change: Callback<String>,
) -> impl IntoView {
    view! {
        <select
            on:change=move |ev| on_change.run(event_target_value(&ev))
        >
            {move || {
                let current = current_value.get();
                options
                    .get()
                    .into_iter()
                    .map(|(value, label)| {
                        let is_selected = current == value;
                        view! {
                            <option value=value.clone() selected=is_selected>{label}</option>
                        }
                    })
                    .collect_view()
            }}
        </select>
    }
}

#[component]
pub fn SortDirToggle(sort_dir: RwSignal<SortDir>) -> impl IntoView {
    view! {
        <button
            class="filter-toggle"
            class:filter-toggle--active=move || sort_dir.get() == SortDir::Desc
            on:click=move |_| sort_dir.update(toggle_sort_dir)
        >
            {move || if sort_dir.get() == SortDir::Asc {
                view! { <span class="material-icons" style="font-size:16px">"arrow_upward"</span> }
            } else {
                view! { <span class="material-icons" style="font-size:16px">"arrow_downward"</span> }
            }}
        </button>
    }
}

#[component]
pub fn ResetFiltersButton(on_reset: Callback<()>) -> impl IntoView {
    view! {
        <button
            class="btn-outline"
            on:click=move |_| on_reset.run(())
        >
            "Reset"
        </button>
    }
}

#[component]
pub fn LiveStatusBanner(status: Signal<WsStatus>) -> impl IntoView {
    view! {
        {move || {
            let current = status.get();
            (current != WsStatus::Live).then(|| {
                let msg = match current {
                    WsStatus::Connecting => "Connecting to live updates…",
                    WsStatus::Disconnected => "Live updates lost — reconnecting…",
                    WsStatus::Live => unreachable!(),
                };
                view! { <p class="msg-warning">{msg}</p> }
            })
        }}
    }
}

/// Generic multi-select dropdown. `options` is `(value, display_label)`.
/// Empty `selected` means "no filter / show all".
#[component]
pub fn MultiSelectDropdown(
    /// Short category label shown in summary when items are selected, e.g. "Areas"
    label: &'static str,
    /// Text shown when nothing is selected
    placeholder: &'static str,
    #[prop(into)] options: Signal<Vec<(String, String)>>,
    selected: RwSignal<HashSet<String>>,
) -> impl IntoView {
    let open = RwSignal::new(false);

    let summary = move || {
        let sel = selected.get();
        if sel.is_empty() {
            placeholder.to_string()
        } else if sel.len() == 1 {
            sel.iter().next().unwrap().clone()
        } else {
            format!("{} {} selected", sel.len(), label)
        }
    };

    view! {
        <div class="multisel">
            <button
                class="multisel-trigger"
                class:multisel-trigger--active=move || !selected.get().is_empty()
                on:click=move |ev| {
                    ev.stop_propagation();
                    open.update(|v| *v = !*v);
                }
            >
                <span class="multisel-summary">{summary}</span>
                <span class="material-icons" style="font-size:14px">
                    {move || if open.get() { "expand_less" } else { "expand_more" }}
                </span>
            </button>
            {move || open.get().then(|| {
                let opts = options.get();
                view! {
                    <div
                        class="multisel-backdrop"
                        on:mousedown=move |_| open.set(false)
                    ></div>
                    <div class="multisel-dropdown">
                        {opts.into_iter().map(|(val, lbl)| {
                            let v_check = val.clone();
                            let v_toggle = val.clone();
                            view! {
                                <label class="multisel-option">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || selected.get().contains(&v_check)
                                        on:change=move |_| {
                                            let v = v_toggle.clone();
                                            selected.update(|s| {
                                                if s.contains(&v) {
                                                    s.remove(&v);
                                                } else {
                                                    s.insert(v);
                                                }
                                            });
                                        }
                                    />
                                    {lbl}
                                </label>
                            }
                        }).collect_view()}
                        {move || (!selected.get().is_empty()).then(|| view! {
                            <button
                                class="multisel-clear"
                                on:click=move |_| selected.set(HashSet::new())
                            >"Clear"</button>
                        })}
                    </div>
                }
            })}
        </div>
    }
}

// ── ErrorBanner ─────────────────────────────────────────────────────────────

/// Renders a `<p class="msg-error">` when the signal contains `Some(message)`.
/// Replaces the inline `{move || err.get().map(|e| view! { ... })}` pattern.
#[component]
pub fn ErrorBanner(#[prop(into)] error: Signal<Option<String>>) -> impl IntoView {
    view! {
        {move || error.get().map(|e| view! { <p class="msg-error">{e}</p> })}
    }
}

// ── Loading Skeletons ───────────────────────────────────────────────────────

/// Renders `count` skeleton row placeholders using existing CSS classes.
#[component]
pub fn SkeletonRows(#[prop(default = 6)] count: usize) -> impl IntoView {
    view! {
        <div class="skeleton-container">
            {(0..count).map(|_| view! { <div class="skeleton skeleton-row"></div> }).collect_view()}
        </div>
    }
}

/// Renders `count` skeleton card placeholders using existing CSS classes.
#[component]
pub fn SkeletonCards(#[prop(default = 6)] count: usize) -> impl IntoView {
    view! {
        <div class="skeleton-container" style="display:grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 0.75rem;">
            {(0..count).map(|_| view! { <div class="skeleton skeleton-card"></div> }).collect_view()}
        </div>
    }
}
