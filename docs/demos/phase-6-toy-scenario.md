# Phase 6 closing demo — synthetic toy scenario

A solo browser playthrough of the Phase-4 synthetic toy scenario to a
visible resolution, plus the reconnect path. This is the Phase-6
milestone-close demo (#190).

## Run it

Single-port build (what production serves):

    cd crates/web && trunk build
    cargo run -p server            # serves API + WS + the bundle on :8000

Open <http://localhost:8000>. The client calls `POST /games`, opens the
WebSocket, and the board renders the freshly-set-up scenario (phase
Mythos, round 0).

## Won path

1. **Start scenario** — the only enabled control at round 0. Click it;
   hands are dealt and the mulligan window opens.
2. **Mulligan** — keep the opening hand (or mulligan), resolving the
   mulligan window into the Investigation phase.
3. **Investigate** — the starting location holds 4 clues and the demo's
   chaos bag is a single +0 token, so each Investigate (intellect 3 vs
   shroud 2) succeeds and moves one clue to you. You have 3 actions per
   turn; **End turn** to start a new one (the round cycles through
   Mythos/Enemy/Upkeep — **Draw encounter** when prompted) and keep
   investigating until you hold 4 clues.
4. **Advance act** — the act deck has two cards, each needing 2 clues.
   Advance once (spends 2, moves to the final act), then **Advance act**
   again (spends 2) to advance past the terminal card, which latches
   `Resolution::Won { id: "demo" }`.
5. The board shows the **"Scenario won — demo"** banner and every action
   control goes disabled.

## Lost path

1. **Start scenario**, resolve the mulligan as above.
2. Instead of advancing the act, end turns and let the Mythos phase
   accumulate agenda doom. The synthetic agenda's doom threshold is 2.
3. When doom reaches the threshold and the agenda advances past its
   terminal card, the engine latches `Resolution::Lost { reason: "agenda" }`.
4. The board shows the **"Scenario lost — agenda"** banner and the
   controls go disabled.

## Reconnect mid-scenario

With a scenario in progress, **close the browser tab**, then reopen
<http://localhost:8000>. The game id persisted in `localStorage` drives a
reconnect: the server replies `Hello`, and the board is restored to the
exact in-progress state — including any in-flight `AwaitingInput` (e.g. a
skill-test commit window). No action is lost.

## Not exercised: live combat

Fight/Evade are wired and headless-tested but **not reachable** in this
demo: the synthetic scenario seeds no enemy, so none ever spawns/engages
through in-browser play. This is superseded by Phase 7's real encounter
content (real cards spawn real enemies through ordinary play), not an open
TODO. The Lost path above runs through agenda doom, which is fully live.
