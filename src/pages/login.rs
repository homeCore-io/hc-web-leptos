//! Login page — username + password form, stores JWT on success.

use crate::auth::{api_login, refresh_user, use_auth, API_BASE};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

#[component]
pub fn LoginPage() -> impl IntoView {
    let auth = use_auth();
    let navigate = use_navigate();

    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let error = RwSignal::new(Option::<String>::None);
    let loading = RwSignal::new(false);

    // Server-reported version, fetched from the unauthenticated /health
    // endpoint so the login screen can show it pre-auth. Same shape as
    // NavShell's version line, just sourced from /health (no token)
    // instead of /system/status (needs token).
    let server_version: RwSignal<Option<String>> = RwSignal::new(None);
    spawn_local(async move {
        if let Ok(resp) = gloo_net::http::Request::get(&format!("{API_BASE}/health"))
            .send()
            .await
        {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(v) = body.get("version").and_then(|v| v.as_str()) {
                    server_version.set(Some(v.to_string()));
                }
            }
        }
    });

    // Redirect already-authenticated users
    let nav_redirect = navigate.clone();
    Effect::new(move |_| {
        if auth.is_authenticated() {
            nav_redirect("/devices", Default::default());
        }
    });

    let submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let u = username.get();
        let p = password.get();
        if u.is_empty() || p.is_empty() {
            error.set(Some("Username and password are required.".into()));
            return;
        }
        loading.set(true);
        error.set(None);
        let nav = navigate.clone();
        spawn_local(async move {
            match api_login(&u, &p).await {
                Ok(tok) => {
                    auth.set_token(tok);
                    refresh_user(auth).await;
                    nav("/devices", Default::default());
                }
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    let is_loading = Signal::derive(move || loading.get());

    view! {
        <div class="login-wrap">
            <div class="login-card">
                <div>
                    <h1>
                        <span class="hc-wordmark">
                            <span class="hc-wordmark__a">"home"</span><span class="hc-wordmark__b">"Core"</span>
                        </span>
                    </h1>
                    <p class="login-tagline">"control surface"</p>
                    <p class="login-tagline login-tagline--version">{move || server_version.get().map(|v| format!("v{v}"))}</p>
                </div>

                <form on:submit=submit>
                    <div style="display:grid;gap:0.85rem;">
                        <label for="username">
                            "Username"
                            <input
                                id="username"
                                class="input"
                                type="text"
                                prop:value=move || username.get()
                                on:input=move |ev| username.set(event_target_value(&ev))
                                placeholder="admin"
                                disabled=is_loading
                            />
                        </label>
                        <label for="password">
                            "Password"
                            <input
                                id="password"
                                class="input"
                                type="password"
                                prop:value=move || password.get()
                                on:input=move |ev| password.set(event_target_value(&ev))
                                disabled=is_loading
                            />
                        </label>
                    </div>

                    {move || error.get().map(|e| view! {
                        <p class="msg-error">{e}</p>
                    })}

                    <button
                        type="submit"
                        class="primary hc-btn-block"
                        disabled=is_loading
                    >
                        {move || if loading.get() { "Signing in…" } else { "Sign in" }}
                    </button>
                </form>
            </div>
        </div>
    }
}
