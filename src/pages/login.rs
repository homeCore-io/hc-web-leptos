//! Login page — username + password form, stores JWT on success.

use crate::auth::{api_login, use_auth};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use leptos_shadcn_ui::Input;

#[component]
pub fn LoginPage() -> impl IntoView {
    let auth = use_auth();
    let navigate = use_navigate();

    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let error = RwSignal::new(Option::<String>::None);
    let loading = RwSignal::new(false);

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
                    <h1>"HomeCore"</h1>
                    <p class="subtitle">"Sign in to your instance"</p>
                </div>

                <form on:submit=submit>
                    <div style="display:grid;gap:0.85rem;">
                        <div class="login-field">
                            <label for="username">"Username"</label>
                            <Input
                                id="username"
                                value=Signal::derive(move || username.get())
                                on_change=Callback::new(move |value| username.set(value))
                                placeholder="admin"
                                disabled=is_loading
                                input_type="text"
                            />
                        </div>
                        <div class="login-field">
                            <label for="password">"Password"</label>
                            <Input
                                id="password"
                                value=Signal::derive(move || password.get())
                                on_change=Callback::new(move |value| password.set(value))
                                input_type="password"
                                disabled=is_loading
                            />
                        </div>
                    </div>

                    {move || error.get().map(|e| view! {
                        <p class="msg-error">{e}</p>
                    })}

                    <div style="margin-top:0.5rem;">
                        <button
                            type="submit"
                            class="primary hc-btn-block"
                            disabled=is_loading
                        >
                            {move || if loading.get() { "Signing in…" } else { "Sign in" }}
                        </button>
                    </div>
                </form>
            </div>
        </div>
    }
}
