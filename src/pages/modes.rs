//! Modes page — boolean variables and system solar modes.

use crate::api::{
    create_mode as create_mode_request, delete_mode as delete_mode_request,
    delete_mode_definition as delete_mode_definition_request, fetch_devices, fetch_modes,
    put_mode_definition as put_mode_definition_request, set_device_state,
};
use crate::auth::use_auth;
use crate::models::*;
use crate::pages::shared::{
    card_size_canvas_class, common_card_prefs_map, json_str_set, load_common_card_prefs,
    load_pref_json, ls_set, set_to_json_array, CardSize, CardSizeSelect, CommonCardPrefs,
    LiveStatusBanner, MultiSelectDropdown, ResetFiltersButton, SearchField,
    SortDir, SortDirToggle, SortSelect,
};
use crate::ws::use_ws;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const MODES_PREFS_KEY: &str = "hc-leptos:modes:prefs";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Name,
    Type,
    Status,
    LastSeen,
}

fn sort_key_from_str(value: Option<&str>) -> SortKey {
    match value {
        Some("type") => SortKey::Type,
        Some("status") => SortKey::Status,
        Some("last_seen") => SortKey::LastSeen,
        _ => SortKey::Name,
    }
}

fn sort_key_to_str(value: SortKey) -> &'static str {
    match value {
        SortKey::Name => "name",
        SortKey::Type => "type",
        SortKey::Status => "status",
        SortKey::LastSeen => "last_seen",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModeFlavor {
    Solar,
    Manual,
    Criteria,
}

impl ModeFlavor {
    fn label(self) -> &'static str {
        match self {
            Self::Solar => "Solar",
            Self::Manual => "Manual",
            Self::Criteria => "Criteria",
        }
    }

    fn filter_value(self) -> &'static str {
        match self {
            Self::Solar => "Solar",
            Self::Manual => "Manual",
            Self::Criteria => "Criteria",
        }
    }

    fn card_class(self) -> &'static str {
        match self {
            Self::Solar => "mode-card--solar",
            Self::Manual => "mode-card--manual",
            Self::Criteria => "mode-card--criteria",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ModeRow {
    id: String,
    name: String,
    flavor: ModeFlavor,
    is_on: bool,
    available: bool,
    built_in: bool,
    last_seen: Option<chrono::DateTime<chrono::Utc>>,
    search_text: String,
}

struct ModePrefs {
    card_size: CardSize,
    search: String,
    status_filter: HashSet<String>,
    type_filter: HashSet<String>,
    sort_by: SortKey,
    sort_dir: SortDir,
}

impl Default for ModePrefs {
    fn default() -> Self {
        Self {
            card_size: CardSize::Medium,
            search: String::new(),
            status_filter: HashSet::new(),
            type_filter: HashSet::new(),
            sort_by: SortKey::Name,
            sort_dir: SortDir::Asc,
        }
    }
}

fn load_prefs() -> ModePrefs {
    let Some(v) = load_pref_json(MODES_PREFS_KEY) else {
        return ModePrefs::default();
    };
    let common = load_common_card_prefs(&v, sort_key_from_str);
    ModePrefs {
        card_size: common.card_size,
        search: common.search,
        status_filter: json_str_set(&v, "status_filter"),
        type_filter: json_str_set(&v, "type_filter"),
        sort_by: common.sort_by,
        sort_dir: common.sort_dir,
    }
}

fn save_prefs(
    card_size: CardSize,
    search: &str,
    status_filter: &HashSet<String>,
    type_filter: &HashSet<String>,
    sort_by: SortKey,
    sort_dir: SortDir,
) {
    let common = CommonCardPrefs {
        card_size,
        search: search.to_string(),
        sort_by,
        sort_dir,
    };
    let mut value = common_card_prefs_map(&common, sort_key_to_str);
    value.insert(
        "status_filter".to_string(),
        set_to_json_array(status_filter),
    );
    value.insert("type_filter".to_string(), set_to_json_array(type_filter));
    ls_set(
        MODES_PREFS_KEY,
        &serde_json::Value::Object(value).to_string(),
    );
}

fn is_builtin_mode(mode_id: &str) -> bool {
    matches!(mode_id, "mode_day" | "mode_night")
}

fn mode_flavor(record: &ModeRecord) -> ModeFlavor {
    if record.config.kind == ModeKind::Solar {
        ModeFlavor::Solar
    } else if record.definition.is_some() {
        ModeFlavor::Criteria
    } else {
        ModeFlavor::Manual
    }
}

fn live_mode_state(
    record: &ModeRecord,
    devices: &HashMap<String, DeviceState>,
) -> Option<DeviceState> {
    devices
        .get(&record.config.id)
        .cloned()
        .or_else(|| record.state.clone())
}

fn mode_row(record: &ModeRecord, devices: &HashMap<String, DeviceState>) -> ModeRow {
    let state = live_mode_state(record, devices);
    let flavor = mode_flavor(record);
    let type_label = flavor.label().to_string();
    let is_on = state.as_ref().map(mode_is_on).unwrap_or(false);
    let available = state
        .as_ref()
        .map(|device| device.available)
        .unwrap_or(true);
    let last_seen = state.as_ref().and_then(last_change_time).cloned();
    let built_in = is_builtin_mode(&record.config.id);

    let mut search_parts = vec![
        record.config.name.clone(),
        record.config.id.clone(),
        type_label.clone(),
    ];

    if built_in {
        search_parts.push("built in".to_string());
    }

    if let Some(definition) = &record.definition {
        search_parts.push(condition_summary(
            &definition.criteria.on_condition,
            &HashMap::new(),
            &HashMap::new(),
        ));
    }

    ModeRow {
        id: record.config.id.clone(),
        name: record.config.name.clone(),
        flavor,
        is_on,
        available,
        built_in,
        last_seen,
        search_text: search_parts.join(" ").to_lowercase(),
    }
}

fn cmp_mode_name(a: &ModeRow, b: &ModeRow) -> std::cmp::Ordering {
    sort_key_str(&a.name).cmp(&sort_key_str(&b.name))
}

fn cmp_mode_rows(a: &ModeRow, b: &ModeRow, sort_by: SortKey) -> std::cmp::Ordering {
    match sort_by {
        SortKey::Name => cmp_mode_name(a, b),
        SortKey::Type => sort_key_str(a.flavor.label())
            .cmp(&sort_key_str(b.flavor.label()))
            .then_with(|| cmp_mode_name(a, b)),
        SortKey::Status => match (a.is_on, b.is_on) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => cmp_mode_name(a, b),
        },
        SortKey::LastSeen => a
            .last_seen
            .cmp(&b.last_seen)
            .then_with(|| cmp_mode_name(a, b)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CriteriaLogic {
    All,
    Any,
}

impl CriteriaLogic {
    fn as_condition_type(self) -> &'static str {
        match self {
            Self::All => "and",
            Self::Any => "or",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DraftModeKind {
    Manual,
    Criteria,
}

#[derive(Debug, Clone, PartialEq)]
enum CriterionDraft {
    DeviceState {
        device_id: String,
        attribute: String,
        op: String,
        value_text: String,
    },
    ModeIs {
        mode_id: String,
        on: bool,
    },
}

impl CriterionDraft {
    fn blank_device_state() -> Self {
        Self::DeviceState {
            device_id: String::new(),
            attribute: String::new(),
            op: "eq".to_string(),
            value_text: String::new(),
        }
    }

    fn blank_mode_is() -> Self {
        Self::ModeIs {
            mode_id: String::new(),
            on: true,
        }
    }

    fn kind_value(&self) -> &'static str {
        match self {
            Self::DeviceState { .. } => "device_state",
            Self::ModeIs { .. } => "mode_is",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct CriteriaBuilderState {
    on_logic: CriteriaLogic,
    on_clauses: Vec<CriterionDraft>,
    off_behavior: CriteriaOffBehavior,
    off_logic: CriteriaLogic,
    off_clauses: Vec<CriterionDraft>,
    reevaluate_every_n_minutes: u32,
}

impl Default for CriteriaBuilderState {
    fn default() -> Self {
        Self {
            on_logic: CriteriaLogic::All,
            on_clauses: vec![CriterionDraft::blank_device_state()],
            off_behavior: CriteriaOffBehavior::Inverse,
            off_logic: CriteriaLogic::All,
            off_clauses: vec![CriterionDraft::blank_device_state()],
            reevaluate_every_n_minutes: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CriteriaSection {
    On,
    Off,
}

fn parse_mode_id_input(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut slug = String::with_capacity(trimmed.len());
    let mut last_was_sep = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            slug.push('_');
            last_was_sep = true;
        }
    }

    let slug = slug.trim_matches('_');
    if slug.is_empty() {
        String::new()
    } else if slug.starts_with("mode_") {
        slug.to_string()
    } else {
        format!("mode_{slug}")
    }
}

fn parse_value_text(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if trimmed.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    if let Ok(int_value) = trimmed.parse::<i64>() {
        return json!(int_value);
    }
    if let Ok(float_value) = trimmed.parse::<f64>() {
        if trimmed.contains('.') {
            return json!(float_value);
        }
    }
    if (trimmed.starts_with('{') || trimmed.starts_with('[') || trimmed.starts_with('"'))
        && serde_json::from_str::<Value>(trimmed).is_ok()
    {
        return serde_json::from_str(trimmed).unwrap_or_else(|_| json!(trimmed));
    }
    json!(trimmed)
}

fn value_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "null".to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        _ => compact_json(value),
    }
}

fn parse_clause_from_value(value: &Value) -> Result<CriterionDraft, String> {
    let Some(obj) = value.as_object() else {
        return Err("Criteria clause must be an object.".to_string());
    };

    match obj.get("type").and_then(Value::as_str) {
        Some("device_state") => Ok(CriterionDraft::DeviceState {
            device_id: obj
                .get("device_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            attribute: obj
                .get("attribute")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            op: obj
                .get("op")
                .and_then(Value::as_str)
                .unwrap_or("eq")
                .to_string(),
            value_text: obj
                .get("value")
                .map(value_to_text)
                .unwrap_or_default(),
        }),
        Some("mode_is") => Ok(CriterionDraft::ModeIs {
            mode_id: obj
                .get("mode_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            on: obj.get("on").and_then(Value::as_bool).unwrap_or(true),
        }),
        Some(other) => Err(format!("Unsupported criterion type '{other}' for inline editing.")),
        None => Err("Criterion type is required.".to_string()),
    }
}

fn parse_group_from_value(value: &Value) -> Result<(CriteriaLogic, Vec<CriterionDraft>), String> {
    let Some(obj) = value.as_object() else {
        return Err("Criteria group must be an object.".to_string());
    };

    match obj.get("type").and_then(Value::as_str) {
        Some("and") | Some("or") => {
            let logic = if obj.get("type").and_then(Value::as_str) == Some("or") {
                CriteriaLogic::Any
            } else {
                CriteriaLogic::All
            };
            let Some(items) = obj.get("conditions").and_then(Value::as_array) else {
                return Err("Grouped criteria must contain a conditions array.".to_string());
            };
            if items.is_empty() {
                return Err("Criteria group cannot be empty.".to_string());
            }
            let clauses = items
                .iter()
                .map(parse_clause_from_value)
                .collect::<Result<Vec<_>, _>>()?;
            Ok((logic, clauses))
        }
        Some("device_state") | Some("mode_is") => {
            Ok((CriteriaLogic::All, vec![parse_clause_from_value(value)?]))
        }
        Some(other) => Err(format!(
            "Unsupported grouped criteria type '{other}' for inline editing."
        )),
        None => Err("Criteria type is required.".to_string()),
    }
}

fn builder_from_criteria(criteria: &CriteriaModeConfig) -> Result<CriteriaBuilderState, String> {
    let (on_logic, on_clauses) = parse_group_from_value(&criteria.on_condition)?;
    let (off_logic, off_clauses) = match criteria.off_behavior {
        CriteriaOffBehavior::Inverse => (CriteriaLogic::All, vec![CriterionDraft::blank_device_state()]),
        CriteriaOffBehavior::Explicit => {
            let Some(off_condition) = criteria.off_condition.as_ref() else {
                return Err("Explicit off criteria is missing.".to_string());
            };
            parse_group_from_value(off_condition)?
        }
    };

    Ok(CriteriaBuilderState {
        on_logic,
        on_clauses,
        off_behavior: criteria.off_behavior,
        off_logic,
        off_clauses,
        reevaluate_every_n_minutes: criteria.reevaluate_every_n_minutes.max(1),
    })
}

fn build_clause_value(clause: &CriterionDraft) -> Result<Value, String> {
    match clause {
        CriterionDraft::DeviceState {
            device_id,
            attribute,
            op,
            value_text,
        } => {
            if device_id.trim().is_empty() {
                return Err("Select a device for each device-state criterion.".to_string());
            }
            if attribute.trim().is_empty() {
                return Err("Enter an attribute for each device-state criterion.".to_string());
            }
            if value_text.trim().is_empty() {
                return Err("Enter a value for each device-state criterion.".to_string());
            }
            Ok(json!({
                "type": "device_state",
                "device_id": device_id.trim(),
                "attribute": attribute.trim(),
                "op": op,
                "value": parse_value_text(value_text),
            }))
        }
        CriterionDraft::ModeIs { mode_id, on } => {
            if mode_id.trim().is_empty() {
                return Err("Select a mode for each mode criterion.".to_string());
            }
            Ok(json!({
                "type": "mode_is",
                "mode_id": mode_id.trim(),
                "on": on,
            }))
        }
    }
}

fn build_condition_value(
    logic: CriteriaLogic,
    clauses: &[CriterionDraft],
) -> Result<Value, String> {
    if clauses.is_empty() {
        return Err("Add at least one criterion.".to_string());
    }

    let built = clauses
        .iter()
        .map(build_clause_value)
        .collect::<Result<Vec<_>, _>>()?;

    if built.len() == 1 {
        Ok(built.into_iter().next().unwrap_or(Value::Null))
    } else {
        Ok(json!({
            "type": logic.as_condition_type(),
            "conditions": built,
        }))
    }
}

fn build_criteria_config(builder: &CriteriaBuilderState) -> Result<CriteriaModeConfig, String> {
    let on_condition = build_condition_value(builder.on_logic, &builder.on_clauses)?;
    let off_condition = if builder.off_behavior == CriteriaOffBehavior::Explicit {
        Some(build_condition_value(
            builder.off_logic,
            &builder.off_clauses,
        )?)
    } else {
        None
    };

    Ok(CriteriaModeConfig {
        on_condition,
        off_behavior: builder.off_behavior,
        off_condition,
        reevaluate_every_n_minutes: builder.reevaluate_every_n_minutes.max(1),
    })
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<invalid>".to_string())
}

fn compare_op_label(value: &str) -> &'static str {
    match value {
        "eq" => "=",
        "ne" => "!=",
        "gt" => ">",
        "gte" => ">=",
        "lt" => "<",
        "lte" => "<=",
        _ => "?",
    }
}

fn short_json_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => compact_json(value),
    }
}

fn condition_summary(
    value: &Value,
    device_labels: &HashMap<String, String>,
    mode_labels: &HashMap<String, String>,
) -> String {
    let Some(obj) = value.as_object() else {
        return compact_json(value);
    };

    match obj.get("type").and_then(Value::as_str) {
        Some("device_state") => {
            let device_id = obj
                .get("device_id")
                .and_then(Value::as_str)
                .unwrap_or("device");
            let device_label = device_labels
                .get(device_id)
                .cloned()
                .unwrap_or_else(|| device_id.to_string());
            let attribute = obj
                .get("attribute")
                .and_then(Value::as_str)
                .unwrap_or("attribute");
            let op = obj.get("op").and_then(Value::as_str).unwrap_or("eq");
            let rendered_value = obj
                .get("value")
                .map(short_json_value)
                .unwrap_or_else(|| "?".to_string());
            format!(
                "{device_label} {attribute} {} {rendered_value}",
                compare_op_label(op)
            )
        }
        Some("mode_is") => {
            let mode_id = obj.get("mode_id").and_then(Value::as_str).unwrap_or("mode");
            let mode_label = mode_labels
                .get(mode_id)
                .cloned()
                .unwrap_or_else(|| mode_id.to_string());
            let state = if obj.get("on").and_then(Value::as_bool).unwrap_or(false) {
                "on"
            } else {
                "off"
            };
            format!("{mode_label} is {state}")
        }
        Some("and") | Some("or") | Some("xor") => {
            let joiner = match obj.get("type").and_then(Value::as_str).unwrap_or("and") {
                "and" => " and ",
                "or" => " or ",
                _ => " xor ",
            };
            obj.get("conditions")
                .and_then(Value::as_array)
                .map(|conditions| {
                    let summaries = conditions
                        .iter()
                        .map(|condition| condition_summary(condition, device_labels, mode_labels))
                        .collect::<Vec<_>>();
                    if summaries.is_empty() {
                        "No criteria".to_string()
                    } else {
                        summaries.join(joiner)
                    }
                })
                .unwrap_or_else(|| compact_json(value))
        }
        Some("not") => obj
            .get("condition")
            .map(|inner| {
                format!(
                    "not ({})",
                    condition_summary(inner, device_labels, mode_labels)
                )
            })
            .unwrap_or_else(|| compact_json(value)),
        Some("time_window") => {
            let start = obj.get("start").and_then(Value::as_str).unwrap_or("?");
            let end = obj.get("end").and_then(Value::as_str).unwrap_or("?");
            format!("time between {start} and {end}")
        }
        Some("hub_variable") => {
            let name = obj
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("variable");
            let op = obj.get("op").and_then(Value::as_str).unwrap_or("eq");
            let rendered_value = obj
                .get("value")
                .map(short_json_value)
                .unwrap_or_else(|| "?".to_string());
            format!("{name} {} {rendered_value}", compare_op_label(op))
        }
        _ => compact_json(value),
    }
}

fn criteria_generated_summary(definition: &ModeDefinition) -> String {
    match definition.generated_rule_ids.len() {
        0 => "No generated rules".to_string(),
        1 => "1 generated rule".to_string(),
        count => format!("{count} generated rules"),
    }
}

fn clause_list(builder: &CriteriaBuilderState, section: CriteriaSection) -> Vec<CriterionDraft> {
    match section {
        CriteriaSection::On => builder.on_clauses.clone(),
        CriteriaSection::Off => builder.off_clauses.clone(),
    }
}

fn clause_logic(builder: &CriteriaBuilderState, section: CriteriaSection) -> CriteriaLogic {
    match section {
        CriteriaSection::On => builder.on_logic,
        CriteriaSection::Off => builder.off_logic,
    }
}

fn set_clause_logic(
    builder: &mut CriteriaBuilderState,
    section: CriteriaSection,
    logic: CriteriaLogic,
) {
    match section {
        CriteriaSection::On => builder.on_logic = logic,
        CriteriaSection::Off => builder.off_logic = logic,
    }
}

fn section_clauses_mut(
    builder: &mut CriteriaBuilderState,
    section: CriteriaSection,
) -> &mut Vec<CriterionDraft> {
    match section {
        CriteriaSection::On => &mut builder.on_clauses,
        CriteriaSection::Off => &mut builder.off_clauses,
    }
}

#[component]
fn CriteriaClauseEditor(
    builder: RwSignal<CriteriaBuilderState>,
    section: CriteriaSection,
    #[prop(into)] device_options: Signal<Vec<(String, String)>>,
    #[prop(into)] mode_options: Signal<Vec<(String, String)>>,
) -> impl IntoView {
    let heading = match section {
        CriteriaSection::On => "Turn On When",
        CriteriaSection::Off => "Turn Off When",
    };

    view! {
        <div class="mode-criteria-block">
            <div class="mode-criteria-block-header">
                <strong>{heading}</strong>
                <div class="mode-criteria-inline">
                    <span class="cell-subtle">"Match"</span>
                    <select
                        on:change=move |ev| {
                            let logic = match event_target_value(&ev).as_str() {
                                "any" => CriteriaLogic::Any,
                                _ => CriteriaLogic::All,
                            };
                            builder.update(|draft| set_clause_logic(draft, section, logic));
                        }
                    >
                        <option
                            value="all"
                            selected=move || clause_logic(&builder.get(), section) == CriteriaLogic::All
                        >
                            "All"
                        </option>
                        <option
                            value="any"
                            selected=move || clause_logic(&builder.get(), section) == CriteriaLogic::Any
                        >
                            "Any"
                        </option>
                    </select>
                </div>
            </div>

            <div class="mode-criteria-list">
                {move || {
                    clause_list(&builder.get(), section)
                        .into_iter()
                        .enumerate()
                        .map(|(index, clause)| {
                            let kind_value = clause.kind_value().to_string();
                            view! {
                                <div class="mode-criteria-row">
                                    <select
                                        on:change=move |ev| {
                                            let next_value = event_target_value(&ev);
                                            builder.update(|draft| {
                                                if let Some(current) = section_clauses_mut(draft, section).get_mut(index) {
                                                    *current = if next_value == "mode_is" {
                                                        CriterionDraft::blank_mode_is()
                                                    } else {
                                                        CriterionDraft::blank_device_state()
                                                    };
                                                }
                                            });
                                        }
                                    >
                                        <option value="device_state" selected=kind_value == "device_state">
                                            "Device State"
                                        </option>
                                        <option value="mode_is" selected=kind_value == "mode_is">
                                            "Mode State"
                                        </option>
                                    </select>

                                    {match clause {
                                        CriterionDraft::DeviceState { device_id, attribute, op, value_text } => {
                                            view! {
                                                <>
                                                    <select
                                                        on:change=move |ev| {
                                                            let value = event_target_value(&ev);
                                                            builder.update(|draft| {
                                                                if let Some(CriterionDraft::DeviceState { device_id, .. }) =
                                                                    section_clauses_mut(draft, section).get_mut(index)
                                                                {
                                                                    *device_id = value.clone();
                                                                }
                                                            });
                                                        }
                                                    >
                                                        <option value="" selected=device_id.is_empty()>"Select device"</option>
                                                        {move || {
                                                            device_options
                                                                .get()
                                                                .into_iter()
                                                                .map(|(value, label)| {
                                                                    let is_selected = value == device_id;
                                                                    view! {
                                                                        <option value=value.clone() selected=is_selected>{label}</option>
                                                                    }
                                                                })
                                                                .collect_view()
                                                        }}
                                                    </select>

                                                    <input
                                                        type="text"
                                                        prop:value=attribute.clone()
                                                        placeholder="attribute"
                                                        on:input=move |ev| {
                                                            let value = event_target_value(&ev);
                                                            builder.update(|draft| {
                                                                if let Some(CriterionDraft::DeviceState { attribute, .. }) =
                                                                    section_clauses_mut(draft, section).get_mut(index)
                                                                {
                                                                    *attribute = value.clone();
                                                                }
                                                            });
                                                        }
                                                    />

                                                    <select
                                                        on:change=move |ev| {
                                                            let value = event_target_value(&ev);
                                                            builder.update(|draft| {
                                                                if let Some(CriterionDraft::DeviceState { op, .. }) =
                                                                    section_clauses_mut(draft, section).get_mut(index)
                                                                {
                                                                    *op = value.clone();
                                                                }
                                                            });
                                                        }
                                                    >
                                                        <option value="eq" selected=op == "eq">"="</option>
                                                        <option value="ne" selected=op == "ne">"!="</option>
                                                        <option value="gt" selected=op == "gt">">"</option>
                                                        <option value="gte" selected=op == "gte">">="</option>
                                                        <option value="lt" selected=op == "lt">"<"</option>
                                                        <option value="lte" selected=op == "lte">"<="</option>
                                                    </select>

                                                    <input
                                                        type="text"
                                                        prop:value=value_text.clone()
                                                        placeholder="value"
                                                        on:input=move |ev| {
                                                            let value = event_target_value(&ev);
                                                            builder.update(|draft| {
                                                                if let Some(CriterionDraft::DeviceState { value_text, .. }) =
                                                                    section_clauses_mut(draft, section).get_mut(index)
                                                                {
                                                                    *value_text = value.clone();
                                                                }
                                                            });
                                                        }
                                                    />
                                                </>
                                            }.into_any()
                                        }
                                        CriterionDraft::ModeIs { mode_id, on } => {
                                            view! {
                                                <>
                                                    <select
                                                        on:change=move |ev| {
                                                            let value = event_target_value(&ev);
                                                            builder.update(|draft| {
                                                                if let Some(CriterionDraft::ModeIs { mode_id, .. }) =
                                                                    section_clauses_mut(draft, section).get_mut(index)
                                                                {
                                                                    *mode_id = value.clone();
                                                                }
                                                            });
                                                        }
                                                    >
                                                        <option value="" selected=mode_id.is_empty()>"Select mode"</option>
                                                        {move || {
                                                            mode_options
                                                                .get()
                                                                .into_iter()
                                                                .map(|(value, label)| {
                                                                    let is_selected = value == mode_id;
                                                                    view! {
                                                                        <option value=value.clone() selected=is_selected>{label}</option>
                                                                    }
                                                                })
                                                                .collect_view()
                                                        }}
                                                    </select>

                                                    <select
                                                        on:change=move |ev| {
                                                            let next_on = event_target_value(&ev) != "off";
                                                            builder.update(|draft| {
                                                                if let Some(CriterionDraft::ModeIs { on, .. }) =
                                                                    section_clauses_mut(draft, section).get_mut(index)
                                                                {
                                                                    *on = next_on;
                                                                }
                                                            });
                                                        }
                                                    >
                                                        <option value="on" selected=on>"On"</option>
                                                        <option value="off" selected=!on>"Off"</option>
                                                    </select>
                                                </>
                                            }.into_any()
                                        }
                                    }}

                                    <button
                                        class="card-ctrl-btn card-ctrl-btn--danger card-ctrl-btn--sm"
                                        on:click=move |_| {
                                            builder.update(|draft| {
                                                let clauses = section_clauses_mut(draft, section);
                                                if clauses.len() > 1 {
                                                    clauses.remove(index);
                                                } else {
                                                    clauses[0] = CriterionDraft::blank_device_state();
                                                }
                                            });
                                        }
                                    >
                                        <span class="material-icons" style="font-size:16px">"delete"</span>
                                        "Remove"
                                    </button>
                                </div>
                            }
                        })
                        .collect_view()
                }}
            </div>

            <div class="card-controls">
                <button
                    class="card-ctrl-btn card-ctrl-btn--secondary card-ctrl-btn--sm"
                    on:click=move |_| {
                        builder.update(|draft| {
                            section_clauses_mut(draft, section).push(CriterionDraft::blank_device_state());
                        });
                    }
                >
                    <span class="material-icons" style="font-size:16px">"add"</span>
                    "Device criterion"
                </button>
                <button
                    class="card-ctrl-btn card-ctrl-btn--secondary card-ctrl-btn--sm"
                    on:click=move |_| {
                        builder.update(|draft| {
                            section_clauses_mut(draft, section).push(CriterionDraft::blank_mode_is());
                        });
                    }
                >
                    <span class="material-icons" style="font-size:16px">"add_circle"</span>
                    "Mode criterion"
                </button>
            </div>
        </div>
    }
}

#[component]
fn CriteriaBuilder(
    builder: RwSignal<CriteriaBuilderState>,
    #[prop(into)] device_options: Signal<Vec<(String, String)>>,
    #[prop(into)] mode_options: Signal<Vec<(String, String)>>,
) -> impl IntoView {
    view! {
        <div class="mode-criteria-editor">
            <div class="mode-criteria-topline">
                <div class="mode-create-field">
                    <span>"Reevaluate Every"</span>
                    <input
                        type="number"
                        min="1"
                        prop:value=move || builder.get().reevaluate_every_n_minutes.to_string()
                        on:input=move |ev| {
                            let next = event_target_value(&ev)
                                .parse::<u32>()
                                .ok()
                                .filter(|value| *value > 0)
                                .unwrap_or(1);
                            builder.update(|draft| {
                                draft.reevaluate_every_n_minutes = next;
                            });
                        }
                    />
                </div>
                <p class="mode-create-help">
                    "Criteria-driven modes compile to managed HomeCore rules. Start narrow: device state and other modes."
                </p>
            </div>

            <CriteriaClauseEditor
                builder
                section=CriteriaSection::On
                device_options=device_options
                mode_options=mode_options
            />

            <div class="mode-criteria-block">
                <div class="mode-criteria-block-header">
                    <strong>"Turn Off Behavior"</strong>
                    <div class="mode-criteria-inline">
                        <span class="cell-subtle">"Use"</span>
                        <select
                            on:change=move |ev| {
                                let next = match event_target_value(&ev).as_str() {
                                    "explicit" => CriteriaOffBehavior::Explicit,
                                    _ => CriteriaOffBehavior::Inverse,
                                };
                                builder.update(|draft| draft.off_behavior = next);
                            }
                        >
                            <option
                                value="inverse"
                                selected=move || builder.get().off_behavior == CriteriaOffBehavior::Inverse
                            >
                                "Inverse of on criteria"
                            </option>
                            <option
                                value="explicit"
                                selected=move || builder.get().off_behavior == CriteriaOffBehavior::Explicit
                            >
                                "Explicit off criteria"
                            </option>
                        </select>
                    </div>
                </div>

                {move || {
                    if builder.get().off_behavior == CriteriaOffBehavior::Explicit {
                        view! {
                            <CriteriaClauseEditor
                                builder
                                section=CriteriaSection::Off
                                device_options=device_options
                                mode_options=mode_options
                            />
                        }.into_any()
                    } else {
                        view! {
                            <p class="mode-create-help">
                                "The mode turns off when the on criteria are no longer true."
                            </p>
                        }.into_any()
                    }
                }}
            </div>
        </div>
    }
}

#[component]
fn ModeCard(
    mode_id: String,
    modes: RwSignal<Vec<ModeRecord>>,
    #[prop(into)] device_options: Signal<Vec<(String, String)>>,
    #[prop(into)] mode_options: Signal<Vec<(String, String)>>,
    on_refresh: Callback<()>,
) -> impl IntoView {
    let auth = use_auth();
    let ws = use_ws();
    let busy = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);
    let show_criteria_editor = RwSignal::new(false);
    let show_solar_editor = RwSignal::new(false);
    let builder = RwSignal::new(CriteriaBuilderState::default());
    let solar_on_offset = RwSignal::new(0i32);
    let solar_off_offset = RwSignal::new(0i32);

    let mode_id_for_memo = mode_id.clone();
    let record: Memo<Option<ModeRecord>> = Memo::new(move |_| {
        modes
            .get()
            .into_iter()
            .find(|record| record.config.id == mode_id_for_memo)
    });

    let card_data: Memo<Option<(ModeRecord, Option<DeviceState>)>> = Memo::new(move |_| {
        record.get().map(|record| {
            let live = live_mode_state(&record, &ws.devices.get());
            (record, live)
        })
    });

    view! {
        <div class="card-slot" data-mode-id=mode_id.clone()>
            {move || {
                let Some((record, live_state)) = card_data.get() else {
                    return view! { <div class="device-card device-card--ghost"></div> }.into_any();
                };

                let flavor = mode_flavor(&record);
                let built_in = is_builtin_mode(&record.config.id);
                let state_on = live_state.as_ref().map(mode_is_on).unwrap_or(false);
                let available = live_state.as_ref().map(|state| state.available).unwrap_or(true);
                let status_badge = if !available {
                    "card-state-badge card-state-badge--tone-tone-offline"
                } else if state_on {
                    "card-state-badge card-state-badge--tone-tone-good"
                } else {
                    "card-state-badge card-state-badge--tone-tone-idle"
                };
                let state_label = if !available {
                    "Offline"
                } else if state_on {
                    "On"
                } else {
                    "Off"
                };

                let mut device_labels = HashMap::new();
                for (id, label) in device_options.get() {
                    device_labels.insert(id, label);
                }
                let mut mode_labels = HashMap::new();
                for (id, label) in mode_options.get() {
                    mode_labels.insert(id, label);
                }

                let on_summary = record
                    .definition
                    .as_ref()
                    .map(|definition| {
                        condition_summary(
                            &definition.criteria.on_condition,
                            &device_labels,
                            &mode_labels,
                        )
                    })
                    .unwrap_or_default();

                let off_summary = record.definition.as_ref().map(|definition| {
                    if definition.criteria.off_behavior == CriteriaOffBehavior::Explicit {
                        definition
                            .criteria
                            .off_condition
                            .as_ref()
                            .map(|condition| condition_summary(condition, &device_labels, &mode_labels))
                            .unwrap_or_else(|| "Explicit off criteria missing".to_string())
                    } else {
                        format!("Not ({on_summary})")
                    }
                });

                let open_criteria_editor = Callback::new({
                    let existing_definition = record.definition.clone();
                    move |_| {
                        error.set(None);
                        notice.set(None);
                        if let Some(definition) = &existing_definition {
                            match builder_from_criteria(&definition.criteria) {
                                Ok(existing_builder) => builder.set(existing_builder),
                                Err(err) => {
                                    error.set(Some(err));
                                    return;
                                }
                            }
                        } else {
                            builder.set(CriteriaBuilderState::default());
                        }
                        show_criteria_editor.update(|open| *open = !*open);
                    }
                });

                let save_criteria = Callback::new({
                    let target_mode_id = record.config.id.clone();
                    move |_| {
                        let target_mode_id = target_mode_id.clone();
                        let token = auth.token_str().unwrap_or_default();
                        let draft = builder.get();
                        let criteria = match build_criteria_config(&draft) {
                            Ok(criteria) => criteria,
                            Err(err) => {
                                error.set(Some(err));
                                return;
                            }
                        };
                        busy.set(true);
                        error.set(None);
                        notice.set(None);
                        spawn_local(async move {
                            match put_mode_definition_request(&token, &target_mode_id, &criteria).await {
                                Ok(_) => {
                                    show_criteria_editor.set(false);
                                    notice.set(Some("Criteria saved.".to_string()));
                                    on_refresh.run(());
                                }
                                Err(err) => error.set(Some(err)),
                            }
                            busy.set(false);
                        });
                    }
                });

                let toggle_solar_editor = Callback::new({
                    let record = record.clone();
                    move |_| {
                        solar_on_offset.set(record.config.on_offset_minutes);
                        solar_off_offset.set(record.config.off_offset_minutes);
                        error.set(None);
                        notice.set(None);
                        show_solar_editor.update(|open| *open = !*open);
                    }
                });

                let save_solar_offsets = Callback::new({
                    let target_mode_id = record.config.id.clone();
                    move |_| {
                        let target_mode_id = target_mode_id.clone();
                        let token = auth.token_str().unwrap_or_default();
                        let on_offset_minutes = solar_on_offset.get();
                        let off_offset_minutes = solar_off_offset.get();
                        busy.set(true);
                        error.set(None);
                        notice.set(None);
                        spawn_local(async move {
                            match set_device_state(
                                &token,
                                &target_mode_id,
                                &json!({
                                    "on_offset_minutes": on_offset_minutes,
                                    "off_offset_minutes": off_offset_minutes,
                                }),
                            )
                            .await
                            {
                                Ok(()) => {
                                    show_solar_editor.set(false);
                                    notice.set(Some("Solar offsets updated.".to_string()));
                                    on_refresh.run(());
                                }
                                Err(err) => error.set(Some(err)),
                            }
                            busy.set(false);
                        });
                    }
                });

                let delete_mode = {
                    let target_mode_id = record.config.id.clone();
                    let target_name = record.config.name.clone();
                    move |_| {
                        let target_mode_id = target_mode_id.clone();
                        let target_name = target_name.clone();
                        let token = auth.token_str().unwrap_or_default();
                        busy.set(true);
                        error.set(None);
                        notice.set(None);
                        spawn_local(async move {
                            match delete_mode_request(&token, &target_mode_id).await {
                                Ok(()) => {
                                    notice.set(Some(format!("Deleted {target_name}.")));
                                    on_refresh.run(());
                                }
                                Err(err) => error.set(Some(err)),
                            }
                            busy.set(false);
                        });
                    }
                };

                let remove_criteria = {
                    let target_mode_id = record.config.id.clone();
                    move |_| {
                        let target_mode_id = target_mode_id.clone();
                        let token = auth.token_str().unwrap_or_default();
                        busy.set(true);
                        error.set(None);
                        notice.set(None);
                        spawn_local(async move {
                            match delete_mode_definition_request(&token, &target_mode_id).await {
                                Ok(()) => {
                                    notice.set(Some("Criteria removed. Mode is manual again.".to_string()));
                                    on_refresh.run(());
                                }
                                Err(err) => error.set(Some(err)),
                            }
                            busy.set(false);
                        });
                    }
                };

                let local_mode_options = Signal::derive({
                    let record_id = record.config.id.clone();
                    move || {
                        mode_options
                            .get()
                            .into_iter()
                            .filter(|(id, _)| id != &record_id)
                            .collect::<Vec<_>>()
                    }
                });

                view! {
                    <div
                        class=format!("device-card {}", flavor.card_class())
                        class:device-card--offline=!available
                    >
                        <div class="card-header">
                            <span class=format!(
                                "card-status-icon status-badge-sm {}",
                                if state_on { "tone-good" } else { "tone-idle" }
                            )>
                                <span class="material-icons" style="font-size:18px">
                                    {if state_on { "toggle_on" } else { "toggle_off" }}
                                </span>
                            </span>
                            <div class="card-header-text">
                                <p class="card-name" title=record.config.name.clone()>{record.config.name.clone()}</p>
                                <p class="card-meta">
                                    {record.config.id.clone()}
                                    <span class="card-meta-sep">" · "</span>
                                    {flavor.label()}
                                    {built_in.then(|| view! {
                                        <>
                                            <span class="card-meta-sep">" · "</span>
                                            "Built-in"
                                        </>
                                    })}
                                </p>
                            </div>
                            <span class=status_badge>{state_label}</span>
                        </div>

                        <div class="card-body">
                            <div class="scene-card-chip-row">
                                <span class="card-state-badge card-state-badge--tone-tone-idle">
                                    {flavor.label()}
                                </span>
                                {record.definition.as_ref().map(|definition| view! {
                                    <span class="card-state-badge card-state-badge--tone-tone-media">
                                        {criteria_generated_summary(definition)}
                                    </span>
                                })}
                                {built_in.then(|| view! {
                                    <span class="card-state-badge card-state-badge--tone-tone-idle">
                                        "Core"
                                    </span>
                                })}
                            </div>

                            {match flavor {
                                ModeFlavor::Solar => view! {
                                    <div class="mode-solar-grid">
                                        <div class="mode-stat">
                                            <span class="mode-stat-label">"Turns On"</span>
                                            <strong>{solar_event_label(record.config.on_event.as_deref())}</strong>
                                            <span class="cell-subtle">
                                                {format!("Offset {} min", record.config.on_offset_minutes)}
                                            </span>
                                        </div>
                                        <div class="mode-stat">
                                            <span class="mode-stat-label">"Turns Off"</span>
                                            <strong>{solar_event_label(record.config.off_event.as_deref())}</strong>
                                            <span class="cell-subtle">
                                                {format!("Offset {} min", record.config.off_offset_minutes)}
                                            </span>
                                        </div>
                                    </div>
                                    <p class="mode-create-help">
                                        "System-managed solar mode. Use it in automations and criteria, but do not delete it."
                                    </p>
                                    <div class="card-controls">
                                        <button
                                            class="card-ctrl-btn card-ctrl-btn--secondary"
                                            disabled=move || busy.get()
                                            on:click=move |_| toggle_solar_editor.run(())
                                        >
                                            <span class="material-icons" style="font-size:18px">"schedule"</span>
                                            {move || if show_solar_editor.get() { "Hide Offsets" } else { "Edit Offsets" }}
                                        </button>
                                    </div>

                                    {move || show_solar_editor.get().then(|| view! {
                                        <div class="mode-inline-editor">
                                            <div class="mode-offset-editor">
                                                <label class="mode-offset-field">
                                                    <span>"On Offset Minutes"</span>
                                                    <input
                                                        type="number"
                                                        prop:value=move || solar_on_offset.get().to_string()
                                                        on:input=move |ev| {
                                                            let next = event_target_value(&ev).parse::<i32>().unwrap_or(0);
                                                            solar_on_offset.set(next);
                                                        }
                                                    />
                                                </label>
                                                <label class="mode-offset-field">
                                                    <span>"Off Offset Minutes"</span>
                                                    <input
                                                        type="number"
                                                        prop:value=move || solar_off_offset.get().to_string()
                                                        on:input=move |ev| {
                                                            let next = event_target_value(&ev).parse::<i32>().unwrap_or(0);
                                                            solar_off_offset.set(next);
                                                        }
                                                    />
                                                </label>
                                            </div>
                                            <div class="card-controls">
                                                <button
                                                    class="card-ctrl-btn card-ctrl-btn--primary"
                                                    disabled=move || busy.get()
                                                    on:click=move |_| save_solar_offsets.run(())
                                                >
                                                    <span class="material-icons" style="font-size:18px">"save"</span>
                                                    {move || if busy.get() { "Saving…" } else { "Save Offsets" }}
                                                </button>
                                            </div>
                                        </div>
                                    })}
                                }.into_any(),
                                ModeFlavor::Manual => view! {
                                    <>
                                        <p class="mode-create-help">
                                            "Manual mode behaves like a boolean variable. Toggle it directly or attach criteria to let HomeCore manage it."
                                        </p>

                                        <div class="card-controls">
                                            <button
                                                class="card-ctrl-btn card-ctrl-btn--on"
                                                disabled=move || busy.get() || state_on
                                                on:click={
                                                    let target_mode_id = record.config.id.clone();
                                                    move |_| {
                                                        let target_mode_id = target_mode_id.clone();
                                                        let token = auth.token_str().unwrap_or_default();
                                                        busy.set(true);
                                                        error.set(None);
                                                        notice.set(None);
                                                        spawn_local(async move {
                                                            match set_device_state(&token, &target_mode_id, &json!({ "on": true })).await {
                                                                Ok(()) => {}
                                                                Err(err) => error.set(Some(err)),
                                                            }
                                                            busy.set(false);
                                                        });
                                                    }
                                                }
                                            >
                                                <span class="material-icons" style="font-size:18px">"toggle_on"</span>
                                                "On"
                                            </button>
                                            <button
                                                class="card-ctrl-btn card-ctrl-btn--off"
                                                disabled=move || busy.get() || !state_on
                                                on:click={
                                                    let target_mode_id = record.config.id.clone();
                                                    move |_| {
                                                        let target_mode_id = target_mode_id.clone();
                                                        let token = auth.token_str().unwrap_or_default();
                                                        busy.set(true);
                                                        error.set(None);
                                                        notice.set(None);
                                                        spawn_local(async move {
                                                            match set_device_state(&token, &target_mode_id, &json!({ "on": false })).await {
                                                                Ok(()) => {}
                                                                Err(err) => error.set(Some(err)),
                                                            }
                                                            busy.set(false);
                                                        });
                                                    }
                                                }
                                            >
                                                <span class="material-icons" style="font-size:18px">"toggle_off"</span>
                                                "Off"
                                            </button>
                                            <button
                                                class="card-ctrl-btn card-ctrl-btn--secondary"
                                                disabled=move || busy.get()
                                                on:click=move |_| open_criteria_editor.run(())
                                            >
                                                <span class="material-icons" style="font-size:18px">"rule"</span>
                                                {move || if show_criteria_editor.get() { "Hide Criteria" } else { "Add Criteria" }}
                                            </button>
                                            {(!built_in).then(|| view! {
                                                <button
                                                    class="card-ctrl-btn card-ctrl-btn--danger"
                                                    disabled=move || busy.get()
                                                    on:click=delete_mode
                                                >
                                                    <span class="material-icons" style="font-size:18px">"delete"</span>
                                                    "Delete"
                                                </button>
                                            })}
                                        </div>

                                        {move || show_criteria_editor.get().then(|| view! {
                                            <div class="mode-inline-editor">
                                                <CriteriaBuilder
                                                    builder
                                                    device_options=device_options
                                                    mode_options=local_mode_options
                                                />
                                                <div class="card-controls">
                                                    <button
                                                        class="card-ctrl-btn card-ctrl-btn--primary"
                                                        disabled=move || busy.get()
                                                        on:click=move |_| save_criteria.run(())
                                                    >
                                                        <span class="material-icons" style="font-size:18px">"auto_fix_high"</span>
                                                        {move || if busy.get() { "Saving…" } else { "Save Criteria" }}
                                                    </button>
                                                </div>
                                            </div>
                                        })}
                                    </>
                                }.into_any(),
                                ModeFlavor::Criteria => view! {
                                    <>
                                        {record.definition.as_ref().map(|definition| view! {
                                            <div class="mode-criteria-summary">
                                                <div class="mode-stat">
                                                    <span class="mode-stat-label">"Turn On When"</span>
                                                    <strong>{on_summary.clone()}</strong>
                                                </div>
                                                <div class="mode-stat">
                                                    <span class="mode-stat-label">"Turn Off"</span>
                                                    <strong>{off_summary.clone().unwrap_or_default()}</strong>
                                                </div>
                                                <div class="mode-criteria-meta">
                                                    <span class="summary-chip">
                                                        {format!(
                                                            "Every {} min",
                                                            definition.criteria.reevaluate_every_n_minutes
                                                        )}
                                                    </span>
                                                    <span class="summary-chip">
                                                        {format!(
                                                            "Off: {}",
                                                            criteria_off_behavior_label(definition.criteria.off_behavior)
                                                        )}
                                                    </span>
                                                    <span class="summary-chip">
                                                        {criteria_generated_summary(definition)}
                                                    </span>
                                                </div>
                                            </div>
                                        })}

                                        <p class="mode-create-help">
                                            "Criteria-driven modes are managed by generated rules. Use this when the mode should reflect other devices or modes."
                                        </p>

                                        <div class="card-controls">
                                            <button
                                                class="card-ctrl-btn card-ctrl-btn--secondary"
                                                disabled=move || busy.get()
                                                on:click=move |_| open_criteria_editor.run(())
                                            >
                                                <span class="material-icons" style="font-size:18px">"edit"</span>
                                                {move || if show_criteria_editor.get() { "Hide Editor" } else { "Edit Criteria" }}
                                            </button>
                                            <button
                                                class="card-ctrl-btn card-ctrl-btn--secondary"
                                                disabled=move || busy.get()
                                                on:click=remove_criteria
                                            >
                                                <span class="material-icons" style="font-size:18px">"toggle_off"</span>
                                                "Remove Criteria"
                                            </button>
                                            {(!built_in).then(|| view! {
                                                <button
                                                    class="card-ctrl-btn card-ctrl-btn--danger"
                                                    disabled=move || busy.get()
                                                    on:click=delete_mode
                                                >
                                                    <span class="material-icons" style="font-size:18px">"delete"</span>
                                                    "Delete"
                                                </button>
                                            })}
                                        </div>

                                        {move || show_criteria_editor.get().then(|| view! {
                                            <div class="mode-inline-editor">
                                                <CriteriaBuilder
                                                    builder
                                                    device_options=device_options
                                                    mode_options=local_mode_options
                                                />
                                                <div class="card-controls">
                                                    <button
                                                        class="card-ctrl-btn card-ctrl-btn--primary"
                                                        disabled=move || busy.get()
                                                        on:click=move |_| save_criteria.run(())
                                                    >
                                                        <span class="material-icons" style="font-size:18px">"save"</span>
                                                        {move || if busy.get() { "Saving…" } else { "Save Criteria" }}
                                                    </button>
                                                </div>
                                            </div>
                                        })}
                                    </>
                                }.into_any(),
                            }}

                            {move || notice.get().map(|msg| view! {
                                <div class="scene-card-feedback scene-card-feedback--success">{msg}</div>
                            })}
                            {move || error.get().map(|msg| view! {
                                <div class="scene-card-feedback scene-card-feedback--error">{msg}</div>
                            })}
                        </div>

                        <div class="card-footer">
                            <div class="scene-card-footer-copy">
                                <span class="card-last-changed">
                                    {if let Some(state) = &live_state {
                                        if last_change_time(state).is_some() {
                                            format!("Last change {}", format_abs(last_change_time(state)))
                                        } else {
                                            "No change history".to_string()
                                        }
                                    } else {
                                        "No live state yet".to_string()
                                    }}
                                </span>
                                <span class="scene-card-footer-meta">{mode_kind_label(record.config.kind)}</span>
                            </div>
                        </div>
                    </div>
                }.into_any()
            }}
        </div>
    }
}

#[component]
pub fn ModesPage() -> impl IntoView {
    let auth = use_auth();
    let ws = use_ws();

    let modes: RwSignal<Vec<ModeRecord>> = RwSignal::new(Vec::new());
    let loading = RwSignal::new(true);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);
    let create_busy = RwSignal::new(false);

    let prefs = load_prefs();
    let card_size = RwSignal::new(prefs.card_size);
    let search = RwSignal::new(prefs.search);
    let status_filter = RwSignal::new(prefs.status_filter);
    let type_filter = RwSignal::new(prefs.type_filter);
    let sort_by = RwSignal::new(prefs.sort_by);
    let sort_dir = RwSignal::new(prefs.sort_dir);


    let create_name = RwSignal::new(String::new());
    let create_id = RwSignal::new(String::new());
    let create_kind = RwSignal::new(DraftModeKind::Manual);
    let create_builder = RwSignal::new(CriteriaBuilderState::default());

    let refresh = Callback::new(move |_| {
        let token = auth.token_str().unwrap_or_default();
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            let modes_result = fetch_modes(&token).await;
            let devices_result = fetch_devices(&token).await;
            match (modes_result, devices_result) {
                (Ok(mut mode_records), Ok(devices)) => {
                    mode_records.sort_by(|a, b| {
                        sort_key_str(&a.config.name).cmp(&sort_key_str(&b.config.name))
                    });
                    modes.set(mode_records);
                    ws.devices.update(|map| {
                        for device in devices {
                            map.insert(device.device_id.clone(), device);
                        }
                    });
                }
                (Err(err), _) | (_, Err(err)) => error.set(Some(err)),
            }
            loading.set(false);
        });
    });

    Effect::new(move |_| {
        refresh.run(());
    });

    Effect::new(move |_| {
        save_prefs(
            card_size.get(),
            &search.get(),
            &status_filter.get(),
            &type_filter.get(),
            sort_by.get(),
            sort_dir.get(),
        );
    });

    let device_options: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let mut devices = ws
            .devices
            .get()
            .values()
            .filter(|device| !is_scene_like(device) && !device.device_id.starts_with("mode_"))
            .map(|device| {
                (
                    device.device_id.clone(),
                    format!("{} · {}", display_name(device), device.device_id),
                )
            })
            .collect::<Vec<_>>();
        devices.sort_by(|a, b| sort_key_str(&a.1).cmp(&sort_key_str(&b.1)));
        devices
    });

    let mode_options: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let mut items = modes
            .get()
            .into_iter()
            .map(|record| {
                (
                    record.config.id.clone(),
                    format!("{} · {}", record.config.name, record.config.id),
                )
            })
            .collect::<Vec<_>>();
        items.sort_by(|a, b| sort_key_str(&a.1).cmp(&sort_key_str(&b.1)));
        items
    });

    let mode_rows: Memo<Vec<ModeRow>> = Memo::new(move |_| {
        let devices = ws.devices.get();
        modes
            .get()
            .into_iter()
            .map(|record| mode_row(&record, &devices))
            .collect()
    });

    let filtered_mode_ids: Memo<Vec<String>> = Memo::new(move |_| {
        let query = search.get().trim().to_lowercase();
        let status = status_filter.get();
        let types = type_filter.get();
        let sort_by_value = sort_by.get();
        let sort_dir_value = sort_dir.get();

        let mut rows = mode_rows
            .get()
            .into_iter()
            .filter(|row| {
                if !query.is_empty() && !row.search_text.contains(&query) {
                    return false;
                }
                if !status.is_empty() {
                    let state_value = if row.is_on { "On" } else { "Off" };
                    if !status.contains(state_value) {
                        return false;
                    }
                }
                if !types.is_empty() && !types.contains(row.flavor.filter_value()) {
                    return false;
                }
                true
            })
            .collect::<Vec<_>>();

        rows.sort_by(|a, b| {
            let cmp = cmp_mode_rows(a, b, sort_by_value);
            if sort_dir_value == SortDir::Desc {
                cmp.reverse()
            } else {
                cmp
            }
        });

        rows.into_iter().map(|row| row.id).collect()
    });

    let total = Signal::derive(move || filtered_mode_ids.get().len());
    let on_count =
        Signal::derive(move || mode_rows.get().into_iter().filter(|row| row.is_on).count());
    let criteria_count = Signal::derive(move || {
        mode_rows
            .get()
            .into_iter()
            .filter(|row| row.flavor == ModeFlavor::Criteria)
            .count()
    });
    let built_in_count = Signal::derive(move || {
        mode_rows
            .get()
            .into_iter()
            .filter(|row| row.built_in)
            .count()
    });

    let active_filter_summary: Memo<Vec<String>> = Memo::new(move |_| {
        let mut chips = Vec::new();
        if !status_filter.get().is_empty() {
            chips.push(format!(
                "Status: {}",
                status_filter
                    .get()
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !type_filter.get().is_empty() {
            chips.push(format!(
                "Type: {}",
                type_filter
                    .get()
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !search.get().trim().is_empty() {
            chips.push(format!("Search: {}", search.get().trim()));
        }
        chips
    });

    let canvas_class = move || card_size_canvas_class(card_size.get());

    let create_mode = move |_| {
        let token = auth.token_str().unwrap_or_default();
        let name = create_name.get().trim().to_string();
        let id = parse_mode_id_input(&create_id.get());

        if name.is_empty() {
            error.set(Some("Mode name is required.".to_string()));
            return;
        }
        if id.is_empty() {
            error.set(Some("Mode id is required.".to_string()));
            return;
        }

        let criteria = if create_kind.get() == DraftModeKind::Criteria {
            match build_criteria_config(&create_builder.get()) {
                Ok(criteria) => Some(criteria),
                Err(err) => {
                    error.set(Some(err));
                    return;
                }
            }
        } else {
            None
        };

        create_busy.set(true);
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match create_mode_request(&token, &id, &name, ModeKind::Manual, criteria.as_ref()).await
            {
                Ok(_) => {
                    create_name.set(String::new());
                    create_id.set(String::new());
                    create_kind.set(DraftModeKind::Manual);
                    create_builder.set(CriteriaBuilderState::default());
                    notice.set(Some(format!("Created {name}.")));
                    refresh.run(());
                }
                Err(err) => error.set(Some(err)),
            }
            create_busy.set(false);
        });
    };

    let sort_options = Signal::derive(|| {
        vec![
            ("name".to_string(), "Name".to_string()),
            ("type".to_string(), "Type".to_string()),
            ("status".to_string(), "Status".to_string()),
            ("last_seen".to_string(), "Last Change".to_string()),
        ]
    });
    let status_options = Signal::derive(|| {
        vec![
            ("On".to_string(), "On".to_string()),
            ("Off".to_string(), "Off".to_string()),
        ]
    });
    let type_options = Signal::derive(|| {
        vec![
            ("Solar".to_string(), "Solar".to_string()),
            ("Manual".to_string(), "Manual".to_string()),
            ("Criteria".to_string(), "Criteria".to_string()),
        ]
    });

    view! {
        <div class="page">
            <div class="heading">
                <div>
                    <h1>"Modes"</h1>
                    <p>
                        {move || format!("{} modes", mode_rows.get().len())}
                        " · "
                        {move || format!("{} on", on_count.get())}
                    </p>
                    <div class="scene-summary-row">
                        <span class="summary-chip">{move || format!("{} criteria-driven", criteria_count.get())}</span>
                        <span class="summary-chip">{move || format!("{} built-in", built_in_count.get())}</span>
                        <span class="summary-chip muted">"Modes behave like rule variables"</span>
                    </div>
                </div>
            </div>

            <LiveStatusBanner status=Signal::derive(move || ws.status.get()) />

            {move || error.get().map(|msg| view! { <p class="msg-error">{msg}</p> })}
            {move || notice.get().map(|msg| view! { <p class="msg-notice">{msg}</p> })}

            <div class="detail-card mode-create-card">
                <div class="card-title-row">
                    <h2 class="card-title">"Create Mode"</h2>
                    <span class="cell-subtle">
                        "Create manual variables or criteria-driven derived modes without leaving this page."
                    </span>
                </div>

                <div class="mode-create-row">
                    <div class="mode-create-field">
                        <span>"Name"</span>
                        <input
                            type="text"
                            prop:value=move || create_name.get()
                            placeholder="e.g. Vacation"
                            on:input=move |ev| create_name.set(event_target_value(&ev))
                        />
                    </div>
                    <div class="mode-create-field">
                        <span>"Id"</span>
                        <input
                            type="text"
                            prop:value=move || create_id.get()
                            placeholder="vacation"
                            on:input=move |ev| create_id.set(event_target_value(&ev))
                        />
                    </div>
                    <div class="mode-create-field">
                        <span>"Kind"</span>
                        <select
                            on:change=move |ev| {
                                let kind = if event_target_value(&ev) == "criteria" {
                                    DraftModeKind::Criteria
                                } else {
                                    DraftModeKind::Manual
                                };
                                create_kind.set(kind);
                            }
                        >
                            <option value="manual" selected=move || create_kind.get() == DraftModeKind::Manual>
                                "Manual"
                            </option>
                            <option value="criteria" selected=move || create_kind.get() == DraftModeKind::Criteria>
                                "Criteria-driven"
                            </option>
                        </select>
                    </div>
                </div>

                <p class="mode-create-help">
                    {move || {
                        let normalized = parse_mode_id_input(&create_id.get());
                        if normalized.is_empty() {
                            "Mode ids are normalized to the required mode_ prefix.".to_string()
                        } else {
                            format!("Will create {normalized}")
                        }
                    }}
                </p>

                {move || (create_kind.get() == DraftModeKind::Criteria).then(|| view! {
                    <CriteriaBuilder
                        builder=create_builder
                        device_options=Signal::derive(move || device_options.get())
                        mode_options=Signal::derive(move || mode_options.get())
                    />
                })}

                <div class="card-controls">
                    <button
                        class="card-ctrl-btn card-ctrl-btn--primary"
                        disabled=move || create_busy.get()
                        on:click=create_mode
                    >
                        <span class="material-icons" style="font-size:18px">"add"</span>
                        {move || if create_busy.get() { "Creating…" } else { "Create Mode" }}
                    </button>
                </div>
            </div>

            <div class="filter-panel panel">
                <div class="filter-bar">
                    <SearchField search placeholder="Search name, id, type, criteria…" />

                    <CardSizeSelect card_size />

                    <SortSelect
                        current_value=Signal::derive(move || sort_key_to_str(sort_by.get()).to_string())
                        options=sort_options
                        on_change=Callback::new(move |value: String| {
                            sort_by.set(sort_key_from_str(Some(&value)));
                        })
                    />

                    <SortDirToggle sort_dir />

                </div>

                <div class="filter-summary">
                    <div class="filter-summary-count">
                        <strong>{move || total.get()}</strong>
                        <span>" modes shown"</span>
                    </div>
                    <div class="filter-summary-chips">
                        {move || {
                            active_filter_summary
                                .get()
                                .into_iter()
                                .map(|chip| view! { <span class="summary-chip">{chip}</span> })
                                .collect_view()
                        }}
                    </div>
                </div>

                <div class="filter-body">
                    <div class="filter-multisel-row">
                        <MultiSelectDropdown
                            label="statuses"
                            placeholder="All statuses"
                            options=status_options
                            selected=status_filter
                        />
                        <MultiSelectDropdown
                            label="types"
                            placeholder="All types"
                            options=type_options
                            selected=type_filter
                        />
                        <ResetFiltersButton on_reset=Callback::new(move |_| {
                            search.set(String::new());
                            status_filter.set(HashSet::new());
                            type_filter.set(HashSet::new());
                            sort_by.set(SortKey::Name);
                            sort_dir.set(SortDir::Asc);
                        }) />
                    </div>
                </div>
            </div>

            {move || {
                if loading.get() && mode_rows.get().is_empty() {
                    view! { <p class="no-controls-msg">"Loading modes…"</p> }.into_any()
                } else if filtered_mode_ids.get().is_empty() {
                    view! {
                        <div class="cards-empty">
                            <div class="scene-empty-state">
                                <span class="material-icons" style="font-size:32px">"tune"</span>
                                <strong>"No modes match the current filters."</strong>
                                <span class="scene-empty-subtitle">
                                    "Try clearing filters or create a new manual or criteria-driven mode."
                                </span>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class=canvas_class()>
                            <For
                                each=move || filtered_mode_ids.get()
                                key=|id| id.clone()
                                children=move |id| {
                                    view! {
                                        <ModeCard
                                            mode_id=id
                                            modes
                                            device_options=Signal::derive(move || device_options.get())
                                            mode_options=Signal::derive(move || mode_options.get())
                                            on_refresh=refresh
                                        />
                                    }
                                }
                            />
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}
