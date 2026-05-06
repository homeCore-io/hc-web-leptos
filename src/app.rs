//! Root application component: provides AuthState context, router, auth guard,
//! and the nav shell wrapper.

use crate::auth::{install_auth_handle, use_auth, AuthState};
use crate::pages::shared::{ToastContainer, ToastContext};
use crate::pages::{
    admin::AdminPage,
    audit::AuditPage,
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
use wasm_bindgen::prelude::*;

/// How often the proactive expiry check fires, in milliseconds.
/// 30 s is short enough that an expired token leads to a redirect within
/// half a minute even when the user is reading cached state without
/// firing API calls, and long enough that the JWT decode cost is noise.
const SESSION_CHECK_INTERVAL_MS: i32 = 30_000;

// ── Root ──────────────────────────────────────────────────────────────────────

#[component]
pub fn App() -> impl IntoView {
    let auth = AuthState::new();
    provide_context(auth);
    provide_context(ToastContext::new());

    // Stash AuthState in a thread-local so `api.rs::handle_session_expiry`
    // can trigger logout from inside `spawn_local` tasks where
    // `use_context` returns None.
    install_auth_handle(auth);

    // Proactive expiry check: bounce the user to /login within
    // SESSION_CHECK_INTERVAL_MS of the JWT's `exp`, even if they're
    // browsing cached WsContext state without making API calls.
    let session_cb = Closure::<dyn FnMut()>::new(move || {
        if auth.is_session_expired() {
            auth.logout();
        }
    });
    if let Some(window) = web_sys::window() {
        let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
            session_cb.as_ref().unchecked_ref(),
            SESSION_CHECK_INTERVAL_MS,
        );
    }
    session_cb.forget();

    // Shared WS context lives at the app root so the WebSocket survives
    // route navigation. Hosting it in NavShell (which is created fresh
    // by every <AuthGuard> wrapper) tore the socket down on every page
    // change and discarded the seeded device/plugin maps.
    let ws_ctx = WsContext::new();
    provide_context(ws_ctx);
    mount_ws(ws_ctx, auth.token);

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
                <Route path=path!("/admin/:tab") view=move || view! {
                    <AuthGuard><AdminPage /></AuthGuard>
                }/>
                <Route path=path!("/audit") view=move || view! {
                    <AuthGuard><AuditPage /></AuthGuard>
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

    // Server-reported version, populated from /system/status alongside
    // the TZ fetch below. Rendered in the sidebar header so the operator
    // can see "what version of homeCore am I running?" at a glance.
    // Stays None until the fetch resolves; the sidebar hides the line
    // until then to avoid a "v" with nothing after it.
    let server_version: RwSignal<Option<String>> = RwSignal::new(None);

    // Fetch the server's configured TZ once when NavShell mounts so
    // every page's UTC→local rendering uses the operator's zone, not
    // UTC. We deliberately don't gate on this — `crate::tz::app_tz()`
    // returns UTC until this resolves, and the same call sites
    // re-render once the signal settles via Leptos's reactive graph.
    // The version field on the same response feeds `server_version`.
    {
        let token = auth.token;
        leptos::task::spawn_local(async move {
            let t = token.get_untracked().unwrap_or_default();
            if let Ok(status) = crate::api::fetch_system_status(&t).await {
                crate::tz::set_app_tz(&status.timezone);
                server_version.set(Some(status.version));
            }
        });
    }

    let username = move || auth.user.get().map(|u| u.username).unwrap_or_default();
    let role = move || auth.user.get().map(|u| u.role).unwrap_or_default();

    let location = use_location();

    // Sidebar collapsed state — persisted to localStorage so reload
    // preserves whatever the user chose.
    let collapsed = RwSignal::new(load_sidebar_collapsed());
    Effect::new(move |_| {
        save_sidebar_collapsed(collapsed.get());
    });

    // Mobile menu drawer state — controls visibility of the sidebar at
    // <768px. Provided as context so SidebarNav links can close the
    // drawer on tap. Intentionally NOT persisted: each visit starts
    // closed, matching iOS-native drawer expectations.
    let mobile_menu_open = RwSignal::new(false);
    provide_context(MobileMenu(mobile_menu_open));

    view! {
        <div
            class="shell"
            class:shell--collapsed=move || collapsed.get()
            class:shell--mobile-menu-open=move || mobile_menu_open.get()
        >
            <aside class="sidebar">
                <div class="sidebar__header">
                    <div class="sidebar__brand">
                        <h1>
                            <a
                                href="/dashboards"
                                class="hc-wordmark"
                                on:click=move |_| mobile_menu_open.set(false)
                            >
                                <span class="hc-wordmark__a">"home"</span><span class="hc-wordmark__b">"Core"</span>
                            </a>
                        </h1>
                        <p class="subtitle">"control surface"</p>
                        <p class="subtitle subtitle--version">{move || server_version.get().map(|v| format!("v{v}"))}</p>
                    </div>
                    <button
                        class="sidebar__collapse-toggle"
                        title=move || if collapsed.get() { "Expand sidebar" } else { "Collapse sidebar" }
                        on:click=move |_| collapsed.update(|v| *v = !*v)
                    >
                        <i class=move || if collapsed.get() {
                            "ph ph-caret-double-right"
                        } else {
                            "ph ph-caret-double-left"
                        }></i>
                    </button>
                </div>
                <SidebarNav />
            </aside>

            // Backdrop covers the rest of the screen while the mobile
            // drawer is open. Tapping it closes the drawer — matches
            // standard mobile pattern.
            <Show when=move || mobile_menu_open.get()>
                <div
                    class="mobile-menu-backdrop"
                    on:click=move |_| mobile_menu_open.set(false)
                ></div>
            </Show>

            <div class="main-col">
                <header class="topbar">
                    // Hamburger — only shown via CSS at <768px. Toggles
                    // the mobile drawer.
                    <button
                        class="topbar__hamburger btn btn-icon"
                        title="Open menu"
                        on:click=move |_| mobile_menu_open.update(|v| *v = !*v)
                    >
                        <i class="ph ph-list" style="font-size:18px"></i>
                    </button>
                    <div class="topbar-context">
                        <span class="topbar-context__kicker">"section"</span>
                        <span class="topbar-context__title">{move || section_title(&location.pathname.get())}</span>
                    </div>
                    <div class="topbar-actions">
                        <div class="user-info">
                            <strong>{username}</strong>
                            {move || {
                                let r = role();
                                (!r.is_empty()).then(|| view! { <span class="role">{r}</span> })
                            }}
                        </div>
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
                            <i class="ph ph-moon" style="font-size:18px"></i>
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

/// Wrapper around the mobile drawer's open signal — provided via Leptos
/// context so any nav-side component (e.g. SidebarNav) can close the
/// drawer when the user taps a nav link.
#[derive(Copy, Clone)]
pub struct MobileMenu(pub RwSignal<bool>);

/// Map the current path to a human-readable section title for the
/// topbar. First-segment lookup; deeper paths show the section
/// they belong to.
fn section_title(pathname: &str) -> &'static str {
    let first = pathname.trim_start_matches('/').split('/').next().unwrap_or("");
    match first {
        "dashboards" => "Overview",
        "devices"    => "Devices",
        "areas"      => "Areas",
        "scenes"     => "Scenes",
        "modes"      => "Modes",
        "events"     => "Activity",
        "rules"      => "Rules",
        "glue"       => "Glue",
        "plugins"    => "Plugins",
        "admin"      => "Admin",
        "audit"      => "Audit",
        _            => "HomeCore",
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

// Icon names are Phosphor identifiers (without the `ph-` prefix);
// the SidebarNav view composes the full class as `ph ph-{icon}`.
const NAV_ITEMS: &[NavItem] = &[
    NavItem { id: "dashboards", href: "/dashboards", icon: "gauge",          label: "Overview" },
    NavItem { id: "devices",    href: "/devices",    icon: "devices",        label: "Devices"  },
    NavItem { id: "areas",      href: "/areas",      icon: "house-line",     label: "Areas"    },
    NavItem { id: "scenes",     href: "/scenes",     icon: "lightbulb",      label: "Scenes"   },
    NavItem { id: "modes",      href: "/modes",      icon: "sliders-horizontal", label: "Modes" },
    NavItem { id: "events",     href: "/events",     icon: "lightning",      label: "Events"   },
    NavItem { id: "rules",      href: "/rules",      icon: "robot",          label: "Rules"    },
    NavItem { id: "glue",       href: "/glue",       icon: "puzzle-piece",   label: "Glue"     },
    NavItem { id: "plugins",    href: "/plugins",    icon: "squares-four",   label: "Plugins"  },
    NavItem { id: "admin",      href: "/admin",      icon: "shield-check",   label: "Admin"    },
    NavItem { id: "audit",      href: "/audit",      icon: "list-checks",    label: "Audit"    },
];

const SIDEBAR_COLLAPSED_KEY: &str = "hc-leptos:sidebar:collapsed";

fn load_sidebar_collapsed() -> bool {
    crate::pages::shared::ls_get(SIDEBAR_COLLAPSED_KEY)
        .map(|s| s == "1" || s == "true")
        .unwrap_or(false)
}

fn save_sidebar_collapsed(collapsed: bool) {
    crate::pages::shared::ls_set(
        SIDEBAR_COLLAPSED_KEY,
        if collapsed { "1" } else { "0" },
    );
}

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
    let mobile_menu = use_context::<MobileMenu>();

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
                                    }><i class="ph ph-arrow-up" style="font-size:14px"></i></button>
                                }
                            })}
                            <a
                                href=href
                                class=active_class
                                title=label
                                on:click=move |_| {
                                    if let Some(MobileMenu(open)) = mobile_menu {
                                        open.set(false);
                                    }
                                }
                            >
                                <i class={format!("ph ph-{icon}")} style="font-size:18px"></i>
                                <span class="sidebar-nav__label">{label}</span>
                            </a>
                        </div>
                    }
                }).collect_view()
            }}
            <button
                class="sidebar-edit-toggle"
                on:click=move |_| editing.update(|v| *v = !*v)
            >
                <i class=move || if editing.get() { "ph ph-check" } else { "ph ph-arrows-down-up" } style="font-size:14px"></i>
                <span class="sidebar-nav__label">{move || if editing.get() { "Done" } else { "Reorder" }}</span>
            </button>
        </nav>
    }
}
