# Phase 6 — Web client v0

## Status

🟡 In progress (kickoff 2026-06-08). Issues filed; design spec at
[`docs/superpowers/specs/2026-06-08-phase-6-web-client-v0-design.md`](../superpowers/specs/2026-06-08-phase-6-web-client-v0-design.md).

## Goal

The Phase-4 **synthetic toy scenario** plays solo in a browser:
Move / Investigate / PlayCard / EndTurn / Mulligan clickable, the
scenario runs to a resolution (Won via `AdvanceAct`, or Lost via
accumulating doom), and reconnecting mid-scenario restores the board.

## Issues

| Order | Issue | State |
|---|---|---|
| — | [#191](https://github.com/talelburg/eldritch/issues/191) — kickoff: design spec + issue breakdown | 🟡 open (this docs PR) |
| P6.1 | [#182](https://github.com/talelburg/eldritch/issues/182) — `protocol` crate extraction + `state` on `Applied` | ✅ PR #193 |
| P6.2 | [#183](https://github.com/talelburg/eldritch/issues/183) — server production-playable: synthetic registries + static WASM serving | ⏳ open |
| P6.3 | [#184](https://github.com/talelburg/eldritch/issues/184) — headless browser test harness + 6th CI job | ⏳ open |
| P6.4 | [#185](https://github.com/talelburg/eldritch/issues/185) — WS client + reactive state store | ⏳ open |
| P6.5 | [#186](https://github.com/talelburg/eldritch/issues/186) — board rendering (read-only) | ⏳ open |
| P6.6 | [#187](https://github.com/talelburg/eldritch/issues/187) — AwaitingInput resolution UI + legality gating | ⏳ open |
| P6.7a | [#188](https://github.com/talelburg/eldritch/issues/188) — core-loop action controls | ⏳ open |
| P6.7b | [#189](https://github.com/talelburg/eldritch/issues/189) — combat/edge action controls | ⏳ open |
| P6.8 | [#190](https://github.com/talelburg/eldritch/issues/190) — resolution surfacing + closing demo | ⏳ open |

## Ordering

| # | Issue | Why this slot | Depends on |
|---|---|---|---|
| P6.1 | #182 protocol crate + `state` on `Applied` ✅ PR #193 | Foundational; unblocks the client speaking the protocol with shared types | kickoff |
| P6.2 | #183 server registries + static WASM | The thing the client connects to | — (parallel w/ P6.1) |
| P6.3 | #184 headless harness + 6th CI job | Testing foundation before TDD-ing components; de-risks browser-in-CI early | — |
| P6.4 | #185 WS client + reactive store | The client's engine room; debug-dump render proves the round-trip | P6.1, P6.3 |
| P6.5 | #186 board rendering | See the state | P6.4 |
| P6.6 | #187 AwaitingInput UI + legality | Core-loop: `Investigate` opens a skill-test commit window (`AwaitingInput`), so this precedes the action controls | P6.5 |
| P6.7a | #188 core-loop action controls | Toy scenario clickable to a **Won** resolution | P6.6 |
| P6.7b | #189 combat/edge action controls | Rounds out the action surface (combat/Lost walk) | P6.7a |
| P6.8 | #190 resolution + closing demo | Milestone close | all |

P6.1 and P6.2 both touch `server`; sequence to avoid churn. P6.3 is
independent and can land any time before P6.4.

## Decisions made

From the 2026-05-01/02 strategy phase (still standing):

- **Frontend stack:** Rust + WASM, Leptos (CSR-only, no SSR). Language
  cohesion (shared `game-core` types) over UI-ecosystem breadth.
- **Exit ramp:** keep the `game-core` ↔ UI boundary clean so a future
  React pivot wouldn't touch the engine.
- **Card art:** link to ArkhamDB's CDN with text-only fallback; never
  re-host. (Deferred to Phase 7 — synthetic cards have no art.)
- **The `web` crate is the only one that compiles to wasm32 in
  production.** `game-core` (and now `protocol`) also compile to wasm32
  because they're pure — load-bearing for sharing engine/protocol code.

Settled in the Phase 6 kickoff brainstorm (2026-06-08; full rationale
in the design spec):

- **D1 — server sends authoritative `GameState` on `Applied`.** The
  client renders the server's snapshot; events stay on the wire but are
  not load-bearing for rendering. Chosen over a client-side event
  reducer (events are notifications, not a complete state-delta log —
  `CardsDrawn { count }` omits which cards) and over client-side action
  replay (viable in solo since `RngState = {seed, draws}` is serialized
  and reproduces randomness, but desyncs on any dropped frame and
  doesn't generalize to multiplayer). State-on-`Applied` is the
  smallest client, self-healing, and generalizes cleanly to Phase 8.
- **D2 — shared `crates/protocol` crate.** `ClientMessage` /
  `ServerMessage` move out of `crates/server/src/wire.rs` into a crate
  depending only on `game-core` (wasm-safe), consumed by both `server`
  and `web`. `web` cannot depend on `server` (axum/sqlx/tokio aren't
  wasm).
- **D3 — single-port production serving + dev proxy.** Production: one
  port serves API + WS + the static WASM bundle (`ServeDir` over
  `dist/`, `index.html` SPA fallback). Dev: Trunk serves the wasm and
  proxies API/WS to the server. The client derives its WS URL
  same-origin from `window.location` — never a hardcoded port — so one
  binary works in both.
- **D4 — headless `wasm-bindgen-test` is the verification gate (6th CI
  job).** Logic is factored into plain functions where natural, but the
  gate is a real headless browser.
- **D5 — synthetic registries in the production server, this phase
  only.** The toy scenario's cards are synthetic
  (`scenarios::test_fixtures::synth_cards::TEST_REGISTRY`), separate
  from `cards::REGISTRY`; both target one process-global `OnceLock`, so
  the server installs the synthetic set, with a `TODO` to swap to the
  real registries when Phase 7 lands The Gathering. Same
  `test_fixtures`-on-in-server tension Phase 4 flagged.

## Open questions

None blocking. Per-PR details: exact `ServeDir`/SPA-fallback wiring and
whether the wasm bundle is embedded or shipped alongside the binary
(P6.2); the precise legality-gating surface (P6.6); CSS/layout
specifics (P6.5).

## Dependencies

- Phase 5 (server + persistence) — the client connects to and depends
  on the working server.
- Phase 0 hello-world Leptos page — the Trunk-buildable skeleton.

## What "done" looks like

- The synthetic toy scenario loads in the browser from `POST /games` +
  WebSocket connect.
- Move / Investigate / PlayCard / EndTurn / Mulligan clickable; the
  scenario runs to a visible resolution (Won/Lost).
- Closing the tab and reopening picks up where the session left off
  (reconnect → `Hello` restores the board, including any in-flight
  `AwaitingInput`).
- Headless `wasm-bindgen-test` covers component render + click→submit
  wiring; the 6th CI job is green.
