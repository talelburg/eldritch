# Phase 6 — Web client v0

## Status

🟢 Complete (PR #210, closing P6.8). Kicked off 2026-06-08; design spec at
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
| P6.4a | [#199](https://github.com/talelburg/eldritch/issues/199) — restore trunk hot-reload dev loop | ✅ PR #200 |
| P6.4b | [#198](https://github.com/talelburg/eldritch/issues/198) — WebSocket liveness (heartbeat + graceful shutdown) | ⏭️ deferred → Phase 8 (closed) |
| P6.5 | [#186](https://github.com/talelburg/eldritch/issues/186) — board rendering (read-only) | ✅ PR #204 |
| P6.6 | [#187](https://github.com/talelburg/eldritch/issues/187) — AwaitingInput resolution UI + legality gating | ✅ PR #207 |
| P6.7a | [#188](https://github.com/talelburg/eldritch/issues/188) — core-loop action controls | ✅ PR #208 |
| P6.7b | [#189](https://github.com/talelburg/eldritch/issues/189) — combat/edge action controls | ✅ PR #209 |
| P6.8 | [#190](https://github.com/talelburg/eldritch/issues/190) — resolution surfacing + closing demo | ✅ PR #210 |

## Ordering

| # | Issue | Why this slot | Depends on |
|---|---|---|---|
| P6.1 | #182 protocol crate + `state` on `Applied` ✅ PR #193 | Foundational; unblocks the client speaking the protocol with shared types | kickoff |
| P6.2 | #183 server registries + static WASM ✅ PR #194 | The thing the client connects to | — (parallel w/ P6.1) |
| P6.3 | #184 headless harness + 6th CI job ✅ PR #195 | Testing foundation before TDD-ing components; de-risks browser-in-CI early | — |
| P6.4 | #185 WS client + reactive store ✅ PR #197 | The client's engine room; debug-dump render proves the round-trip | P6.1, P6.3 |
| P6.4a | #199 hot-reload dev loop ✅ PR #200 | Restore hot-reload **before** the content-UI slots so P6.5+ iterate fast; moved the WS to a distinct `/ws/{id}` route so trunk can proxy REST + WS without colliding | P6.4 |
| ~~P6.4b~~ | #198 WS liveness — **deferred to Phase 8** | Its triggering symptom is a WSL2 dev artifact, not a real-host bug; genuine liveness is Phase-8 production hardening. See *Decisions made* | — |
| P6.5 | #186 board rendering ✅ PR #204 | See the state | P6.4 |
| P6.6 | #187 AwaitingInput UI + legality ✅ PR #207 | Core-loop: `Investigate` opens a skill-test commit window (`AwaitingInput`), so this precedes the action controls | P6.5 |
| P6.7a | #188 core-loop action controls ✅ PR #208 | Toy scenario clickable to a **Won** resolution | P6.6 |
| P6.7b | #189 combat/edge action controls ✅ PR #209 | Rounds out the action surface (combat/Lost walk) | P6.7a |
| P6.8 | #190 resolution + closing demo ✅ PR #210 | Milestone close | all |

P6.1 and P6.2 both touch `server`; sequence to avoid churn. P6.3 is
independent and can land any time before P6.4. P6.4a
([#199](https://github.com/talelburg/eldritch/issues/199), trunk
hot-reload) shipped before the content UI. P6.4b
([#198](https://github.com/talelburg/eldritch/issues/198), WS liveness)
was **deferred to Phase 8 and closed** — see *Decisions made*. P6.5
(#186, board rendering ✅ PR #204) shipped the read-only board and P6.6
(#187, `AwaitingInput` UI + legality ✅ PR #207) shipped the interaction
plumbing, P6.7a (#188, core-loop action controls ✅ PR #208) shipped the
action buttons, and P6.7b (#189, combat/edge controls ✅ PR #209) added
Fight/Evade/Draw. P6.8 (#190, resolution surfacing + closing demo
✅ PR #210) shipped the Won/Lost banner, the resolution legality gate, and
the closing-demo doc — the milestone is complete.

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

Settled while scoping P6.4b (#198), then **deferred to Phase 8**:

- **WS liveness (heartbeat + graceful shutdown) was deferred, and #198
  closed.** The symptom that motivated it — Ctrl-C the server with a tab
  open and the client stays `connected` instead of `reconnecting` — is a
  **WSL2 `localhost`-forwarding artifact**, not a real-host failure. On a
  normal host a server death sends TCP `FIN`/`RST` and the existing
  reconnect-on-close fires; the WSL2 relay (Windows browser → WSL server)
  can leave the socket half-open. So #198 is **not load-bearing for any
  Phase-6 acceptance** (the stated "reconnect" is tab-close-and-reopen →
  localStorage rejoin, which works). The genuine need a heartbeat
  addresses — **client-side** silent drops (wifi loss, sleep, flaky
  mobile) — is Phase-8 production/multiplayer hardening, captured in the
  phase-8 doc.
- **Finding for the Phase-8 `leptos-use` evaluation:** verified against
  the source that `leptos-use`'s `use_websocket` heartbeat is
  **send-only** — it emits a periodic frame but does **not** detect a
  missing response / close on a pong-timeout, so it would *not* have
  provided the liveness detection by itself. Its URL is also fixed at
  call time (no reactive URL), which complicates our stale-id
  reconnect-to-a-new-game flow. Both feed the Phase-8 adopt-vs-hand-roll
  (and `leptos-ws-pro`) decision.

Settled implementing P6.5 (PR #204):

- **`leptos-use` deferred again — the read-only board did not trigger
  it.** P6.4 flagged P6.5's "responsive board layout" as a candidate
  adopt point (`use_media_query` / `use_element_size`). It isn't one: a
  read-only text board branches no component trees on viewport size, so
  any responsiveness is plain CSS (flexbox/grid + media queries) with
  zero Rust deps. Next concrete candidate stays Phase-8 auth
  (`use_cookie`); the adopt decision rides with the Phase-8
  `leptos-use` evaluation noted above.
- **Headless component tests share one browser page; don't clear
  `<body>`.** `wasm-bindgen-test` runs every `#[wasm_bindgen_test]` in a
  single page and `mount_to_body` *appends*, so DOM accumulates across
  tests — a test asserting an element is *absent* must scope to its own
  mounted subtree (the empty-board test selects the last
  `.board` section). `set_inner_html("")` on `<body>` is **not** an
  option: it deletes the harness's own status elements and the runner
  then reports "failed to detect test as having been run." Load-bearing
  for every later interaction test (P6.6+).

Settled implementing P6.6 (PR #207):

- **`AwaitingInput` carries no machine-readable variant discriminator, so
  the client renders `CommitCards` only.** `InputRequest` is the
  "Phase-1 minimal shape" (a free-text `prompt` + opaque `ResumeToken`);
  nothing tells the client which of the seven `InputResponse` variants a
  prompt wants, and the toy scenario only ever emits the skill-test
  commit prompt. Routing the other six off prompt-string heuristics would
  be brittle and speculative. **So P6.7's action controls need not handle
  any other input variant**; the structured discriminator (client renders
  the right control for any variant) is [#205](https://github.com/talelburg/eldritch/issues/205)
  (Phase 7, when real cards first emit `PickIndex`/`PickInvestigator`/…).
- **Legality gating is coarse and keys off the *outcome*, not engine
  preconditions.** `web::legality::enabled_controls` gates on the
  `AwaitingInput` pause, the `mulligan_pending`/`mythos_draw_pending`
  cursors, and the phase — it does **not** mirror resources, action
  budget, or clue presence (the server stays authoritative and rejects
  illegal actions). **P6.7 binds its buttons' `disabled` to this set** as
  a UX affordance, not a correctness gate. Because the pause keys off the
  `AwaitingInput` outcome, it covers every engine suspension mode (not
  just the commit window). Richer "show the player exactly what's legal"
  affordances are [#206](https://github.com/talelburg/eldritch/issues/206)
  (Phase 8).

Settled implementing P6.7a (PR #208):

- **Action affordances live in a dedicated controls panel; `board.rs`
  stays read-only.** `ActionControls` (`crates/web/src/controls.rs`)
  renders the buttons and inline Move/PlayCard pickers; it does not make
  board locations or hand cards directly clickable. Making the board
  itself interactive (click a location to move, a card to play) is the
  preferred surface for real player UX and is **deliberately deferred** —
  a future-phase change, not an oversight. Future UI PRs adding richer
  interaction should weigh promoting interactivity into the board against
  extending the panel. The Mulligan multi-select is kept separate from the
  P6.6 commit window (`input.rs`); the extract-into-a-shared-component
  trigger is a third multi-select-hand use (the deferred upkeep discard
  prompt, #205).
- **`StartScenario` is the browser entry point, gated on `round == 0`.**
  The server hands a freshly created game the raw `setup()` state (phase
  Mythos, round 0, empty hands), whose only legal action is
  `StartScenario`. `enabled_controls` keys that off `round == 0` (the
  round counter only increments from 1, so it uniquely marks the pre-start
  state). This was a post-review fix: the initial cut omitted
  `StartScenario`, so the client loaded with every control disabled and no
  way to begin — and the headless tests masked it by building in-progress
  states with the builder's default `round 0`, an impossible real state.

Settled implementing P6.7b (PR #209):

- **Fight/Evade use the empty-picker (Move) pattern, not hide-unless-
  engaged.** The two controls are enabled by `Phase::Investigation` like
  any other core-loop action, and a single parameterized `enemy_picker`
  renders one target button per enemy with `engaged_with == Some(active)`
  — zero buttons when none are engaged, exactly as `move_picker` renders
  nothing with no connections. This keeps the enemy scan out of the
  legality layer (which stays phase-coarse) and out of any new
  conditional-render path. A future combat-UI PR weighing "only show
  Fight when a target exists" should treat that as a deliberate UX change,
  not a gap. `Fight`/`Evade` are named-field struct variants, so they're
  adapted to the picker's `fn(InvestigatorId, EnemyId) -> PlayerAction`
  constructor slot via thin `fight_action`/`evade_action` wrappers.
- **Fight/Evade are wired + headless-tested but not reachable in the live
  toy scenario yet.** `synthetic::setup()` seeds the encounter deck with
  only `_synth_treachery`; `_synth_enemy` exists but is pushed onto the
  deck only by integration tests, so no enemy ever spawns/engages through
  in-browser play and the combat pickers stay empty. The headless tests
  build an engaged-enemy state directly. Making combat reachable
  (seeding the enemy so a draw spawns + engages it) is **deferred to P6.8**
  (#190) — the closing demo's *Lost* path runs through agenda doom, not
  combat, so this is demo polish, not a milestone blocker. Draw *is*
  reachable now (rendered in Investigation like Investigate/AdvanceAct).

Settled implementing P6.8 (PR #210):

- **Resolution is surfaced read-only and dominates legality.** The
  Won/Lost banner is a `resolution_banner` helper in the read-only
  `board.rs` (phase_bar pattern), not a control; and `enabled_controls`
  gates on `game.resolution.is_some()` **first**, before every
  cursor/pause/phase — a latched resolution is terminal, so no action is
  clickable once the scenario ends (the server rejects them regardless;
  this is the UX affordance).
- **Live combat in the toy scenario was deliberately not built —
  superseded by Phase 7, not a tracked TODO.** P6.8 is the milestone
  close, so there is no later Phase-6 slot to defer into, and the
  synthetic scenario is throwaway (D5: swapped when Phase 7 lands The
  Gathering). Phase 7's real encounter cards spawn enemies through
  ordinary play, making Fight/Evade reachable for free — which is why the
  phase-7 doc carries no combat line item. The only cost: the Phase-6
  closing demo (`docs/demos/phase-6-toy-scenario.md`) shows the Lost path
  via agenda doom, not a live Fight/Evade click.

## Open questions

None. The legality-gating surface is settled (P6.6, ✅ PR #207) — see
*Decisions made*.

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
