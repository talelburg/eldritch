# Phase 5 — Server + persistence

## Status

📐 Architecture only. No issues filed.

## Goal

Engine on Axum + SQLite event log; reconnect works.

## Decisions made

From the 2026-05-01/02 strategy phase:

- **Wire protocol:** server-authoritative event streaming. Browsers send `PlayerAction`s; server validates, applies, emits events, appends to the log, broadcasts events to all connected browsers. Browsers apply events to a local view for rendering only — never authoritative.
- **Persistence model:** event-sourced. Game state is *derived* by replaying the action log; periodic snapshots are a perf optimization, not the source of truth.
- **Action log granularity:** fine-grained. Every token reveal, skill-test modifier, commit is its own event. Per-player UI-only actions (browsing hand, hovering cards) do NOT go in the global log.
- **Two log streams:**
  - **Action log** (in DB) — the gameplay event sequence per scenario, load-bearing for resume / undo / replay / debug.
  - **Operational logs** (`tracing` crate) — server-application logs (connections, errors, timing). Different purposes; different homes.
- **Hosting (v1):** smallest thing that works — single small VM ($5–10/mo) or self-host on home hardware. No k8s, no autoscaling, no CDN. A single Dockerfile is cheap insurance for portability but not required for v1.
- **Production form:** the server binary serves the API/websocket and the static WASM bundle on one port. (Two ports only in dev.)

## Open questions

⏳ **Scoping TBD.** Issues for this phase haven't been filed. When Phase 4 closes, file:

- **Database schema and migration tooling.** SQLite, likely with `sqlx` or `rusqlite`. Migration strategy.
- **Event-log schema.** One table per scenario with `(scenario_id, sequence_number, action_json)` rows? Or normalized? Tradeoff between query convenience and replay simplicity (a flat blob is simpler).
- **Periodic state snapshots** for resume performance. Every N actions? Every X minutes?
- **Websocket session model.** One websocket per player per scenario? Reconnect-with-resume-token?
- **Action validation flow.** `PlayerAction` parsing at the wire boundary, then `apply()`, then broadcast. Error paths for rejected actions.
- **`game-core` ↔ server boundary.** `game-core` stays pure (no I/O); the server is the host that loads/saves state and applies actions. Define the trait or function surface server uses.
- **Mid-scenario resume mechanics.** A scenario abandoned in progress reloads from log → state, players reconnect, engine continues.

## Dependencies

- Phase 4 (scenario plumbing) — server needs a complete-enough engine to actually persist meaningful state.
- Phase 0 server hello-world (already shipped) gives us Axum + Tokio scaffolding to grow into.

## What "done" looks like

- Two browsers can connect to a running server, both see the same scenario state.
- Closing both browsers, restarting the server, and reconnecting reproduces the exact state via action-log replay.
- Reconnecting a single browser mid-scenario picks up where it left off without losing in-flight choices.
