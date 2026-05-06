//! Structured per-section forms for the admin Configuration tab.
//!
//! Each section of homecore.toml is described declaratively as a
//! `Section` (title + dotted TOML path + Vec<FieldSpec>). The
//! `SectionCard` component renders the accordion + form for one
//! section and handles loading the section's current values out of
//! the parsed config + saving via PUT /system/config patch.
//!
//! Adding a new section is one entry in `all_sections()`. Adding a
//! field to an existing section is one entry in its FieldSpec list.

use crate::api::put_system_config_patch;
use crate::auth::use_auth;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::{json, Map, Value};

// ── Field definitions ──────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub enum FieldKind {
    /// Plain string input.
    Text,
    /// Path string — same input as Text, just rendered with monospace.
    Path,
    /// Integer (i64). Stored as JSON Number.
    Integer,
    /// Floating-point (f64). Stored as JSON Number.
    Float,
    /// Boolean checkbox.
    Bool,
    /// Vec<String> rendered as a one-per-line textarea.
    StringList,
}

#[derive(Clone, Debug)]
pub struct FieldSpec {
    /// TOML field name within the section.
    pub key: &'static str,
    /// Operator-facing label.
    pub label: &'static str,
    /// Form input style.
    pub kind: FieldKind,
    /// Helper text under the input.
    pub help: &'static str,
}

#[derive(Clone, Debug)]
pub struct Section {
    /// Title shown in the card header.
    pub title: &'static str,
    /// Dotted TOML path (e.g. "auth.admin_uds").
    pub path: &'static str,
    /// One-line description shown under the title.
    pub help: &'static str,
    /// Whether the card opens expanded by default (most-edited sections).
    pub default_open: bool,
    /// Field declarations.
    pub fields: Vec<FieldSpec>,
}

// ── Section catalog ───────────────────────────────────────────────────────────

pub fn all_sections() -> Vec<Section> {
    vec![
        Section {
            title: "Server",
            path: "server",
            help: "REST + WebSocket API listener.",
            default_open: true,
            fields: vec![
                FieldSpec { key: "host", label: "Host",   kind: FieldKind::Text,    help: "Bind address. 0.0.0.0 = all interfaces." },
                FieldSpec { key: "port", label: "Port",   kind: FieldKind::Integer, help: "TCP port for the API. Default 8080." },
            ],
        },
        Section {
            title: "Embedded MQTT broker",
            path: "broker",
            help: "Built-in rumqttd broker. Plugins reach it directly when host-networked.",
            default_open: true,
            fields: vec![
                FieldSpec { key: "host",     label: "Host",     kind: FieldKind::Text,    help: "127.0.0.1 = single-host. 0.0.0.0 = LAN-reachable for multi-host plugins." },
                FieldSpec { key: "port",     label: "Port",     kind: FieldKind::Integer, help: "Default 1883." },
                FieldSpec { key: "tls_port", label: "TLS port", kind: FieldKind::Integer, help: "Optional TLS listener. Requires cert_path + key_path." },
                FieldSpec { key: "cert_path", label: "Cert path", kind: FieldKind::Path,  help: "PEM certificate for the TLS listener." },
                FieldSpec { key: "key_path",  label: "Key path",  kind: FieldKind::Path,  help: "PEM private key for the TLS listener." },
                FieldSpec { key: "external_url", label: "External broker URL", kind: FieldKind::Text, help: "Optional. mqtt:// URL to an external broker (Mosquitto, etc.). When set, the embedded broker is bypassed." },
            ],
        },
        Section {
            title: "Location",
            path: "location",
            help: "Used for solar event calculation (sunrise/sunset triggers).",
            default_open: true,
            fields: vec![
                FieldSpec { key: "latitude",  label: "Latitude",  kind: FieldKind::Float, help: "Decimal degrees. Positive = north." },
                FieldSpec { key: "longitude", label: "Longitude", kind: FieldKind::Float, help: "Decimal degrees. Positive = east." },
                FieldSpec { key: "timezone",  label: "Timezone",  kind: FieldKind::Text,  help: "IANA name, e.g. America/New_York." },
            ],
        },
        Section {
            title: "Web admin UI",
            path: "web_admin",
            help: "Static-file server for the bundled Leptos admin UI.",
            default_open: true,
            fields: vec![
                FieldSpec { key: "enabled",   label: "Enabled",      kind: FieldKind::Bool, help: "Serve the UI at /." },
                FieldSpec { key: "dist_path", label: "dist_path",    kind: FieldKind::Path, help: "Path to the trunk build output. Relative paths resolve against base_dir." },
            ],
        },
        Section {
            title: "Authentication",
            path: "auth",
            help: "JWT lifetimes, refresh-token retention, audit log retention, and secret persistence.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "token_expiry_hours",        label: "Access-token expiry (hours)", kind: FieldKind::Integer, help: "Default 24." },
                FieldSpec { key: "refresh_token_expiry_days", label: "Refresh-token expiry (days)", kind: FieldKind::Integer, help: "Default 30." },
                FieldSpec { key: "audit_retention_days",      label: "Audit retention (days)",      kind: FieldKind::Integer, help: "Default 365." },
                FieldSpec { key: "jwt_secret_file",           label: "JWT secret file",             kind: FieldKind::Path,    help: "Default <base_dir>/jwt_secret. File is auto-generated on first boot." },
                FieldSpec { key: "initial_admin_password_file", label: "Initial admin password file", kind: FieldKind::Path,  help: "First-boot only. Default <base_dir>/INITIAL_ADMIN_PASSWORD. Empty disables the file output." },
                FieldSpec { key: "whitelist", label: "IP whitelist (CIDR)", kind: FieldKind::StringList, help: "DEPRECATED — prefer admin_uds. Each line a CIDR; addresses bypass JWT and get Admin." },
            ],
        },
        Section {
            title: "Admin Unix domain socket",
            path: "auth.admin_uds",
            help: "Same-host admin tooling endpoint. Replaces the IP whitelist.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "enabled", label: "Enabled", kind: FieldKind::Bool,    help: "Listen on a UDS for admin operations from the same host." },
                FieldSpec { key: "path",    label: "Socket path", kind: FieldKind::Path, help: "Default /run/homecore/admin.sock." },
                FieldSpec { key: "group",   label: "POSIX group", kind: FieldKind::Text, help: "Group that owns the socket. Members can connect." },
                FieldSpec { key: "mode",    label: "Mode",        kind: FieldKind::Text, help: "Octal (e.g. \"0660\")." },
                FieldSpec { key: "allowed_uids", label: "Extra allowed UIDs", kind: FieldKind::StringList, help: "Comma-list each line; the process UID is always allowed." },
            ],
        },
        Section {
            title: "Storage",
            path: "storage",
            help: "Paths to the device-state DB and the time-series history DB.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "state_db_path",   label: "State DB path",   kind: FieldKind::Path, help: "redb file. Default <base_dir>/data/state.redb." },
                FieldSpec { key: "history_db_path", label: "History DB path", kind: FieldKind::Path, help: "SQLite file. Default <base_dir>/data/history.db." },
            ],
        },
        Section {
            title: "Battery",
            path: "battery",
            help: "Low-battery watcher thresholds + optional notify shortcut.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "threshold_pct",      label: "Low threshold (%)", kind: FieldKind::Float, help: "Percentage at or below which a device is flagged low." },
                FieldSpec { key: "recover_band_pct",   label: "Recovery band (%)", kind: FieldKind::Float, help: "Recovery requires battery > threshold + recover_band_pct (hysteresis)." },
                FieldSpec { key: "notify_channel",     label: "Notify channel",    kind: FieldKind::Text,  help: "Optional. hc-notify channel name to fire on low/recover edges. Empty = rule-driven only." },
                FieldSpec { key: "notify_on_recovered", label: "Notify on recover", kind: FieldKind::Bool,  help: "Send a notification when a device returns above threshold." },
            ],
        },
        Section {
            title: "InfluxDB v2 metrics export",
            path: "influx",
            help: "Push device-state changes to InfluxDB as line-protocol points.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "enabled",             label: "Enabled",           kind: FieldKind::Bool,       help: "Master switch." },
                FieldSpec { key: "url",                 label: "URL",               kind: FieldKind::Text,       help: "InfluxDB v2 base URL, e.g. http://10.0.10.20:8086" },
                FieldSpec { key: "token",               label: "API token",         kind: FieldKind::Text,       help: "Token with write permission to the bucket." },
                FieldSpec { key: "org",                 label: "Org",               kind: FieldKind::Text,       help: "Organization name." },
                FieldSpec { key: "bucket",              label: "Bucket",            kind: FieldKind::Text,       help: "Target bucket." },
                FieldSpec { key: "flush_interval_secs", label: "Flush interval (s)", kind: FieldKind::Integer,   help: "Max seconds to buffer before POSTing. Default 10." },
                FieldSpec { key: "batch_size",          label: "Batch size",        kind: FieldKind::Integer,    help: "Max points per write. Default 1000." },
                FieldSpec { key: "channel_capacity",    label: "Channel capacity",  kind: FieldKind::Integer,    help: "Bounded backlog before dropping oldest points. Default 10000." },
                FieldSpec { key: "include_devices",     label: "Include devices (globs)", kind: FieldKind::StringList, help: "One pattern per line. Empty = no devices export. Use [\"*\"] to include all." },
                FieldSpec { key: "exclude_attributes",  label: "Exclude attributes", kind: FieldKind::StringList, help: "Drop noisy attributes (last_seen, uptime, …)." },
                FieldSpec { key: "export_bools",        label: "Export bool fields", kind: FieldKind::Bool,      help: "Emit bool attrs as 0/1 numeric fields." },
            ],
        },
        Section {
            title: "Rules",
            path: "rules",
            help: "Directory containing automation rule files (RON).",
            default_open: false,
            fields: vec![
                FieldSpec { key: "dir", label: "Rules directory", kind: FieldKind::Path, help: "Default <base_dir>/rules. Hot-reloaded on change." },
            ],
        },
        Section {
            title: "Profiles",
            path: "profiles",
            help: "Directory containing topic-mapper ecosystem profiles.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "dir", label: "Profiles directory", kind: FieldKind::Path, help: "Default <base_dir>/config/profiles." },
            ],
        },
        Section {
            title: "Calendars",
            path: "calendars",
            help: "Directory of .ics calendar files for time-based rule triggers.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "dir",            label: "Calendars directory",     kind: FieldKind::Path,    help: "Default <base_dir>/config/calendars." },
                FieldSpec { key: "expansion_days", label: "RRULE expansion (days)",  kind: FieldKind::Integer, help: "How far forward to expand recurring events. Default 400." },
            ],
        },
        Section {
            title: "Startup",
            path: "startup",
            help: "Behavior during the first seconds after launch.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "initial_publish_delay_secs", label: "Initial publish delay (s)", kind: FieldKind::Integer, help: "Wait before publishing first state — gives plugins time to subscribe." },
            ],
        },
        Section {
            title: "Shutdown",
            path: "shutdown",
            help: "Behavior during graceful shutdown.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "graceful_timeout_secs", label: "Graceful timeout (s)", kind: FieldKind::Integer, help: "Max seconds to wait for plugins to flush before SIGKILL." },
            ],
        },
        Section {
            title: "Scheduler",
            path: "scheduler",
            help: "Solar / cron / time-of-day rule engine settings.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "catchup_window_minutes", label: "Catch-up window (min)", kind: FieldKind::Integer, help: "Fire missed time/solar triggers within N minutes of restart." },
            ],
        },
        Section {
            title: "Logging — global",
            path: "logging",
            help: "Top-level logging configuration. Per-target settings are below.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "level", label: "Level", kind: FieldKind::Text, help: "trace, debug, info, warn, error." },
            ],
        },
        Section {
            title: "Logging — stderr",
            path: "logging.stderr",
            help: "Console output (stderr). Visible via `docker logs`.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "enabled", label: "Enabled", kind: FieldKind::Bool, help: "" },
                FieldSpec { key: "format",  label: "Format",  kind: FieldKind::Text, help: "pretty | compact | json" },
                FieldSpec { key: "ansi",    label: "ANSI colors", kind: FieldKind::Bool, help: "" },
            ],
        },
        Section {
            title: "Logging — file",
            path: "logging.file",
            help: "Rolling log files on disk.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "enabled",         label: "Enabled",          kind: FieldKind::Bool,    help: "" },
                FieldSpec { key: "prefix",          label: "Filename prefix",  kind: FieldKind::Text,    help: "homecore -> homecore.YYYY-MM-DD.log" },
                FieldSpec { key: "rotation",        label: "Rotation",         kind: FieldKind::Text,    help: "minutely | hourly | daily | never" },
                FieldSpec { key: "prune_after_days", label: "Prune after (days)", kind: FieldKind::Integer, help: "Delete rotated files older than N days. 0 = never prune." },
                FieldSpec { key: "format",          label: "Format",           kind: FieldKind::Text,    help: "pretty | compact | json" },
            ],
        },
        Section {
            title: "Logging — rules file",
            path: "logging.rules_file",
            help: "Separate rolling file for rule engine evaluations.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "enabled",          label: "Enabled",          kind: FieldKind::Bool,    help: "" },
                FieldSpec { key: "prefix",           label: "Filename prefix",  kind: FieldKind::Text,    help: "" },
                FieldSpec { key: "rotation",         label: "Rotation",         kind: FieldKind::Text,    help: "" },
                FieldSpec { key: "prune_after_days", label: "Prune after (days)", kind: FieldKind::Integer, help: "" },
                FieldSpec { key: "format",           label: "Format",           kind: FieldKind::Text,    help: "" },
            ],
        },
        Section {
            title: "Logging — syslog",
            path: "logging.syslog",
            help: "Forward logs to syslogd (host-mode only; not applicable in Docker).",
            default_open: false,
            fields: vec![
                FieldSpec { key: "enabled",  label: "Enabled",  kind: FieldKind::Bool, help: "" },
                FieldSpec { key: "facility", label: "Facility", kind: FieldKind::Text, help: "user | local0..local7" },
                FieldSpec { key: "ident",    label: "Ident",    kind: FieldKind::Text, help: "Process name reported to syslog." },
            ],
        },
        Section {
            title: "Logging — live stream",
            path: "logging.stream",
            help: "Ring buffer + WebSocket endpoint backing GET /logs/stream.",
            default_open: false,
            fields: vec![
                FieldSpec { key: "enabled",   label: "Enabled",   kind: FieldKind::Bool,    help: "" },
                FieldSpec { key: "ring_size", label: "Ring size", kind: FieldKind::Integer, help: "How many recent log records to retain for late subscribers." },
            ],
        },
    ]
}

// ── Field helpers ──────────────────────────────────────────────────────────

fn lookup_value<'a>(parsed: &'a Value, dotted_path: &str, key: &str) -> Option<&'a Value> {
    let mut cur = parsed;
    for seg in dotted_path.split('.') {
        cur = cur.get(seg)?;
    }
    cur.get(key)
}

fn value_to_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn value_to_bool(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Bool(true)))
}

fn value_to_string_list(v: Option<&Value>) -> String {
    match v {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn parse_string_list(text: &str) -> Vec<String> {
    text.lines()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

// ── SectionCard component ──────────────────────────────────────────────────

#[component]
pub fn SectionCard(section: Section, parsed: ReadSignal<Value>) -> impl IntoView {
    let auth = use_auth();
    let open = RwSignal::new(section.default_open);
    let busy = RwSignal::new(false);
    let error: RwSignal<Option<String>> = RwSignal::new(None);
    let notice: RwSignal<Option<String>> = RwSignal::new(None);

    // One signal per field, seeded from the parsed config when this card
    // first paints. Re-seed on every parsed-config update so the form
    // stays in sync with whatever's on disk (e.g. after a save reloads).
    let field_count = section.fields.len();
    let states: Vec<RwSignal<String>> = (0..field_count)
        .map(|_| RwSignal::new(String::new()))
        .collect();
    // Snapshot of last-loaded-from-disk values, used to detect unsaved
    // edits. Updated whenever the parsed config refreshes (mount + save
    // round-trip).
    let baseline: Vec<RwSignal<String>> = (0..field_count)
        .map(|_| RwSignal::new(String::new()))
        .collect();
    let states_clone = states.clone();
    let baseline_clone = baseline.clone();
    let dotted_path = section.path.to_string();
    let fields_for_seed = section.fields.clone();
    Effect::new(move |_| {
        let p = parsed.get();
        for (i, spec) in fields_for_seed.iter().enumerate() {
            let v = lookup_value(&p, &dotted_path, spec.key);
            let s = match spec.kind {
                FieldKind::Text | FieldKind::Path => value_to_text(v),
                FieldKind::Integer | FieldKind::Float => value_to_text(v),
                FieldKind::Bool => value_to_bool(v).to_string(),
                FieldKind::StringList => value_to_string_list(v),
            };
            states_clone[i].set(s.clone());
            baseline_clone[i].set(s);
        }
    });

    // Dirty indicator — true when any signal diverges from its baseline.
    // Reactive so the header dot lights up the moment the operator types.
    let dirty_states = states.clone();
    let dirty_baseline = baseline.clone();
    let dirty = Signal::derive(move || {
        dirty_states
            .iter()
            .zip(dirty_baseline.iter())
            .any(|(s, b)| s.get() != b.get())
    });

    use std::sync::Arc;
    let states_arc: Arc<Vec<RwSignal<String>>> = Arc::new(states.clone());
    let fields_arc: Arc<Vec<FieldSpec>> = Arc::new(section.fields.clone());
    let path_arc: Arc<String> = Arc::new(section.path.to_string());

    let title = section.title;
    let help = section.help;
    let path = section.path;
    // Wrap in Arc so the Show-children closure can re-clone without
    // moving the outer binding (which would make it FnOnce — Show
    // wants Fn).
    let fields_for_view: Arc<Vec<FieldSpec>> = Arc::new(section.fields.clone());
    let states_for_view: Arc<Vec<RwSignal<String>>> = Arc::new(states.clone());

    view! {
        <div class="hc-section-card" style="margin-bottom:0.5rem">
            <div
                class="hc-section-card__header"
                on:click=move |_| open.update(|v| *v = !*v)
            >
                <i
                    class=move || if open.get() { "ph ph-caret-down" } else { "ph ph-caret-right" }
                    style="font-size:0.9rem; width:0.9rem"
                ></i>
                <h3 style="margin:0; font-size:0.95rem; flex:1; font-weight:600">{title}</h3>
                {move || dirty.get().then(|| view! {
                    <span
                        title="Unsaved edits"
                        style="display:inline-block; width:0.55rem; height:0.55rem; \
                               border-radius:50%; background:#f59e0b; flex-shrink:0"
                    ></span>
                })}
                <code style="font-size:0.75rem; color:var(--hc-text-muted)">{format!("[{}]", path)}</code>
            </div>

            <Show when=move || open.get()>
                <p style="margin:0.4rem 0 0.65rem; color:var(--hc-text-muted); font-size:0.85rem">{help}</p>

                <div class="hc-form-grid">
                    {
                        let fields = Arc::clone(&fields_for_view);
                        let states = Arc::clone(&states_for_view);
                        fields.iter().enumerate().map(|(i, spec)| {
                            let signal = states[i];
                            render_field(spec.clone(), signal)
                        }).collect_view()
                    }
                </div>

                {move || error.get().map(|e| view! {
                    <div class="msg-error" style="margin-top:0.5rem">{e}</div>
                })}
                {move || notice.get().map(|n| view! {
                    <div class="msg-success" style="margin-top:0.5rem">{n}</div>
                })}

                <div style="margin-top:0.6rem; display:flex; justify-content:flex-end">
                    <button
                        class="hc-btn hc-btn--sm hc-btn--primary"
                        disabled=move || busy.get()
                        on:click={
                            let states_arc = Arc::clone(&states_arc);
                            let fields_arc = Arc::clone(&fields_arc);
                            let path_arc = Arc::clone(&path_arc);
                            move |_| {
                                let token = match auth.token.get_untracked() {
                                    Some(t) => t,
                                    None => return,
                                };
                                let states_local = Arc::clone(&states_arc);
                                let fields_local = Arc::clone(&fields_arc);
                                let path_local = Arc::clone(&path_arc);
                                busy.set(true);
                                error.set(None);
                                notice.set(None);

                                let mut field_map = Map::new();
                                let mut parse_err: Option<String> = None;
                                for (i, spec) in fields_local.iter().enumerate() {
                                    let raw = states_local[i].get_untracked();
                                    let val = match spec.kind {
                                        FieldKind::Text | FieldKind::Path => {
                                            if raw.is_empty() {
                                                continue;
                                            }
                                            Value::String(raw)
                                        }
                                        FieldKind::Integer => match raw.trim().parse::<i64>() {
                                            Ok(n) => json!(n),
                                            Err(_) => {
                                                parse_err = Some(format!("{}: not a valid integer", spec.label));
                                                break;
                                            }
                                        },
                                        FieldKind::Float => match raw.trim().parse::<f64>() {
                                            Ok(n) => json!(n),
                                            Err(_) => {
                                                parse_err = Some(format!("{}: not a valid number", spec.label));
                                                break;
                                            }
                                        },
                                        FieldKind::Bool => Value::Bool(raw == "true"),
                                        FieldKind::StringList => Value::Array(
                                            parse_string_list(&raw)
                                                .into_iter()
                                                .map(Value::String)
                                                .collect(),
                                        ),
                                    };
                                    field_map.insert(spec.key.to_string(), val);
                                }

                                if let Some(e) = parse_err {
                                    error.set(Some(e));
                                    busy.set(false);
                                    return;
                                }

                                let path_str: String = (*path_local).clone();
                                let patch = json!({ &path_str: field_map });
                                spawn_local(async move {
                                    match put_system_config_patch(&token, &patch).await {
                                        Ok(_) => {
                                            notice.set(Some(format!(
                                                "[{}] saved. A restart may be required.",
                                                path_str
                                            )));
                                        }
                                        Err(e) => error.set(Some(e)),
                                    }
                                    busy.set(false);
                                });
                            }
                        }
                    >"Save"</button>
                </div>
            </Show>
        </div>
    }
}

fn render_field(spec: FieldSpec, signal: RwSignal<String>) -> impl IntoView {
    let label = spec.label;
    let help = spec.help;
    let mono = matches!(spec.kind, FieldKind::Path);

    // Constrain input width by kind so a `port` doesn't take 600px just
    // because its container can. Path/StringList stay full-width since
    // their values genuinely benefit from the room.
    let width_class = match spec.kind {
        FieldKind::Integer => "hc-form-input hc-form-input--xs",
        FieldKind::Float => "hc-form-input hc-form-input--sm",
        FieldKind::Text => "hc-form-input hc-form-input--md",
        FieldKind::Path => "hc-form-input hc-form-input--full",
        FieldKind::StringList => "hc-form-input hc-form-input--full",
        FieldKind::Bool => "hc-form-checkbox",
    };

    let input_view = match spec.kind {
        FieldKind::Bool => view! {
            <input
                type="checkbox"
                class=width_class
                prop:checked=move || signal.get() == "true"
                on:change=move |ev| {
                    let target: web_sys::HtmlInputElement = event_target(&ev);
                    signal.set(if target.checked() { "true".into() } else { "false".into() });
                }
            />
        }
        .into_any(),
        FieldKind::StringList => view! {
            <textarea
                class=width_class
                style="min-height:3.25rem; font-family:inherit"
                prop:value=move || signal.get()
                on:input=move |ev| signal.set(event_target_value(&ev))
            ></textarea>
        }
        .into_any(),
        _ => {
            let extra_style = if mono { "font-family:monospace" } else { "" };
            view! {
                <input
                    type="text"
                    class=width_class
                    style=extra_style
                    prop:value=move || signal.get()
                    on:input=move |ev| signal.set(event_target_value(&ev))
                />
            }
            .into_any()
        }
    };

    // display:contents lets the label + control participate directly in
    // the parent grid so labels in column 1 align across all rows
    // without each row being its own grid container.
    view! {
        <div style="display:contents">
            <label class="hc-form-label">{label}</label>
            <div class="hc-form-control">
                {input_view}
                {(!help.is_empty()).then(|| view! {
                    <span class="hc-form-help">{help}</span>
                })}
            </div>
        </div>
    }
}

fn event_target<T: wasm_bindgen::JsCast>(ev: &leptos::ev::Event) -> T {
    use wasm_bindgen::JsCast;
    ev.target().unwrap().dyn_into::<T>().unwrap()
}
