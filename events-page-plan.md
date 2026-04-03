# Events Page Plan

## Goal

Build a single troubleshooting surface in `hc-web-leptos` that combines:

- event history
- live event stream
- log history
- live log stream
- correlation between logs and events

This should become the one-stop area for understanding what happened, why it happened, and what HomeCore did in response.

## Current State

### Frontend

- `hc-web-leptos` already has an `Events` nav link in `src/app.rs`
- there is no actual `/events` route or page yet
- the app already has a shared device WebSocket context in `src/ws.rs`, but it is device-focused:
  - filters server-side to `device_state_changed,device_availability_changed,scene_activated` only
  - the events page must open its own independent WS connections (events + logs) — do not reuse `WsContext`

### Backend

`core` already provides:

- `GET /api/v1/events`
  recent event history; filters: `limit`, `type`, `device_id`
- `GET /api/v1/events/stream`
  live event WebSocket
- `GET /api/v1/logs/stream`
  live log WebSocket; already supports `?history=N` (max 500) to replay ring-buffer on connect,
  plus `?level=` and `?target=` filters — the history replay makes a REST endpoint mostly
  redundant, but `GET /api/v1/logs` is still worth adding for API consistency

`EventLogQuery` currently exposes only `limit`, `type`, and `device_id`.

`LogLine` shape:

```rust
pub struct LogLine {
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub target: String,
    pub message: String,
    pub fields: serde_json::Value,   // unstructured — no guaranteed schema
}
```

`fields` is a freeform JSON object; entity keys like `device_id`, `rule_id` are emitted
by convention from tracing spans but are not enforced. Phase 2 metadata extraction requires
an audit of what each crate actually emits before the normalization scope can be finalized.

The current gaps are:

- no `GET /api/v1/logs` REST history endpoint
- event history filtering is narrow: only `type` and `device_id`; no `rule_id`, `plugin_id`, `scene_id`, `mode_id`
- `LogLine.fields` entity keys are unaudited and unguaranteed
- no first-class correlation model between logs and events

## Product Direction

Do not split this into separate “Events” and “Logs” pages.

The page should be a unified activity timeline with normalized entries from both sources.

Each row should have a common shape:

- `timestamp`
- `source`
  `event` or `log`
- `kind`
  event type or log target/category
- `severity`
- `summary`
- optional `device_id`
- optional `plugin_id`
- optional `rule_id`
- optional `scene_id`
- optional `mode_id`
- optional `correlation_id`
- raw payload / details

## Main Use Cases

- watch live system activity in real time
- search recent history for failures or suspicious behavior
- filter activity to a specific device, plugin, rule, or mode
- correlate a rule firing with the device changes and logs around it
- correlate a log line back to the event or command path that produced it
- inspect raw structured payloads when needed

## Page Modes

The page should support three working modes:

### 1. Live

Merged real-time stream of logs and events.

Capabilities:

- pause / resume
- auto-scroll toggle
- keep newest entries visible
- show source and severity clearly
- quick filters without leaving the stream

### 2. History

Search and sort older entries.

Capabilities:

- text search
- time range
- source filters
- level / type filters
- entity filters
- newest-first or oldest-first sort

### 3. Correlation

Focused troubleshooting mode that groups related items.

Capabilities:

- “show related” from any row
- group by correlation id when available
- fallback grouping by time window + entity keys
- display the likely chain:
  trigger -> rule -> action -> device change -> follow-on logs/events

## Recommended UI Layout

### Top Toolbar

- source toggles
  `All`, `Events`, `Logs`
- live toggle
- pause / resume
- time range
- text search
- clear filters

### Filter Panel

- event type
- log level
- device
- plugin
- rule
- mode
- scene
- target
- “correlated only” later

### Main Timeline

- dense merged rows
- badges for source, severity, entity
- grouped timestamps
- virtualized list once volume gets large

### Detail Panel

- summary
- structured fields
- raw JSON
- related entries
- quick links to Devices / Scenes / Modes / Automations pages

## Visual Behavior

- color-code source and severity, but keep it restrained
- logs and events should look related, not like two unrelated widgets
- default sort for live and recent history should be newest first
- make correlated entries easy to scan with subtle linking, not giant nesting

## Recommended Backend Direction

Do not build a whole new debugging subsystem first.

Start by closing the practical API gaps and normalizing metadata.

### Phase 1 Backend

Add:

- `GET /api/v1/logs`
  log ring-buffer history

Support log history filters:

- `limit`
- `level`
- `target`
- `q` text search

Support richer event history filters:

- existing `type`
- existing `device_id`
- add `rule_id`
- add `plugin_id`
- add `scene_id`
- add `mode_id`

### Phase 2 Backend

Normalize metadata extraction.

For logs, derive stable fields from `LogLine.fields` and standard tracing usage:

- `device_id`
- `plugin_id`
- `rule_id`
- `scene_id`
- `mode_id`
- `correlation_id`

For events, expose a normalized envelope alongside the raw event body.

### Phase 3 Backend

Introduce a correlation model.

Preferred fields:

- `entry_id`
- `correlation_id`
- `cause_id`
- `span_kind`

At minimum, propagate correlation ids through:

- automation execution
- device command paths
- scene activation
- mode changes

### Phase 4 Backend

Optional unified API:

- `GET /api/v1/activity`
- `GET /api/v1/activity/stream`

This is not required for v1 if the frontend can merge:

- `/events`
- `/logs`
- `/events/stream`
- `/logs/stream`

## Recommended Frontend Direction

Build the page around a normalized `ActivityEntry` model in `hc-web-leptos`, even if the backend stays split at first.

### Proposed Frontend Model

```rust
struct ActivityEntry {
    id: String,
    // Events: use seq number as string (e.g. "e-1234").
    // Logs: no stable server-assigned ID; generate a synthetic ID on the client
    // using a hash of timestamp + target + message, or a client-side incrementing counter.
    timestamp: chrono::DateTime<chrono::Utc>,
    source: ActivitySource,      // event | log
    kind: String,                // event type or log target/category
    severity: Option<String>,    // error|warn|info|debug|trace
    summary: String,
    device_id: Option<String>,
    plugin_id: Option<String>,
    rule_id: Option<String>,
    scene_id: Option<String>,
    mode_id: Option<String>,
    correlation_id: Option<String>,
    raw: serde_json::Value,
}
```

### Phase 1 Frontend

Add:

- `/events` route
- `src/pages/events.rs`
- REST client functions for event history and, once available, log history
- page-local live stream handling for events and logs
- merged timeline UI
- filter panel
- detail drawer

Capabilities:

- merged live timeline
- event history
- log history once backend exists
- source/type/level/device filtering
- text search on loaded data

### Phase 2 Frontend

Add richer filters:

- plugin
- rule
- scene
- mode
- target

Add URL-backed filter state so a troubleshooting view can be shared or bookmarked.

### Phase 3 Frontend

Add correlation workflows:

- “show related”
- correlation group expansion
- side panel focused on one execution chain

### Phase 4 Frontend

Add operator polish:

- export selected slice
- saved search presets
- keyboard-friendly navigation
- optional compact / expanded row density

## Page Component Breakdown

Suggested pieces:

- `src/pages/events.rs`
  route-level page
- `ActivityToolbar`
- `ActivityFilters`
- `ActivityTimeline`
- `ActivityRow`
- `ActivityDetailPanel`
- `use_activity_stream`
  merges log + event WebSockets
- `normalize_event_entry`
- `normalize_log_entry`

## Sorting and Filtering Rules

Default sort:

- newest first

Supported sorts:

- newest first
- oldest first
- severity
- source

Supported filters:

- source
- event type
- log level
- device
- plugin
- rule
- mode
- scene
- target
- text query

## Correlation Strategy

### Best Version

Use true correlation ids emitted by backend operations.

### Practical First Version

Before real correlation ids exist, provide “related entries” using:

- same `device_id`
- same `rule_id`
- same `plugin_id`
- same `scene_id`
- same `mode_id`
- nearby timestamps

This is weaker than true correlation, but still useful for troubleshooting.

## Recommended Delivery Order

1. Add `GET /api/v1/logs` history endpoint in `core`
2. Expand event/log metadata filters in `core`
3. Add `/events` page in `hc-web-leptos`
4. Build merged live timeline from `/events/stream` + `/logs/stream`
5. Add detail drawer and entity filters
6. Add log/event metadata normalization for better grouping
7. Add correlation ids and “show related”

## Concrete Implementation Plan

### Backend Tasks

#### Task 1: Log History API

- add a log ring-buffer query type similar to `EventLogQuery`
- add `GET /api/v1/logs`
- support:
  - `limit` (default 50, max 500 — match WS `?history=` cap)
  - `level`
  - `target`
  - `q`
- note: `GET /api/v1/logs/stream` already replays ring-buffer history via `?history=N` on connect;
  the REST endpoint is for API consistency, not new infrastructure

#### Task 2: Event Filter Expansion

- extend `EventLogQuery`
- extract and expose:
  - `rule_id`
  - `plugin_id`
  - `scene_id`
  - `mode_id`

#### Task 3: Log Metadata Extraction

- audit what entity fields each crate actually emits into `LogLine.fields` before designing the
  normalized shape — the field names are convention, not schema-enforced
- add a normalized log history entry shape based on audit findings
- extract standard entity fields from `LogLine.fields`
- avoid forcing the client to parse `message`

#### Task 4: Correlation Fields

- identify the key command/execution paths
- add `correlation_id` to logs first
- propagate into related event emissions where feasible

#### Task 5: Docs

- update `core/docs/openapi.yaml`
- document both split and future unified activity concepts

### Frontend Tasks

#### Task 1: Route and API Client

- add `events::EventsPage`
- add `/events` route in `src/app.rs`
- add REST client wrappers:
  - `fetch_events`
  - `fetch_logs`

#### Task 2: Normalized Activity Model

- add activity models in `src/models.rs` or a focused module
- normalize event rows and log rows into one timeline type

#### Task 3: Live Streaming

- add `use_activity_stream` hook managing two independent WS connections:
  - `/api/v1/events/stream`
  - `/api/v1/logs/stream`
- each connection gets its own reconnect loop with exponential backoff (same pattern as `ws.rs`)
- do not share or reuse the global `WsContext` — that connection filters to device events only
- merge incoming messages from both streams into a single `RwSignal<VecDeque<ActivityEntry>>`
- enforce a hard cap on the in-memory buffer (e.g. 500 entries); drop oldest on overflow
  - this must be Phase 1, not deferred — 500 log history + live stream fills the DOM quickly in WASM
- batch incoming WS messages into Leptos signal updates (e.g. accumulate in a 100 ms window)
  to prevent reactive thrash during startup or high-volume periods
- support pause/resume: when paused, buffer incoming entries up to the cap without triggering
  reactive updates; flush to the signal on resume

#### Task 4: Timeline UI

- filter panel
- summary counters
- merged row list with fixed max-height + overflow scroll (virtual scroll not required at 500-entry cap)
- detail drawer

#### Task 5: Correlation UX

- “show related”
- entity badges
- grouped execution slices

## Recommendation

The correct v1 is not “perfect correlation”. The correct v1 is:

- merged logs + events
- live + history
- strong filtering
- clean detail inspection

Then add true correlation once backend identifiers are good enough.

That keeps the page immediately useful without blocking on a full tracing redesign.
