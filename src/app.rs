//! Root application component: provides AuthState context, router, auth guard,
//! and the nav shell wrapper.

use crate::auth::{use_auth, AuthState};
use crate::pages::shared::{ToastContainer, ToastContext};
use crate::pages::{
    admin::AdminPage,
    areas::AreasPage,
    dashboards::DashboardsPage,
    device_cards::DeviceCardsPage,
    device_detail::DeviceDetailPage,
    events::EventsPage,
    glue::{GlueDetailPage, GluePage},
    login::LoginPage,
    modes::ModesPage,
    plugins::{PluginDetailPage, PluginsPage},
    rule_detail::{EditRulePage, NewRulePage},
    rules::RulesPage,
    scene_detail::{NativeSceneDetailPage, NewScenePage, PluginSceneDetailPage},
    scenes::ScenesPage,
};
use crate::ws::{mount_ws, WsContext};
use leptos::prelude::*;
use leptos_router::{
    components::{Route, Router, Routes},
    hooks::use_location,
    path,
};

// ── Root ──────────────────────────────────────────────────────────────────────

#[component]
pub fn App() -> impl IntoView {
    provide_context(AuthState::new());
    provide_context(ToastContext::new());

    // Restore saved theme preference on load.
    if let Ok(Some(storage)) = web_sys::window().unwrap().local_storage() {
        if let Ok(Some(theme)) = storage.get_item("hc-leptos:theme") {
            if !theme.is_empty() {
                let doc = web_sys::window().unwrap().document().unwrap();
                let _ = doc.document_element().unwrap().set_attribute("data-theme", &theme);
            }
        }
    }

    view! {
        <Router>
            <Routes fallback=|| view! { <p class="msg-error">"Page not found."</p> }>
                <Route path=path!("/")        view=HomeRedirect />
                <Route path=path!("/login")   view=LoginPage />
                <Route path=path!("/dashboards") view=move || view! {
                    <AuthGuard><DashboardsPage /></AuthGuard>
                }/>
                <Route path=path!("/areas") view=move || view! {
                    <AuthGuard><AreasPage /></AuthGuard>
                }/>
                <Route path=path!("/devices") view=move || view! {
                    <AuthGuard><DeviceCardsPage /></AuthGuard>
                }/>
                <Route path=path!("/devices/:id") view=move || view! {
                    <AuthGuard><DeviceDetailPage /></AuthGuard>
                }/>
                <Route path=path!("/scenes") view=move || view! {
                    <AuthGuard><ScenesPage /></AuthGuard>
                }/>
                <Route path=path!("/scenes/new") view=move || view! {
                    <AuthGuard><NewScenePage /></AuthGuard>
                }/>
                <Route path=path!("/scenes/native/:id") view=move || view! {
                    <AuthGuard><NativeSceneDetailPage /></AuthGuard>
                }/>
                <Route path=path!("/scenes/plugin/:id") view=move || view! {
                    <AuthGuard><PluginSceneDetailPage /></AuthGuard>
                }/>
                <Route path=path!("/modes") view=move || view! {
                    <AuthGuard><ModesPage /></AuthGuard>
                }/>
                <Route path=path!("/events") view=move || view! {
                    <AuthGuard><EventsPage /></AuthGuard>
                }/>
                <Route path=path!("/glue") view=move || view! {
                    <AuthGuard><GluePage /></AuthGuard>
                }/>
                <Route path=path!("/glue/:id") view=move || view! {
                    <AuthGuard><GlueDetailPage /></AuthGuard>
                }/>
                <Route path=path!("/plugins") view=move || view! {
                    <AuthGuard><PluginsPage /></AuthGuard>
                }/>
                <Route path=path!("/plugins/:id") view=move || view! {
                    <AuthGuard><PluginDetailPage /></AuthGuard>
                }/>
                <Route path=path!("/rules") view=move || view! {
                    <AuthGuard><RulesPage /></AuthGuard>
                }/>
                <Route path=path!("/rules/new") view=move || view! {
                    <AuthGuard><NewRulePage /></AuthGuard>
                }/>
                <Route path=path!("/rules/:id") view=move || view! {
                    <AuthGuard><EditRulePage /></AuthGuard>
                }/>
                <Route path=path!("/admin") view=move || view! {
                    <AuthGuard><AdminPage /></AuthGuard>
                }/>
            </Routes>
        </Router>
        <ToastContainer />
    }
}

// ── Home redirect ─────────────────────────────────────────────────────────────

#[component]
fn HomeRedirect() -> impl IntoView {
    let navigate = leptos_router::hooks::use_navigate();
    Effect::new(move |_| {
        navigate("/dashboards", Default::default());
    });
    view! {}
}

// ── Auth guard ────────────────────────────────────────────────────────────────
//
// Reactive: redirects to login when the token signal becomes None (logout or
// session expiry detected by the API layer).

#[component]
fn AuthGuard(children: Children) -> impl IntoView {
    let auth = use_auth();
    let navigate = leptos_router::hooks::use_navigate();

    // Redirect to login when token becomes None.
    Effect::new(move |_| {
        if auth.token.get().is_none() {
            navigate("/login", Default::default());
        }
    });

    // Render children only when authenticated.  If the token is cleared
    // (logout or API 401 detection), the Effect above redirects to /login.
    if auth.is_authenticated() {
        view! { <NavShell>{children()}</NavShell> }.into_any()
    } else {
        view! {}.into_any()
    }
}

// ── Nav shell ─────────────────────────────────────────────────────────────────

#[component]
fn NavShell(children: Children) -> impl IntoView {
    let auth = use_auth();

    // Provide the shared WS context for all child pages.
    let ws_ctx = WsContext::new();
    provide_context(ws_ctx);
    mount_ws(ws_ctx, auth.token);

    let username = move || auth.user.get().map(|u| u.username).unwrap_or_default();
    let role = move || auth.user.get().map(|u| u.role).unwrap_or_default();

    view! {
        <div class="shell">
            <aside class="sidebar">
                <div>
                    <h1><a href="/dashboards">"HomeCore"</a></h1>
                    <p class="subtitle">"Leptos web client"</p>
                </div>
                <SidebarNav />
            </aside>

            <div class="main-col">
                <header class="topbar">
                    <div class="user-info">
                        <strong>{username}</strong>
                        {move || {
                            let r = role();
                            (!r.is_empty()).then(|| view! { <span class="role">{r}</span> })
                        }}
                    </div>
                    <div class="topbar-actions">
                        <button
                            class="btn btn-icon"
                            title="Toggle theme"
                            on:click=move |_| {
                                let doc = web_sys::window().unwrap().document().unwrap();
                                let root = doc.document_element().unwrap();
                                let current = root.get_attribute("data-theme").unwrap_or_default();
                                let next = if current == "dark" { "" } else { "dark" };
                                let _ = root.set_attribute("data-theme", next);
                                if let Ok(Some(storage)) = web_sys::window().unwrap().local_storage() {
                                    let _ = storage.set_item("hc-leptos:theme", next);
                                }
                            }
                        >
                            <span class="material-icons" style="font-size:18px">"dark_mode"</span>
                        </button>
                        <button
                            class="btn btn-outline"
                            on:click=move |_| auth.logout()
                        >
                            "Logout"
                        </button>
                    </div>
                </header>
                <main class="content">
                    {children()}
                </main>
            </div>
        </div>
    }
}

// ── Sidebar Navigation ──────────────────────────────────────────────────────

const NAV_ORDER_KEY: &str = "hc-leptos:nav-order";

struct NavItem {
    id: &'static str,
    href: &'static str,
    icon: &'static str,
    label: &'static str,
}

const NAV_ITEMS: &[NavItem] = &[
    NavItem { id: "dashboards", href: "/dashboards", icon: "dashboard", label: "Overview" },
    NavItem { id: "devices", href: "/devices", icon: "devices_other", label: "Devices" },
    NavItem { id: "areas", href: "/areas", icon: "home_work", label: "Areas" },
    NavItem { id: "scenes", href: "/scenes", icon: "lightbulb", label: "Scenes" },
    NavItem { id: "modes", href: "/modes", icon: "tune", label: "Modes" },
    NavItem { id: "events", href: "/events", icon: "bolt", label: "Events" },
    NavItem { id: "rules", href: "/rules", icon: "smart_toy", label: "Rules" },
    NavItem { id: "glue", href: "/glue", icon: "extension", label: "Glue" },
    NavItem { id: "plugins", href: "/plugins", icon: "widgets", label: "Plugins" },
    NavItem { id: "admin", href: "/admin", icon: "admin_panel_settings", label: "Admin" },
];

fn load_nav_order() -> Vec<&'static str> {
    if let Some(json_str) = crate::pages::shared::ls_get(NAV_ORDER_KEY) {
        if let Ok(arr) = serde_json::from_str::<Vec<String>>(&json_str) {
            // Return items in stored order, appending any new items not in the stored list
            let mut ordered: Vec<&'static str> = Vec::new();
            for id in &arr {
                if let Some(item) = NAV_ITEMS.iter().find(|i| i.id == id.as_str()) {
                    ordered.push(item.id);
                }
            }
            for item in NAV_ITEMS {
                if !ordered.contains(&item.id) {
                    ordered.push(item.id);
                }
            }
            return ordered;
        }
    }
    NAV_ITEMS.iter().map(|i| i.id).collect()
}

fn save_nav_order(order: &[&str]) {
    let arr: Vec<serde_json::Value> = order.iter().map(|id| serde_json::Value::String(id.to_string())).collect();
    crate::pages::shared::ls_set(NAV_ORDER_KEY, &serde_json::Value::Array(arr).to_string());
}

#[component]
fn SidebarNav() -> impl IntoView {
    let location = use_location();
    let order: RwSignal<Vec<&'static str>> = RwSignal::new(load_nav_order());
    let editing = RwSignal::new(false);

    view! {
        <nav>
            {move || {
                let ids = order.get();
                let pathname = location.pathname.get();
                ids.iter().map(|id| {
                    let item = NAV_ITEMS.iter().find(|i| i.id == *id).unwrap();
                    let href = item.href;
                    let icon = item.icon;
                    let label = item.label;
                    let item_id = item.id;
                    let active_class = if pathname.starts_with(href) { "active" } else { "" };

                    view! {
                        <div class="sidebar-nav-item">
                            {editing.get().then(|| {
                                view! {
                                    <button class="sidebar-nav-move" title="Move up" on:click=move |_| {
                                        order.update(|o| {
                                            if let Some(pos) = o.iter().position(|x| *x == item_id) {
                                                if pos > 0 { o.swap(pos, pos - 1); save_nav_order(o); }
                                            }
                                        });
                                    }><span class="material-icons" style="font-size:14px">"arrow_upward"</span></button>
                                }
                            })}
                            <a href=href class=active_class>
                                <span class="material-icons" style="font-size:18px">{icon}</span>
                                {label}
                            </a>
                        </div>
                    }
                }).collect_view()
            }}
            <button
                class="sidebar-edit-toggle"
                on:click=move |_| editing.update(|v| *v = !*v)
            >
                <span class="material-icons" style="font-size:14px">{move || if editing.get() { "check" } else { "swap_vert" }}</span>
                {move || if editing.get() { "Done" } else { "Reorder" }}
            </button>
        </nav>
    }
}
