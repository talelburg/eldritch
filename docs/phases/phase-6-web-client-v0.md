# Phase 6 — Web client v0

## Status

📐 Architecture only. No issues filed.

## Goal

Toy scenario playable in browser, solo.

## Decisions made

From the 2026-05-01/02 strategy phase:

- **Frontend stack:** Rust + WASM, Leptos as the framework (leading candidate in the Rust UI space as of 2026-05). Language cohesion (same Rust everywhere, shared `game-core` types) over UI-ecosystem breadth.
- **Trade-off acknowledged:** smaller UI ecosystem than React, larger bundles, slower iteration on UI work. Acceptable for the project's hobby-scale audience.
- **Exit ramp:** if Rust/WASM proves to hinder the project too much, pivot to React. Plan accordingly — keep the boundary between `game-core` (forever, used by both server and client) and the UI layer (potentially replaceable) clean so a future pivot wouldn't require rewriting the engine.
- **CSR-only, no SSR.** App is behind auth; no SEO concern. Dev loop uses Trunk's dev server proxying API/websocket to the server.
- **Card art:** link to ArkhamDB's CDN URLs with text-only fallback. Do NOT re-host art (their CDN, their problem; also keeps us off the radar legally).
- **The `web` crate is the only one that compiles to wasm32 in production.** `game-core` happens to also compile to wasm32 because it's pure — that's load-bearing (server and client share engine code).

## Open questions

⏳ **Scoping TBD.** Issues haven't been filed. When Phase 5 closes, file:

- **Component model.** What's the granularity of a Leptos component? Per-card? Per-zone (hand, in-play, discard)? Per-investigator?
- **State management.** How does the client maintain a derived view of `GameState` from the event stream? Direct `apply()` on a local copy?
- **Player-input UX.** Click a card → submit `PlayerAction::PlayCard`. Drag-drop for commits? Hover for card text? Card-zoom on focus?
- **Choice UX.** When the engine emits `AwaitingInput`, present the choice via modal? Inline? Per-prompt-kind component dispatch?
- **Card image strategy.** Link to ArkhamDB CDN URLs; cache locally? Text-only fallback for missing art?
- **Auth UX.** Phase 8 sets up OAuth; Phase 6 needs at least a stub login-or-bust gate (or be served behind the auth middleware that Phase 8 lands).
- **Two-process dev loop ergonomics.** Document the standard developer dance (server + Trunk concurrently, what to restart when, common gotchas).

## Dependencies

- Phase 5 (server + persistence) — the client connects to and depends on a working server.
- Phase 0 hello-world Leptos page (already shipped) gives us a Trunk-buildable skeleton.

## What "done" looks like

- Phase-4's toy scenario plays in the browser, solo, with the human-facing actions (Move / Investigate / PlayCard / EndTurn / Mulligan) all clickable.
- The scenario runs to a resolution; resolution effects are visible in the campaign log (or wherever Phase 4 lands campaign-log surfacing).
- Reconnect-mid-scenario works at the UX level — closing the tab and reopening picks up where the session left off.
