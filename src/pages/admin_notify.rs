//! Notify channels editor — `/admin/notifications` tab.
//!
//! Renders one card per `[[notify.channels]]` entry with type-aware
//! sub-forms.  Channels are saved as a single array-of-tables write,
//! so the operator gets all-or-nothing semantics: either the whole
//! list lands on disk or nothing changes.
//!
//! Supported provider types: `email`, `pushover`, `telegram`.

use crate::api::{fetch_system_config, put_system_config_array_of_tables};
use crate::auth::use_auth;
use crate::pages::shared::ErrorBanner;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
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

/// Editable shape of a single channel row.  All variants share `name`;
/// the rest is provider-specific so the form can render the right
/// fields without an unused-field grab-bag.
#[derive(Clone, Debug)]
pub struct ChannelRow {
    pub name: String,
    pub kind: ChannelKind,
    // email
    pub smtp_host: String,
    pub smtp_port: String,
    pub username: String,
    pub password: String,
    pub from: String,
    pub to: String, // comma-separated
    pub starttls: bool,
    // pushover
    pub api_token: String,
    pub user_key: String,
    pub device: String,
    pub priority: String,
    // telegram
    pub bot_token: String,
    pub chat_id: String,
    pub markdown: bool,
}

impl ChannelRow {
    fn new(kind: ChannelKind) -> Self {
        Self {
            name: String::new(),
            kind,
            smtp_host: String::new(),
            smtp_port: String::from("587"),
            username: String::new(),
            password: String::new(),
            from: String::new(),
            to: String::new(),
            starttls: true,
            api_token: String::new(),
            user_key: String::new(),
            device: String::new(),
            priority: String::new(),
            bot_token: String::new(),
            chat_id: String::new(),
            markdown: false,
        }
    }

    fn from_value(v: &Value) -> Option<Self> {
        let obj = v.as_object()?;
        let name = obj.get("name")?.as_str()?.to_string();
        let tag = obj.get("type").and_then(|x| x.as_str())?;
        let kind = ChannelKind::from_tag(tag)?;
        let mut row = Self::new(kind);
        row.name = name;

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

        row.smtp_host = s("smtp_host");
        row.smtp_port = i("smtp_port");
        if row.smtp_port.is_empty() {
            row.smtp_port = "587".into();
        }
        row.username = s("username");
        row.password = s("password");
        row.from = s("from");
        row.to = to_csv("to");
        row.starttls = b("starttls", true);

        row.api_token = s("api_token");
        row.user_key = s("user_key");
        row.device = s("device");
        row.priority = i("priority");

        row.bot_token = s("bot_token");
        row.chat_id = s("chat_id");
        row.markdown = b("markdown", false);

        Some(row)
    }

    fn to_value(&self) -> Result<Value, String> {
        if self.name.trim().is_empty() {
            return Err("Channel name cannot be empty".into());
        }
        let mut m = serde_json::Map::new();
        m.insert("name".into(), Value::String(self.name.trim().into()));
        m.insert("type".into(), Value::String(self.kind.as_tag().into()));

        match self.kind {
            ChannelKind::Email => {
                if self.smtp_host.trim().is_empty() {
                    return Err(format!("[{}] smtp_host required", self.name));
                }
                m.insert("smtp_host".into(), Value::String(self.smtp_host.trim().into()));
                if !self.smtp_port.trim().is_empty() {
                    let port: i64 = self
                        .smtp_port
                        .trim()
                        .parse()
                        .map_err(|_| format!("[{}] smtp_port not a number", self.name))?;
                    m.insert("smtp_port".into(), Value::Number(port.into()));
                }
                m.insert("username".into(), Value::String(self.username.trim().into()));
                m.insert("password".into(), Value::String(self.password.clone()));
                m.insert("from".into(), Value::String(self.from.trim().into()));
                let to: Vec<Value> = self
                    .to
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| Value::String(s.into()))
                    .collect();
                if to.is_empty() {
                    return Err(format!("[{}] at least one recipient required", self.name));
                }
                m.insert("to".into(), Value::Array(to));
                m.insert("starttls".into(), Value::Bool(self.starttls));
            }
            ChannelKind::Pushover => {
                if self.api_token.trim().is_empty() || self.user_key.trim().is_empty() {
                    return Err(format!(
                        "[{}] api_token and user_key required",
                        self.name
                    ));
                }
                m.insert(
                    "api_token".into(),
                    Value::String(self.api_token.trim().into()),
                );
                m.insert(
                    "user_key".into(),
                    Value::String(self.user_key.trim().into()),
                );
                if !self.device.trim().is_empty() {
                    m.insert("device".into(), Value::String(self.device.trim().into()));
                }
                if !self.priority.trim().is_empty() {
                    let p: i64 = self
                        .priority
                        .trim()
                        .parse()
                        .map_err(|_| format!("[{}] priority not a number", self.name))?;
                    m.insert("priority".into(), Value::Number(p.into()));
                }
            }
            ChannelKind::Telegram => {
                if self.bot_token.trim().is_empty() || self.chat_id.trim().is_empty() {
                    return Err(format!(
                        "[{}] bot_token and chat_id required",
                        self.name
                    ));
                }
                m.insert(
                    "bot_token".into(),
                    Value::String(self.bot_token.trim().into()),
                );
                m.insert("chat_id".into(), Value::String(self.chat_id.trim().into()));
                m.insert("markdown".into(), Value::Bool(self.markdown));
            }
        }

        Ok(Value::Object(m))
    }
}

#[component]
pub fn NotificationsTab() -> impl IntoView {
    let auth = use_auth();
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
                        .map(|arr| {
                            arr.iter().filter_map(ChannelRow::from_value).collect::<Vec<_>>()
                        })
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
            let name = row.name.trim();
            if !seen.insert(name.to_string()) {
                error.set(Some(format!("Duplicate channel name: {name:?}")));
                return;
            }
        }

        saving.set(true);
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match put_system_config_array_of_tables(&token, "notify.channels", &items).await {
                Ok(_) => {
                    notice.set(Some(
                        "Saved. Restart core for changes to take effect.".into(),
                    ));
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
                {move || {
                    let snapshot = rows.get();
                    if snapshot.is_empty() {
                        view! {
                            <p style="color:var(--hc-text-muted); margin:0.5rem 0">
                                "No notification channels configured."
                            </p>
                        }.into_any()
                    } else {
                        view! {
                            <div style="display:flex; flex-direction:column; gap:0.75rem">
                                {(0..snapshot.len()).map(|idx| view! {
                                    <ChannelCard idx=idx rows=rows />
                                }).collect_view()}
                            </div>
                        }.into_any()
                    }
                }}
            </Show>

            // ── Add + Save ───────────────────────────────────────────────
            <div style="display:flex; gap:0.5rem; flex-wrap:wrap; margin-top:1rem; align-items:center">
                <span style="color:var(--hc-text-muted); margin-right:0.25rem">"Add:"</span>
                <button
                    class="hc-btn hc-btn--sm hc-btn--outline"
                    on:click=move |_| add_channel(ChannelKind::Email)
                >"+ Email"</button>
                <button
                    class="hc-btn hc-btn--sm hc-btn--outline"
                    on:click=move |_| add_channel(ChannelKind::Pushover)
                >"+ Pushover"</button>
                <button
                    class="hc-btn hc-btn--sm hc-btn--outline"
                    on:click=move |_| add_channel(ChannelKind::Telegram)
                >"+ Telegram"</button>

                <span style="flex:1"></span>

                <button
                    class="hc-btn hc-btn--primary"
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
fn ChannelCard(idx: usize, rows: RwSignal<Vec<ChannelRow>>) -> impl IntoView {
    let row = move || rows.with(|r| r.get(idx).cloned());

    let update = move |f: Box<dyn Fn(&mut ChannelRow)>| {
        rows.update(|r| {
            if let Some(row) = r.get_mut(idx) {
                f(row);
            }
        });
    };
    let update = std::sync::Arc::new(update);

    let remove = move |_| {
        rows.update(|r| {
            if idx < r.len() {
                r.remove(idx);
            }
        });
    };

    view! {
        <div class="hc-card hc-card--inset" style="padding:0.75rem">
            {move || row().map(|r| {
                let kind_label = r.kind.label();
                let upd = update.clone();

                let upd_name = upd.clone();
                let upd_kind = upd.clone();

                let kind_select = view! {
                    <select
                        class="hc-input"
                        on:change=move |ev| {
                            let val = event_target_value(&ev);
                            if let Some(k) = ChannelKind::from_tag(&val) {
                                upd_kind(Box::new(move |row: &mut ChannelRow| row.kind = k.clone()));
                            }
                        }
                        prop:value=r.kind.as_tag()
                    >
                        <option value="email">"Email (SMTP)"</option>
                        <option value="pushover">"Pushover"</option>
                        <option value="telegram">"Telegram"</option>
                    </select>
                };

                view! {
                    <div style="display:flex; align-items:center; gap:0.5rem; margin-bottom:0.5rem">
                        <span class="admin-badge admin-badge--user">{kind_label}</span>
                        <strong style="flex:1">
                            {if r.name.is_empty() { "(unnamed)".to_string() } else { r.name.clone() }}
                        </strong>
                        <button
                            class="hc-btn hc-btn--sm hc-btn--danger-outline"
                            on:click=remove
                            title="Remove channel"
                        >"Remove"</button>
                    </div>

                    <div style="display:grid; grid-template-columns: max-content 1fr; gap:0.5rem 0.75rem; align-items:center">
                        <label>"Name"</label>
                        <input class="hc-input"
                            prop:value=r.name.clone()
                            on:input=move |ev| {
                                let v = event_target_value(&ev);
                                upd_name(Box::new(move |row: &mut ChannelRow| row.name = v.clone()));
                            }
                            placeholder="e.g. phone, ops-email"
                        />

                        <label>"Type"</label>
                        {kind_select}

                        {kind_specific_fields(idx, r.clone(), rows)}
                    </div>
                }.into_any()
            })}
        </div>
    }
}

fn kind_specific_fields(idx: usize, row: ChannelRow, rows: RwSignal<Vec<ChannelRow>>) -> AnyView {
    let upd = std::sync::Arc::new(move |f: Box<dyn Fn(&mut ChannelRow)>| {
        rows.update(|r| {
            if let Some(row) = r.get_mut(idx) {
                f(row);
            }
        });
    });

    match row.kind {
        ChannelKind::Email => {
            let u1 = upd.clone();
            let u2 = upd.clone();
            let u3 = upd.clone();
            let u4 = upd.clone();
            let u5 = upd.clone();
            let u6 = upd.clone();
            let u7 = upd.clone();
            view! {
                <label>"SMTP host"</label>
                <input class="hc-input"
                    prop:value=row.smtp_host.clone()
                    on:input=move |ev| { let v = event_target_value(&ev);
                        u1(Box::new(move |r: &mut ChannelRow| r.smtp_host = v.clone())); }
                    placeholder="smtp.example.com"
                />

                <label>"SMTP port"</label>
                <input class="hc-input" type="number"
                    prop:value=row.smtp_port.clone()
                    on:input=move |ev| { let v = event_target_value(&ev);
                        u2(Box::new(move |r: &mut ChannelRow| r.smtp_port = v.clone())); }
                />

                <label>"Username"</label>
                <input class="hc-input"
                    prop:value=row.username.clone()
                    on:input=move |ev| { let v = event_target_value(&ev);
                        u3(Box::new(move |r: &mut ChannelRow| r.username = v.clone())); }
                />

                <label>"Password"</label>
                <input class="hc-input" type="password"
                    prop:value=row.password.clone()
                    on:input=move |ev| { let v = event_target_value(&ev);
                        u4(Box::new(move |r: &mut ChannelRow| r.password = v.clone())); }
                />

                <label>"From"</label>
                <input class="hc-input"
                    prop:value=row.from.clone()
                    on:input=move |ev| { let v = event_target_value(&ev);
                        u5(Box::new(move |r: &mut ChannelRow| r.from = v.clone())); }
                    placeholder="HomeCore <alerts@example.com>"
                />

                <label>"To (comma-separated)"</label>
                <input class="hc-input"
                    prop:value=row.to.clone()
                    on:input=move |ev| { let v = event_target_value(&ev);
                        u6(Box::new(move |r: &mut ChannelRow| r.to = v.clone())); }
                    placeholder="ops@example.com, oncall@example.com"
                />

                <label>"STARTTLS"</label>
                <label style="justify-self:start">
                    <input type="checkbox"
                        prop:checked=row.starttls
                        on:change=move |ev| {
                            let v = event_target_checked(&ev);
                            u7(Box::new(move |r: &mut ChannelRow| r.starttls = v));
                        }
                    />
                    " "<span style="color:var(--hc-text-muted)">"Use STARTTLS (port 587). Uncheck for implicit TLS (port 465)."</span>
                </label>
            }.into_any()
        }
        ChannelKind::Pushover => {
            let u1 = upd.clone();
            let u2 = upd.clone();
            let u3 = upd.clone();
            let u4 = upd.clone();
            view! {
                <label>"API token"</label>
                <input class="hc-input"
                    prop:value=row.api_token.clone()
                    on:input=move |ev| { let v = event_target_value(&ev);
                        u1(Box::new(move |r: &mut ChannelRow| r.api_token = v.clone())); }
                />

                <label>"User key"</label>
                <input class="hc-input"
                    prop:value=row.user_key.clone()
                    on:input=move |ev| { let v = event_target_value(&ev);
                        u2(Box::new(move |r: &mut ChannelRow| r.user_key = v.clone())); }
                />

                <label>"Device (optional)"</label>
                <input class="hc-input"
                    prop:value=row.device.clone()
                    on:input=move |ev| { let v = event_target_value(&ev);
                        u3(Box::new(move |r: &mut ChannelRow| r.device = v.clone())); }
                    placeholder="leave blank for all devices"
                />

                <label>"Priority (-2..2)"</label>
                <input class="hc-input" type="number" min="-2" max="2"
                    prop:value=row.priority.clone()
                    on:input=move |ev| { let v = event_target_value(&ev);
                        u4(Box::new(move |r: &mut ChannelRow| r.priority = v.clone())); }
                />
            }.into_any()
        }
        ChannelKind::Telegram => {
            let u1 = upd.clone();
            let u2 = upd.clone();
            let u3 = upd.clone();
            view! {
                <label>"Bot token"</label>
                <input class="hc-input"
                    prop:value=row.bot_token.clone()
                    on:input=move |ev| { let v = event_target_value(&ev);
                        u1(Box::new(move |r: &mut ChannelRow| r.bot_token = v.clone())); }
                />

                <label>"Chat ID"</label>
                <input class="hc-input"
                    prop:value=row.chat_id.clone()
                    on:input=move |ev| { let v = event_target_value(&ev);
                        u2(Box::new(move |r: &mut ChannelRow| r.chat_id = v.clone())); }
                />

                <label>"Markdown"</label>
                <label style="justify-self:start">
                    <input type="checkbox"
                        prop:checked=row.markdown
                        on:change=move |ev| {
                            let v = event_target_checked(&ev);
                            u3(Box::new(move |r: &mut ChannelRow| r.markdown = v));
                        }
                    />
                    " "<span style="color:var(--hc-text-muted)">"Render messages as MarkdownV2."</span>
                </label>
            }.into_any()
        }
    }
}
