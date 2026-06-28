# Event log panel (left of the board) — design

**Date:** 2026-06-28
**Issue:** [#505](https://github.com/talelburg/eldritch/issues/505)
**Status:** approved (brainstorm) — pending implementation plan

## Goal

Add a read-only event-log view to the **left of the board** that shows the full
game's event history, newest at the bottom, grouped by the action that produced
each batch. A developer-facing debugging aid: "what happened, when, and was it a
bug?"

## Decisions (settled in brainstorm)

- **Entry format: raw `Debug` (`{:?}`).** Every event renders verbatim with all
  fields, e.g. `EnemyEngaged { enemy: EnemyId(100), investigator: InvestigatorId(1) }`.
  No human-readable mapping to maintain; faithful and complete. Codes/ids show
  raw (no card-name enrichment).
- **History: full game, accumulated.** Every event since the game started,
  cleared on `Hello` (new game / reconnect). Newest batch at the **bottom**;
  the scroll container **auto-scrolls to the bottom** on update.
- **Grouping: by action, headed with the menu-choice label.** Each `Applied`
  batch is one group. The header is the **human label of the menu choice the
  player submitted** (e.g. `▸ Play 01059 from hand`, `▸ Move to Cellar`), not the
  raw wire action.

### Why the header comes from the client, not the protocol

Every player action over the wire is a single opaque variant —
`PlayerAction::ResolveInput { response }`, where `response` is
`Confirm | Skip | PickSingle(OptionId) | PickMultiple { selected }`. There is **no
`Move`/`PlayCard` action on the wire**; those are engine-internal `TurnAction`s.
So a protocol-carried action would render as
`ResolveInput { response: PickSingle(OptionId(2)) }` — opaque (the `OptionId` only
means something against the menu that was on screen).

The genuinely useful label — the chosen menu option's text — is known **on the
client** at submit time (`InputRequest::options: Vec<ChoiceOption { id, label }>`).
So the client captures it when it submits and pairs it with the resulting batch.
No protocol or server change is needed.

This pairing is exact in solo play: there are **zero server-initiated applies**
(`crates/server/src/ws.rs` calls `session.apply()` only in response to a client
`Submit { action }` — no timers, no auto-advance), and the UI is turn-based (one
submit in flight at a time), so the next `Applied` the client receives is the
result of the submit it just made. (Multiplayer — another client's action
arriving with no local pending label — is out of scope; it falls back to a
generic header.)

### Framework events

Framework events (Mythos/Upkeep/enemy-phase) are **not** special-cased. They
cascade synchronously from whatever submit drove them, pausing at each
`AwaitingInput`. So ending a turn produces a batch headed by that submit's label
(e.g. `▸ End turn`), then each continuation pause is its own batch headed by the
choice that resumed it (e.g. `▸ Skip`). The engine already emits
`PhaseStarted`/`PhaseEnded`/`TurnEnded`/`AgendaAdvanced`/… as ordinary events, so
framework boundaries are legible inline. No phase sub-segmentation.

## Architecture / components

A thin client-only slice: store accumulation + a label helper + a new view +
layout. No protocol or server changes.

### 1. Client store — `crates/web/src/store.rs`

Add an accumulated log and a one-shot pending header:

```rust
/// One applied submit's worth of events, for the event-log view.
#[derive(Debug, Clone, PartialEq)]
pub struct LogBatch {
    /// Human label of the menu choice that produced this batch
    /// (e.g. "Play 01059 from hand"); a generic fallback when unknown.
    pub header: String,
    pub events: Vec<Event>,
}

pub struct ClientState {
    // ...existing fields...
    /// Full accumulated event history, grouped per applied submit, oldest
    /// first. Cleared by `Hello` (new game / reconnect), like `last_events`.
    pub log: Vec<LogBatch>,
    /// The header label for the *next* `Applied` batch, set by the input view
    /// at submit time and consumed (taken) when that batch arrives. Cleared on
    /// `Rejected` (the submit produced no batch) and `Hello`.
    pub pending_label: Option<String>,
}
```

Reducer changes (`reduce`):
- `Applied { events, .. }`: `let header = state.pending_label.take().unwrap_or_else(|| "(action)".into()); state.log.push(LogBatch { header, events: events.clone() });` then keep the existing `last_events = events` (the skill-test panel still reads it) and difficulty capture. One events-vec clone per submit — negligible.
- `Rejected { .. }`: also `state.pending_label = None;` (the rejected submit yields no batch, so its label must not bleed onto the next one) — alongside the existing `last_rejection` set.
- `Hello { .. }`: `state.log = Vec::new(); state.pending_label = None;` (alongside the existing clears).

This reducer is the **primary testable unit** (runs on the native test target —
`store` is compiled on both targets).

### 2. Label helper + view — `crates/web/src/event_log.rs` (new, both targets)

A pure, DOM-free helper maps a submit to its header so it is unit-testable on the
native target:

```rust
/// The event-log header for a submitted response, given the prompt it answered.
/// - `PickSingle(id)` → that option's `label` (fallback `"Pick <n>"` if absent).
/// - `Confirm`        → `"Confirm"`.
/// - `Skip`           → `"Skip"`.
/// - `PickMultiple`   → `"Commit <n> card(s)"`.
pub(crate) fn response_label(
    request: &InputRequest,
    response: &InputResponse,
) -> String { /* ... */ }
```

`EventLogView` reads the store and renders the log oldest-first (newest at the
bottom):

```
<aside class="event-log">
  <h2>"Event log"</h2>
  <div class="log-scroll" node_ref=scroll_ref>
    // for each batch, oldest first:
    <div class="log-batch">
      <div class="log-action">"▸ " {batch.header}</div>
      <div class="log-event">{event_line}</div>   // one per event, "{:?}"
    </div>
  </div>
</aside>
```

`event_log` is declared `pub mod event_log;` (both targets) so `response_label`'s
tests run under `cargo test`. The component body is target-agnostic except the
**auto-scroll** effect — a `#[cfg(target_arch = "wasm32")]` effect keyed on the
total batch/event count that sets `scroll_ref.scrollTop = scrollHeight` so the
newest line is visible. The scroll effect (needs a real DOM) is the one piece not
unit-tested; the pure `response_label` and the store reducer carry the coverage.

### 3. Input view captures the header — `crates/web/src/input.rs`

`AwaitingInputView` is the sole gameplay submit site (wasm-only). At each of its
four submit sites (Skip / PickSingle option button / Confirm / PickMultiple
commit), set the pending label in the store immediately before sending:

```rust
store.update(|s| s.pending_label = Some(crate::event_log::response_label(&request, &response)));
let _ = tx.unbounded_send(ClientMessage::Submit { action: PlayerAction::ResolveInput { response } });
```

`store` (an `RwSignal`, `Copy`) is already read at the top of the component, so it
can be captured into the click closures. For the `PickSingle` buttons the chosen
option's `label` is already in scope, so the header can be set directly from it
(identical to `response_label`'s `PickSingle` arm) to avoid cloning the request
into every button closure; the other three sites call `response_label`.

### 4. Layout — `crates/web/src/app.rs` + `crates/web/style.css`

Wrap the log and board in a flex row, log first (left):

```rust
view! {
    <main>
        <h1>"Eldritch"</h1>
        <div class="layout">
            <crate::event_log::EventLogView/>
            <BoardView/>
        </div>
        // picker / skill-test / input overlays unchanged
    </main>
}
```

`EventLogView` renders on both targets (like `BoardView`), so it sits outside the
existing `#[cfg(target_arch = "wasm32")]` overlay block. CSS:
`.layout { display: flex; gap: 1rem; align-items: flex-start; }`,
`.event-log { flex: 0 0 auto; width: 22rem; }`,
`.log-scroll { max-height: 80vh; overflow-y: auto; font-family: monospace;
font-size: 0.8rem; }`, with light per-batch separation and a distinct
`.log-action` weight. Exact values are cosmetic and may be tuned during build.

## Data flow

```
prompt (AwaitingInput) shown by AwaitingInputView
  → user picks option → set store.pending_label = response_label(request, response)
  → Submit{ResolveInput{response}} → server apply → broadcast Applied{state, events, outcome}
  → client transport → store.reduce(Applied): header = pending_label.take(); push LogBatch{header, events}
  → EventLogView re-renders (oldest→newest) → auto-scroll to bottom
```

## Testing

- **Store reducer** (`store.rs`, native): `Applied` appends a `LogBatch` whose
  `header` is the taken `pending_label` (and the generic fallback when it is
  `None`); consecutive `Applied`s accumulate in order; `Rejected` clears
  `pending_label` without pushing a batch; `Hello` clears `log` and
  `pending_label`; `last_events`/difficulty behavior unchanged.
- **Label helper** (`event_log.rs`, native): `response_label` returns the option
  label for `PickSingle`, `"Confirm"`/`"Skip"` for those, and
  `"Commit <n> card(s)"` for `PickMultiple`; `PickSingle` with an unknown id
  falls back to `"Pick <n>"`.
- **Gauntlet:** fmt/clippy/test/doc + wasm-build/wasm-clippy/wasm-test (the new
  component must compile to wasm; the scroll effect is wasm-only).

## Out of scope (YAGNI)

- No filtering, search, or collapsing.
- No per-event timestamps (insertion order is the signal).
- No card-name / id enrichment in event bodies (raw codes per the format
  decision); headers use the menu label the client already has.
- No phase sub-segmentation within a batch (inline phase events suffice).
- No persistence across reload (log is rebuilt from `Hello` onward).
- No multiplayer header attribution (a non-local submit's batch gets the generic
  fallback header).

## Open questions

None blocking. Cosmetic CSS values (width, max-height, separators) are tunable
during implementation.
