# Phase 6 — Web client v0: design spec

**Date:** 2026-06-08
**Status:** Design approved; issues to be filed.
**Milestone:** `phase-6-web-client-v0`

## Goal

The Phase-4 **synthetic toy scenario** plays solo in a browser:
Move / Investigate / PlayCard / EndTurn / Mulligan are clickable, the
scenario runs to a resolution (Won via `AdvanceAct`, or Lost via
accumulating doom), and reconnecting mid-scenario restores the board.

This is `phase-6-web-client-v0`'s "done." The first **real** scenario
(The Gathering) and real card content are Phase 7.

## Non-goals (explicit)

- **No card art.** Synthetic cards have no ArkhamDB codes; render
  text-only. CDN art (link, don't re-host) is Phase 7.
- **No auth.** The anonymous hub stays; a login gate is Phase 8.
- **No multiplayer UX.** Solo only. (The state-delivery choice below
  still generalizes cleanly to multiplayer.)
- **No polish.** Functional panel/list layout, minimal CSS — legible,
  not pretty.
- **No real content.** Toy synthetic fixture only.

## Context: what already exists

- **Server (Phase 5, done).** `POST /games` → `GameId`; WebSocket at
  `/games/{id}/ws`; wire protocol `ClientMessage::Submit{action}` ↔
  `ServerMessage::{Hello{state,outcome}, Applied{events,outcome},
  Rejected{reason}}`. Server-authoritative: clients submit
  `PlayerAction`s, the server validates via `apply()`, persists to the
  SQLite action log, and broadcasts.
- **Web crate.** Leptos 0.8 CSR, still the Phase-0 hello-world.
  `game-core` is already a wasm dependency, so the client has the real
  `GameState` / `Event` / `PlayerAction` / `EngineOutcome` types.

## Three gaps Phase 6 must close

1. **Wire types live in `crates/server/src/wire.rs`.** `web` cannot
   depend on `server` (axum/sqlx/tokio don't build to wasm32), so the
   protocol types need a shared home.
2. **The production server binary installs no registries.** `main.rs`
   never calls `cards::install` or `scenarios::install` — only the
   tests do. A real `POST /games` would fail `UnknownScenario`, and
   `PlayCard` would reject. The binary also doesn't serve the WASM
   bundle (Phase 5 deferred single-port static serving to Phase 6).
3. **`Applied` carries `events`, not state.** The client gets full
   `GameState` once (`Hello`), then only event deltas — and the events
   are **notifications, not a complete state-delta log** (e.g.
   `CardsDrawn { count }` names *how many*, not *which* cards; several
   event doc-comments say "state inspection has the contents"). So the
   client cannot rebuild state from events alone.

## Architecture decisions

### D1 — State delivery: server sends authoritative state on `Applied`

Add `state: Box<GameState>` to `ServerMessage::Applied` (keep `events`
and `outcome`). The client always renders the server's snapshot
(`state.set(new_state)`); events stay on the wire for future
log/animation use but are not load-bearing for rendering.

**Why (vs. the alternatives):**
- *Client folds events into a reducer* — rejected. Requires making
  events a complete delta channel (audit every event forever) and
  duplicates engine mutation logic in a second implementation.
- *Client replays actions via the real `apply()`* — viable in solo
  (RNG is reproducible: `RngState = {seed, draws}` is serialized in
  `GameState`, chaos draws/shuffles reproduce from it, so an identical
  action yields bit-identical state). Rejected anyway: it makes the
  client a replica engine-runner that desyncs permanently on any
  dropped/reordered frame, and doesn't generalize to multiplayer
  (non-acting clients can't replay an action they didn't author).
- *Server sends state* — chosen. Smallest client (can't desync),
  self-healing (full-state frames are idempotent), and the cleanest
  multiplayer story (in Phase 8 the server feeds non-acting clients the
  same way, with zero new machinery). Cost — a full-state frame per
  action — is negligible at 1-investigator scale; if it ever bites,
  add snapshot+delta then (same posture as the deferred #174).

### D2 — Shared `protocol` crate

Extract `ClientMessage` / `ServerMessage` from
`crates/server/src/wire.rs` into a new `crates/protocol` crate that
depends only on `game-core`. Both `server` and `web` depend on it.
`protocol` is pure serde types (no I/O), so it compiles to wasm32 —
same load-bearing purity that lets `game-core` be shared.

### D3 — Single-port server (production) + dev proxy

**Production:** one process on one port serves the JSON API
(`POST /games`, `/health`), the WebSocket (`/games/{id}/ws`), **and**
the client bundle (`crates/web/dist/`: `index.html`, JS loader, `.wasm`).
Mechanically: a fallback `ServeDir` over `dist/` behind the existing
routes, with `index.html` as the SPA fallback. This static-serving
route is the Phase-6 deliverable Phase 5 deferred.

**Dev:** two processes — `cargo run -p server` (API+WS on `:8000`) and
`trunk serve --proxy-backend=http://localhost:8000` (wasm + hot-reload
on `:3000`, proxying API/WS to `:8000`). Already documented in
`CLAUDE.md`'s "Dev loop (two terminals)".

**Load-bearing client detail:** the client derives its WebSocket URL
from `window.location` (same-origin) — never a hardcoded port — so one
client binary works in production (`:8000`) and dev (`:3000` →
proxied). Pinned down in P6.4.

### D4 — Testing: headless browser (`wasm-bindgen-test`), a 6th CI job

The verification gate is `wasm-bindgen-test` driving a real headless
browser, asserting on rendered DOM and click→submit wiring. This adds
a sixth CI job (browser-in-CI). Logic is still factored into plain
functions where natural (e.g. a pure "which controls are legal" helper,
a pure `ServerMessage` → signal reducer), but the gate is headless.
Components that consume engine state (board render, AwaitingInput
prompt) are tested by feeding a constructed `GameState` / outcome — no
live socket required.

### D5 — Synthetic registries in the production server (this phase only)

The toy scenario's cards are synthetic
(`scenarios::test_fixtures::synth_cards::TEST_REGISTRY`), separate from
`cards::REGISTRY`; both target the same process-global `OnceLock`, so
only one can be installed. To play the toy scenario, the Phase-6 server
installs the **synthetic** scenario + card registries, knowingly, as
Phase-6 content, with an in-source `TODO` to swap to the real
`cards`/`scenarios` registries when Phase 7 lands The Gathering. This
is the same `test_fixtures`-on-in-server tension Phase 4 flagged.

## Client architecture (layers)

Bottom-up, mapped to the issues below:

1. **Transport + reactive store** (P6.4): a WebSocket connection to the
   same-origin `/games/{id}/ws`; parse `Hello`/`Applied`/`Rejected`
   into Leptos signals (`GameState`, `EngineOutcome`, connection
   status, last rejection); submit `ClientMessage::Submit`; reconnect
   on close (re-`Hello` restores state).
2. **Board view** (P6.5): read-only render of `GameState`.
3. **Interaction plumbing** (P6.6): the `AwaitingInput` prompt UI and a
   pure legality-gating helper.
4. **Action controls** (P6.7a/P6.7b): the action buttons, built on (3).
5. **Resolution surfacing + closing demo** (P6.8).

## Issue breakdown & ordering

| # | Issue | Why this slot | Depends on |
|---|---|---|---|
| **kickoff** | Design spec + issue breakdown (this doc) | Mirrors Phase 5's #167 | — |
| **P6.1** | `protocol` crate extraction + `state` on `Applied` (D1, D2) | Foundational; unblocks the client speaking the protocol with shared types | kickoff |
| **P6.2** | Server production-playable: install synthetic registries + serve static WASM single-port (D3, D5) | The thing the client connects to | — (parallel w/ P6.1) |
| **P6.3** | Headless test harness + 6th CI job (D4); smoke-tests the existing App | Testing foundation before TDD-ing components; de-risks browser-in-CI early | — |
| **P6.4** | WS client + reactive state store; same-origin WS URL; reconnect | The client's engine room; a debug-dump render proves the round-trip | P6.1, P6.3 |
| **P6.5** | Board rendering (read-only): phase/round, act/agenda+doom, locations+clues, investigator panel, enemies | See the state | P6.4 |
| **P6.6** | `AwaitingInput` resolution UI + legality gating | Foundational interaction layer — `Investigate` opens a skill-test commit window (`AwaitingInput`), so this is core-loop, not edge | P6.5 |
| **P6.7a** | Core-loop action controls: Mulligan, Investigate, EndTurn, DrawEncounterCard, AdvanceAct, PlayCard, Move | Toy scenario clickable to a **Won** resolution | P6.6 |
| **P6.7b** | Combat/edge action controls: Fight, Evade, Draw (enemy-target pickers) | Rounds out the action surface; matters on the combat/Lost walk | P6.7a |
| **P6.8** | Resolution surfacing (Won/Lost banner) + closing demo (browser playthrough to resolution + reconnect; headless drive of the wiring) | Milestone close | all |

**Ordering rationale.** P6.1+P6.2 are the server-side foundation
(parallelizable; both touch `server`, so sequence to avoid churn).
P6.3 lands the test harness before any real component so the rest is
TDD'd. P6.4→P6.7b build the client bottom-up (transport → read →
interaction plumbing → actions). P6.8 ties it off.

**Key finding driving the P6.6/P6.7 split.** The engine returns
`AwaitingInput` from live paths — `skill_test.rs` opens a **commit
window** on every skill test. So in solo, `Investigate` pauses for
input every time; handling `AwaitingInput` is core-loop-critical, which
is why it precedes (and is a dependency of) the action controls rather
than being treated as edge polish.

## What "done" looks like

- The synthetic toy scenario loads in the browser from `POST /games` +
  WebSocket connect.
- Move / Investigate / PlayCard / EndTurn / Mulligan are clickable; the
  scenario runs to a resolution; the resolution is visible (Won/Lost
  banner / log).
- Closing the tab and reopening picks up where the session left off
  (reconnect → `Hello` restores the board, including any in-flight
  `AwaitingInput`).
- Headless `wasm-bindgen-test` covers the component render + click→submit
  wiring; the 6th CI job is green.

## Open questions

None blocking. Details deferred to the relevant PR:
- Exact `ServeDir`/SPA-fallback wiring and whether the wasm bundle is
  embedded or shipped alongside the binary (P6.2).
- The precise legality-gating surface (which controls enable in which
  phase/state) — settled against the toy scenario in P6.6.
- Minimal CSS / layout specifics (P6.5).
