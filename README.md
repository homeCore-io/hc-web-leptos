# hc-web-leptos

`hc-web-leptos` is the administrative web interface for homeCore.

It is not intended to be the end-user dashboard surface. This client is for
operators and power users who need to inspect devices, review state, manage
metadata, and work with other system resources through an authenticated,
data-dense UI.

## Current Scope

Implemented today:

- Login flow against homeCore auth
- Devices list with filters, sorting, column preferences, and live updates
- Device detail page with metadata editing, device-specific controls, and history

Planned but not yet implemented in this client:

- Scenes
- Modes
- Events
- Automations
- Dashboards

The sidebar already exposes those sections as part of the intended admin shell,
but only the devices workflow is complete at this stage.

## Stack

- Rust
- Leptos CSR
- Leptos Router
- Thaw UI components
- Trunk for local development and bundling

## Development

Install Trunk if needed:

```bash
cargo install trunk
```

Run the app locally:

```bash
trunk serve
```

By default, `Trunk.toml` proxies:

- `/api` to `http://10.0.10.200:8080/api`
- `/ws` to `ws://10.0.10.200:8080/api/v1/events/stream`

Update `Trunk.toml` if your local homeCore instance is running elsewhere.

## Architecture Notes

- Auth is JWT-based and stored in local storage.
- The shell is intentionally admin-oriented: sidebar navigation, top bar, and
  dense content views.
- The devices page is the current primary surface and should be treated as the
  reference pattern for future admin modules.
- Live device state is hydrated from REST and then kept fresh through the
  homeCore event stream over WebSocket.

## Near-Term Priorities

1. Harden and refine the devices page as the baseline admin experience.
2. Bring the route shell and navigation into line with what is actually implemented.
3. Expand the admin surface to scenes, modes, events, automations, and dashboards
   once the device-management patterns are solid.
