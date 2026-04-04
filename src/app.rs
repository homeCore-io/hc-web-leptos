//! Root application component: provides AuthState context, router, auth guard,
//! and the nav shell wrapper.

use crate::auth::{use_auth, AuthState};
use crate::pages::{
    areas::AreasPage,
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
use leptos_shadcn_ui::{Button, ButtonVariant};

// ── Root ──────────────────────────────────────────────────────────────────────

#[component]
pub fn App() -> impl IntoView {
    provide_context(AuthState::new());

    view! {
        <Router>
            <Routes fallback=|| view! { <p class="msg-error">"Page not found."</p> }>
                <Route path=path!("/")        view=HomeRedirect />
                <Route path=path!("/login")   view=LoginPage />
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
            </Routes>
        </Router>
    }
}

// ── Home redirect ─────────────────────────────────────────────────────────────

#[component]
fn HomeRedirect() -> impl IntoView {
    let navigate = leptos_router::hooks::use_navigate();
    Effect::new(move |_| {
        navigate("/devices", Default::default());
    });
    view! {}
}

// ── Auth guard ────────────────────────────────────────────────────────────────
//
// Non-reactive: checks auth once at render time.
// The Effect handles the redirect; the guard shows nothing until auth resolves.

#[component]
fn AuthGuard(children: Children) -> impl IntoView {
    let auth = use_auth();
    let navigate = leptos_router::hooks::use_navigate();

    if auth.is_authenticated() {
        view! { <NavShell>{children()}</NavShell> }.into_any()
    } else {
        Effect::new(move |_| {
            navigate("/login", Default::default());
        });
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

    let location = use_location();

    let username = move || auth.user.get().map(|u| u.username).unwrap_or_default();
    let role = move || auth.user.get().map(|u| u.role).unwrap_or_default();

    // Active class helper — reacts to route changes
    let active = move |prefix: &'static str| {
        move || {
            let pathname = location.pathname.get();
            if pathname.starts_with(prefix) {
                "active"
            } else {
                ""
            }
        }
    };

    view! {
        <div class="shell">
            <aside class="sidebar">
                <div>
                    <h1><a href="/devices">"HomeCore"</a></h1>
                    <p class="subtitle">"Leptos web client"</p>
                </div>
                <nav>
                    <a href="/devices" class=active("/devices")>
                        <span class="material-icons" style="font-size:18px">"dashboard"</span>
                        "Devices"
                    </a>
                    <a href="/areas" class=active("/areas")>
                        <span class="material-icons" style="font-size:18px">"home_work"</span>
                        "Areas"
                    </a>
                    <a href="/scenes" class=active("/scenes")>
                        <span class="material-icons" style="font-size:18px">"lightbulb"</span>
                        "Scenes"
                    </a>
                    <a href="/modes" class=active("/modes")>
                        <span class="material-icons" style="font-size:18px">"tune"</span>
                        "Modes"
                    </a>
                    <a href="/events" class=active("/events")>
                        <span class="material-icons" style="font-size:18px">"bolt"</span>
                        "Events"
                    </a>
                    <a href="/rules" class=active("/rules")>
                        <span class="material-icons" style="font-size:18px">"smart_toy"</span>
                        "Rules"
                    </a>
                    <a href="/glue" class=active("/glue")>
                        <span class="material-icons" style="font-size:18px">"extension"</span>
                        "Glue"
                    </a>
                    <a href="/plugins" class=active("/plugins")>
                        <span class="material-icons" style="font-size:18px">"widgets"</span>
                        "Plugins"
                    </a>
                    <a href="/dashboards" class=active("/dashboards")>
                        <span class="material-icons" style="font-size:18px">"dashboard"</span>
                        "Dashboards"
                    </a>
                </nav>
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
                    <Button
                        variant=ButtonVariant::Outline
                        on_click=Callback::new(move |_| auth.logout())
                    >
                        "Logout"
                    </Button>
                </header>
                <main class="content">
                    {children()}
                </main>
            </div>
        </div>
    }
}
