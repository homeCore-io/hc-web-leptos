//! Areas page — room and zone management.

use crate::api::{
    create_area as create_area_request, delete_area as delete_area_request, fetch_areas,
    fetch_devices, set_area_devices as set_area_devices_request,
    update_area as update_area_request,
};
use crate::auth::use_auth;
use crate::models::*;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_shadcn_ui::{Button, ButtonVariant, Input};

#[component]
pub fn AreasPage() -> impl IntoView {
    let auth = use_auth();

    let areas: RwSignal<Vec<Area>> = RwSignal::new(vec![]);
    let devices: RwSignal<Vec<DeviceState>> = RwSignal::new(vec![]);
    let loading = RwSignal::new(true);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);
    let busy = RwSignal::new(false);

    let selected_area_id = RwSignal::new(Option::<String>::None);
    let create_name = RwSignal::new(String::new());
    let edit_name = RwSignal::new(String::new());
    let assigned_ids = RwSignal::new(Vec::<String>::new());
    let assignment_search = RwSignal::new(String::new());
    let available_selection = RwSignal::new(Vec::<String>::new());
    let assigned_selection = RwSignal::new(Vec::<String>::new());
    let delete_confirm = RwSignal::new(String::new());

    let refresh = move || {
        let token = auth.token_str().unwrap_or_default();
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            let areas_result = fetch_areas(&token).await;
            let devices_result = fetch_devices(&token).await;

            match (areas_result, devices_result) {
                (Ok(mut areas_list), Ok(mut devices_list)) => {
                    areas_list.sort_by(|a, b| {
                        sort_key_str(&display_area_name(&a.name))
                            .cmp(&sort_key_str(&display_area_name(&b.name)))
                    });
                    devices_list.retain(|device| !is_scene_like(device));
                    devices_list.sort_by(|a, b| {
                        sort_key_str(display_name(a)).cmp(&sort_key_str(display_name(b)))
                    });
                    areas.set(areas_list);
                    devices.set(devices_list);
                }
                (Err(e), _) | (_, Err(e)) => error.set(Some(e)),
            }

            loading.set(false);
        });
    };

    Effect::new(move |_| {
        refresh();
    });

    Effect::new(move |_| {
        let list = areas.get();
        let selected = selected_area_id.get();

        if list.is_empty() {
            if selected.is_some() {
                selected_area_id.set(None);
            }
            edit_name.set(String::new());
            assigned_ids.set(vec![]);
            available_selection.set(vec![]);
            assigned_selection.set(vec![]);
            delete_confirm.set(String::new());
            return;
        }

        let active = selected
            .as_deref()
            .and_then(|id| list.iter().find(|area| area.id == id))
            .cloned()
            .unwrap_or_else(|| list[0].clone());

        if selected.as_deref() != Some(active.id.as_str()) {
            selected_area_id.set(Some(active.id.clone()));
        }
        edit_name.set(display_area_name(&active.name));
        assigned_ids.set(active.device_ids.clone());
        available_selection.set(vec![]);
        assigned_selection.set(vec![]);
        delete_confirm.set(String::new());
    });

    let selected_area: Memo<Option<Area>> = Memo::new(move |_| {
        let id = selected_area_id.get();
        areas
            .get()
            .into_iter()
            .find(|area| Some(area.id.clone()) == id)
    });

    let filtered_available_devices: Memo<Vec<DeviceState>> = Memo::new(move |_| {
        let query = assignment_search.get().trim().to_lowercase();
        let selected_ids = assigned_ids.get();

        let mut list: Vec<DeviceState> = devices
            .get()
            .into_iter()
            .filter(|device| !selected_ids.contains(&device.device_id))
            .filter(|device| {
                if query.is_empty() {
                    return true;
                }

                let haystack = format!(
                    "{} {} {} {} {}",
                    display_name(device),
                    device.device_id,
                    display_area_value(device.area.as_deref()),
                    presentation_device_type_label(device),
                    device.plugin_id,
                )
                .to_lowercase();
                haystack.contains(&query)
            })
            .collect();

        list.sort_by(|a, b| {
            sort_key_str(display_name(a)).cmp(&sort_key_str(display_name(b)))
        });

        list
    });

    let filtered_assigned_devices: Memo<Vec<DeviceState>> = Memo::new(move |_| {
        let query = assignment_search.get().trim().to_lowercase();
        let selected_ids = assigned_ids.get();

        let mut list: Vec<DeviceState> = devices
            .get()
            .into_iter()
            .filter(|device| selected_ids.contains(&device.device_id))
            .filter(|device| {
                if query.is_empty() {
                    return true;
                }

                let haystack = format!(
                    "{} {} {} {} {}",
                    display_name(device),
                    device.device_id,
                    display_area_value(device.area.as_deref()),
                    presentation_device_type_label(device),
                    device.plugin_id,
                )
                .to_lowercase();
                haystack.contains(&query)
            })
            .collect();

        list.sort_by(|a, b| {
            sort_key_str(display_name(a)).cmp(&sort_key_str(display_name(b)))
        });

        list
    });

    view! {
        <div class="page">
            <div class="heading">
                <div>
                    <h1>"Areas / Rooms"</h1>
                    <p>
                        "Manage HomeCore-defined areas. Names are stored internally as snake_case and displayed as human-readable room labels."
                    </p>
                </div>
                <Button
                    variant=ButtonVariant::Outline
                    on_click=Callback::new(move |_| refresh())
                    disabled=Signal::derive(move || loading.get())
                >
                    {move || if loading.get() { "Refreshing…" } else { "Refresh" }}
                </Button>
            </div>

            {move || error.get().map(|e| view! { <p class="msg-error">{e}</p> })}
            {move || notice.get().map(|n| view! { <p class="msg-notice">{n}</p> })}

            <div class="detail-card">
                <div class="card-title-row">
                    <h2 class="card-title">"Create Area"</h2>
                    <span class="cell-subtle">"Input can be natural text; HomeCore will normalize it."</span>
                </div>
                <div class="areas-create-row">
                    <Input
                        value=Signal::derive(move || create_name.get())
                        on_change=Callback::new(move |value| create_name.set(value))
                        placeholder="e.g. Dining Room"
                    />
                    <Button
                        variant=ButtonVariant::Default
                        disabled=Signal::derive(move || busy.get() || create_name.get().trim().is_empty())
                        on_click=Callback::new(move |_| {
                            let token = auth.token_str().unwrap_or_default();
                            let name = create_name.get();
                            busy.set(true);
                            error.set(None);
                            notice.set(None);
                            spawn_local(async move {
                                match create_area_request(&token, &name).await {
                                    Ok(area) => {
                                        let label = display_area_name(&area.name);
                                        selected_area_id.set(Some(area.id.clone()));
                                        create_name.set(String::new());
                                        notice.set(Some(format!("Created area {}.", label)));
                                        refresh();
                                    }
                                    Err(e) => error.set(Some(format!("Create failed: {e}"))),
                                }
                                busy.set(false);
                            });
                        })
                    >
                        {move || if busy.get() { "Creating…" } else { "Create area" }}
                    </Button>
                </div>
            </div>

            <div class="areas-layout">
                <div class="detail-card">
                    <div class="card-title-row">
                        <h2 class="card-title">"Defined Areas"</h2>
                        <span class="cell-subtle">{move || format!("{} total", areas.get().len())}</span>
                    </div>

                    {move || {
                        let list = areas.get();
                        if loading.get() && list.is_empty() {
                            view! { <p class="no-controls-msg">"Loading areas…"</p> }.into_any()
                        } else if list.is_empty() {
                            view! { <p class="no-controls-msg">"No areas defined yet."</p> }.into_any()
                        } else {
                            view! {
                                <div class="areas-list">
                                    <For
                                        each=move || areas.get()
                                        key=|area| area.id.clone()
                                        children=move |area| {
                                            let area_id = area.id.clone();
                                            let active_area_id = area_id.clone();
                                            let click_area_id = area_id.clone();
                                            let label = display_area_name(&area.name);
                                            let raw = area.name.clone();
                                            let count = area.device_ids.len();
                                            view! {
                                                <button
                                                    class="area-list-item"
                                                    class:active=move || selected_area_id.get().as_deref() == Some(active_area_id.as_str())
                                                    on:click=move |_| selected_area_id.set(Some(click_area_id.clone()))
                                                >
                                                    <span class="area-list-name">{label}</span>
                                                    <span class="area-list-meta">
                                                        <code class="mono">{raw}</code>
                                                        {if count == 1 {
                                                            "1 device".to_string()
                                                        } else {
                                                            format!("{count} devices")
                                                        }}
                                                    </span>
                                                </button>
                                            }
                                        }
                                    />
                                </div>
                            }.into_any()
                        }
                    }}
                </div>

                <div class="detail-card">
                    {move || {
                        if let Some(area) = selected_area.get() {
                            let area_id = area.id.clone();
                            let rename_area_id = area_id.clone();
                            let assign_area_id = area_id.clone();
                            let delete_area_id = area_id.clone();
                            let area_code = area.name.clone();
                            let area_label = display_area_name(&area.name);
                            let delete_area_label = area_label.clone();
                            view! {
                                <>
                                <div class="card-title-row">
                                    <h2 class="card-title">"Edit Area"</h2>
                                    <span class="cell-subtle"><code class="mono">{area_code.clone()}</code></span>
                                </div>

                                <div class="edit-grid">
                                    <div class="edit-field">
                                        <label>"Display Name"</label>
                                        <Input
                                            value=Signal::derive(move || edit_name.get())
                                            on_change=Callback::new(move |value| edit_name.set(value))
                                            placeholder="e.g. Dining Room"
                                        />
                                        <span class="cell-subtle">
                                            "Saved internally as normalized snake_case."
                                        </span>
                                    </div>
                                </div>

                                <div class="edit-actions">
                                    <Button
                                        variant=ButtonVariant::Default
                                        disabled=Signal::derive(move || busy.get() || edit_name.get().trim().is_empty())
                                        on_click=Callback::new(move |_| {
                                            let token = auth.token_str().unwrap_or_default();
                                            let name = edit_name.get();
                                            let current_id = rename_area_id.clone();
                                            busy.set(true);
                                            error.set(None);
                                            notice.set(None);
                                            spawn_local(async move {
                                                match update_area_request(&token, &current_id, &name).await {
                                                    Ok(updated) => {
                                                        let label = display_area_name(&updated.name);
                                                        selected_area_id.set(Some(updated.id.clone()));
                                                        notice.set(Some(format!("Renamed area to {}.", label)));
                                                        refresh();
                                                    }
                                                    Err(e) => error.set(Some(format!("Rename failed: {e}"))),
                                                }
                                                busy.set(false);
                                            });
                                        })
                                    >
                                        {move || if busy.get() { "Saving…" } else { "Save name" }}
                                    </Button>
                                </div>

                                <div class="card-title-row">
                                    <h2 class="card-title">"Assigned Devices"</h2>
                                    <span class="cell-subtle">{move || format!("{} in room", assigned_ids.get().len())}</span>
                                </div>

                                <div class="areas-create-row">
                                    <Input
                                        value=Signal::derive(move || assignment_search.get())
                                        on_change=Callback::new(move |value| assignment_search.set(value))
                                        input_type="search"
                                        placeholder="Filter devices by name, type, plugin, area…"
                                    />
                                    <Button
                                        variant=ButtonVariant::Outline
                                        disabled=Signal::derive(move || busy.get())
                                        on_click=Callback::new(move |_| {
                                            let token = auth.token_str().unwrap_or_default();
                                            let current_id = assign_area_id.clone();
                                            let desired = assigned_ids.get();
                                            busy.set(true);
                                            error.set(None);
                                            notice.set(None);
                                            spawn_local(async move {
                                                match set_area_devices_request(&token, &current_id, &desired).await {
                                                    Ok(updated) => {
                                                        let label = display_area_name(&updated.name);
                                                        notice.set(Some(format!("Updated device assignment for {}.", label)));
                                                        refresh();
                                                    }
                                                    Err(e) => error.set(Some(format!("Assignment failed: {e}"))),
                                                }
                                                busy.set(false);
                                            });
                                        })
                                    >
                                        {move || if busy.get() { "Saving…" } else { "Save assignments" }}
                                    </Button>
                                </div>

                                <div class="areas-transfer">
                                    <div class="areas-transfer-col">
                                        <div class="card-title-row">
                                            <h3 class="card-title">"Available Devices"</h3>
                                            <span class="cell-subtle">
                                                {move || format!("{} shown", filtered_available_devices.get().len())}
                                            </span>
                                        </div>
                                        <div class="areas-device-list dual-list">
                                            <For
                                                each=move || filtered_available_devices.get()
                                                key=|device| device.device_id.clone()
                                                children=move |device| {
                                                    let device_id = device.device_id.clone();
                                                    let selected_device_id = device_id.clone();
                                                    let toggled_device_id = device_id.clone();
                                                    let name = display_name(&device).to_string();
                                                    view! {
                                                        <button
                                                            class="area-device-row transfer-row"
                                                            class:selected=move || available_selection.get().contains(&selected_device_id)
                                                            on:click=move |_| {
                                                                available_selection.update(|ids| {
                                                                    if ids.contains(&toggled_device_id) {
                                                                        ids.retain(|id| id != &toggled_device_id);
                                                                    } else {
                                                                        ids.push(toggled_device_id.clone());
                                                                        ids.sort();
                                                                    }
                                                                });
                                                            }
                                                        >
                                                            <span class="area-device-name">{name}</span>
                                                        </button>
                                                    }
                                                }
                                            />
                                        </div>
                                    </div>

                                    <div class="areas-transfer-actions">
                                        <Button
                                            variant=ButtonVariant::Outline
                                            disabled=Signal::derive(move || available_selection.get().is_empty())
                                            on_click=Callback::new(move |_| {
                                                let moved = available_selection.get();
                                                assigned_ids.update(|ids| {
                                                    for id in &moved {
                                                        if !ids.contains(id) {
                                                            ids.push(id.clone());
                                                        }
                                                    }
                                                    ids.sort();
                                                });
                                                available_selection.set(vec![]);
                                            })
                                        >
                                            "Add →"
                                        </Button>
                                        <Button
                                            variant=ButtonVariant::Outline
                                            disabled=Signal::derive(move || assigned_selection.get().is_empty())
                                            on_click=Callback::new(move |_| {
                                                let moved = assigned_selection.get();
                                                assigned_ids.update(|ids| {
                                                    ids.retain(|id| !moved.contains(id));
                                                });
                                                assigned_selection.set(vec![]);
                                            })
                                        >
                                            "← Remove"
                                        </Button>
                                    </div>

                                    <div class="areas-transfer-col">
                                        <div class="card-title-row">
                                            <h3 class="card-title">"In Room"</h3>
                                            <span class="cell-subtle">
                                                {move || format!("{} shown", filtered_assigned_devices.get().len())}
                                            </span>
                                        </div>
                                        <div class="areas-device-list dual-list">
                                            <For
                                                each=move || filtered_assigned_devices.get()
                                                key=|device| device.device_id.clone()
                                                children=move |device| {
                                                    let device_id = device.device_id.clone();
                                                    let selected_device_id = device_id.clone();
                                                    let toggled_device_id = device_id.clone();
                                                    let name = display_name(&device).to_string();
                                                    view! {
                                                        <button
                                                            class="area-device-row transfer-row"
                                                            class:selected=move || assigned_selection.get().contains(&selected_device_id)
                                                            on:click=move |_| {
                                                                assigned_selection.update(|ids| {
                                                                    if ids.contains(&toggled_device_id) {
                                                                        ids.retain(|id| id != &toggled_device_id);
                                                                    } else {
                                                                        ids.push(toggled_device_id.clone());
                                                                        ids.sort();
                                                                    }
                                                                });
                                                            }
                                                        >
                                                            <span class="area-device-name">{name}</span>
                                                        </button>
                                                    }
                                                }
                                            />
                                        </div>
                                    </div>
                                </div>

                                <div class="danger-zone">
                                    <div class="danger-zone-copy">
                                        <h3>"Delete Area"</h3>
                                        <p>
                                            {"Deleting "} <strong>{area_label.clone()}</strong>
                                            {" will clear the area from all assigned devices."}
                                        </p>
                                    </div>
                                    <div class="danger-zone-controls">
                                        <div class="edit-field">
                                            <label>{format!("Type {} to confirm", area_code)}</label>
                                            <Input
                                                value=Signal::derive(move || delete_confirm.get())
                                                on_change=Callback::new(move |value| delete_confirm.set(value))
                                                placeholder="snake_case area code"
                                            />
                                        </div>
                                        <button
                                            class="danger"
                                            disabled=move || busy.get() || delete_confirm.get().trim() != area_code
                                            on:click=move |_| {
                                                let token = auth.token_str().unwrap_or_default();
                                                let current_id = delete_area_id.clone();
                                                let deleted_label = delete_area_label.clone();
                                                busy.set(true);
                                                error.set(None);
                                                notice.set(None);
                                                spawn_local(async move {
                                                    match delete_area_request(&token, &current_id).await {
                                                        Ok(()) => {
                                                            notice.set(Some(format!("Deleted area {}.", deleted_label)));
                                                            selected_area_id.set(None);
                                                            refresh();
                                                        }
                                                        Err(e) => error.set(Some(format!("Delete failed: {e}"))),
                                                    }
                                                    busy.set(false);
                                                });
                                            }
                                        >
                                            {move || if busy.get() { "Deleting…" } else { "Delete area" }}
                                        </button>
                                    </div>
                                </div>
                                </>
                            }
                                .into_any()
                        } else {
                            view! {
                                <p class="no-controls-msg">"Select an area to edit its devices and metadata."</p>
                            }
                                .into_any()
                        }
                    }}
                </div>
            </div>
        </div>
    }
}
