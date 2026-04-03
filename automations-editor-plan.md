# Rules Editor Plan

## Goal

Build a full rule editor in `hc-web-leptos` that covers:

- list, create, edit, clone, delete, enable/disable rules
- structured forms for triggers, conditions, and actions
- dry-run (test) mode
- fire history
- stale-ref warnings

This is the most complex editor in the app. The design must handle a very wide type
surface (16 trigger variants, 12 condition variants, ~35 action variants) while staying
maintainable in Leptos 0.8 CSR.

Note: the backend API path is `/api/v1/automations` — that is the wire name and does
not change. The UI, routes, and code use "rule" / "rules" throughout.

---

## Current State

### Frontend

- Nav link to `/automations` exists in `src/app.rs` — will be updated to `/rules`
- No `/rules` route, no page, no API functions exist in `api.rs`
- Devices, scenes, modes, areas pages are complete and provide reusable patterns

### Backend API

All endpoints are implemented and tested. Wire path is `/api/v1/automations`:

```
GET    /api/v1/automations               list all rules (query: limit, offset, enabled, type)
POST   /api/v1/automations               create rule
GET    /api/v1/automations/{id}          get single rule
PUT    /api/v1/automations/{id}          full update (returns updated rule)
PATCH  /api/v1/automations/{id}          partial update (enabled, priority, tags)
DELETE /api/v1/automations/{id}          delete
POST   /api/v1/automations/{id}/test     dry-run; returns per-condition detail + would-fire
POST   /api/v1/automations/{id}/clone    clone rule; returns new rule
GET    /api/v1/automations/{id}/history  last 20 fire events for this rule
GET    /api/v1/automations/stale-refs    list rules with deleted device references
POST   /api/v1/automations/import        bulk import from JSON
GET    /api/v1/automations/export        bulk export as JSON
PATCH  /api/v1/automations               bulk enable/disable by IDs
```

Rule JSON shape (subset): `id`, `name`, `enabled`, `priority`, `tags`, `trigger`, `conditions`,
`actions`, `error`, `cooldown_secs`, `log_events`, `log_triggers`, `log_actions`,
`required_expression`, `cancel_on_false`, `trigger_condition`, `variables`,
`trigger_label`, `run_mode`.

---

## Architecture Decisions

### Working State Model

Do not attempt to represent the full `Rule` type system as typed Leptos signals.
The trigger/condition/action variants form a deep, branching hierarchy. Typed Rust
structs would require replicating `hc-types/rule.rs` in the frontend and keeping it
in sync.

Instead, use a **hybrid working state**:

```rust
// Top-level scalar fields — typed signals (fast reactivity, controlled inputs)
name:          RwSignal<String>
enabled:       RwSignal<bool>
priority:      RwSignal<i32>
tags:          RwSignal<Vec<String>>
cooldown_secs: RwSignal<String>   // input as string, parse on save
trigger_label: RwSignal<String>

// Per-section structured JSON (each item is its own signal for isolation)
trigger:    RwSignal<serde_json::Value>   // single object
conditions: RwSignal<Vec<RwSignal<serde_json::Value>>>
actions:    RwSignal<Vec<RwSignal<serde_json::Value>>>

// Advanced fields (collapsed by default)
required_expression: RwSignal<String>
cancel_on_false:     RwSignal<bool>
trigger_condition:   RwSignal<String>
log_events:          RwSignal<bool>
log_triggers:        RwSignal<bool>
log_actions:         RwSignal<bool>
run_mode:            RwSignal<serde_json::Value>
variables:           RwSignal<String>   // raw JSON text, parsed on save
```

On save, assemble the full JSON from all signals and PUT to the API.
On load, decompose the rule JSON into the signals above.

This is consistent with how the scenes editor represents device state as
`Vec<SceneMemberDraft>` with `payload_text: String`.

### Conditions: row-level isolation

Each condition row is `RwSignal<serde_json::Value>`. The type selector writes the
`type` field; type-specific sub-fields write their keys into the same `Value`.
Reordering swaps the outer `Vec` entries.

Compound conditions (`not`, `and`, `or`, `xor`) contain a nested conditions list.
In Phase 1, nested conditions are edited as a JSON textarea fallback.
In Phase 2, they get recursive `ConditionList` rendering.

### Actions: row-level isolation + block nesting

Same pattern as conditions. Each action row is `RwSignal<serde_json::Value>`.
Block actions (`parallel`, `conditional`, `repeat_until`, `repeat_while`,
`repeat_count`, `ping_host`, `wait_for_event`, `wait_for_expression`) have nested
action lists. Phase 1: JSON textarea for nested lists. Phase 2: recursive
`ActionList` rendering.

---

## Routes

```
/rules              RulesPage (list)
/rules/new          RuleEditorPage (create mode)
/rules/:id          RuleEditorPage (edit mode)
```

Update `src/app.rs`: change the nav link href and label from `/automations` / "Automations"
to `/rules` / "Rules", and register the three routes.

---

## Page Structure

### RulesPage (`src/pages/rules.rs`)

```
[search field] [status: All/Active/Disabled] [trigger type chip filter] [+ New Rule]

Rule list:
  ─────────────────────────────────────────────────────────
  [priority] [name]  [trigger type badge]  [enabled toggle]  [Edit] [Clone] [Delete]
  (red banner if rule.error is set)
  ─────────────────────────────────────────────────────────
```

Filters are ephemeral (not URL-backed in Phase 1).
Tags shown as chips under the name when present.

### RuleEditorPage (`src/pages/rule_detail.rs`)

Two-column layout (mirrors scene_detail.rs pattern):

```
┌─────────────────────────────┬─────────────────────────────┐
│ Left panel                  │ Right panel                  │
│                             │                              │
│ Rule Metadata               │ Conditions                   │
│  name, enabled, priority    │  [+ Add Condition]           │
│  tags, cooldown, run_mode   │  [type selector] [fields]    │
│                             │  ...                         │
│ Trigger                     │                              │
│  [type dropdown]            │ Actions                      │
│  [type-specific fields]     │  [+ Add Action]              │
│                             │  [type selector] [fields]    │
│ Advanced (collapsed)        │  [↑↓] [×]                   │
│  required_expression        │  ...                         │
│  trigger_condition          │                              │
│  variables                  │                              │
│  logging toggles            │                              │
│                             │                              │
└─────────────────────────────┴─────────────────────────────┘

[Test Run]  [Save]  [Cancel]   [Clone]  [Delete]   [Fire History]
```

On mobile / narrow: single-column stack.

---

## Component Breakdown

```
src/pages/
  rules.rs            — RulesPage (list)
  rule_detail.rs      — RuleEditorPage (editor)

src/components/
  rule_meta.rs        — RuleMetaForm (name, enabled, priority, tags, cooldown, run_mode)
  trigger_editor.rs   — TriggerEditor (type selector + per-type sub-forms)
  condition_list.rs   — ConditionList, ConditionRow, per-type sub-forms
  action_list.rs      — ActionList, ActionRow, per-type sub-forms
  rule_test_panel.rs  — TestRunPanel (trigger test, per-condition result detail)
  rule_history.rs     — RuleFireHistory (list of recent fire events)
  json_editor.rs      — reusable JsonEditor (textarea + parse error display)
  run_mode_editor.rs  — RunModeEditor (parallel / single / restart / queued)
  device_picker.rs    — DeviceAutocomplete (reuse across trigger/condition/action)
  mode_picker.rs      — ModeDropdown (reuse across forms)
  rule_picker.rs      — RuleDropdown (for run_rule_actions, pause_rule, etc.)
  scene_picker.rs     — SceneDropdown (for activate_scene_per_mode)
```

`DeviceAutocomplete`, `ModeDropdown`, `RuleDropdown`, `SceneDropdown` each accept a
value prop and an `on_change` callback — no shared global state, just fetched on mount.

---

## Phase 1 Trigger Form Coverage

All 16 trigger types get a full structured form:

| Trigger | Fields |
|---|---|
| `device_state_changed` | device picker (primary + additional device_ids), attribute text, `to`/`from`/`not_to`/`not_from` JSON inputs, `for_duration_secs`, `change_kind` dropdown |
| `device_availability_changed` | device picker, `to` tri-state (any/online/offline), `for_duration_secs` |
| `time_of_day` | time input (`HH:MM`), day-of-week checkbox group (Mon–Sun) |
| `sun_event` | event dropdown (sunrise/sunset/solar_noon/civil_dawn/civil_dusk), offset_minutes number input |
| `webhook_received` | path text field |
| `manual_trigger` | info text only |
| `custom_event` | event_type text field |
| `system_started` | info text only |
| `cron` | expression text field + format hint |
| `periodic` | every_n number + unit dropdown (minutes/hours/days/weeks) |
| `button_event` | device picker, event dropdown (pushed/held/double_tapped/released), optional button_number |
| `numeric_threshold` | device picker, attribute text, op dropdown (above/below/crosses_above/crosses_below), value number, optional `for_duration_secs` |
| `hub_variable_changed` | optional name text field (blank = any) |
| `mode_changed` | optional mode picker (blank = any), optional `to` tri-state (any/on/off) |
| `calendar_event` | optional calendar_id text, optional title_contains text, offset_minutes |

---

## Phase 1 Condition Form Coverage

| Condition | Fields |
|---|---|
| `device_state` | device picker, attribute text, op dropdown (eq/ne/gt/gte/lt/lte), value JSON input |
| `time_window` | start + end time inputs |
| `script_expression` | multiline Rhai textarea |
| `time_elapsed` | device picker, attribute text, duration_secs number |
| `device_last_change` | device picker, optional kind dropdown, optional source text, optional actor_id/actor_name |
| `private_boolean_is` | name text, value bool toggle |
| `hub_variable` | name text, op dropdown, value JSON input |
| `mode_is` | mode picker, on bool toggle |
| `not` / `and` / `or` / `xor` | **Phase 1**: JSON textarea for nested conditions array |

---

## Phase 1 Action Form Coverage

### Full forms (high-value / common):

| Action | Fields |
|---|---|
| `set_device_state` | device picker, `state` JSON editor, `track_event_value` toggle |
| `delay` | duration_secs number, cancelable toggle, optional cancel_key text |
| `notify` | channel text, message textarea, optional title text |
| `log_message` | message text, level dropdown (trace/debug/info/warn/error) |
| `set_mode` | mode picker, command dropdown (on/off/toggle) |
| `set_hub_variable` | name text, value JSON input, optional op dropdown |
| `fire_event` | event_type text, payload JSON editor |
| `run_script` | multiline Rhai textarea |
| `comment` | text field |
| `exit_rule` | no fields (info text) |
| `stop_rule_chain` | no fields (info text) |
| `publish_mqtt` | topic text, payload text, retain toggle |
| `call_service` | url text, method dropdown, body JSON editor, optional timeout_ms, retries, response_event |
| `run_rule_actions` | rule picker (UUID) |
| `pause_rule` / `resume_rule` | rule picker |
| `cancel_delays` | optional key text |
| `cancel_rule_timers` | optional rule picker |
| `set_private_boolean` | name text, value bool toggle |
| `set_variable` | name text, value JSON input, optional op dropdown |
| `capture_device_state` | key text, device_ids multi-picker |
| `restore_device_state` | key text |
| `wait_for_event` | optional event_type text, optional device picker, optional attribute text, optional timeout_ms |
| `wait_for_expression` | expression Rhai textarea, optional poll_interval_ms, timeout_ms, hold_duration_ms |
| `fade_device` | device picker, target JSON editor, duration_secs number, optional steps |

### JSON textarea fallback (complex block types — Phase 2 gets full treatment):

| Action | Phase 1 treatment |
|---|---|
| `parallel` | JSON textarea for `actions` array |
| `conditional` | JSON textarea for full action object |
| `repeat_until` | JSON textarea for full action object |
| `repeat_while` | JSON textarea for full action object |
| `repeat_count` | JSON textarea for full action object |
| `ping_host` | JSON textarea for full action object |
| `set_device_state_per_mode` | JSON textarea for full action object |
| `activate_scene_per_mode` | JSON textarea for full action object |
| `delay_per_mode` | JSON textarea for full action object |

The JSON fallback path uses the same `JsonEditor` component with parse validation.
Any action type not in the full-form list also falls through to the JSON editor.

---

## Phase 2 — Nested Block Editors

Full recursive editors for block action types:

- `parallel { actions }` — sub-`ActionList` inside a collapsible block
- `conditional { condition, then_actions, [else_if], else_actions }` — Rhai expression input + two sub-`ActionList` panes + else-if chain editor
- `repeat_until/while { condition, actions, max_iterations, interval_ms }` — expression input + sub-`ActionList`
- `repeat_count { count, actions, interval_ms }` — count number + sub-`ActionList`
- `ping_host { host, count, timeout_ms, then_actions, else_actions }` — fields + two sub-`ActionList` panes
- `set_device_state_per_mode` — mode-state pair list editor
- `activate_scene_per_mode` — mode-scene pair list editor
- `delay_per_mode` — mode-duration pair list editor

Recursive rendering: `ActionList` is a component that accepts `Vec<RwSignal<Value>>`
— the same component renders at any nesting depth.

For compound conditions (`not`, `and`, `or`, `xor`): recursive `ConditionList`.

---

## Phase 3 — List Page Features

- Clone button on each rule row
- Stale-ref warning banner on affected rules (call `GET /stale-refs` on page load)
- Fire history tab on editor page (call `GET /automations/{id}/history`)
- Test Run panel with per-condition pass/fail detail
- Bulk enable/disable (checkbox mode + floating toolbar)
- Tag filter chips
- Priority sort

---

## Phase 4 — Advanced Features

- Rule groups (if backend supports it)
- Import/export (JSON file download + upload)
- URL-backed filter state
- Keyboard navigation

---

## JsonEditor Component

Reusable across scenes and rules:

```rust
// src/components/json_editor.rs
#[component]
fn JsonEditor(
    value: RwSignal<Value>,
    #[prop(optional)] placeholder: &'static str,
    #[prop(optional)] rows: u32,
) -> impl IntoView
```

Renders a textarea displaying `serde_json::to_string_pretty(&value)`.
On change, attempts to parse; shows inline error if invalid, blocks save.
The save gate in `RuleEditorPage` checks all JSON editors for parse errors
before submitting.

---

## API Functions to Add (`src/api.rs`)

Names use "rule" terminology; paths hit `/api/v1/automations`:

```rust
pub async fn fetch_rules(token: &str) -> Result<Vec<Value>, String>
pub async fn fetch_rule(token: &str, id: &str) -> Result<Value, String>
pub async fn create_rule(token: &str, body: &Value) -> Result<Value, String>
pub async fn update_rule(token: &str, id: &str, body: &Value) -> Result<Value, String>
pub async fn patch_rule(token: &str, id: &str, body: &Value) -> Result<Value, String>
pub async fn delete_rule(token: &str, id: &str) -> Result<(), String>
pub async fn clone_rule(token: &str, id: &str) -> Result<Value, String>
pub async fn test_rule(token: &str, id: &str) -> Result<Value, String>
pub async fn rule_fire_history(token: &str, id: &str) -> Result<Value, String>
pub async fn rule_stale_refs(token: &str) -> Result<Value, String>
```

All use `Value` rather than typed Rule structs — keeps the frontend independent of
`hc-types` and avoids a large parallel type system.

---

## Save Flow

```
[Save button clicked]
  → validate: all JSON editors parse cleanly
  → validate: name is non-empty
  → assemble body:
      {
        "name": name.get(),
        "enabled": enabled.get(),
        "priority": priority.get(),
        "tags": tags.get(),
        "trigger": trigger.get(),
        "conditions": conditions.get().iter().map(|s| s.get()).collect(),
        "actions": actions.get().iter().map(|s| s.get()).collect(),
        "cooldown_secs": parse(cooldown_secs.get()),
        ... advanced fields ...
      }
  → if new: POST /automations → navigate to /rules/{new_id}
  → if existing: PUT /automations/{id} → refresh editor state from response
  → show success/error banner (same pattern as scene_detail.rs)
```

---

## Add Action / Add Condition Flow

When the user clicks "+ Add Action":
1. Append `json!({"type": "log_message", "message": ""})` as a new `RwSignal<Value>`
2. The new row renders with the default type pre-selected and its form open
3. User changes the type dropdown → replace the `Value` with a skeleton for the new type

Each type has a `default_skeleton()` that produces minimal valid JSON for that type.
This avoids the new-row starting with invalid JSON.

---

## Reordering

Phase 1: Up/Down arrow buttons on each row (swap adjacent signals in the outer Vec).
Phase 2: Drag-and-drop (revisit when needed).

---

## Unsaved Change Guard

If the user navigates away with unsaved changes, show a confirmation dialog.
Track `has_unsaved_changes: RwSignal<bool>` set on any signal mutation since
last save or load.

---

## Trigger Type Badge Colors

For the list page — color-code trigger type chips for quick scanning:

- Device triggers (`device_state_changed`, `device_availability_changed`, `button_event`, `numeric_threshold`): blue
- Time triggers (`time_of_day`, `sun_event`, `cron`, `periodic`, `calendar_event`): amber
- System/event triggers (`custom_event`, `system_started`, `hub_variable_changed`, `mode_changed`, `webhook_received`): purple
- Manual trigger: gray

---

## Recommended Delivery Order

1. Add API functions to `api.rs` (rule-named, automations-pathed)
2. Update nav link: `/automations` → `/rules`, label "Automations" → "Rules"
3. Build `RulesPage` (list only — no editor yet)
   - fetch + display rules
   - enable/disable toggle via PATCH
   - delete with confirmation
   - link to editor (no-op until step 4)
4. Build `RuleEditorPage` skeleton
   - load existing rule into working state
   - save (PUT) + create (POST) wiring
   - metadata form only (name, enabled, priority, tags)
5. Add `TriggerEditor` (all 16 trigger types)
6. Add `ConditionList` + condition forms (Phase 1 coverage)
7. Add `ActionList` + action forms (Phase 1 coverage — full forms + JSON fallback)
8. Clone button (POST clone → navigate to new rule)
9. Test Run panel
10. Fire History panel
11. Stale-ref warnings on list page
12. Phase 2: nested block editors
13. Phase 3: list page features (bulk ops, tag filters, priority sort)
