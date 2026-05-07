//! Overview / Dashboard page — `/dashboards`
//!
//! Single admin overview page backed by the dashboard API.  On first visit,
//! creates a sensible default dashboard with overview counters, mode chips,
//! and scene buttons.  All cards are live via WebSocket.

use crate::api::{
    activate_scene, create_dashboard, fetch_battery_settings, fetch_dashboards, fetch_devices,
    fetch_scenes, set_default_dashboard, set_device_state, update_dashboard,
};
use crate::auth::use_auth;
use crate::models::*;
use crate::pages::device_cards::DeviceCard;
use crate::pages::shared::{ErrorBanner, SkeletonCards};
use crate::ws::use_ws;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::{json, Value};

// ── Option data ─────────────────────────────────────────────────────────────

fn device_type_options() -> Vec<(&'static str, &'static str)> {
    vec![
        ("", "Any"),
        ("light", "Light"),
        ("dimmer", "Dimmer"),
        ("switch", "Switch"),
        ("lock", "Lock"),
        ("shade", "Shade"),
        ("contact_sensor", "Contact Sensor"),
        ("motion_sensor", "Motion Sensor"),
        ("occupancy_sensor", "Occupancy Sensor"),
        ("leak_sensor", "Leak Sensor"),
        ("vibration_sensor", "Vibration Sensor"),
        ("temperature_sensor", "Temperature Sensor"),
        ("humidity_sensor", "Humidity Sensor"),
        ("environment_sensor", "Temp / Humidity"),
        ("media_player", "Media Player"),
        ("keypad", "Keypad"),
        ("remote", "Remote"),
        ("timer", "Timer"),
        ("sensor", "Sensor"),
        ("device", "Other Device"),
    ]
}

fn attribute_options() -> Vec<(&'static str, &'static str)> {
    vec![
        ("on", "On / Off"),
        ("contact", "Contact (open/closed)"),
        ("motion", "Motion"),
        ("locked", "Locked"),
        ("leak", "Leak detected"),
        ("water", "Water detected"),
        ("occupied", "Occupied"),
        ("vibration", "Vibration"),
        ("temperature", "Temperature"),
        ("humidity", "Humidity"),
        ("brightness_pct", "Brightness %"),
        ("position", "Position"),
        ("battery", "Battery"),
    ]
}

fn value_options_for_attribute(attr: &str) -> Vec<(&'static str, &'static str)> {
    match attr {
        "on" | "locked" | "motion" | "occupied" | "vibration" | "leak" | "water" => {
            vec![("true", "True / Yes"), ("false", "False / No")]
        }
        "contact" => vec![("open", "Open"), ("closed", "Closed")],
        _ => vec![("true", "True"), ("false", "False")],
    }
}

/// Icon options for dashboard widgets. Values are Phosphor identifiers
/// (slot into "ph ph-{name}" by the view).
fn icon_options() -> Vec<(&'static str, &'static str)> {
    vec![
        ("lightbulb", "Lightbulb"),
        ("door", "Door"),
        ("frame-corners", "Window"),
        ("lock", "Lock"),
        ("lock-open", "Lock Open"),
        ("drop", "Water / Leak"),
        ("wifi-slash", "Offline"),
        ("person-simple-walk", "Motion"),
        ("thermometer-simple", "Thermostat"),
        ("shield", "Shield"),
        ("broadcast", "Sensors"),
        ("eye", "Eye"),
        ("power", "Power"),
        ("lightning", "Bolt"),
        ("house", "Home"),
        ("info", "Info"),
        ("warning", "Warning"),
        ("warning-circle", "Error"),
    ]
}

/// Pre-built overview counter configurations.
/// Icon names are Phosphor identifiers (slot into "ph ph-{name}" by the view).
fn overview_presets() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        (
            "Lights On",
            "lightbulb",
            json!({"counter_type":"device_filter","device_type":"light,dimmer","attribute":"on","value":true,"icon":"lightbulb","link_url":"/devices","metrics":["custom"],"card_size":"small"}),
        ),
        (
            "Open Doors",
            "door",
            json!({"counter_type":"device_filter","device_type":"contact_sensor","attribute":"contact","value":"open","icon":"door","link_url":"/devices","metrics":["custom"],"card_size":"small"}),
        ),
        (
            "Leak Sensors",
            "drop",
            json!({"counter_type":"device_filter","device_type":"leak_sensor","attribute":"leak","value":true,"icon":"drop","link_url":"/devices","metrics":["custom"],"card_size":"small"}),
        ),
        (
            "Offline Devices",
            "wifi-slash",
            json!({"counter_type":"availability","icon":"wifi-slash","link_url":"/devices","metrics":["custom"],"card_size":"small"}),
        ),
        (
            "Motion Active",
            "person-simple-walk",
            json!({"counter_type":"device_filter","device_type":"motion_sensor","attribute":"motion","value":true,"icon":"person-simple-walk","link_url":"/devices","metrics":["custom"],"card_size":"small"}),
        ),
        (
            "Locks Unlocked",
            "lock-open",
            json!({"counter_type":"device_filter","device_type":"lock","attribute":"locked","value":false,"icon":"lock-open","link_url":"/devices","metrics":["custom"],"card_size":"small"}),
        ),
    ]
}

// ── Page ────────────────────────────────────────────────────────────────────

#[component]
pub fn DashboardsPage() -> impl IntoView {
    let auth = use_auth();
    let ws = use_ws();
    let dashboard: RwSignal<Option<DashboardResponse>> = RwSignal::new(None);
    let loading = RwSignal::new(true);
    let error = RwSignal::new(Option::<String>::None);

    // Seed the shared device map from REST (same as DeviceCardsPage).
    Effect::new(move |_| {
        let token = auth.token_str().unwrap_or_default();
        spawn_local(async move {
            if let Ok(list) = fetch_devices(&token).await {
                ws.devices.update(|m| {
                    for d in list {
                        m.insert(d.device_id.clone(), d);
                    }
                });
            }
        });
    });

    Effect::new(move |_| {
        let token = match auth.token.get() {
            Some(t) => t,
            None => return,
        };
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match load_or_create_dashboard(&token).await {
                Ok(db) => dashboard.set(Some(db)),
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    });

    view! {
        <div class="page">
            <ErrorBanner error=error />
            {move || {
                if loading.get() {
                    view! { <SkeletonCards count=6 /> }.into_any()
                } else if let Some(db) = dashboard.get() {
                    view! { <DashboardView initial=db /> }.into_any()
                } else {
                    view! {
                        <div class="hc-empty">
                            <i class="ph ph-gauge hc-empty__icon"></i>
                            <div class="hc-empty__title">"No dashboard"</div>
                            <p class="hc-empty__body">
                                "Dashboards collect cards, scenes, and stat chips into a customized \
                                 home screen. Once one exists, it'll appear here."
                            </p>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

fn default_overview_widgets() -> Vec<DashboardWidget> {
    vec![
        // Anchor: full-width "House Status" hero. Tile set defaults to
        // all 6 systems; the renderer auto-hides any with no relevant
        // devices in the current device map.
        DashboardWidget {
            id: "house_status".into(),
            r#type: DashboardWidgetType::HouseStatusHero,
            title: "House Status".into(),
            subtitle: None,
            refresh_policy: DashboardRefreshPolicy::Live,
            config: json!({
                "systems": ["lighting", "climate", "security", "battery", "media", "energy", "activity"],
                "layout": "wide",
            }),
        },
        // Quick Actions starter pack: modes + scenes for one-tap control.
        DashboardWidget {
            id: "modes".into(),
            r#type: DashboardWidgetType::ModeChips,
            title: "Modes".into(),
            subtitle: None,
            refresh_policy: DashboardRefreshPolicy::Live,
            config: json!({"card_size": "medium"}),
        },
        DashboardWidget {
            id: "scenes".into(),
            r#type: DashboardWidgetType::SceneRow,
            title: "Scenes".into(),
            subtitle: None,
            refresh_policy: DashboardRefreshPolicy::Live,
            config: json!({"card_size": "medium"}),
        },
    ]
}

async fn load_or_create_dashboard(token: &str) -> Result<DashboardResponse, String> {
    let dashboards = fetch_dashboards(token).await?;
    if dashboards.is_empty() {
        let def = DashboardDefinition {
            id: String::new(),
            name: "Overview".into(),
            description: Some("Default admin overview".into()),
            owner_user_id: String::new(),
            visibility: DashboardVisibility::Private,
            tags: vec![],
            icon: "dashboard".into(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            sections: vec![],
            layouts: vec![],
            widgets: default_overview_widgets(),
        };
        let created = create_dashboard(token, &def).await?;
        let _ = set_default_dashboard(token, &created.dashboard.id).await;
        return Ok(DashboardResponse {
            is_default: true,
            ..created
        });
    }

    let mut db = dashboards
        .iter()
        .find(|d| d.is_default)
        .cloned()
        .unwrap_or_else(|| dashboards[0].clone());

    // Migrate stale template dashboards: if layouts reference widgets that
    // don't exist in the widget list, strip the layouts and sections so saves
    // don't fail validation.  Also replace empty/template widgets with defaults.
    let widget_ids: std::collections::HashSet<String> =
        db.dashboard.widgets.iter().map(|w| w.id.clone()).collect();
    let stale_layouts = db.dashboard.layouts.iter().any(|l| {
        l.placements
            .iter()
            .any(|p| !widget_ids.contains(&p.widget_id))
    });
    if stale_layouts {
        db.dashboard.layouts.clear();
        db.dashboard.sections.clear();
        // If widgets are all from a template (no counter_type, no card_size),
        // replace with our defaults.
        let has_our_widgets =
            db.dashboard.widgets.iter().any(|w| {
                w.config.get("card_size").is_some() || w.config.get("counter_type").is_some()
            });
        if !has_our_widgets {
            db.dashboard.widgets = default_overview_widgets();
        }
        // Save the cleaned version back.
        let _ = update_dashboard(token, &db.dashboard.id, &db.dashboard).await;
    }

    Ok(db)
}

// ── Dashboard View ──────────────────────────────────────────────────────────

#[component]
fn DashboardView(initial: DashboardResponse) -> impl IntoView {
    let auth = use_auth();
    let original = RwSignal::new(initial.dashboard.clone());
    let dashboard_id = initial.dashboard.id.clone();
    let widgets: RwSignal<Vec<DashboardWidget>> = RwSignal::new(initial.dashboard.widgets.clone());
    let edit_mode = RwSignal::new(false);
    let editing_widget: RwSignal<Option<String>> = RwSignal::new(None);
    let saving = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);

    let id_for_save = dashboard_id.clone();
    let on_save = Callback::new(move |_: ()| {
        let token = auth.token_str().unwrap_or_default();
        let mut def = original.get_untracked();
        def.widgets = widgets.get_untracked();
        // Clean up layouts: remove placements referencing deleted widgets
        let live_ids: std::collections::HashSet<String> =
            def.widgets.iter().map(|w| w.id.clone()).collect();
        for layout in &mut def.layouts {
            layout
                .placements
                .retain(|p| live_ids.contains(&p.widget_id));
        }
        // Remove sections referencing breakpoints with no placements
        let active_breakpoints: std::collections::HashSet<_> = def
            .layouts
            .iter()
            .filter(|l| !l.placements.is_empty())
            .map(|l| l.breakpoint)
            .collect();
        def.sections
            .retain(|s| active_breakpoints.contains(&s.breakpoint));
        let id = id_for_save.clone();
        saving.set(true);
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match update_dashboard(&token, &id, &def).await {
                Ok(resp) => {
                    original.set(resp.dashboard);
                    notice.set(Some("Dashboard saved.".into()));
                    edit_mode.set(false);
                }
                Err(e) => error.set(Some(format!("Save failed: {e}"))),
            }
            saving.set(false);
        });
    });

    let on_reset = Callback::new(move |_: ()| {
        widgets.set(original.get_untracked().widgets.clone());
        editing_widget.set(None);
        notice.set(Some("Reset to last saved state.".into()));
    });

    // Reset to the factory default dashboard (hero + Quick Actions).
    // User must Save to persist; gives them a chance to back out.
    let on_factory_reset = Callback::new(move |_: ()| {
        widgets.set(default_overview_widgets());
        editing_widget.set(None);
        notice.set(Some(
            "Loaded default layout (hero + Quick Actions). Click Save to keep it.".into(),
        ));
    });

    view! {
        <div class="dashboard-header">
            <h1>"Overview"</h1>
            <DashboardToolbar
                edit_mode=edit_mode
                saving=saving
                on_save=on_save
                on_reset=on_reset
                on_factory_reset=on_factory_reset
            />
        </div>
        <ErrorBanner error=error />
        {move || notice.get().map(|n| view! { <p class="msg-notice">{n}</p> })}
        <WidgetGrid widgets=widgets edit_mode=edit_mode editing_widget=editing_widget />
        {move || edit_mode.get().then(|| view! { <AddCardPanel widgets=widgets /> })}
    }
}

#[component]
fn DashboardToolbar(
    edit_mode: RwSignal<bool>,
    saving: RwSignal<bool>,
    on_save: Callback<()>,
    on_reset: Callback<()>,
    on_factory_reset: Callback<()>,
) -> impl IntoView {
    view! {
        <div class="dashboard-toolbar">
            <button class="btn btn-outline" on:click=move |_| edit_mode.update(|v| *v = !*v)>
                <i class=move || if edit_mode.get() { "ph ph-x" } else { "ph ph-pencil-simple" } style="font-size:16px"></i>
                {move || if edit_mode.get() { " Done" } else { " Edit" }}
            </button>
            {move || edit_mode.get().then(|| view! {
                <button class="btn btn-primary" disabled=move || saving.get() on:click=move |_| on_save.run(())>
                    {move || if saving.get() { "Saving..." } else { "Save" }}
                </button>
                <button class="btn btn-outline" on:click=move |_| on_reset.run(())>"Revert"</button>
                <button
                    class="btn btn-outline"
                    title="Replace widgets with the default Overview layout (hero + Quick Actions). Save to persist."
                    on:click=move |_| on_factory_reset.run(())
                >"Load default"</button>
            })}
        </div>
    }
}

// ── Widget Grid ─────────────────────────────────────────────────────────────

#[component]
fn WidgetGrid(
    widgets: RwSignal<Vec<DashboardWidget>>,
    edit_mode: RwSignal<bool>,
    editing_widget: RwSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <div class="dashboard-grid">
            {move || {
                widgets.get().into_iter().enumerate().map(|(idx, widget)| {
                    let wid = widget.id.clone();
                    let wtype = widget.r#type;
                    let title = widget.title.clone();
                    let config = widget.config.clone();
                    let size = config.get("card_size").and_then(|v| v.as_str()).unwrap_or("medium");
                    // Pill-type widgets get transparent wrapper; card-type get bordered wrapper
                    let is_pill = matches!(wtype,
                        DashboardWidgetType::ModeChips | DashboardWidgetType::SceneRow
                    ) || (wtype == DashboardWidgetType::StatSummary && (
                        config.get("counter_type").and_then(|v| v.as_str()).is_some()
                        || config.get("chip_mode").and_then(|v| v.as_bool()).unwrap_or(false)
                    ));
                    // Hero widget: layout is config-driven.
                    //   "wide"    → full canvas width, tiles in a horizontal row (default)
                    //   "compact" → narrow column alongside other widgets, tiles stacked vertically
                    let is_hero = wtype == DashboardWidgetType::HouseStatusHero;
                    let hero_layout = config
                        .get("layout")
                        .and_then(|v| v.as_str())
                        .unwrap_or("wide");
                    let hero_layout_class = match hero_layout {
                        "compact" => "dashboard-widget--hero-compact",
                        _ => "dashboard-widget--hero-wide",
                    };
                    let size_class: String = if is_hero {
                        format!("dashboard-widget dashboard-widget--hero {hero_layout_class}")
                    } else if is_pill {
                        match size {
                            "small" => "dashboard-widget dashboard-widget--pill dashboard-widget--sm".into(),
                            "large" => "dashboard-widget dashboard-widget--pill dashboard-widget--lg".into(),
                            _ => "dashboard-widget dashboard-widget--pill dashboard-widget--md".into(),
                        }
                    } else {
                        match size {
                            "small" => "dashboard-widget dashboard-widget--sm".into(),
                            "large" => "dashboard-widget dashboard-widget--lg".into(),
                            _ => "dashboard-widget dashboard-widget--md".into(),
                        }
                    };

                    let wid_rm = wid.clone();
                    let wid_cfg = wid.clone();
                    let wid_sz = wid.clone();
                    let wid_ed = wid.clone();
                    let wid_ed2 = wid.clone();

                    let card_content = match wtype {
                        DashboardWidgetType::DeviceTile => view! { <SingleDeviceCard config=config.clone() /> }.into_any(),
                        DashboardWidgetType::DeviceGrid => view! { <EntitiesCard title=title.clone() config=config.clone() /> }.into_any(),
                        DashboardWidgetType::StatSummary => {
                            let ct = config.get("counter_type").and_then(|v| v.as_str()).unwrap_or("");
                            if config.get("chip_mode").and_then(|v| v.as_bool()).unwrap_or(false) {
                                view! { <StatChipsCard config=config.clone() /> }.into_any()
                            } else if ct == "device_filter" || ct == "availability" {
                                view! { <OverviewCard title=title.clone() config=config.clone() /> }.into_any()
                            } else {
                                view! { <GenericStatCard title=title.clone() config=config.clone() /> }.into_any()
                            }
                        },
                        DashboardWidgetType::ModeChips => view! { <ModeChipsCard /> }.into_any(),
                        DashboardWidgetType::SceneRow => view! { <SceneButtonsCard /> }.into_any(),
                        DashboardWidgetType::HouseStatusHero => view! { <HouseStatusHero config=config.clone() /> }.into_any(),
                        _ => view! { <div class="dashboard-widget-fallback"><span class="cell-subtle">{format!("{:?}", wtype)}</span></div> }.into_any(),
                    };

                    view! {
                        <div class=size_class>
                            {edit_mode.get().then(|| {
                                view! {
                                    <div class="widget-edit-bar">
                                        <button class="widget-edit-btn" title="Move up"
                                            on:click=move |_| widgets.update(|w| { if idx > 0 { w.swap(idx, idx - 1); } })
                                        ><i class="ph ph-arrow-up" style="font-size:16px"></i></button>
                                        <button class="widget-edit-btn" title="Move down"
                                            on:click=move |_| widgets.update(|w| { if idx + 1 < w.len() { w.swap(idx, idx + 1); } })
                                        ><i class="ph ph-arrow-down" style="font-size:16px"></i></button>
                                        <SizeToggle widget_id=wid_sz.clone() widgets=widgets />
                                        <button class="widget-edit-btn" title="Configure"
                                            on:click=move |_| {
                                                let cur = editing_widget.get_untracked();
                                                if cur.as_deref() == Some(wid_cfg.as_str()) { editing_widget.set(None); }
                                                else { editing_widget.set(Some(wid_cfg.clone())); }
                                            }
                                        ><i class="ph ph-gear" style="font-size:16px"></i></button>
                                        <button class="widget-edit-btn widget-edit-btn--danger" title="Remove"
                                            on:click=move |_| { let id = wid_rm.clone(); widgets.update(|w| w.retain(|x| x.id != id)); }
                                        ><i class="ph ph-x" style="font-size:16px"></i></button>
                                    </div>
                                }
                            })}

                            {card_content}

                            {(edit_mode.get() && editing_widget.get().as_deref() == Some(wid_ed.as_str())).then(|| {
                                view! { <WidgetConfigEditor widget_id=wid_ed2.clone() widgets=widgets on_close=Callback::new(move |_: ()| editing_widget.set(None)) /> }
                            })}
                        </div>
                    }
                }).collect_view()
            }}
        </div>
    }
}

#[component]
fn SizeToggle(widget_id: String, widgets: RwSignal<Vec<DashboardWidget>>) -> impl IntoView {
    let wid = widget_id;
    view! {
        <button class="widget-edit-btn" title="Cycle size (S/M/L)" on:click=move |_| {
            let wid = wid.clone();
            widgets.update(|w| {
                if let Some(widget) = w.iter_mut().find(|x| x.id == wid) {
                    let current = widget.config.get("card_size").and_then(|v| v.as_str()).unwrap_or("medium");
                    let next = match current { "small" => "medium", "medium" => "large", _ => "small" };
                    widget.config.as_object_mut().map(|m| m.insert("card_size".into(), json!(next)));
                }
            });
        }>
            <i class="ph ph-frame-corners" style="font-size:16px"></i>
        </button>
    }
}

// ── Widget Config Editor ────────────────────────────────────────────────────

#[component]
fn WidgetConfigEditor(
    widget_id: String,
    widgets: RwSignal<Vec<DashboardWidget>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let ws = use_ws();
    let wid = widget_id.clone();

    // Get current widget snapshot
    let current = widgets.get_untracked().into_iter().find(|w| w.id == wid);
    let Some(widget) = current else {
        return view! { <p class="cell-subtle">"Widget not found"</p> }.into_any();
    };

    let wtype = widget.r#type;
    let config = widget.config.clone();
    let title_input = RwSignal::new(widget.title.clone());

    let device_options = Memo::new(move |_| {
        let mut opts: Vec<(String, String)> = ws
            .devices
            .get()
            .values()
            .filter(|d| !d.device_id.starts_with("mode_"))
            .map(|d| {
                (
                    d.device_id.clone(),
                    format!("{} ({})", display_name(d), d.area.as_deref().unwrap_or("—")),
                )
            })
            .collect();
        opts.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        opts
    });

    match wtype {
        DashboardWidgetType::DeviceTile => {
            let current_id = config
                .get("device_ids")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let selected = RwSignal::new(current_id);
            let wid = widget_id.clone();
            view! {
                <div class="widget-config-editor">
                    <label>"Device"</label>
                    <select class="input" prop:value=move || selected.get() on:change=move |ev| selected.set(event_target_value(&ev))>
                        <option value="">"Select..."</option>
                        <For each=move || device_options.get() key=|(id, _)| id.clone()
                            children=move |(id, name)| view! { <option value=id.clone()>{name}</option> } />
                    </select>
                    <div class="widget-config-actions">
                        <button class="btn btn-primary btn-sm" on:click=move |_| {
                            let wid = wid.clone();
                            let did = selected.get_untracked();
                            widgets.update(|w| { if let Some(widget) = w.iter_mut().find(|x| x.id == wid) {
                                widget.config.as_object_mut().map(|m| {
                                    m.insert("device_ids".into(), json!([did]));
                                    m.entry("selection_mode").or_insert(json!("manual"));
                                });
                            }});
                            on_close.run(());
                        }>"Apply"</button>
                        <button class="btn btn-outline btn-sm" on:click=move |_| on_close.run(())>"Cancel"</button>
                    </div>
                </div>
            }.into_any()
        }
        DashboardWidgetType::DeviceGrid => {
            let current_ids: Vec<String> = config
                .get("device_ids")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let selected_ids: RwSignal<Vec<String>> = RwSignal::new(current_ids);
            let wid = widget_id.clone();
            view! {
                <div class="widget-config-editor">
                    <label>"Title"</label>
                    <input class="input" type="text" prop:value=move || title_input.get() on:input=move |ev| title_input.set(event_target_value(&ev)) />
                    <label>"Devices"</label>
                    <DeviceCheckboxList device_options=device_options selected=selected_ids />
                    <div class="widget-config-actions">
                        <button class="btn btn-primary btn-sm" on:click=move |_| {
                            let wid = wid.clone();
                            let ids = selected_ids.get_untracked();
                            let title = title_input.get_untracked();
                            widgets.update(|w| { if let Some(widget) = w.iter_mut().find(|x| x.id == wid) {
                                widget.title = title;
                                widget.config.as_object_mut().map(|m| {
                                    m.insert("device_ids".into(), json!(ids));
                                    m.entry("selection_mode").or_insert(json!("manual"));
                                });
                            }});
                            on_close.run(());
                        }>"Apply"</button>
                        <button class="btn btn-outline btn-sm" on:click=move |_| on_close.run(())>"Cancel"</button>
                    </div>
                </div>
            }.into_any()
        }
        DashboardWidgetType::StatSummary => {
            let is_chip = config
                .get("chip_mode")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_chip {
                // Stat chips editor: device picker + attribute selector
                let current_ids: Vec<String> = config
                    .get("device_ids")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let current_attrs: Vec<String> = config
                    .get("attributes")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_else(|| vec!["temperature".into(), "humidity".into()]);
                let selected_ids: RwSignal<Vec<String>> = RwSignal::new(current_ids);
                let sel_on = RwSignal::new(current_attrs.contains(&"on".into()));
                let sel_contact = RwSignal::new(current_attrs.contains(&"contact".into()));
                let sel_locked = RwSignal::new(current_attrs.contains(&"locked".into()));
                let sel_motion = RwSignal::new(current_attrs.contains(&"motion".into()));
                let sel_temp = RwSignal::new(current_attrs.contains(&"temperature".into()));
                let sel_hum = RwSignal::new(current_attrs.contains(&"humidity".into()));
                let sel_bat = RwSignal::new(current_attrs.contains(&"battery".into()));
                let sel_lux = RwSignal::new(current_attrs.contains(&"illuminance".into()));
                let wid = widget_id.clone();
                view! {
                    <div class="widget-config-editor">
                        <label>"Devices"</label>
                        <DeviceCheckboxList device_options=device_options selected=selected_ids />
                        <label>"Attributes to show (leave unchecked for device status)"</label>
                        <div class="widget-config-checkrow">
                            <label class="device-checkbox-item"><input type="checkbox" prop:checked=move || sel_on.get() on:change=move |_| sel_on.update(|v| *v = !*v) />" On/Off"</label>
                            <label class="device-checkbox-item"><input type="checkbox" prop:checked=move || sel_contact.get() on:change=move |_| sel_contact.update(|v| *v = !*v) />" Contact"</label>
                            <label class="device-checkbox-item"><input type="checkbox" prop:checked=move || sel_locked.get() on:change=move |_| sel_locked.update(|v| *v = !*v) />" Locked"</label>
                            <label class="device-checkbox-item"><input type="checkbox" prop:checked=move || sel_motion.get() on:change=move |_| sel_motion.update(|v| *v = !*v) />" Motion"</label>
                            <label class="device-checkbox-item"><input type="checkbox" prop:checked=move || sel_temp.get() on:change=move |_| sel_temp.update(|v| *v = !*v) />" Temperature"</label>
                            <label class="device-checkbox-item"><input type="checkbox" prop:checked=move || sel_hum.get() on:change=move |_| sel_hum.update(|v| *v = !*v) />" Humidity"</label>
                            <label class="device-checkbox-item"><input type="checkbox" prop:checked=move || sel_bat.get() on:change=move |_| sel_bat.update(|v| *v = !*v) />" Battery"</label>
                            <label class="device-checkbox-item"><input type="checkbox" prop:checked=move || sel_lux.get() on:change=move |_| sel_lux.update(|v| *v = !*v) />" Illuminance"</label>
                        </div>
                        <div class="widget-config-actions">
                            <button class="btn btn-primary btn-sm" on:click=move |_| {
                                let wid = wid.clone();
                                let ids = selected_ids.get_untracked();
                                let mut attrs: Vec<&str> = Vec::new();
                                if sel_on.get_untracked() { attrs.push("on"); }
                                if sel_contact.get_untracked() { attrs.push("contact"); }
                                if sel_locked.get_untracked() { attrs.push("locked"); }
                                if sel_motion.get_untracked() { attrs.push("motion"); }
                                if sel_temp.get_untracked() { attrs.push("temperature"); }
                                if sel_hum.get_untracked() { attrs.push("humidity"); }
                                if sel_bat.get_untracked() { attrs.push("battery"); }
                                if sel_lux.get_untracked() { attrs.push("illuminance"); }
                                // Empty attrs = show status_text fallback
                                widgets.update(|w| { if let Some(widget) = w.iter_mut().find(|x| x.id == wid) {
                                    widget.config = json!({"chip_mode":true,"device_ids":ids,"attributes":attrs,"metrics":["custom"],"card_size":"large"});
                                }});
                                on_close.run(());
                            }>"Apply"</button>
                            <button class="btn btn-outline btn-sm" on:click=move |_| on_close.run(())>"Cancel"</button>
                        </div>
                    </div>
                }.into_any()
            } else {
                // Overview counter editor
                let ct = config
                    .get("counter_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("device_filter")
                    .to_string();
                let dt = RwSignal::new(
                    config
                        .get("device_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                );
                let attr = RwSignal::new(
                    config
                        .get("attribute")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                );
                let val = RwSignal::new(
                    config
                        .get("value")
                        .map(|v| match v {
                            Value::Bool(b) => b.to_string(),
                            Value::String(s) => s.clone(),
                            _ => v.to_string(),
                        })
                        .unwrap_or_default(),
                );
                let icon = RwSignal::new(
                    config
                        .get("icon")
                        .and_then(|v| v.as_str())
                        .unwrap_or("info")
                        .to_string(),
                );
                let ds = RwSignal::new(
                    config
                        .get("display_style")
                        .and_then(|v| v.as_str())
                        .unwrap_or("badge")
                        .to_string(),
                );
                let is_avail = ct == "availability";
                let wid = widget_id.clone();
                view! {
                    <div class="widget-config-editor">
                        <label>"Title"</label>
                        <input class="input" type="text" prop:value=move || title_input.get() on:input=move |ev| title_input.set(event_target_value(&ev)) />
                        <label>"Display style"</label>
                        <select class="input" prop:value=move || ds.get() on:change=move |ev| ds.set(event_target_value(&ev))>
                            <option value="badge">"Badge (two-line, larger)"</option>
                            <option value="chip">"Chip (single-line, compact)"</option>
                        </select>
                        <label>"Icon"</label>
                        <select class="input" prop:value=move || icon.get() on:change=move |ev| icon.set(event_target_value(&ev))>
                            {icon_options().into_iter().map(|(v, l)| view! { <option value=v>{l}</option> }).collect_view()}
                        </select>
                        {(!is_avail).then(|| view! {
                            <label>"Device type"</label>
                            <select class="input" prop:value=move || dt.get() on:change=move |ev| dt.set(event_target_value(&ev))>
                                {device_type_options().into_iter().map(|(v, l)| view! { <option value=v>{l}</option> }).collect_view()}
                            </select>
                            <label>"Attribute"</label>
                            <select class="input" prop:value=move || attr.get() on:change=move |ev| attr.set(event_target_value(&ev))>
                                <option value="">"Any"</option>
                                {attribute_options().into_iter().map(|(v, l)| view! { <option value=v>{l}</option> }).collect_view()}
                            </select>
                            <label>"Value"</label>
                            <select class="input" prop:value=move || val.get() on:change=move |ev| val.set(event_target_value(&ev))>
                                <option value="">"Any"</option>
                                {move || {
                                    let a = attr.get();
                                    value_options_for_attribute(&a).into_iter().map(|(v, l)| view! { <option value=v>{l}</option> }).collect_view()
                                }}
                            </select>
                        })}
                        <div class="widget-config-actions">
                            <button class="btn btn-primary btn-sm" on:click=move |_| {
                                let wid = wid.clone();
                                let title = title_input.get_untracked();
                                let icon_val = icon.get_untracked();
                                let ds_val = ds.get_untracked();
                                let config = if is_avail {
                                    json!({"counter_type":"availability","icon":icon_val,"display_style":ds_val,"link_url":"/devices","metrics":["custom"],"card_size":"small"})
                                } else {
                                    let val_str = val.get_untracked();
                                    let parsed_val: Value = match val_str.as_str() { "true" => Value::Bool(true), "false" => Value::Bool(false), s if s.parse::<f64>().is_ok() => json!(s.parse::<f64>().unwrap()), s => Value::String(s.to_string()) };
                                    json!({"counter_type":"device_filter","device_type":dt.get_untracked(),"attribute":attr.get_untracked(),"value":parsed_val,"icon":icon_val,"display_style":ds_val,"link_url":"/devices","metrics":["custom"],"card_size":"small"})
                                };
                                widgets.update(|w| { if let Some(widget) = w.iter_mut().find(|x| x.id == wid) { widget.title = title; widget.config = config; }});
                                on_close.run(());
                            }>"Apply"</button>
                            <button class="btn btn-outline btn-sm" on:click=move |_| on_close.run(())>"Cancel"</button>
                        </div>
                    </div>
                }.into_any()
            }
        }
        DashboardWidgetType::HouseStatusHero => {
            // Layout: "wide" (full canvas, horizontal tiles) or "compact"
            // (narrow column, vertical tiles).
            let cur_layout = config
                .get("layout")
                .and_then(|v| v.as_str())
                .unwrap_or("wide")
                .to_string();
            let layout_sig = RwSignal::new(cur_layout);

            // Per-system checkboxes. Render in a fixed canonical order;
            // the user's existing ordering (if customized) is preserved
            // for systems they've enabled and present in their config.
            const ALL_SYSTEMS: &[(&str, &str)] = &[
                ("lighting", "Lighting"),
                ("climate", "Climate"),
                ("security", "Security"),
                ("battery", "Battery"),
                ("media", "Media"),
                ("energy", "Energy"),
                ("activity", "Activity"),
            ];
            let cur_systems: std::collections::HashSet<String> = config
                .get("systems")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_else(|| ALL_SYSTEMS.iter().map(|(k, _)| k.to_string()).collect());
            let systems_sig: RwSignal<std::collections::HashSet<String>> =
                RwSignal::new(cur_systems);

            let wid = widget_id.clone();
            view! {
                <div class="widget-config-editor">
                    <label>"Layout"</label>
                    <select
                        class="input"
                        prop:value=move || layout_sig.get()
                        on:change=move |ev| layout_sig.set(event_target_value(&ev))
                    >
                        <option value="wide">"Wide — full row across the canvas"</option>
                        <option value="compact">"Compact — narrow column, vertical tiles"</option>
                    </select>

                    <label>"Systems shown"</label>
                    <div class="device-checkbox-list">
                        {ALL_SYSTEMS.iter().map(|(key, label)| {
                            let key_str = key.to_string();
                            let key_for_check = key_str.clone();
                            let key_for_toggle = key_str.clone();
                            let is_checked = move || systems_sig.get().contains(&key_for_check);
                            view! {
                                <label class="device-checkbox-row">
                                    <input
                                        type="checkbox"
                                        prop:checked=is_checked
                                        on:change=move |ev| {
                                            let checked = event_target_checked(&ev);
                                            let k = key_for_toggle.clone();
                                            systems_sig.update(|s| {
                                                if checked { s.insert(k); } else { s.remove(&k); }
                                            });
                                        }
                                    />
                                    <span>{*label}</span>
                                </label>
                            }
                        }).collect_view()}
                    </div>
                    <p class="cell-subtle" style="font-size:0.78rem; margin-top:0.4rem;">
                        "Systems with no relevant devices in your live device map are auto-hidden anyway. \
                         The battery alert threshold is configured server-side in homecore.toml ([battery] threshold_pct)."
                    </p>

                    <div class="widget-config-actions">
                        <button class="btn btn-primary btn-sm" on:click=move |_| {
                            let wid = wid.clone();
                            let layout = layout_sig.get_untracked();
                            let chosen = systems_sig.get_untracked();
                            // Preserve canonical order across enabled systems
                            let systems: Vec<String> = ALL_SYSTEMS.iter()
                                .map(|(k, _)| k.to_string())
                                .filter(|k| chosen.contains(k))
                                .collect();
                            widgets.update(|w| {
                                if let Some(widget) = w.iter_mut().find(|x| x.id == wid) {
                                    // Note: any pre-existing battery_threshold_pct in old
                                    // configs is dropped here on save — clean migration.
                                    widget.config = json!({
                                        "layout": layout,
                                        "systems": systems,
                                    });
                                }
                            });
                            on_close.run(());
                        }>"Apply"</button>
                        <button class="btn btn-outline btn-sm" on:click=move |_| on_close.run(())>"Cancel"</button>
                    </div>
                </div>
            }.into_any()
        }
        _ => {
            // ModeChips, SceneRow, etc. — no config to edit
            view! {
                <div class="widget-config-editor">
                    <p class="cell-subtle">"This card has no configurable settings."</p>
                    <button class="btn btn-outline btn-sm" on:click=move |_| on_close.run(())>"Close"</button>
                </div>
            }.into_any()
        }
    }
}

// ── Device Checkbox List ────────────────────────────────────────────────────

#[component]
fn DeviceCheckboxList(
    device_options: Memo<Vec<(String, String)>>,
    selected: RwSignal<Vec<String>>,
) -> impl IntoView {
    let search = RwSignal::new(String::new());

    view! {
        <div class="device-checkbox-list">
            <input class="input" type="text" placeholder="Filter devices..."
                prop:value=move || search.get()
                on:input=move |ev| search.set(event_target_value(&ev)) />
            <div class="device-checkbox-scroll">
                {move || {
                    let q = search.get().to_lowercase();
                    let sel = selected.get();
                    device_options.get().into_iter()
                        .filter(|(_, name)| q.is_empty() || name.to_lowercase().contains(&q))
                        .map(|(id, name)| {
                            let id_check = id.clone();
                            let checked = sel.contains(&id);
                            view! {
                                <label class="device-checkbox-item">
                                    <input type="checkbox" prop:checked=checked
                                        on:change=move |_| {
                                            let id = id_check.clone();
                                            selected.update(|v| {
                                                if let Some(pos) = v.iter().position(|x| *x == id) { v.remove(pos); }
                                                else { v.push(id); }
                                            });
                                        } />
                                    <span>{name}</span>
                                </label>
                            }
                        }).collect_view()
                }}
            </div>
            <span class="cell-subtle">{move || format!("{} selected", selected.get().len())}</span>
        </div>
    }
}

// ── Card: Single Device ─────────────────────────────────────────────────────

#[component]
fn SingleDeviceCard(config: Value) -> impl IntoView {
    let device_id = config
        .get("device_ids")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if device_id.is_empty() {
        return view! { <p class="cell-subtle" style="padding:0.75rem">"No device configured"</p> }
            .into_any();
    }
    view! { <DeviceCard device_id=device_id /> }.into_any()
}

// ── Card: Entities (HA-style) ───────────────────────────────────────────────

#[component]
fn EntitiesCard(title: String, config: Value) -> impl IntoView {
    let ws = use_ws();
    let device_ids: Vec<String> = config
        .get("device_ids")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if device_ids.is_empty() {
        return view! {
            <div class="entities-card">
                <h3 class="entities-card-title">{title}</h3>
                <p class="cell-subtle" style="padding:0.5rem 0.75rem">"No devices. Use settings to add."</p>
            </div>
        }.into_any();
    }

    view! {
        <div class="entities-card">
            <h3 class="entities-card-title">{title}</h3>
            {device_ids.into_iter().map(|did| {
                let did2 = did.clone();
                let device: Memo<Option<DeviceState>> = Memo::new(move |_| ws.devices.get().get(&did).cloned());
                view! { <EntityRow device=device device_id=did2 /> }
            }).collect_view()}
        </div>
    }.into_any()
}

#[component]
fn EntityRow(device: Memo<Option<DeviceState>>, device_id: String) -> impl IntoView {
    let auth = use_auth();
    let busy = RwSignal::new(false);
    let did = device_id;

    view! {
        {move || {
            let Some(d) = device.get() else {
                return view! { <div class="entities-row"><span class="cell-subtle">"..."</span></div> }.into_any();
            };
            let name = display_name(&d).to_string();
            let icon = status_icon_name(&d);
            let status = status_text(&d);
            let tone = status_tone(&d);
            let tone_class = format!("entities-row-status entities-row-status--{}", tone.css_class());
            let has_toggle = supports_inline_toggle(&d);
            let has_lock = supports_inline_lock(&d);
            let is_on = bool_attr(d.attributes.get("on")).unwrap_or(false);
            let is_locked = bool_attr(d.attributes.get("locked")).unwrap_or(false);
            let has_brightness = num_attr(d.attributes.get("brightness_pct")).is_some();
            let brightness_val = num_attr(d.attributes.get("brightness_pct")).unwrap_or(0.0) as i64;
            let did_toggle = did.clone();
            let did_lock = did.clone();
            let did_bright = did.clone();
            let avail = d.available;

            view! {
                <div class="entities-row" class:entities-row--offline=!avail>
                    <div class="entities-row-info">
                        <i class={format!("ph ph-{} entities-row-icon", icon)}></i>
                        <span class="entities-row-name">{name}</span>
                    </div>
                    <div class="entities-row-controls">
                        <span class=tone_class>{status}</span>
                        {(has_toggle && avail).then(|| {
                            let did_t = did_toggle.clone();
                            view! {
                                <button
                                    class=move || if is_on { "entities-toggle entities-toggle--on" } else { "entities-toggle" }
                                    disabled=move || busy.get()
                                    on:click=move |_| {
                                        let token = auth.token_str().unwrap_or_default();
                                        let id = did_t.clone();
                                        let v = !is_on;
                                        busy.set(true);
                                        spawn_local(async move { let _ = set_device_state(&token, &id, &json!({"on": v})).await; busy.set(false); });
                                    }
                                >
                                    <i class=if is_on { "ph ph-toggle-right" } else { "ph ph-toggle-left" } style="font-size:22px"></i>
                                </button>
                            }
                        })}
                        {(has_lock && avail).then(|| {
                            let did_l = did_lock.clone();
                            view! {
                                <button
                                    class=move || if is_locked { "entities-toggle entities-toggle--on" } else { "entities-toggle" }
                                    disabled=move || busy.get()
                                    title=if is_locked { "Unlock" } else { "Lock" }
                                    on:click=move |_| {
                                        let token = auth.token_str().unwrap_or_default();
                                        let id = did_l.clone();
                                        let v = !is_locked;
                                        busy.set(true);
                                        spawn_local(async move { let _ = set_device_state(&token, &id, &json!({"locked": v})).await; busy.set(false); });
                                    }
                                >
                                    <i class=if is_locked { "ph ph-lock" } else { "ph ph-lock-open" } style="font-size:22px"></i>
                                </button>
                            }
                        })}
                        {(has_brightness && avail).then(|| {
                            let did_b = did_bright.clone();
                            view! {
                                <input type="range" min="0" max="100" class="entities-slider"
                                    prop:value=brightness_val.to_string()
                                    on:change=move |ev| {
                                        let token = auth.token_str().unwrap_or_default();
                                        let id = did_b.clone();
                                        let val: i64 = event_target_value(&ev).parse().unwrap_or(0);
                                        spawn_local(async move { let _ = set_device_state(&token, &id, &json!({"brightness_pct": val})).await; });
                                    } />
                            }
                        })}
                    </div>
                </div>
            }.into_any()
        }}
    }
}

// ── Card: Overview Counter ──────────────────────────────────────────────────

#[component]
fn OverviewCard(title: String, config: Value) -> impl IntoView {
    let ws = use_ws();
    let counter_type = config
        .get("counter_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let device_type = config
        .get("device_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let attribute = config
        .get("attribute")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let match_value = config.get("value").cloned().unwrap_or(Value::Null);
    let icon = config
        .get("icon")
        .and_then(|v| v.as_str())
        .unwrap_or("info")
        .to_string();
    let _link_url = config
        .get("link_url")
        .and_then(|v| v.as_str())
        .unwrap_or("/devices")
        .to_string();
    let style = config
        .get("display_style")
        .and_then(|v| v.as_str())
        .unwrap_or("badge")
        .to_string();

    let is_alert = counter_type == "availability"
        || attribute == "leak"
        || attribute == "water"
        || (attribute == "contact" && match_value == json!("open"));

    let ct = counter_type;
    let dt = device_type;
    let attr = attribute;
    let mv = match_value;
    let count = Memo::new(move |_| {
        let devices = ws.devices.get();
        if ct == "availability" {
            return devices.values().filter(|d| !d.available).count();
        }
        devices
            .values()
            .filter(|d| {
                if !dt.is_empty() {
                    let key = presentation_device_type_key(d);
                    let raw = d.device_type.as_deref().unwrap_or("");
                    let types: Vec<&str> = dt.split(',').map(|t| t.trim()).collect();
                    let matched = types.iter().any(|t| key == *t || raw == *t);
                    if !matched {
                        return false;
                    }
                }
                if !attr.is_empty() && !mv.is_null() {
                    let actual = d.attributes.get(attr.as_str());
                    match &mv {
                        Value::Bool(e) => bool_attr(actual) == Some(*e),
                        Value::String(e) => {
                            // Try exact string match first, then try bool_attr
                            // interpretation (e.g., "open" → true, "closed" → false)
                            if str_attr(actual) == Some(e.as_str()) {
                                true
                            } else if let Some(bool_meaning) =
                                bool_attr(Some(&serde_json::Value::String(e.clone())))
                            {
                                bool_attr(actual) == Some(bool_meaning)
                            } else {
                                false
                            }
                        }
                        Value::Number(n) => num_attr(actual) == n.as_f64(),
                        _ => false,
                    }
                } else {
                    true
                }
            })
            .count()
    });

    // Navigate to /devices with pre-set filters matching this counter.
    let config_for_nav = config.clone();
    let nav_fn = move || {
        use crate::pages::shared::ls_set;
        let ct = config_for_nav
            .get("counter_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let dt = config_for_nav
            .get("device_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // DeviceCardsPage filters by presentation_device_type_label (e.g., "Light", "Contact Sensor")
        let mut type_filter = Vec::new();
        if !dt.is_empty() {
            for t in dt.split(',') {
                let label = match t.trim() {
                    "light" => "Light",
                    "dimmer" => "Dimmer",
                    "switch" => "Switch",
                    "lock" => "Lock",
                    "shade" => "Shade",
                    "contact_sensor" => "Contact Sensor",
                    "motion_sensor" => "Motion Sensor",
                    "occupancy_sensor" => "Occupancy Sensor",
                    "leak_sensor" => "Leak Sensor",
                    "vibration_sensor" => "Vibration Sensor",
                    "temperature_sensor" => "Temperature Sensor",
                    "humidity_sensor" => "Humidity Sensor",
                    "environment_sensor" => "Temp / Humidity Sensor",
                    "media_player" => "Media Player",
                    "keypad" => "Keypad",
                    "remote" => "Remote",
                    "timer" => "Timer",
                    "sensor" => "Sensor",
                    other => other,
                };
                type_filter.push(label.to_string());
            }
        }
        let avail: Vec<String> = if ct == "availability" {
            vec!["offline".to_string()]
        } else {
            vec![]
        };
        let prefs = serde_json::json!({
            "search": "",
            "card_size": "medium",
            "sort_by": "name",
            "sort_dir": "asc",
            "type_filter": type_filter,
            "avail_filter": avail,
            "area_filter": [],
            "plugin_filter": [],
        });
        ls_set("hc-leptos:cards:prefs", &prefs.to_string());
        // Use full page navigation so the devices page loads fresh with new prefs
        if let Some(window) = web_sys::window() {
            let _ = window.location().set_href("/devices");
        }
    };
    let nav1 = nav_fn.clone();
    let nav2 = nav_fn;
    let icon2 = icon.clone();
    let title2 = title.clone();

    if style == "chip" {
        view! {
            <div class=move || {
                let c = count.get();
                if c == 0 && is_alert { "overview-chip overview-chip--good" }
                else if c > 0 && is_alert { "overview-chip overview-chip--alert" }
                else { "overview-chip" }
            } on:click=move |_| nav1()>
                <i class={format!("ph ph-{}", icon)}></i>
                <span class="overview-chip-count">{move || count.get().to_string()}</span>
                <span class="overview-chip-label">{title}</span>
            </div>
        }
        .into_any()
    } else {
        view! {
            <div class=move || {
                let c = count.get();
                if c == 0 && is_alert { "overview-card overview-card--good" }
                else if c > 0 && is_alert { "overview-card overview-card--alert" }
                else { "overview-card" }
            } on:click=move |_| nav2()>
                <i class={format!("ph ph-{} overview-card-icon", icon2)}></i>
                <div class="overview-card-body">
                    <span class="overview-card-label">{title2}</span>
                    <span class="overview-card-count">{move || count.get().to_string()}</span>
                </div>
            </div>
        }
        .into_any()
    }
}

// ── Card: Generic Stat (template fallback) ──────────────────────────────────

#[component]
fn GenericStatCard(title: String, config: Value) -> impl IntoView {
    let ws = use_ws();
    let metrics: Vec<String> = config
        .get("metrics")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    view! {
        <div class="generic-stat-card">
            <h3 class="generic-stat-title">{title}</h3>
            <div class="generic-stat-grid">
                {metrics.into_iter().map(|metric| {
                    let m = metric.clone();
                    let val = Memo::new(move |_| {
                        let devices = ws.devices.get();
                        match m.as_str() {
                            "devices" => devices.len().to_string(),
                            "on" => devices.values().filter(|d| bool_attr(d.attributes.get("on")) == Some(true)).count().to_string(),
                            "offline" => devices.values().filter(|d| !d.available).count().to_string(),
                            "media_playing" => devices.values().filter(|d| is_media_player(d) && playback_state(d) == "playing").count().to_string(),
                            other => other.to_string(),
                        }
                    });
                    let label = metric.replace('_', " ");
                    view! {
                        <div class="generic-stat-item">
                            <span class="generic-stat-value">{move || val.get()}</span>
                            <span class="generic-stat-label">{label}</span>
                        </div>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}

// ── Card: Stat Chips ────────────────────────────────────────────────────────

/// Resolve a device attribute for chip display, trying common variants.
fn resolve_sensor_value(d: &DeviceState, attr: &str) -> Option<(String, String)> {
    // Try exact match first
    if let Some(val) = d.attributes.get(attr) {
        let display = if let Some(n) = num_attr(Some(val)) {
            format!("{:.1}", n)
        } else if let Some(b) = bool_attr(Some(val)) {
            // Display boolean attributes in a friendly way
            match attr {
                "on" => if b { "On" } else { "Off" }.to_string(),
                "contact" => if b { "Open" } else { "Closed" }.to_string(),
                "locked" => if b { "Locked" } else { "Unlocked" }.to_string(),
                "motion" => if b { "Motion" } else { "Clear" }.to_string(),
                "occupied" | "occupancy" => if b { "Occupied" } else { "Clear" }.to_string(),
                "leak" | "water" => if b { "Wet" } else { "Dry" }.to_string(),
                _ => if b { "Yes" } else { "No" }.to_string(),
            }
        } else {
            val.to_string().trim_matches('"').to_string()
        };
        let unit = match attr {
            "temperature" | "temperature_f" => "°F",
            "temperature_c" => "°C",
            "humidity" => "%",
            "battery" | "battery_level" | "brightness_pct" => "%",
            "illuminance" | "illuminance_lux" => " lux",
            _ => "",
        };
        return Some((attr.to_string(), format!("{display}{unit}")));
    }
    // Try variants for temperature
    if attr == "temperature" {
        for variant in &["temperature_f", "temperature_c"] {
            if let Some(val) = d.attributes.get(*variant) {
                if let Some(n) = num_attr(Some(val)) {
                    let unit = if *variant == "temperature_c" {
                        "°C"
                    } else {
                        "°F"
                    };
                    return Some(("temperature".into(), format!("{:.1}{unit}", n)));
                }
            }
        }
    }
    if attr == "battery" {
        if let Some(val) = d.attributes.get("battery_level") {
            if let Some(n) = num_attr(Some(val)) {
                return Some(("battery".into(), format!("{:.0}%", n)));
            }
        }
    }
    None
}

/// Full-width "House Status" hero: a row of system tiles computed from the
/// live device map. Each tile shows a system name + headline value + pill,
/// and is clickable to navigate into the relevant filtered surface.
///
/// Plugin-aware: a tile is hidden if no devices in the current map are
/// relevant to its system (e.g. no thermostats → no Climate tile).
///
/// `config.systems` is an array of identifiers (`["lighting", "climate",
/// "security", "media", "energy", "activity"]`). A user can disable a
/// system by removing it from the array; only listed systems render.
#[component]
fn HouseStatusHero(config: Value) -> impl IntoView {
    let ws = use_ws();
    let nav = leptos_router::hooks::use_navigate();

    // Default to all 7 if config doesn't specify; preserve order so users
    // can rearrange tiles by reordering the array.
    let systems_enabled: Vec<String> = config
        .get("systems")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_else(|| {
            [
                "lighting", "climate", "security", "battery", "media", "energy", "activity",
            ]
            .iter()
            .map(|s| (*s).to_string())
            .collect()
        });

    // Battery threshold is now server-authoritative (drives event emission +
    // notify shortcut, not just visual count). Fetch once on mount; the
    // hero re-renders reactively when the value lands.
    let auth = use_auth();
    let battery_threshold: RwSignal<f64> = RwSignal::new(20.0);
    spawn_local(async move {
        let Some(t) = auth.token_str() else {
            return;
        };
        if let Ok(v) = fetch_battery_settings(&t).await {
            if let Some(n) = v.get("threshold_pct").and_then(|x| x.as_f64()) {
                battery_threshold.set(n);
            }
        }
    });

    view! {
        <div class="hc-hero">
            <header class="hc-hero__head">
                <span class="hc-hero__title">"house status"</span>
                <span class="hc-hero__live">
                    <span class="hc-hero__live-dot"></span>
                    "live"
                </span>
            </header>
            <div class="hc-hero__tiles">
                {move || {
                    let devices = ws.devices.get();
                    let status = ws.status.get();
                    let threshold = battery_threshold.get();
                    systems_enabled
                        .iter()
                        .filter_map(|sys| {
                            let tile = compute_hero_tile(sys, &devices, status, threshold);
                            tile.map(|t| {
                                let nav = nav.clone();
                                let target = t.target.clone();
                                view! {
                                    <button
                                        class="hc-hero__tile"
                                        on:click=move |_| nav(&target, Default::default())
                                    >
                                        <div class="hc-hero__system-row">
                                            <i class={format!("ph ph-{} hc-hero__system-icon", t.icon)}></i>
                                            <span class="hc-hero__system-name">{t.name}</span>
                                        </div>
                                        <div class="hc-hero__value">
                                            {t.value}
                                            {t.unit.map(|u| view! {
                                                <span class="hc-hero__unit">{u}</span>
                                            })}
                                        </div>
                                        {t.pill.map(|(label, kind)| view! {
                                            <span class={format!("hc-hero__pill hc-hero__pill--{}", kind)}>
                                                {label}
                                            </span>
                                        })}
                                    </button>
                                }
                            })
                        })
                        .collect_view()
                }}
            </div>
        </div>
    }
}

/// Computed hero tile data. `None` means "no relevant devices in the
/// map → hide this tile". `pill` is `(label, kind)` where kind is one
/// of `ok | warn | alert | idle`.
struct HeroTile {
    name: &'static str,
    icon: &'static str,
    value: String,
    unit: Option<&'static str>,
    pill: Option<(&'static str, &'static str)>,
    target: String,
}

fn compute_hero_tile(
    system: &str,
    devices: &std::collections::HashMap<String, DeviceState>,
    status: crate::ws::WsStatus,
    battery_threshold: f64,
) -> Option<HeroTile> {
    use crate::ws::WsStatus;
    match system {
        "lighting" => {
            let lights: Vec<&DeviceState> = devices
                .values()
                .filter(|d| matches!(d.device_type.as_deref(), Some("light") | Some("dimmer")))
                .collect();
            if lights.is_empty() {
                return None;
            }
            let on = lights
                .iter()
                .filter(|d| {
                    d.attributes
                        .get("on")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .count();
            let unavail = lights.iter().filter(|d| !d.available).count();
            let pill = if unavail > 0 {
                Some(("unreachable", "warn"))
            } else if on > 0 {
                Some(("active", "ok"))
            } else {
                Some(("all off", "idle"))
            };
            Some(HeroTile {
                name: "lighting",
                icon: "lightbulb",
                value: on.to_string(),
                unit: Some(if on == 1 { "on" } else { "on" }),
                pill,
                target: "/devices?focus=lighting".into(),
            })
        }

        "climate" => {
            let thermos: Vec<&DeviceState> = devices
                .values()
                .filter(|d| d.device_type.as_deref() == Some("thermostat"))
                .collect();
            if thermos.is_empty() {
                return None;
            }
            let temps: Vec<f64> = thermos
                .iter()
                .filter_map(|d| {
                    d.attributes
                        .get("current_temperature")
                        .and_then(|v| v.as_f64())
                })
                .collect();
            let avg = if temps.is_empty() {
                None
            } else {
                Some(temps.iter().sum::<f64>() / temps.len() as f64)
            };
            let any_calling = thermos.iter().any(|d| {
                matches!(
                    d.attributes.get("call_for").and_then(Value::as_str),
                    Some("heat") | Some("cool")
                )
            });
            let calling_kind = thermos.iter().find_map(|d| {
                d.attributes
                    .get("call_for")
                    .and_then(Value::as_str)
                    .and_then(|s| match s {
                        "heat" => Some(("heating", "warn")),
                        "cool" => Some(("cooling", "ok")),
                        _ => None,
                    })
            });
            let pill = if any_calling {
                calling_kind
            } else {
                Some(("idle", "idle"))
            };
            Some(HeroTile {
                name: "climate",
                icon: "thermometer-simple",
                value: avg.map(|t| format!("{t:.0}")).unwrap_or_else(|| "—".into()),
                unit: avg.map(|_| "°"),
                pill,
                target: "/devices?focus=climate".into(),
            })
        }

        "security" => {
            // Single source of truth via `should_include_in_security`:
            // honours explicit tags AND explicit excludes, falling back
            // to "all locks + contact sensors" only when neither store
            // has an opinion on the device. OVERVIEW-SECURITY-OPT-IN-1.
            let in_security_set = |d: &&DeviceState| -> bool {
                should_include_in_security(d)
            };
            let locks: Vec<&DeviceState> = devices
                .values()
                .filter(|d| d.device_type.as_deref() == Some("lock"))
                .filter(in_security_set)
                .collect();
            let contacts: Vec<&DeviceState> = devices
                .values()
                .filter(|d| d.device_type.as_deref() == Some("contact_sensor"))
                .filter(in_security_set)
                .collect();
            if locks.is_empty() && contacts.is_empty() {
                return None;
            }
            let unlocked = locks
                .iter()
                .filter(|d| {
                    !d.attributes
                        .get("locked")
                        .and_then(Value::as_bool)
                        .unwrap_or(true)
                })
                .count();
            let open = contacts
                .iter()
                .filter(|d| {
                    d.attributes
                        .get("open")
                        .and_then(Value::as_bool)
                        .or_else(|| d.attributes.get("contact").and_then(Value::as_bool))
                        .unwrap_or(false)
                })
                .count();
            let pill = if unlocked > 0 || open > 0 {
                Some(("attention", "warn"))
            } else {
                Some(("secure", "ok"))
            };
            let value = if unlocked == 0 && open == 0 {
                "all closed".to_string()
            } else if unlocked > 0 && open > 0 {
                format!("{unlocked} + {open}")
            } else if unlocked > 0 {
                format!("{unlocked} unlocked")
            } else {
                format!("{open} open")
            };
            Some(HeroTile {
                name: "security",
                icon: "shield-check",
                value,
                unit: None,
                pill,
                target: "/devices?focus=security".into(),
            })
        }

        "battery" => {
            // Any device with battery info — covers both percentage-based
            // sensors (Z-Wave, Hue, Yolink) AND kind-based ones (Ecowitt
            // emits battery_low + battery_kind without a percentage).
            // Auto-hide if there's nothing battery-powered in the map.
            let battery_devices: Vec<&DeviceState> =
                devices.values().filter(|d| has_battery_info(d)).collect();
            if battery_devices.is_empty() {
                return None;
            }
            let low_count = battery_devices
                .iter()
                .filter(|d| is_battery_low(d, battery_threshold).unwrap_or(false))
                .count();
            // Lowest percentage among devices that actually report one;
            // kind-based devices (Ecowitt) contribute via low_count only.
            let lowest_pct: Option<f64> = battery_devices
                .iter()
                .filter_map(|d| battery_pct(d))
                .fold(None, |acc, p| Some(acc.map_or(p, |a: f64| a.min(p))));
            let pill = if low_count > 0 {
                Some(("low", "alert"))
            } else {
                Some(("ok", "ok"))
            };
            let value = if low_count == 0 {
                match lowest_pct {
                    Some(p) => format!("{:.0}%", p),
                    // Only kind-based devices, all OK — show a status
                    // word rather than a misleading "0%".
                    None => "ok".into(),
                }
            } else if low_count == 1 {
                "1 low".to_string()
            } else {
                format!("{low_count} low")
            };
            // Embed the threshold so the devices page filter matches the
            // tile's count exactly when the user clicks through.
            let target = format!("/devices?focus=battery&below={}", battery_threshold as i64);
            Some(HeroTile {
                name: "battery",
                icon: "battery-low",
                value,
                unit: None,
                pill,
                target,
            })
        }

        "media" => {
            let speakers: Vec<&DeviceState> = devices
                .values()
                .filter(|d| d.device_type.as_deref() == Some("media_player"))
                .collect();
            if speakers.is_empty() {
                return None;
            }
            let playing = speakers.iter().find(|d| playback_state(d) == "playing");
            match playing {
                Some(d) => Some(HeroTile {
                    name: "media",
                    icon: "speaker-hifi",
                    value: d.name.clone(),
                    unit: None,
                    pill: Some(("playing", "ok")),
                    // Drop into the playing device's detail page directly
                    // so the user can control it. Other media players
                    // are one click away via the focus filter.
                    target: format!("/devices/{}", d.device_id),
                }),
                None => Some(HeroTile {
                    name: "media",
                    icon: "speaker-hifi",
                    value: "idle".into(),
                    unit: None,
                    pill: Some(("paused", "idle")),
                    target: "/devices?focus=media".into(),
                }),
            }
        }

        "energy" => {
            let monitors: Vec<&DeviceState> = devices
                .values()
                .filter(|d| d.device_type.as_deref() == Some("power_monitor"))
                .collect();
            if monitors.is_empty() {
                return None;
            }
            let total_w: f64 = monitors
                .iter()
                .filter_map(|d| {
                    d.attributes
                        .get("power_w")
                        .or_else(|| d.attributes.get("watts"))
                        .and_then(|v| v.as_f64())
                })
                .sum();
            let (value, unit) = if total_w >= 1000.0 {
                (format!("{:.1}", total_w / 1000.0), Some("kW"))
            } else {
                (format!("{:.0}", total_w), Some("W"))
            };
            Some(HeroTile {
                name: "energy",
                icon: "lightning",
                value,
                unit,
                pill: Some(("ok", "ok")),
                target: "/devices?focus=energy".into(),
            })
        }

        "activity" => {
            let (label, kind, value) = match status {
                WsStatus::Live => ("streaming", "ok", "live".to_string()),
                WsStatus::Connecting => ("connecting", "warn", "—".into()),
                WsStatus::Disconnected => ("offline", "alert", "—".into()),
            };
            Some(HeroTile {
                name: "activity",
                icon: "pulse",
                value,
                unit: None,
                pill: Some((label, kind)),
                target: "/events".into(),
            })
        }

        _ => None,
    }
}

#[component]
fn StatChipsCard(config: Value) -> impl IntoView {
    let ws = use_ws();
    let device_ids: Vec<String> = config
        .get("device_ids")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let attributes: Vec<String> = config
        .get("attributes")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_else(|| vec!["temperature".into(), "humidity".into()]);

    if device_ids.is_empty() {
        return view! { <div class="stat-chips-row"><span class="cell-subtle" style="padding:0.5rem">"No devices. Use settings to add."</span></div> }.into_any();
    }

    view! {
        <div class="stat-chips-row">
            {move || {
                let devices = ws.devices.get();
                if devices.is_empty() {
                    return view! { <span class="stat-chip"><span class="stat-chip-label">"Loading..."</span></span> }.into_any();
                }
                let chips: Vec<_> = device_ids.iter().map(|did| {
                    match devices.get(did) {
                        Some(d) => {
                            let name = display_name(d).to_string();
                            let icon = status_icon_name(d);
                            let tone = status_tone(d);
                            // Try requested attributes; fall back to status_text
                            let resolved: Vec<(String, String)> = attributes.iter()
                                .filter_map(|attr| resolve_sensor_value(d, attr))
                                .collect();
                            let (label, value) = if resolved.is_empty() {
                                (name, status_text(d))
                            } else if resolved.len() == 1 {
                                (name, resolved[0].1.clone())
                            } else {
                                let vals: Vec<String> = resolved.iter().map(|(_, v)| v.clone()).collect();
                                (name, vals.join(" · "))
                            };
                            let tone_class = match tone {
                                StatusTone::Good => "stat-chip stat-chip--active",
                                StatusTone::Warn => "stat-chip stat-chip--warn",
                                StatusTone::Offline => "stat-chip stat-chip--offline",
                                _ => "stat-chip",
                            };
                            view! {
                                <span class=tone_class>
                                    <i class={format!("ph ph-{} stat-chip-icon", icon)}></i>
                                    <span class="stat-chip-label">{label}</span>
                                    <span class="stat-chip-value">{value}</span>
                                </span>
                            }.into_any()
                        }
                        None => {
                            view! {
                                <span class="stat-chip">
                                    <span class="stat-chip-label">{did.clone()}</span>
                                    <span class="stat-chip-value">"?"</span>
                                </span>
                            }.into_any()
                        }
                    }
                }).collect();
                chips.collect_view().into_any()
            }}
        </div>
    }.into_any()
}

// ── Card: Mode Chips ────────────────────────────────────────────────────────

#[component]
fn ModeChipsCard() -> impl IntoView {
    let ws = use_ws();
    let auth = use_auth();
    let mode_ids = Memo::new(move |_| {
        let mut ids: Vec<String> = ws
            .devices
            .get()
            .keys()
            .filter(|id| id.starts_with("mode_"))
            .cloned()
            .collect();
        ids.sort();
        ids
    });

    view! {
        <div class="mode-chips-row">
            <For each=move || mode_ids.get() key=|id| id.clone()
                children=move |mode_id| {
                    let mid = mode_id.clone();
                    let mid_click = mode_id.clone();
                    let device: Memo<Option<DeviceState>> = Memo::new(move |_| ws.devices.get().get(&mid).cloned());
                    let busy = RwSignal::new(false);
                    view! {
                        {move || device.get().map(|d| {
                            let name = display_name(&d).to_string();
                            let is_on = bool_attr(d.attributes.get("on")).unwrap_or(false);
                            let mid_c = mid_click.clone();
                            view! {
                                <button class=if is_on { "mode-chip mode-chip--on" } else { "mode-chip" }
                                    disabled=move || busy.get()
                                    on:click=move |_| {
                                        let token = auth.token_str().unwrap_or_default();
                                        let id = mid_c.clone(); let v = !is_on; busy.set(true);
                                        spawn_local(async move { let _ = set_device_state(&token, &id, &json!({"on": v})).await; busy.set(false); });
                                    }>
                                    <i class=if is_on { "ph ph-toggle-right" } else { "ph ph-toggle-left" } style="font-size:16px"></i>
                                    {name}
                                </button>
                            }
                        })}
                    }
                } />
        </div>
    }
}

// ── Card: Scene Buttons ─────────────────────────────────────────────────────

#[component]
fn SceneButtonsCard() -> impl IntoView {
    let auth = use_auth();
    let ws = use_ws();
    let scenes: RwSignal<Vec<Scene>> = RwSignal::new(vec![]);
    let busy: RwSignal<Option<String>> = RwSignal::new(None);

    Effect::new(move |_| {
        let token = match auth.token.get() {
            Some(t) => t,
            None => return,
        };
        spawn_local(async move {
            if let Ok(mut data) = fetch_scenes(&token).await {
                data.sort_by(|a, b| a.name.cmp(&b.name));
                scenes.set(data);
            }
        });
    });

    view! {
        <div class="scene-buttons-row">
            <For each=move || scenes.get() key=|s| s.id.clone()
                children=move |scene| {
                    let sid = scene.id.clone();
                    let sid_click = sid.clone();
                    let name = scene.name.clone();
                    let recent = Memo::new(move |_| ws.scene_activations.get().get(&sid).is_some());
                    view! {
                        <button class=move || if recent.get() { "scene-btn scene-btn--recent" } else { "scene-btn" }
                            disabled=move || busy.get().is_some()
                            on:click=move |_| {
                                let token = auth.token_str().unwrap_or_default();
                                let id = sid_click.clone(); busy.set(Some(id.clone()));
                                spawn_local(async move { let _ = activate_scene(&token, &id).await; busy.set(None); });
                            }>
                            <i class="ph ph-play" style="font-size:16px"></i>
                            {name}
                        </button>
                    }
                } />
        </div>
    }
}

// ── Add Card Panel ──────────────────────────────────────────────────────────

#[component]
fn AddCardPanel(widgets: RwSignal<Vec<DashboardWidget>>) -> impl IntoView {
    let ws = use_ws();
    let adding: RwSignal<Option<String>> = RwSignal::new(None);
    let title_input = RwSignal::new(String::new());
    let device_id_input = RwSignal::new(String::new());
    let selected_device_ids: RwSignal<Vec<String>> = RwSignal::new(vec![]);
    let preset_input = RwSignal::new(String::new());

    let reset_form = move || {
        title_input.set(String::new());
        device_id_input.set(String::new());
        selected_device_ids.set(vec![]);
        preset_input.set(String::new());
    };

    let add_widget = move |wtype: DashboardWidgetType, title: String, config: Value| {
        let id = format!("w_{}", uuid::Uuid::new_v4().simple());
        widgets.update(|w| {
            w.push(DashboardWidget {
                id,
                r#type: wtype,
                title,
                subtitle: None,
                refresh_policy: DashboardRefreshPolicy::Live,
                config,
            })
        });
        adding.set(None);
        reset_form();
    };

    let device_options = Memo::new(move |_| {
        let mut opts: Vec<(String, String)> = ws
            .devices
            .get()
            .values()
            .filter(|d| !d.device_id.starts_with("mode_"))
            .map(|d| {
                (
                    d.device_id.clone(),
                    format!("{} ({})", display_name(d), d.area.as_deref().unwrap_or("—")),
                )
            })
            .collect();
        opts.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        opts
    });

    view! {
        <div class="add-card-panel">
            <h3>"Add Card"</h3>
            {move || {
                if adding.get().is_none() {
                    view! {
                        <div class="add-card-type-grid">
                            <button class="add-card-type-btn" on:click=move |_| adding.set(Some("device".into()))>
                                <i class="ph ph-devices"></i> "Single Device"
                            </button>
                            <button class="add-card-type-btn" on:click=move |_| adding.set(Some("entities".into()))>
                                <i class="ph ph-list"></i> "Entities"
                            </button>
                            <button class="add-card-type-btn" on:click=move |_| adding.set(Some("overview".into()))>
                                <i class="ph ph-chart-line"></i> "Overview Counter"
                            </button>
                            <button class="add-card-type-btn" on:click=move |_| adding.set(Some("chips".into()))>
                                <i class="ph ph-broadcast"></i> "Stat Chips"
                            </button>
                            <button class="add-card-type-btn" on:click=move |_| { add_widget(DashboardWidgetType::ModeChips, "Modes".into(), json!({"card_size":"medium"})); }>
                                <i class="ph ph-sliders-horizontal"></i> "Mode Chips"
                            </button>
                            <button class="add-card-type-btn" on:click=move |_| { add_widget(DashboardWidgetType::SceneRow, "Scenes".into(), json!({"card_size":"medium"})); }>
                                <i class="ph ph-play-circle"></i> "Scene Buttons"
                            </button>
                        </div>
                    }.into_any()
                } else {
                    let card_type = adding.get().unwrap_or_default();
                    view! {
                        <div class="add-card-form">
                            <div class="add-card-form-header">
                                <strong>{match card_type.as_str() {
                                    "device" => "Add Single Device",
                                    "entities" => "Add Entities Card",
                                    "overview" => "Add Overview Counter",
                                    "chips" => "Add Stat Chips",
                                    _ => "Add Card",
                                }}</strong>
                                <button class="btn btn-outline btn-sm" on:click=move |_| { adding.set(None); reset_form(); }>"Cancel"</button>
                            </div>

                            // ── Single Device ─────────────────────
                            {(card_type == "device").then(|| view! {
                                <div class="add-card-fields">
                                    <label>"Device"</label>
                                    <select class="input" prop:value=move || device_id_input.get() on:change=move |ev| device_id_input.set(event_target_value(&ev))>
                                        <option value="">"Select device..."</option>
                                        <For each=move || device_options.get() key=|(id, _)| id.clone()
                                            children=move |(id, name)| view! { <option value=id.clone()>{name}</option> } />
                                    </select>
                                    <button class="btn btn-primary" disabled=move || device_id_input.get().is_empty()
                                        on:click=move |_| { add_widget(DashboardWidgetType::DeviceTile, "Device".into(), json!({"selection_mode":"manual","device_ids":[device_id_input.get()],"card_size":"small"})); }
                                    >"Add"</button>
                                </div>
                            })}

                            // ── Entities ──────────────────────────
                            {(card_type == "entities").then(|| view! {
                                <div class="add-card-fields">
                                    <label>"Title"</label>
                                    <input class="input" type="text" prop:value=move || title_input.get()
                                        on:input=move |ev| title_input.set(event_target_value(&ev)) placeholder="e.g. Living Room" />
                                    <label>"Devices"</label>
                                    <DeviceCheckboxList device_options=device_options selected=selected_device_ids />
                                    <button class="btn btn-primary"
                                        disabled=move || title_input.get().trim().is_empty() || selected_device_ids.get().is_empty()
                                        on:click=move |_| {
                                            let ids = selected_device_ids.get();
                                            add_widget(DashboardWidgetType::DeviceGrid, title_input.get(),
                                                json!({"selection_mode":"manual","device_ids":ids,"card_size":"medium"}));
                                        }
                                    >"Add"</button>
                                </div>
                            })}

                            // ── Overview Counter (preset-based) ───
                            {(card_type == "overview").then(|| view! {
                                <div class="add-card-fields">
                                    <label>"Preset"</label>
                                    <select class="input" prop:value=move || preset_input.get()
                                        on:change=move |ev| preset_input.set(event_target_value(&ev))>
                                        <option value="">"Select a preset..."</option>
                                        {overview_presets().into_iter().enumerate().map(|(i, (name, _, _))| {
                                            view! { <option value=i.to_string()>{name}</option> }
                                        }).collect_view()}
                                    </select>
                                    <button class="btn btn-primary"
                                        disabled=move || preset_input.get().is_empty()
                                        on:click=move |_| {
                                            let idx: usize = preset_input.get().parse().unwrap_or(0);
                                            let presets = overview_presets();
                                            if let Some((name, _, config)) = presets.into_iter().nth(idx) {
                                                add_widget(DashboardWidgetType::StatSummary, name.into(), config);
                                            }
                                        }
                                    >"Add"</button>
                                    <p class="cell-subtle">"Use the settings button after adding to customize."</p>
                                </div>
                            })}

                            // ── Stat Chips ────────────────────────
                            {(card_type == "chips").then(|| view! {
                                <div class="add-card-fields">
                                    <label>"Devices"</label>
                                    <DeviceCheckboxList device_options=device_options selected=selected_device_ids />
                                    <button class="btn btn-primary"
                                        disabled=move || selected_device_ids.get().is_empty()
                                        on:click=move |_| {
                                            let ids = selected_device_ids.get();
                                            add_widget(DashboardWidgetType::StatSummary, "Sensors".into(),
                                                json!({"chip_mode":true,"device_ids":ids,"attributes":[],"metrics":["custom"],"card_size":"large"}));
                                        }
                                    >"Add"</button>
                                </div>
                            })}
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}
