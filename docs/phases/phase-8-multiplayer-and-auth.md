# Phase 8 — Multiplayer + auth

## Status

📐 Architecture only. No issues filed.

## Goal

Two-machine multiplayer playthrough of The Gathering.

## Decisions made

From the 2026-05-01/02 strategy phase:

- **Multiplayer model:** synchronous-only. All players online together (same room or Discord call assumed). If someone is missing, the group can't play. Don't invest in async-specific UX (turn-deadline reminders, per-turn push notifications).
- **The event log gives us resume + undo + debugging for free; async is just not a target use case.**
- **Auth (v1):** invite-only via GitHub / Google OAuth + a manual email allowlist. No signup flow, password reset, email verification, or abuse handling at v1 — those are the first things to revisit if scope ever expands.
- **Audience and posture:** hobby project for the user and a small group of friends. Not commercial, not a public service, no monetization, no SLA. Build to a "make game night work" bar.
- **Legal posture:** repo is public under MIT; "unofficial fan tool" disclaimer in README. No re-hosted card art. The "Eldritch" name avoids the "Arkham Horror" trademark — keep it that way.

## Open questions

⏳ **Scoping TBD.** When Phase 7 closes, file:

- **Session/group model.** A "group" is a stable set of players; a "session" is one scenario play. Database tables for both.
- **Lobby UX.** How do players gather before starting a scenario? Pre-game-share-link? Group-saved-state?
- **Sync semantics.** When inv1 takes an action, inv2's client updates by replaying the broadcast events. Order of player inputs across the websocket has to be deterministic — server is authoritative on ordering.
- **Disconnection mid-scenario.** What if a player drops? Can the group continue, or is the scenario paused until they reconnect?
- **WebSocket liveness (deferred from Phase 6, [#198](https://github.com/talelburg/eldritch/issues/198)).** The Phase-6 client reconnects only on an *observed* socket close. That's fine on a real host (a server death sends TCP `FIN`/`RST`), but it can't detect a **client-side silent drop** — wifi loss, laptop sleep, flaky mobile — where the connection goes dead with no close frame. Multiplayer needs this: a player whose socket silently dies should be detected (for the disconnection handling above), and each client should notice a dead server and reconnect. Scope to file: (a) an application-level heartbeat with **pong-timeout detection** (`protocol` `Ping`/`Pong` + a server echo + a client timer that closes on no-reply — browsers don't expose protocol ping/pong to JS, so it must be app-level); (b) server **graceful shutdown** on `SIGINT` (close WS frames before exit) for clean redeploys. **`leptos-use` / `leptos-ws-pro` evaluation belongs here** (this is the real "second concrete need" trigger from the Phase-6 deferral). Note from Phase-6 scoping: `leptos-use`'s `use_websocket` heartbeat is **send-only** (no pong-timeout detection) and its URL is fixed at call time — so it wouldn't supply the detection by itself; weigh that against `leptos-ws-pro` (a Leptos WS lib advertising reconnection + heartbeat) and hand-rolling.
- **Cross-investigator UI.** Each player sees their own hand, the shared board state, and the other players' public information.
- **OAuth flow.** GitHub + Google. Manual email allowlist gate (stored where? Config file, env var, or DB table?).
- **Out-of-game communication.** No. Players use Discord etc. The app doesn't try to be a chat platform.

## Dependencies

- Phase 5 (server + persistence) — the websocket sync layer.
- Phase 6 (web client v0) — the UI to host multiplayer.
- Phase 7 (The Gathering) — a real scenario to multiplayer-test against.

## What "done" looks like

Two browsers on two machines, each logged in via OAuth, play through The Gathering together. The action log is consistent across both; either can disconnect and reconnect mid-game. Server gracefully handles the dropped session.
