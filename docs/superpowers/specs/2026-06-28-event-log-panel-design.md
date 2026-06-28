# Event log panel (left of the board) — design

**Date:** 2026-06-28
**Issue:** [#505](https://github.com/talelburg/eldritch/issues/505)
**Status:** approved (brainstorm) — pending implementation plan

## Goal

Add a read-only event-log view to the **left of the board** that shows the full
game's event history, newest at the bottom, grouped by the player action that
produced each batch. A developer-facing debugging aid: "what happened, when, and
was it a bug?"

## Decisions (settled in brainstorm)

- **Entry format: raw `Debug` (`{:?}`).** Every event renders verbatim with all
  fields, e.g. `EnemyEngaged { enemy: EnemyId(100), investigator: InvestigatorId(1) }`.
  No human-readable mapping to maintain; faithful and complete. Codes/ids show
  raw (no card-name enrichment).
- **History: full game, accumulated.** Every event since the game started,
  cleared on `Hello` (new game / reconnect). Newest batch at the **bottom**;
  the scroll container **auto-scrolls to the bottom** on update.
- **Grouping: by player action.** Each `Applied` batch is one group with a
  header phrased as the *trigger* — `▸ from {action:?}` — so it reads as
  causation ("these events were caused by EndTurn"), not identity.
- **Framework events** (Mythos/Upkeep/enemy-phase) are **not** special-cased.
  They cascade synchronously from whatever action drove them (typically
  `EndTurn`, then `ResolveInput` for each continuation pause), so they group
  under that action. The engine already emits `PhaseStarted`/`PhaseEnded`/
  `TurnEnded`/`AgendaAdvanced`/… as ordinary events, so framework boundaries are
  legible inline. No phase sub-segmentation.

### Why action grouping is sound

There are **zero server-initiated applies**: `crates/server/src/ws.rs` calls
`session.apply()` only in response to a client `Submit { action }` — no timers,
no auto-advance. So every event the client ever receives is already inside an
`Applied` batch caused by exactly one `PlayerAction`. Attributing each batch to
its action is therefore exact, not a heuristic.

## Architecture / components

A thin slice across protocol → server → client store → a new view, plus layout.

### 1. Protocol — `crates/protocol/src/lib.rs`

Add `action: PlayerAction` to `ServerMessage::Applied`:

```rust
Applied {
    state: Box<GameState>,
    events: Vec<Event>,
    outcome: EngineOutcome,
    action: PlayerAction, // the submitted action that produced this batch
},
```

`PlayerAction` already derives the needed `Serialize`/`Deserialize`/`Clone`/
`Debug`/`PartialEq` (it travels in `ClientMessage`). This is a wire-format
change; the client/server already negotiate a version and surface
`ConnStatus::VersionMismatch`, so a mismatched pair fails loudly rather than
mis-parsing. Update the existing `Applied` serde round-trip test to construct
the new field.

### 2. Server — `crates/server/src/ws.rs`

The submitted `action` is already in scope at the broadcast site
(`handle_client_message`). Clone it into the broadcast (one cheap clone per
accepted action; `apply` takes the original by value):

```rust
Ok(ClientMessage::Submit { action }) => {
    let mut session = room.session.lock().await;
    let action_for_log = action.clone();
    match session.apply(action).await {
        ...
        Ok((events, outcome)) => {
            let _ = room.tx.send(ServerMessage::Applied {
                state: Box::new(session.state.clone()),
                events,
                outcome,
                action: action_for_log,
            });
            None
        }
        ...
    }
}
```

### 3. Client store — `crates/web/src/store.rs`

Add an accumulated log alongside the existing fields:

```rust
/// One applied action's worth of events, for the event-log view.
#[derive(Debug, Clone, PartialEq)]
pub struct LogBatch {
    pub action: PlayerAction,
    pub events: Vec<Event>,
}

pub struct ClientState {
    // ...existing fields...
    /// Full accumulated event history, grouped per applied action, oldest
    /// first. Cleared by `Hello` (new game / reconnect), like `last_events`.
    pub log: Vec<LogBatch>,
}
```

Reducer changes:
- `Applied { state, events, outcome, action }`: push
  `LogBatch { action, events: events.clone() }`, then keep the existing
  `last_events = events` (the skill-test panel still reads `last_events`) and
  the existing difficulty capture. One events-vec clone per action — negligible.
- `Hello`: `state.log = Vec::new()` (alongside the existing `last_events` /
  difficulty clears).

This reducer is the **primary testable unit**.

### 4. New view — `crates/web/src/event_log.rs`

`EventLogView` reads the store and renders the log oldest-first (newest at the
bottom). Formatting lives in a **pure, DOM-free helper** so it is unit-testable:

```rust
/// The lines for one batch: a `▸ from {action:?}` header followed by one
/// `{event:?}` line per event. Pure; unit-tested without a DOM.
fn batch_lines(batch: &LogBatch) -> BatchLines { /* header String + Vec<String> */ }
```

Markup (sketch):

```
<aside class="event-log">
  <h2>"Event log"</h2>
  <div class="log-scroll" node_ref=scroll_ref>
    // for each batch, oldest first:
    <div class="log-batch">
      <div class="log-action">{header}</div>      // "▸ from EndTurn"
      <div class="log-event">{event_line}</div>   // one per event, "{:?}"
    </div>
  </div>
</aside>
```

**Auto-scroll:** a wasm-only effect keyed on the total event/batch count sets
`scroll_ref.scrollTop = scrollHeight` so the newest line is visible. Gated
`#[cfg(target_arch = "wasm32")]` (uses `web-sys`); the rest of the component is
target-agnostic. The scroll effect is the one piece not unit-tested (it needs a
real DOM); the pure formatter and the store reducer carry the coverage.

### 5. Layout — `crates/web/src/app.rs` + `crates/web/style.css`

Wrap the log and board in a flex row, log first (left):

```rust
view! {
    <main>
        <h1>"Eldritch"</h1>
        <div class="layout">
            <EventLogView/>
            <BoardView/>
        </div>
        // picker / skill-test / input overlays unchanged
    </main>
}
```

CSS: `.layout { display: flex; gap: 1rem; align-items: flex-start; }`,
`.event-log { flex: 0 0 auto; width: 22rem; }`,
`.log-scroll { max-height: 80vh; overflow-y: auto; font-family: monospace;
font-size: 0.8rem; }`, with light per-batch separation and a distinct
`.log-action` weight. Exact values are cosmetic and may be tuned during build.

## Data flow

```
player Submit{action}
  → server apply → broadcast Applied{state, events, outcome, action}
  → client transport → store.reduce → push LogBatch{action, events}
  → EventLogView re-renders (oldest→newest) → auto-scroll to bottom
```

## Testing

- **Store reducer** (`store.rs`, native): `Applied` appends a `LogBatch` with the
  action + events; consecutive `Applied`s accumulate in order; `Hello` clears
  `log`; `last_events`/difficulty behavior unchanged.
- **Pure formatter** (`event_log.rs`, native): `batch_lines` produces the
  `▸ from {action:?}` header and one `{:?}` line per event, in order.
- **Protocol** (`protocol`): updated `Applied` serde round-trip includes
  `action`.
- **Gauntlet:** fmt/clippy/test/doc + wasm-build/wasm-clippy/wasm-test (the new
  component must compile to wasm; the scroll effect is wasm-only).

## Out of scope (YAGNI)

- No filtering, search, or collapsing.
- No per-event timestamps (insertion order is the signal).
- No card-name / id enrichment (raw codes per the format decision).
- No phase sub-segmentation within a batch (inline phase events suffice).
- No persistence across reload (log is rebuilt from `Hello` onward).

## Open questions

None blocking. Cosmetic CSS values (width, max-height, separators) are tunable
during implementation.
