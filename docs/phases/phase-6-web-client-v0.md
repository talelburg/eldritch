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
| — | [#191](https://github.com/talelburg/eldritch/issues/191) — kickoff: design spec + issue breakdown | ✅ PR #192 |
| P6.1 | [#182](https://github.com/talelburg/eldritch/issues/182) — `protocol` crate extraction + `state` on `Applied` | ✅ PR #193 |
| P6.2 | [#183](https://github.com/talelburg/eldritch/issues/183) — server production-playable: synthetic registries + static WASM serving | ✅ PR #194 |
| P6.3 | [#184](https://github.com/talelburg/eldritch/issues/184) — headless browser test harness + 6th CI job | ✅ PR #195 |
| P6.4 | [#185](https://github.com/talelburg/eldritch/issues/185) — WS client + reactive state store | ✅ PR #197 |
| P6.4a | [#199](https://github.com/talelburg/eldritch/issues/199) — restore trunk hot-reload dev loop | ⏳ open |
| P6.4b | [#198](https://github.com/talelburg/eldritch/issues/198) — WebSocket liveness (heartbeat + graceful shutdown) | ⏳ open |
| P6.5 | [#186](https://github.com/talelburg/eldritch/issues/186) — board rendering (read-only) | ⏳ open |
| P6.6 | [#187](https://github.com/talelburg/eldritch/issues/187) — AwaitingInput resolution UI + legality gating | ⏳ open |
| P6.7a | [#188](https://github.com/talelburg/eldritch/issues/188) — core-loop action controls | ⏳ open |
| P6.7b | [#189](https://github.com/talelburg/eldritch/issues/189) — combat/edge action controls | ⏳ open |
| P6.8 | [#190](https://github.com/talelburg/eldritch/issues/190) — resolution surfacing + closing demo | ⏳ open |

## Ordering

| # | Issue | Why this slot | Depends on |
|---|---|---|---|
| P6.1 | #182 protocol crate + `state` on `Applied` ✅ PR #193 | Foundational; unblocks the client speaking the protocol with shared types | kickoff |
| P6.2 | #183 server registries + static WASM ✅ PR #194 | The thing the client connects to | — (parallel w/ P6.1) |
| P6.3 | #184 headless harness + 6th CI job ✅ PR #195 | Testing foundation before TDD-ing components; de-risks browser-in-CI early | — |
| P6.4 | #185 WS client + reactive store ✅ PR #197 | The client's engine room; debug-dump render proves the round-trip | P6.1, P6.3 |
| P6.4a | #199 hot-reload dev loop | Restore hot-reload **before** the content-UI slots so P6.5+ iterate fast; likely moves the WS to a distinct `/ws/{id}` path | P6.4 |
| P6.4b | #198 WS liveness | Heartbeat + graceful shutdown so the client detects silently-dropped connections — finishes the transport before content builds on it; revisit `leptos-use` here (P6.4 deferral trigger) | P6.4a |
| P6.5 | #186 board rendering | See the state | P6.4 |
| P6.6 | #187 AwaitingInput UI + legality | Core-loop: `Investigate` opens a skill-test commit window (`AwaitingInput`), so this precedes the action controls | P6.5 |
| P6.7a | #188 core-loop action controls | Toy scenario clickable to a **Won** resolution | P6.6 |
| P6.7b | #189 combat/edge action controls | Rounds out the action surface (combat/Lost walk) | P6.7a |
| P6.8 | #190 resolution + closing demo | Milestone close | all |

P6.1 and P6.2 both touch `server`; sequence to avoid churn. P6.3 is
independent and can land any time before P6.4. P6.4a and P6.4b are
foundation follow-ups from P6.4 (surfaced during its manual testing,
[#198](https://github.com/talelburg/eldritch/issues/198) /
[#199](https://github.com/talelburg/eldritch/issues/199)) that land
**before** the content UI; both touch the WS route/transport, so do the
route move (P6.4a) first to avoid reworking it under P6.4b.

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

Settled implementing P6.2 (PR #194):

- **Bundle ships alongside the binary, not embedded.** D3's static
  serving is a `ServeDir` over `crates/web/dist` (default; overridable
  via `AppState::new_with_dist`) — artifacts are read from disk at
  runtime, not compiled in. Embedding stays deferred (no deploy story
  needs it yet).
- **`ServeDir` uses `.fallback()`, not `.not_found_service()`.** The
  SPA fallback to `index.html` must return `200 OK` (the browser's JS
  resolves the route); `not_found_service` would force `404`.

Settled implementing P6.3 (PR #195):

- **Headless component tests live in `crates/web/tests/`, importing
  `web::app::*`.** The `web` crate is now lib + bin (`lib.rs`/`app.rs` +
  a thin `main.rs` shim) so `tests/` can reach the components — this is
  the pattern every later P6 component test follows. Each such test file
  **must** carry a crate-level `#![cfg(target_arch = "wasm32")]`: a
  `wasm-bindgen-test` mounts into a DOM that doesn't exist off-wasm, so
  without the gate the native `test`/`clippy` jobs would compile (and
  `test` would *run*) a browser-only test and break. Only the
  `wasm-test` job (wasm32 via `wasm-pack test --headless --firefox`)
  runs them.

Settled implementing P6.4 (PR #197):

- **`protocol` owns the full client/server contract — HTTP DTOs and
  identity, not just the WS messages.** `GameId` and the `POST /games`
  DTOs (`CreateGameRequest`/`CreateGameResponse`) were hoisted out of
  `server` into `protocol` so the wasm client reuses the real types
  rather than redefining `String`/local copies. `GameId` carries no
  `sqlx`/`axum` impls, so the move kept `protocol` dependency-free; id
  *minting* (`uuid`) stays server-side as `id::random_game_id`, and
  `server` re-exports `protocol::GameId`. **Convention for later slots:
  a new client-facing endpoint's request/response types belong in
  `protocol`.**
- **`leptos-use` deferred — revisit on the *second* concrete need.**
  P6.4 needed only a WebSocket + localStorage, both served by
  already-present `gloo-net` + ~6 lines of `web-sys`, with a pure
  `reduce` kept library-agnostic. `leptos-use` version-tracks Leptos (an
  upgrade tax) and adopting it is purely additive, so it was **not**
  taken. Adopt it when a second concrete cluster appears — e.g.
  responsive board layout in **P6.5** (`use_media_query` /
  `use_element_size`) or auth in **Phase 8** (`use_cookie`) — at which
  point it is justified by real needs, not speculation.

## Open questions

None blocking. Per-PR details: the precise legality-gating surface
(P6.6); CSS/layout specifics (P6.5).

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
