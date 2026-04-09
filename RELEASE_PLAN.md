# hc-web-leptos Release Plan — Default Admin Interface

**Created:** 2026-04-08
**Goal:** Make hc-web-leptos the default bundled web admin interface for homeCore v0.2.0

---

## Current State

- **16 of 17 nav pages implemented** — only Dashboards is missing
- **~50 of ~80 backend API endpoints consumed** by the frontend
- All core CRUD (devices, scenes, rules, modes, areas, glue, plugins, admin) is complete
- Full typed rule editor with 16 triggers, all conditions/actions, dry-run, fire history
- Real-time WebSocket updates for device state, plugin status, events, logs
- Dark mode, responsive layout, toast notifications, skeleton loading
- Auth with JWT, role-based access, 401 auto-redirect

---

## Phase 1 — Dashboard Page (Priority: Critical)

**Why:** First page users see after login. Currently redirects to `/devices` — needs a proper landing page with system overview.

**Backend endpoints available (all exist, none consumed yet):**
- `GET /dashboards` — list dashboards
- `POST /dashboards` — create dashboard
- `GET /dashboards/templates` — list built-in templates
- `POST /dashboards/templates/:id` — instantiate template
- `GET /dashboards/:id` — get dashboard
- `PUT /dashboards/:id` — update dashboard
- `DELETE /dashboards/:id` — delete dashboard
- `GET /dashboards/:id/export` — export dashboard JSON
- `POST /dashboards/import` — import dashboard JSON
- `POST /dashboards/:id/duplicate` — clone dashboard
- `POST /dashboards/:id/default` — set as user default

**New file:** `src/pages/dashboards.rs`

### Tasks

1. **Add API functions** in `src/api.rs`:
   - `fetch_dashboards()`, `fetch_dashboard(id)`, `create_dashboard()`, `update_dashboard(id)`
   - `delete_dashboard(id)`, `duplicate_dashboard(id)`, `set_default_dashboard(id)`
   - `fetch_dashboard_templates()`, `instantiate_template(id)`
   - `export_dashboard(id)`, `import_dashboard(json)`

2. **Add types** in `src/models.rs`:
   - Import/define Dashboard, DashboardWidget, DashboardTemplate types from hc-types

3. **Implement DashboardsPage** (`/dashboards`):
   - List view of user's dashboards (cards or grid)
   - "New Dashboard" button (blank or from template)
   - Set default dashboard
   - Delete, duplicate, export actions per dashboard

4. **Implement DashboardDetailPage** (`/dashboards/:id`):
   - Widget grid layout (read-only initially, then editable)
   - Widget types to support (based on backend schema):
     - Device status summary (count by type, availability)
     - Mode indicators (active modes)
     - Recent events feed
     - Scene quick-activate buttons
     - Plugin health overview
     - System stats (uptime, memory, DB size)
   - Edit mode toggle: rearrange/add/remove widgets
   - Save layout changes

5. **Update router** in `src/app.rs`:
   - Add `/dashboards` → DashboardsPage
   - Add `/dashboards/:id` → DashboardDetailPage
   - Change HomeRedirect (`/`) to redirect to user's default dashboard (fall back to `/devices`)

6. **CSS** in `style/main.css`:
   - Dashboard grid layout (CSS Grid with configurable columns)
   - Widget card styles (consistent with existing `.detail-card`)
   - Edit mode visual indicators

### Estimated scope: ~800-1200 lines Rust, ~200 lines CSS

---

## Phase 2 — Rule Groups (Priority: High) — IMPLEMENTED

**Status:** Implemented. Groups panel on rules page with filter, create, enable/disable, delete.

**What was done:**
- `RuleGroup` type added to `models.rs` (id, name, description, rule_ids)
- API functions: `fetch_rule_groups()`, `create_rule_group()`, `update_rule_group()`,
  `delete_rule_group()`, `rule_group_action()`
- Collapsible `RuleGroupsPanel` component on rules page with:
  - Group chip list with rule count, active/selected highlighting
  - Click group to filter rules list to that group only
  - Enable/disable all rules in group buttons
  - Delete group button
  - Create new group inline form
- CSS for group chips, panel, and create form

**Remaining (nice-to-have):**
- [ ] Group assignment dropdown in rule detail page
- [ ] Drag rules between groups
- [ ] Group editing (rename, change rule membership)

---

## Phase 3 — Calendar Integration (Priority: Medium) — IMPLEMENTED

**Status:** Implemented. Calendars section in Admin page + backend enhancements.

**Backend changes:**
- New `POST /calendars/upload` endpoint: upload ICS file content directly (JSON body)
- `CalEvent.end` field added: parsed from DTEND/DURATION in ICS files
- `parse_ics_duration()` helper: RFC 5545 DURATION parsing (PT1H30M, P1D, etc.)
- New `Condition::CalendarActive` variant: passes when a calendar event is currently active
  (start <= now < end), with optional `calendar_id` and `title_contains` filters
- CalendarActive evaluation in rule engine with CalendarHandle wired in at startup

**Frontend changes:**
- `CalendarsSection` component in Admin page with:
  - Calendar list (name, event count, upcoming count, source URL, fetch time)
  - "Events" button per calendar to view upcoming events in a table
  - "Delete" button per calendar
  - "Add Calendar by URL" form (URL + optional name)
  - "Upload ICS File" form (file picker)
- CalendarActive condition in rule editor (calendar_id + title_contains fields)
- API functions: `fetch_calendars()`, `add_calendar_by_url()`, `upload_calendar()`,
  `delete_calendar()`, `fetch_calendar_events()`

---

## Phase 4 — Import/Export UI (Priority: Medium) — MERGED INTO ADMIN PAGE

**Status:** Implemented. Merged into Admin page as the "Backup & Data" card.

**What was done:**
- Export rules and scenes as JSON download buttons in Admin "Backup & Data" card
- Import rules and scenes via file picker + upload in Admin "Backup & Data" card
- Import result summary (imported/skipped/errors counts)
- Full backup download preserved in same card
- API functions added: `export_rules()`, `import_rules()`, `export_scenes()`, `import_scenes()`

---

## Phase 5 — Matter/Z-Wave Commissioning (Priority: Low — defer if Matter plugin deferred)

**Why:** Users with Matter devices need to commission and manage them. Only needed if hc-matter ships.

**Backend endpoints (all exist, none consumed):**
- `POST /plugins/matter/commission` — commission device (QR code or pairing code)
- `GET /plugins/matter/nodes` — list commissioned nodes
- `POST /plugins/matter/reinterview` — reinterview node
- `DELETE /plugins/matter/nodes/:id` — remove node

### Tasks

1. **Add API functions** in `src/api.rs`:
   - `matter_commission(payload)`, `matter_nodes()`, `matter_reinterview(id)`, `matter_remove_node(id)`

2. **Add Matter section to PluginDetailPage** (conditionally rendered when plugin_id == "matter"):
   - Commissioned nodes list with status
   - "Commission New" form (pairing code input, optional QR scan via camera API)
   - Per-node actions: reinterview, remove
   - Node metadata display (vendor, product, endpoints)

### Estimated scope: ~300-400 lines Rust, ~60 lines CSS
### Decision: **Skip for v0.2.0 if hc-matter is deferred to v0.3**

---

## Phase 6 — Admin Enhancements (Priority: Medium) — MERGED INTO ADMIN PAGE

**Status:** Partially implemented. Merged into restructured Admin page.

**What was done:**
- Admin page split into sub-components: `SystemStatusSection`, `UserManagementSection`,
  `ChangePasswordSection`, `BackupDataSection`, `LogLevelSection`, `StaleRefsSection`, `DeviceCleanupSection`
- **Device schema viewer** added to device detail page (`/devices/:id`) as collapsible
  "Device Schema" card with lazy-load on expand. API: `fetch_device_schema()`
- **Stale refs** enhanced with "Edit Rule" action links per stale rule and summary count
- **System status** expanded with "Started" timestamp display

**Remaining:**
- [ ] Backup restore UI (needs backend restore endpoint)
- [ ] MQTT broker connection count (needs backend field in SystemStatus)
- [ ] Plugin restart history (needs backend endpoint)

---

## Phase 7 — Polish & UX (Priority: Low)

### Tasks

1. **Persistent filter preferences**:
   - Save filter/sort state per page to localStorage
   - Restore on page revisit within session

2. **Keyboard shortcuts**:
   - `Ctrl+K` / `Cmd+K` — global search (devices, rules, scenes)
   - `Esc` — close modals/panels
   - Already have infrastructure for this in Leptos event handlers

3. **Empty states**:
   - Audit all list pages for empty state messaging
   - Add "No devices yet — connect a plugin to get started" style messages
   - Link to relevant setup pages from empty states

4. **Loading performance**:
   - Audit API calls on page load — avoid waterfalls
   - Use `spawn_local` parallel fetches where pages need multiple resources
   - Skeleton loading already exists; verify coverage on all pages

5. **Mobile responsive audit**:
   - Test all pages at 375px, 768px breakpoints
   - Sidebar collapse behavior on mobile (already partially implemented)
   - Touch target sizes for buttons and links

6. **Error boundary hardening**:
   - Wrap each page in Leptos `ErrorBoundary`
   - Graceful degradation on individual widget/section failures
   - Retry buttons on transient errors

### Estimated scope: ~200-400 lines Rust, ~100 lines CSS

---

## Implementation Order & Dependencies

```
Phase 1 (Dashboard)     ←── Critical path, do first
  ↓
Phase 2 (Rule Groups)   ←── Enhances rules page, independent
  ↓
Phase 3 (Calendars)     ←── Extends admin, independent
Phase 4 (Import/Export) ←── Extends existing pages, independent
  ↓
Phase 6 (Admin)         ←── Builds on admin page
  ↓
Phase 7 (Polish)        ←── Final pass
  ↓
Phase 5 (Matter)        ←── Only if hc-matter ships, can defer
```

Phases 2, 3, 4 are independent and can be done in any order or in parallel.

---

## Files Modified Per Phase

| Phase | New Files | Modified Files |
|-------|-----------|----------------|
| 1 — Dashboard | `pages/dashboards.rs` | `app.rs`, `api.rs`, `models.rs`, `pages/mod.rs`, `main.css` |
| 2 — Rule Groups | — | `pages/rules.rs`, `pages/rule_detail.rs`, `api.rs`, `models.rs`, `main.css` |
| 3 — Calendars | — | `pages/admin.rs`, `api.rs`, `models.rs`, `main.css` |
| 4 — Import/Export | — | `pages/admin.rs`, `api.rs`, `main.css` (DONE — merged into admin) |
| 5 — Matter | — | `pages/plugins.rs`, `api.rs`, `models.rs`, `main.css` |
| 6 — Admin | — | `pages/admin.rs`, `pages/device_detail.rs`, `api.rs`, `main.css` (DONE — partial) |
| 7 — Polish | — | Multiple page files, `main.css` |

---

## Total Estimated Scope

| Phase | Rust LOC | CSS LOC | Priority |
|-------|----------|---------|----------|
| 1 — Dashboard | 800-1200 | 200 | Critical |
| 2 — Rule Groups | ~~300-500~~ done | ~~80~~ done | High |
| 3 — Calendars | ~~200-350~~ done | ~~50~~ done | Medium |
| 4 — Import/Export | ~~200-300~~ done | ~~30~~ done | Medium |
| 5 — Matter | 300-400 | 60 | Low (defer?) |
| 6 — Admin | ~~300-400~~ partial | ~~60~~ partial | Medium |
| 7 — Polish | 200-400 | 100 | Low |
| **Total** | **2300-3550** | **580** | — |

---

## Release Criteria

Before hc-web-leptos ships as the default admin interface:

- [ ] **Phase 1 complete** — Dashboard page functional with at least system overview widgets
- [x] **Phase 2 complete** — Rule groups visible and manageable
- [x] **Phase 4 complete** — Import/export for rules and scenes (merged into Admin "Backup & Data")
- [ ] **Phase 6 complete** — Schema viewer done, stale refs enhanced; backup restore pending backend
- [ ] **All pages tested** at 375px, 768px, 1280px, 1920px viewports
- [ ] **No console errors** on any page during normal operation
- [ ] **Auth flow solid** — login, logout, token refresh, role-based nav hiding
- [ ] **WebSocket reconnect** tested with server restart scenarios
- [ ] **Dark mode** verified on all new/modified pages
- [ ] **Lighthouse audit** — target > 80 on Performance, > 90 on Accessibility

### Nice-to-have for v0.2.0:
- [x] Phase 3 (Calendars)
- [ ] Phase 7 (Polish)

### Deferred to v0.3.0:
- [ ] Phase 5 (Matter) — ships with hc-matter stabilization
- [ ] Advanced dashboard widget editor
- [ ] Drag-and-drop dashboard layout builder
