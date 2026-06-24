# `PlayFromHand` frame (Slice D, #423 Task 4) — Design

> **Status:** design, awaiting implementation plan.
> **Branch:** `engine/effect-callsite-migration` (Slice D / #423). Follows the
> skill-test-outcome timing-point work; this is original Slice D **Task 4**.
> **Supersedes** the `PlayFromHand` sketch in
> `docs/superpowers/plans/2026-06-23-effect-frame-callsite-migration.md` (Task 4):
> that used a two-stage frame (`Dispose` / `AfterEnterWindow`) with a manual
> `open_queued_reaction_window`; this design is single-shot — the drive loop
> opens the after-enters-play window itself.

## Problem

Two production sites still run hand-play effects **synchronously** via
`apply_effect` — the last non-test callers of the wrapper Slice D is retiring:

- **`cards.rs::complete_play`** — runs a played card's `OnPlay` effects, then for
  an **asset** removes it from hand, mints its in-play instance, pushes it to
  `cards_in_play`, emits the `EnteredPlay` timing event, and manually opens the
  after-enters-play reaction window; for an **event** it leaves the discard to
  the apply-loop `flush_pending_played_event`. Shared by the Fast path (inline)
  and the non-Fast path (resumed after the attack-of-opportunity loop via
  `resume_play_card`).
- **`reaction_windows.rs::play_fast_event`** — runs a Fast event's matched
  `OnEvent` effect, then eagerly flushes the event to discard.

Both must become **push-and-return**: push the effect for the global `drive`
loop, with the type-disposal moved into an enclosing frame the loop runs when
the effect frame pops — the same shape as `EncounterCard` (Task 2).

A secondary cleanup falls out: `complete_play` currently *manually* opens the
`EnteredPlay` reaction window (`emit_event` + an inline
`open_queued_reaction_window` check). Once disposal is a drive-loop frame, the
loop's existing window arm opens it — so the manual open is retired.

## Design

### The frame

```rust
// in state/game_state.rs, a Continuation variant
PlayFromHand {
    investigator: InvestigatorId,
    code: CardCode,
    /// Hand slot of an asset still in hand (assets enter play at disposal).
    /// Unused for an event — `begin_event_play` already removed it and stashed
    /// it in `pending_played_event`.
    hand_index: u8,
},
```

Single-shot — **no `stage` field**. Pushed **below** the `OnPlay`/`OnEvent`
effect, so the stack reads (top→bottom) `[effect][PlayFromHand][…]`. It never
awaits input.

### Disposal — `cards::dispose_play_from_hand`

Mirrors `encounter::dispose_encounter_card_if_top`: peek the `PlayFromHand`
frame, clone its fields, **pop it**, then dispose by destination
(`resolve_play_target(&code)`):

- **Event (`PlayDestination::Discard`)** → `flush_pending_played_event(cx)` (the
  card was removed from hand + stashed by `begin_event_play`), return `Done`.
- **Asset (`PlayDestination::InPlay`)** → remove from hand at `hand_index`,
  `threat_area::new_in_play_instance`, push to `cards_in_play`, then
  `emit_event(TimingEvent::EnteredPlay { instance, controller })`, return `Done`.

Because the frame is **popped before** `emit_event`, a reaction window that
`emit_event` queues (`EnteredPlay` is reaction-only — Research Librarian 01032)
lands on top of the stack where `PlayFromHand` was; the **drive loop's existing
window arm opens it** on the next iteration. No manual `open_queued_reaction_window`,
and no second stage to guard against re-running disposal after the window
closes (the frame is already gone).

### Drive-loop arm (`mod.rs`)

Add `Some(Continuation::PlayFromHand { .. }) => { match cards::dispose_play_from_hand(cx) { … } }`,
mirroring the `EncounterCard` arm (`mod.rs:243`): on `Done` fall through (the
loop re-evaluates `last()` and opens any queued `EnteredPlay` window); propagate
`AwaitingInput` (none expected from disposal itself) / `Rejected`.

Add a defensive `Some(Continuation::PlayFromHand { .. }) => Rejected` arm in
`resolve_input` (mirroring the `EncounterCard` arm, `mod.rs:504`) — the frame
never awaits input.

### Call-site changes

- **`complete_play`** (shared, so both the Fast inline path and the non-Fast
  `resume_play_card`-after-AoO path inherit it): replace the synchronous
  `apply_effect` loop + the asset enter-play tail + the manual window open with:
  combine the `OnPlay` effects into one `Effect::Seq`, push a `PlayFromHand`
  frame, `push_effect` the `Seq` above it, return `Done`. (The `CardPlayed`
  announce, action spend, and AoO already happened in `play_card`/before — no
  change there.)
- **`play_fast_event`**: replace the `match apply_effect(…)` block with: push a
  `PlayFromHand` frame (above the live reaction window), `push_effect` the
  `OnEvent` effect, return `Done`. Drop the eager `flush_pending_played_event`.

### The single-`CardDiscarded` invariant

`PlayFromHand`'s disposal is the **single** event-flush site. It runs *during*
this apply's `drive` loop (right after the effect frame pops), so it flushes
before any later same-apply suspension — which was the only reason
`play_fast_event` flushed eagerly (a window-close cascade into the Mythos draw
or an upkeep discard, both `AwaitingInput`, would otherwise strand the event).
Consequences:

- Remove the eager `flush_pending_played_event` from `play_fast_event`.
- Remove the apply-loop `flush_pending_played_event` call (it is now always a
  no-op — `pending_played_event` is cleared by disposal before the loop ends).
  Keep the `flush_pending_played_event` function itself (disposal calls it).

Find every `flush_pending_played_event` caller and confirm exactly one path
flushes a given played event. A test asserts a single `CardDiscarded` per event
play.

## `hand_index` stability (assumption, unchanged from today)

The asset is removed from hand at `hand_index` at disposal, *after* its `OnPlay`
effect resolved — identical to today's `complete_play` (which removes after the
synchronous `apply_effect`). No in-scope asset has an `OnPlay` effect that
reorders the hand (the suspending/hand-touching `OnPlay` cards are all events,
disposed via `pending_played_event`, not `hand_index`). Behaviour-preserving.

## Why it's safe (behaviour preservation)

`crates/cards/*` is the regression net — untouched. Load-bearing cards:

- **Dynamite Blast 01024** — suspending `OnPlay` (location choice) as a non-Fast
  event, *and* as a suspending Fast event. Proves the pushed effect suspends and
  resumes, with the event flushed exactly once on completion.
- **Research Librarian 01032** — after-enters-play reaction window. Proves the
  drive loop opens the `EnteredPlay` window after asset disposal (the retired
  manual open).
- **Emergency Cache 01088** — non-Fast event, `OnPlay GainResources`. Proves the
  normal event path (announce → AoO → `complete_play` → push → flush).
- **Machete 01020 / assets** — enter play (no `OnPlay`), proving asset disposal.

The only intended change is internal (effects pushed not run synchronously; the
window opened by the loop not manually) — **no observable event-order or
state change** for these cards; their assertions stay as-is.

## New tests

- **Single discard:** a non-Fast event play (Emergency Cache 01088) emits exactly
  one `CardDiscarded { from: Zone::Hand }` and leaves no `pending_played_event`.
- **Fast event single discard:** a Fast event played in a reaction window
  (Dynamite Blast 01024 as Fast) emits exactly one `CardDiscarded`.
- **Asset enter-play through the frame:** an asset play results in the instance
  in `cards_in_play` + an `EnteredPlay`-driven reaction window opening when a
  matching reaction is in play (Research Librarian 01032), and none when not.

## Out of scope (deferred)

- Removing the asset from hand at *announce* time (holding it in the frame like
  events) to harden against a hypothetical hand-reordering asset `OnPlay`. Not
  needed in scope; keeps `complete_play`'s current timing.
- The richer mid-action invalidation `TODO(#417)` on `complete_play` (rolling
  back a whole play on a resumed `OnPlay` reject) — unchanged by this task.

## File map

| File | Change |
|---|---|
| `crates/game-core/src/state/game_state.rs` | add `Continuation::PlayFromHand { investigator, code, hand_index }` (+ `is_phase_anchor`/`awaits_input` classification like `EncounterCard`) |
| `crates/game-core/src/engine/dispatch/cards.rs` | `complete_play` → push `PlayFromHand` + `push_effect`; add `dispose_play_from_hand`; drop the manual window open; switch the `apply_effect` import to `push_effect` |
| `crates/game-core/src/engine/dispatch/reaction_windows.rs` | `play_fast_event` → push `PlayFromHand` + `push_effect`; drop the eager flush; drop the `apply_effect` import (last user) |
| `crates/game-core/src/engine/dispatch/mod.rs` | drive-loop `PlayFromHand` arm (mirrors `EncounterCard`); `resolve_input` defensive-reject arm; remove the apply-loop `flush_pending_played_event` call |
| `crates/cards/tests/play_card.rs` (or sibling) | new tests (single discard; asset enter-play window) |
