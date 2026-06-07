# Phase 5 — Server + persistence: design spec

**Date:** 2026-06-07
**Status:** Approved design, pre-implementation
**Milestone:** `phase-5-server-and-persistence`

## Goal

Run the pure `game-core` engine behind an Axum server with a SQLite,
event-sourced action log, such that game state survives a server
restart and clients can reconnect mid-scenario without losing in-flight
choices.

Phase 5 is **headless**: it ships the server, wire protocol, and
persistence, exercised by Rust WebSocket integration-test clients. The
real browser client is Phase 6; auth and per-seat turn enforcement are
Phase 8.

## Locked decisions (from the 2026-05-01/02 strategy phase)

- Server-authoritative event streaming. Clients send `PlayerAction`s;
  the server validates via `apply()`, appends to the log, and broadcasts
  events. Client views are non-authoritative, render-only.
- Event-sourced persistence: state is *derived* by replaying the action
  log. Snapshots are a perf optimization, not the source of truth.
- Fine-grained action log; UI-only actions (hovering, browsing hand) do
  not enter the global log.
- Two log streams: the action log (DB, load-bearing) vs operational logs
  (`tracing` crate, server-application diagnostics).
- Single-port production binary (API + WS + static WASM); two ports only
  in dev.

## Decisions settled in this brainstorm

1. **Demonstration is headless.** Phase 5 proves itself with Rust WS
   integration-test clients, not a browser. "Two browsers see the same
   state" becomes "two WS clients receive identical event streams." Keeps
   the Phase 5 / Phase 6 boundary clean.
2. **Persistence library: `sqlx`** with SQLite. Async, integrates with
   tokio/axum, built-in migration runner. No `spawn_blocking` offload.
3. **Action log storage: flat blob rows** — `(game_id, seq, action_json)`.
   Replay folds `apply()` over the rows ordered by `seq`. Matches the
   engine's existing flat `Vec<Action>` model; no schema churn when the
   `Action` enum changes.
4. **Seeding: store the `setup()` output as a seed-state blob** in the
   `games` row, rather than re-deriving from `scenario_id` at replay.
   This is a *correctness* seed, not a perf snapshot. It decouples replay
   from `setup()` ever needing to be deterministic or seed-parameterized.
   `GameState.rng` (`RngState = (seed, draws)`) is already part of the
   serialized state, and in-play randomness is recorded as explicit
   `EngineRecord` actions, so seed-state + folded actions reproduce state
   bit-for-bit.
5. **Snapshots deferred.** Replaying a single scenario's log is trivially
   fast at this scale. Filed as a p2-later perf issue; built only when
   profiling demands it.
6. **Anonymous hub, no enforcement.** Any connected client of a game may
   submit actions; the server applies and broadcasts to all of the game's
   connections. No identity, no seat/turn ownership. Auth and per-seat
   enforcement are explicitly Phase 8.
7. **Single-port static WASM serving deferred to Phase 6.** The strategy
   decision stands, but the WASM bundle does not exist until Phase 6, so
   the `ServeDir` + fallback wiring lands there. Phase 5 ships the API/WS
   server only. (This is a correction to the Phase 5 phase-doc's "done"
   wording, which was written browser-first.)

## Architecture

### Crate boundary — `game-core` stays pure, the server owns all I/O

`game-core` continues to expose only the pure entry point
`apply(state: GameState, action: Action) -> ApplyResult`. All I/O —
database, websockets, connection state — lives in the `server` crate.

A new `GameSession` type **in the `server` crate** is the host wrapper:

```text
GameSession {
    game_id: GameId,
    state:   GameState,        // current derived state
    outcome: EngineOutcome,    // Done | AwaitingInput | (last) Rejected
}
```

- `create(scenario_id)` → invokes the scenario registry's `setup()`,
  persists the seed state, returns a fresh session.
- `apply(PlayerAction)` → folds through `game_core::apply`, persists the
  action on acceptance, returns `(events, outcome)`. On `Rejected`,
  nothing is persisted (the engine guarantees state is unchanged).
- `load(game_id)` → rehydrate by replay: read seed state + actions,
  fold.

`cards::REGISTRY` and `scenarios::REGISTRY` are installed once in
`main()` at startup via the existing `install` functions.

### Persistence — `sqlx` + SQLite, two tables

```sql
-- migrations/0001_init.sql
CREATE TABLE games (
    game_id      TEXT PRIMARY KEY,
    scenario_id  TEXT NOT NULL,
    seed_state   TEXT NOT NULL,   -- serde_json of the setup() GameState
    created_at   TEXT NOT NULL
);

CREATE TABLE actions (
    game_id  TEXT NOT NULL REFERENCES games(game_id),
    seq      INTEGER NOT NULL,
    action   TEXT NOT NULL,       -- serde_json of Action
    PRIMARY KEY (game_id, seq)
);
```

**Replay:** deserialize `games.seed_state` into a `GameState`, then fold
`apply()` over `actions` ordered by `seq`. The result equals the live
state bit-for-bit.

**Write path:** an accepted `apply` appends one `actions` row at the next
`seq`. The append and any session-cache update must agree on `seq`;
`seq` is owned by the persistence layer (max existing `seq` + 1 per
game).

### Wire protocol (WebSocket, JSON)

Client → Server:

- `Submit(PlayerAction)` — includes `ResolveInput` for in-flight choices.

Game creation/join is an HTTP path (below), not a WS message.

Server → Client:

- `Hello { state: GameState, outcome: EngineOutcome }` — sent on
  connect/reconnect. The full render baseline, including any
  `AwaitingInput`.
- `Applied { events: Vec<Event>, outcome: EngineOutcome }` — broadcast to
  every connection of the game after each accepted action.
- `Rejected { reason }` — sent **only to the submitting client**.

### Lifecycle + connection hub

- `POST /games { scenario_id }` → runs `setup()`, writes the `games` row,
  returns `game_id`.
- `GET /games/:id/ws` → upgrades to a websocket, joins the game's
  broadcast group (a tokio `broadcast` channel), receives `Hello`, then
  streams `Applied` frames.
- The server holds live `GameSession`s in an in-memory map keyed by
  `game_id`. On a cache miss (e.g. after a restart, or first access to an
  older game), it lazily `load`s the session by replay from the DB.

### Error / rejection paths

- A `PlayerAction` that the engine rejects produces `Rejected { reason }`
  to the sender only; no DB write, no broadcast.
- Malformed wire frames (undeserializable) are answered with a protocol
  error to the sender; the connection stays open.
- While a session is `AwaitingInput`, any non-`ResolveInput` `Submit`
  rejects (the engine already enforces this); the server surfaces the
  rejection to the sender.

## Issues + ordering (Shape B)

Filed against the `phase-5-server-and-persistence` milestone, plus one
kickoff tracking issue for this design PR.

| Order | Issue | Category | Summary |
|---|---|---|---|
| — | Phase 5 kickoff: design spec + issue breakdown | infra | Tracks this docs PR (spec + initial phase-doc update). |
| P5.1 | sqlx + SQLite wiring | infra | Workspace deps, `SqlitePool`, migration harness, `0001_init.sql` (games + actions), startup smoke test. |
| P5.2 | `GameSession` + store | engine/server | create-from-setup, apply-and-persist, load-by-replay; tested against in-memory SQLite. |
| P5.3 | Axum WS endpoint + broadcast hub | server | `Hello` / `Applied` / `Rejected` protocol, per-game broadcast group, connection map. |
| P5.4 | Game lifecycle HTTP | server | `POST /games`, lazy session rehydrate on cache miss. |
| P5.5 | Reconnect + restart resume | server | Mid-scenario reconnect delivers current state incl. `AwaitingInput`; server restart rebuilds via replay. |
| P5.6 | Closing demo (test) | test | Two Rust WS clients on one game receive identical event streams; restart + reconnect reproduces state; mid-scenario reconnect delivers the in-flight choice. |

**Dependency order:** P5.1 → P5.2 → P5.3 → P5.4 → P5.5 → P5.6.

### Out of scope / deferred

- **Snapshots** for replay perf → p2-later issue (filed, not built).
- **Single-port static WASM serving** (`ServeDir` + fallback) → Phase 6,
  when the bundle exists.
- **Auth / seat ownership / turn enforcement** → Phase 8.

## What "done" looks like (headless)

- Two Rust WS clients connect to one running game and receive identical
  event streams.
- Closing both clients, restarting the server, and reconnecting
  reproduces the exact state via action-log replay.
- A single client reconnecting mid-scenario receives the in-flight
  `AwaitingInput` and can resolve it.

## Dependencies

- Phase 4 (scenario plumbing) — closed; provides a complete-enough engine
  to persist meaningful state.
- Phase 0 server hello-world — provides the Axum + Tokio scaffolding to
  grow into.
