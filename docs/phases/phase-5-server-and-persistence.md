# Phase 5 — Server + persistence

## Status

🟡 In progress (since 2026-06-07). Design spec landed; issues filed.

Design spec: [`docs/superpowers/specs/2026-06-07-phase-5-server-and-persistence-design.md`](../superpowers/specs/2026-06-07-phase-5-server-and-persistence-design.md).

## Goal

Engine on Axum + SQLite event log; reconnect works. **Headless** this
phase — proven with Rust WebSocket integration-test clients. The browser
client is Phase 6; auth + per-seat enforcement are Phase 8.

## Issues

| Order | Issue | State |
|---|---|---|
| — | [#167](https://github.com/talelburg/eldritch/issues/167) — kickoff: design spec + issue breakdown | 🟡 open (this docs PR) |
| P5.1 | [#168](https://github.com/talelburg/eldritch/issues/168) — sqlx + SQLite wiring: pool, migrations, action-log schema | ⏳ open |
| P5.2 | [#169](https://github.com/talelburg/eldritch/issues/169) — GameSession host: create / apply-and-persist / load-by-replay | ⏳ open |
| P5.3 | [#170](https://github.com/talelburg/eldritch/issues/170) — Axum WS endpoint + per-game broadcast hub | ⏳ open |
| P5.4 | [#171](https://github.com/talelburg/eldritch/issues/171) — Game lifecycle HTTP: POST /games + lazy rehydrate | ⏳ open |
| P5.5 | [#172](https://github.com/talelburg/eldritch/issues/172) — Reconnect + restart resume: deliver in-flight AwaitingInput | ⏳ open |
| P5.6 | [#173](https://github.com/talelburg/eldritch/issues/173) — closing demo: two WS clients, restart+reconnect replay | ⏳ open |

Deferred out of the phase: [#174](https://github.com/talelburg/eldritch/issues/174) — periodic state snapshots for replay perf (p2-later, build only when profiling demands it).

## Ordering

Strict dependency chain: P5.1 → P5.2 → P5.3 → P5.4 → P5.5 → P5.6.

- **P5.1** is the foundation everything sits on (DB + schema).
- **P5.2** is the pure-engine ↔ persistence seam (host-side `GameSession`); testable headless against in-memory SQLite before any networking exists.
- **P5.3–P5.5** layer the network: the WS hub, then lifecycle + lazy rehydrate, then resume semantics.
- **P5.6** is the closing demo that exercises the whole stack (mirrors Phase 4's #157).

## Decisions made

From the 2026-05-01/02 strategy phase (still standing):

- **Server-authoritative event streaming.** Clients send `PlayerAction`s; the server validates via `apply()`, appends to the log, broadcasts events. Client views are non-authoritative.
- **Event-sourced persistence.** State is derived by replaying the action log; snapshots are perf-only.
- **Two log streams:** action log (DB, load-bearing) vs operational logs (`tracing`).
- **Single-port production binary** (API + WS + static WASM); two ports only in dev.

Settled in the Phase 5 brainstorm (2026-06-07):

- **Headless demonstration.** Phase 5 proves itself with Rust WS integration-test clients, not a browser. Keeps the Phase 5 / Phase 6 boundary clean.
- **`sqlx` + SQLite**, async, with the built-in migration runner. No `spawn_blocking` offload.
- **Flat action-log rows** `(game_id, seq, action_json)`; replay folds `apply()` over rows ordered by `seq`. No schema churn when `Action` changes.
- **Seed-state blob, not setup-replay.** The `setup()` output is stored once as `games.seed_state`; replay = seed-state + folded actions. Decouples replay correctness from `setup()` determinism. (`GameState.rng` is already serialized; in-play randomness is recorded as explicit `EngineRecord` actions.)
- **Anonymous hub, no enforcement.** Any connected client may submit; the server applies + broadcasts to the game's connections. Identity/seats/turn-ownership are Phase 8.
- **Single-port static WASM serving lands in Phase 6**, not Phase 5 — the WASM bundle doesn't exist until then. Phase 5 ships the API/WS server only. (Corrects the original browser-first "done" wording below.)
- **Snapshots deferred** to #174; build only when replay is measurably slow.

## Open questions

None blocking. Remaining choices are issue-local implementation detail (e.g. `GameId` representation, exact WS frame encoding) and will be settled in their respective PRs.

## Dependencies

- Phase 4 (scenario plumbing) — closed; provides a complete-enough engine to persist meaningful state.
- Phase 0 server hello-world (already shipped) — Axum + Tokio scaffolding to grow into.

## What "done" looks like

- Two Rust WS clients connect to one running game and receive identical event streams.
- Closing both clients, restarting the server, and reconnecting reproduces the exact state via action-log replay.
- A single client reconnecting mid-scenario receives the in-flight `AwaitingInput` and can resolve it.
