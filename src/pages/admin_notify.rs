//! Notify channels editor — `/admin/notifications` tab.
//!
//! Renders one card per `[[notify.channels]]` entry with type-aware
//! sub-forms.  Channels are saved as a single array-of-tables write,
//! so the operator gets all-or-nothing semantics: either the whole
//! list lands on disk or nothing changes.
//!
//! Each editable field is its own `RwSignal<String>` per row so
//! typing into an input never causes the parent Vec to re-emit and
//! drop focus mid-key.

use crate::api::{fetch_system_config, put_system_config_array_of_tables};
use crate::auth::use_auth;
use crate::pages::shared::ErrorBanner;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelKind {
    Email,
    Pushover,
    Telegram,
}

impl ChannelKind {
    fn as_tag(&self) -> &'static str {
        match self {
            ChannelKind::Email => "email",
            ChannelKind::Pushover => "pushover",
            ChannelKind::Telegram => "telegram",
        }
    }

    fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "email" => Some(ChannelKind::Email),
            "pushover" => Some(ChannelKind::Pushover),
            "telegram" => Some(ChannelKind::Telegram),
            _ => None,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            ChannelKind::Email => "Email (SMTP)",
            ChannelKind::Pushover => "Pushover",
            ChannelKind::Telegram => "Telegram",
        }
    }
}

/// One row of stable signals — each field has its own RwSignal so
/// inputs can bind directly without going through the parent Vec.
/// Typing only updates the field's own signal, leaving every other
/// input (and the rows Vec) untouched, so focus survives keystrokes.
#[derive(Clone, Copy)]
pub struct ChannelRow {
    pub kind: RwSignal<ChannelKind>,
    pub name: RwSignal<String>,
    // email
    pub smtp_host: RwSignal<String>,
    pub smtp_port: RwSignal<String>,
    pub username: RwSignal<String>,
    pub password: RwSignal<String>,
    pub from: RwSignal<String>,
    pub to: RwSignal<String>, // comma-separated
    pub starttls: RwSignal<bool>,
    // pushover
    pub api_token: RwSignal<String>,
    pub user_key: RwSignal<String>,
    pub device: RwSignal<String>,
    pub priority: RwSignal<String>,
    // telegram
    pub bot_token: RwSignal<String>,
    pub chat_id: RwSignal<String>,
    pub markdown: RwSignal<bool>,
}

impl ChannelRow {
    fn new(kind: ChannelKind) -> Self {
        Self {
            kind: RwSignal::new(kind),
            name: RwSignal::new(String::new()),
            smtp_host: RwSignal::new(String::new()),
            smtp_port: RwSignal::new(String::from("587")),
            username: RwSignal::new(String::new()),
            password: RwSignal::new(String::new()),
            from: RwSignal::new(String::new()),
            to: RwSignal::new(String::new()),
            starttls: RwSignal::new(true),
            api_token: RwSignal::new(String::new()),
            user_key: RwSignal::new(String::new()),
            device: RwSignal::new(String::new()),
            priority: RwSignal::new(String::new()),
            bot_token: RwSignal::new(String::new()),
            chat_id: RwSignal::new(String::new()),
            markdown: RwSignal::new(false),
        }
    }

    fn from_value(v: &Value) -> Option<Self> {
        let obj = v.as_object()?;
        let name = obj.get("name")?.as_str()?.to_string();
        let tag = obj.get("type").and_then(|x| x.as_str())?;
        let kind = ChannelKind::from_tag(tag)?;
        let row = Self::new(kind);
        row.name.set(name);

        let s = |k: &str| -> String {
            obj.get(k)
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default()
        };
        let i = |k: &str| -> String {
            obj.get(k)
                .and_then(|x| x.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_default()
        };
        let b = |k: &str, default: bool| -> bool {
            obj.get(k).and_then(|x| x.as_bool()).unwrap_or(default)
        };
        let to_csv = |k: &str| -> String {
            obj.get(k)
                .and_then(|x| x.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default()
        };

        row.smtp_host.set(s("smtp_host"));
        let port = i("smtp_port");
        row.smtp_port.set(if port.is_empty() { "587".into() } else { port });
        row.username.set(s("username"));
        row.password.set(s("password"));
        row.from.set(s("from"));
        row.to.set(to_csv("to"));
        row.starttls.set(b("starttls", true));

        row.api_token.set(s("api_token"));
        row.user_key.set(s("user_key"));
        row.device.set(s("device"));
        row.priority.set(i("priority"));

        row.bot_token.set(s("bot_token"));
        row.chat_id.set(s("chat_id"));
        row.markdown.set(b("markdown", false));

        Some(row)
    }

    fn to_value(&self) -> Result<Value, String> {
        let name = self.name.get_untracked().trim().to_string();
        if name.is_empty() {
            return Err("Channel name cannot be empty".into());
        }
        let kind = self.kind.get_untracked();

        let mut m = serde_json::Map::new();
        m.insert("name".into(), Value::String(name.clone()));
        m.insert("type".into(), Value::String(kind.as_tag().into()));

        match kind {
            ChannelKind::Email => {
                let smtp_host = self.smtp_host.get_untracked();
                if smtp_host.trim().is_empty() {
                    return Err(format!("[{name}] smtp_host required"));
                }
                m.insert("smtp_host".into(), Value::String(smtp_host.trim().into()));
                let smtp_port = self.smtp_port.get_untracked();
                if !smtp_port.trim().is_empty() {
                    let port: i64 = smtp_port
                        .trim()
                        .parse()
                        .map_err(|_| format!("[{name}] smtp_port not a number"))?;
                    m.insert("smtp_port".into(), Value::Number(port.into()));
                }
                m.insert(
                    "username".into(),
                    Value::String(self.username.get_untracked().trim().into()),
                );
                m.insert("password".into(), Value::String(self.password.get_untracked()));
                m.insert(
                    "from".into(),
                    Value::String(self.from.get_untracked().trim().into()),
                );
                let to: Vec<Value> = self
                    .to
                    .get_untracked()
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| Value::String(s.into()))
                    .collect();
                if to.is_empty() {
                    return Err(format!("[{name}] at least one recipient required"));
                }
                m.insert("to".into(), Value::Array(to));
                m.insert("starttls".into(), Value::Bool(self.starttls.get_untracked()));
            }
            ChannelKind::Pushover => {
                let api_token = self.api_token.get_untracked();
                let user_key = self.user_key.get_untracked();
                if api_token.trim().is_empty() || user_key.trim().is_empty() {
                    return Err(format!("[{name}] api_token and user_key required"));
                }
                m.insert("api_token".into(), Value::String(api_token.trim().into()));
                m.insert("user_key".into(), Value::String(user_key.trim().into()));
                let device = self.device.get_untracked();
                if !device.trim().is_empty() {
                    m.insert("device".into(), Value::String(device.trim().into()));
                }
                let priority = self.priority.get_untracked();
                if !priority.trim().is_empty() {
                    let p: i64 = priority
                        .trim()
                        .parse()
                        .map_err(|_| format!("[{name}] priority not a number"))?;
                    m.insert("priority".into(), Value::Number(p.into()));
                }
            }
            ChannelKind::Telegram => {
                let bot_token = self.bot_token.get_untracked();
                let chat_id = self.chat_id.get_untracked();
                if bot_token.trim().is_empty() || chat_id.trim().is_empty() {
                    return Err(format!("[{name}] bot_token and chat_id required"));
                }
                m.insert("bot_token".into(), Value::String(bot_token.trim().into()));
                m.insert("chat_id".into(), Value::String(chat_id.trim().into()));
                m.insert("markdown".into(), Value::Bool(self.markdown.get_untracked()));
            }
        }

        Ok(Value::Object(m))
    }
}

#[component]
pub fn NotificationsTab() -> impl IntoView {
    let auth = use_auth();
    // The Vec only changes shape on add/remove. Editing fields mutates
    // per-row signals and never touches this signal, so focus survives.
    let rows: RwSignal<Vec<ChannelRow>> = RwSignal::new(Vec::new());
    let loading = RwSignal::new(true);
    let saving = RwSignal::new(false);
    let error: RwSignal<Option<String>> = RwSignal::new(None);
    let notice: RwSignal<Option<String>> = RwSignal::new(None);

    let load = move || {
        let token = match auth.token.get_untracked() {
            Some(t) => t,
            None => return,
        };
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match fetch_system_config(&token).await {
                Ok(cfg) => {
                    let channels = cfg
                        .get("notify")
                        .and_then(|n| n.get("channels"))
                        .and_then(|c| c.as_array())
                        .map(|arr| arr.iter().filter_map(ChannelRow::from_value).collect::<Vec<_>>())
                        .unwrap_or_default();
                    rows.set(channels);
                }
                Err(e) => error.set(Some(format!("Load failed: {e}"))),
            }
            loading.set(false);
        });
    };

    Effect::new(move |_| {
        if auth.token.get().is_some() {
            load();
        }
    });

    let save_all = move |_| {
        let token = match auth.token.get_untracked() {
            Some(t) => t,
            None => return,
        };
        let snapshot = rows.get_untracked();

        // Validate + serialize before sending so partial writes don't
        // leave the file in a half-edited state.
        let mut items: Vec<Value> = Vec::with_capacity(snapshot.len());
        for row in &snapshot {
            match row.to_value() {
                Ok(v) => items.push(v),
                Err(e) => {
                    error.set(Some(e));
                    return;
                }
            }
        }

        // Reject duplicate names — rule engine looks them up by name.
        let mut seen = std::collections::HashSet::new();
        for row in &snapshot {
            let name = row.name.get_untracked();
            let trimmed = name.trim();
            if !seen.insert(trimmed.to_string()) {
                error.set(Some(format!("Duplicate channel name: {trimmed:?}")));
                return;
            }
        }

        saving.set(true);
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match put_system_config_array_of_tables(&token, "notify.channels", &items).await {
                Ok(_) => {
                    notice.set(Some("Saved. Restart core for changes to take effect.".into()));
                }
                Err(e) => error.set(Some(format!("Save failed: {e}"))),
            }
            saving.set(false);
        });
    };

    let add_channel = move |kind: ChannelKind| {
        rows.update(|r| r.push(ChannelRow::new(kind)));
    };

    view! {
        <section class="hc-card">
            <h2>"Notification Channels"</h2>
            <p class="hc-card__hint">
                "Each channel maps to a "<code>"[[notify.channels]]"</code>" entry. \
                 Channel names are referenced by rule "<code>"Notify { channel: \"...\" }"</code>" actions; \
                 the special name "<code>"all"</code>" fans out to every registered channel."
            </p>

            <ErrorBanner error=error />
            {move || notice.get().map(|n| view! {
                <div class="msg-success" style="margin-bottom:0.75rem">{n}</div>
            })}

            <Show when=move || !loading.get() fallback=|| view! { <p>"Loading…"</p> }>
                <For
                    each=move || rows.get().into_iter().enumerate()
                    key=|(idx, _)| *idx
                    children=move |(idx, row)| view! { <ChannelCard idx=idx row=row rows=rows /> }
                />
                {move || rows.get().is_empty().then(|| view! {
                    <p style="color:var(--hc-text-muted); margin:0.5rem 0">
                        "No notification channels configured."
                    </p>
                })}
            </Show>

            // ── Add + Save ───────────────────────────────────────────────
            <div style="display:flex; gap:0.5rem; flex-wrap:wrap; margin-top:1rem; align-items:center">
                <span style="color:var(--hc-text-muted); margin-right:0.25rem">"Add:"</span>
                <button class="hc-btn hc-btn--sm hc-btn--outline"
                    on:click=move |_| add_channel(ChannelKind::Email)
                >"+ Email"</button>
                <button class="hc-btn hc-btn--sm hc-btn--outline"
                    on:click=move |_| add_channel(ChannelKind::Pushover)
                >"+ Pushover"</button>
                <button class="hc-btn hc-btn--sm hc-btn--outline"
                    on:click=move |_| add_channel(ChannelKind::Telegram)
                >"+ Telegram"</button>

                <span style="flex:1"></span>

                <button class="hc-btn hc-btn--primary"
                    on:click=save_all
                    disabled=move || saving.get()
                >
                    {move || if saving.get() { "Saving…" } else { "Save All" }}
                </button>
            </div>
        </section>
    }
}

#[component]
fn ChannelCard(idx: usize, row: ChannelRow, rows: RwSignal<Vec<ChannelRow>>) -> impl IntoView {
    let remove = move |_| {
        rows.update(|r| {
            if idx < r.len() {
                r.remove(idx);
            }
        });
    };

    view! {
        <div class="hc-card hc-card--inset" style="padding:0.75rem; margin-bottom:0.6rem">
            <div style="display:flex; align-items:center; gap:0.5rem; margin-bottom:0.5rem">
                <span class="admin-badge admin-badge--user">
                    {move || row.kind.get().label()}
                </span>
                <strong style="flex:1">
                    {move || {
                        let n = row.name.get();
                        if n.is_empty() { "(unnamed)".to_string() } else { n }
                    }}
                </strong>
                <button class="hc-btn hc-btn--sm hc-btn--danger-outline"
                    on:click=remove
                    title="Remove channel"
                >"Remove"</button>
            </div>

            <div class="hc-form-grid">
                <label class="hc-form-label">"Name"</label>
                <div class="hc-form-control">
                    <input class="hc-form-input hc-form-input--md" type="text"
                        prop:value=move || row.name.get()
                        on:input=move |ev| row.name.set(event_target_value(&ev))
                        placeholder="e.g. phone, ops-email"
                    />
                </div>

                <label class="hc-form-label">"Type"</label>
                <div class="hc-form-control">
                    <select class="hc-input"
                        on:change=move |ev| {
                            let v = event_target_value(&ev);
                            if let Some(k) = ChannelKind::from_tag(&v) {
                                row.kind.set(k);
                            }
                        }
                        prop:value=move || row.kind.get().as_tag()
                    >
                        <option value="email">"Email (SMTP)"</option>
                        <option value="pushover">"Pushover"</option>
                        <option value="telegram">"Telegram"</option>
                    </select>
                </div>

                {move || match row.kind.get() {
                    ChannelKind::Email    => email_fields(row).into_any(),
                    ChannelKind::Pushover => pushover_fields(row).into_any(),
                    ChannelKind::Telegram => telegram_fields(row).into_any(),
                }}
            </div>
        </div>
    }
}

fn email_fields(row: ChannelRow) -> impl IntoView {
    view! {
        <label class="hc-form-label">"SMTP host"</label>
        <div class="hc-form-control">
            <input class="hc-form-input hc-form-input--md" type="text"
                prop:value=move || row.smtp_host.get()
                on:input=move |ev| row.smtp_host.set(event_target_value(&ev))
                placeholder="smtp.example.com"
            />
        </div>

        <label class="hc-form-label">"SMTP port"</label>
        <div class="hc-form-control">
            <input class="hc-form-input hc-form-input--xs" type="number"
                prop:value=move || row.smtp_port.get()
                on:input=move |ev| row.smtp_port.set(event_target_value(&ev))
            />
        </div>

        <label class="hc-form-label">"Username"</label>
        <div class="hc-form-control">
            <input class="hc-form-input hc-form-input--md" type="text"
                prop:value=move || row.username.get()
                on:input=move |ev| row.username.set(event_target_value(&ev))
            />
        </div>

        <label class="hc-form-label">"Password"</label>
        <div class="hc-form-control">
            <input class="hc-form-input hc-form-input--md" type="password"
                prop:value=move || row.password.get()
                on:input=move |ev| row.password.set(event_target_value(&ev))
            />
        </div>

        <label class="hc-form-label">"From"</label>
        <div class="hc-form-control">
            <input class="hc-form-input hc-form-input--md" type="text"
                prop:value=move || row.from.get()
                on:input=move |ev| row.from.set(event_target_value(&ev))
                placeholder="HomeCore <alerts@example.com>"
            />
        </div>

        <label class="hc-form-label">"To"</label>
        <div class="hc-form-control">
            <input class="hc-form-input hc-form-input--md" type="text"
                prop:value=move || row.to.get()
                on:input=move |ev| row.to.set(event_target_value(&ev))
                placeholder="ops@example.com, oncall@example.com"
            />
            <span class="hc-form-help">"Comma-separated."</span>
        </div>

        <label class="hc-form-label">"STARTTLS"</label>
        <div class="hc-form-control">
            <input type="checkbox" class="hc-form-checkbox"
                prop:checked=move || row.starttls.get()
                on:change=move |ev| {
                    let target: web_sys::HtmlInputElement =
                        wasm_bindgen::JsCast::unchecked_into(ev.target().unwrap());
                    row.starttls.set(target.checked());
                }
            />
            <span class="hc-form-help">"Port 587 STARTTLS. Uncheck for implicit TLS (465)."</span>
        </div>
    }
}

fn pushover_fields(row: ChannelRow) -> impl IntoView {
    view! {
        <label class="hc-form-label">"API token"</label>
        <div class="hc-form-control">
            <input class="hc-form-input hc-form-input--md" type="text"
                prop:value=move || row.api_token.get()
                on:input=move |ev| row.api_token.set(event_target_value(&ev))
            />
        </div>

        <label class="hc-form-label">"User key"</label>
        <div class="hc-form-control">
            <input class="hc-form-input hc-form-input--md" type="text"
                prop:value=move || row.user_key.get()
                on:input=move |ev| row.user_key.set(event_target_value(&ev))
            />
        </div>

        <label class="hc-form-label">"Device"</label>
        <div class="hc-form-control">
            <input class="hc-form-input hc-form-input--md" type="text"
                prop:value=move || row.device.get()
                on:input=move |ev| row.device.set(event_target_value(&ev))
                placeholder="leave blank for all devices"
            />
        </div>

        <label class="hc-form-label">"Priority"</label>
        <div class="hc-form-control">
            <input class="hc-form-input hc-form-input--xs" type="number" min="-2" max="2"
                prop:value=move || row.priority.get()
                on:input=move |ev| row.priority.set(event_target_value(&ev))
            />
            <span class="hc-form-help">"-2 to 2 (Pushover priority)."</span>
        </div>
    }
}

fn telegram_fields(row: ChannelRow) -> impl IntoView {
    view! {
        <label class="hc-form-label">"Bot token"</label>
        <div class="hc-form-control">
            <input class="hc-form-input hc-form-input--md" type="text"
                prop:value=move || row.bot_token.get()
                on:input=move |ev| row.bot_token.set(event_target_value(&ev))
            />
        </div>

        <label class="hc-form-label">"Chat ID"</label>
        <div class="hc-form-control">
            <input class="hc-form-input hc-form-input--md" type="text"
                prop:value=move || row.chat_id.get()
                on:input=move |ev| row.chat_id.set(event_target_value(&ev))
            />
        </div>

        <label class="hc-form-label">"Markdown"</label>
        <div class="hc-form-control">
            <input type="checkbox" class="hc-form-checkbox"
                prop:checked=move || row.markdown.get()
                on:change=move |ev| {
                    let target: web_sys::HtmlInputElement =
                        wasm_bindgen::JsCast::unchecked_into(ev.target().unwrap());
                    row.markdown.set(target.checked());
                }
            />
            <span class="hc-form-help">"Render messages as MarkdownV2."</span>
        </div>
    }
}
