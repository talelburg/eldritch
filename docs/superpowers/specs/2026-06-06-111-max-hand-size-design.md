# #111 — Enforce maximum hand size at upkeep (step 4.5)

**Issue:** [#111](https://github.com/talelburg/eldritch/issues/111) (`engine`, `p1-next`)
**Date:** 2026-06-06

## Goal

Enforce the standard maximum hand size: at upkeep step 4.5, each
investigator with more than 8 cards in hand discards down to 8. The
engine does not model this today, so hands grow unbounded.

## Verified rule

Rules Reference, step 4.5 (verified against
`data/rules-reference/ahc01_rules_reference_web.pdf`):

> In player order, each investigator with more than 8 cards in hand
> chooses and discards cards from his or her hand until he or she has 8
> cards remaining in hand.

Load-bearing facts:

- Cap is **8**.
- Fires at **upkeep 4.5** — after the 4.4 draw (which can push a hand
  over the cap), before 4.6 (round end / transition to Mythos).
- The investigator **chooses** which cards (hence an interactive
  prompt — `AwaitingInput`).
- Processed **in player order**.

## Scope decisions

These were settled during brainstorming and deliberately narrow the
issue's stated scope under YAGNI:

- **`const HAND_SIZE_LIMIT: u8 = 8`** — no per-investigator
  `max_hand_size` field, no constant-modifier-query wiring. No card in
  the Core/Dunwich scope modifies hand size, so the field and query
  integration the issue asks for would be unexercised. A future
  hand-size-modifying card introduces the field when it is actually
  needed.
- **New `InputResponse::DiscardCards { indices: Vec<u32> }`** — a
  single multi-pick response discards exactly the overflow in one
  round-trip (matches the issue's acceptance: "accepts a 2-card
  discard"). `u32` indices for wire-format symmetry with
  `CommitCards` / `PickIndex`; downcast at validation. A dedicated
  variant rather than reusing `CommitCards` because "commit" is the
  wrong verb for a discard and conflating them risks routing
  confusion.

## State

One new suspension field on `GameState`, mirroring the existing
`hunter_move_pending` / `spawn_engage_pending` / `mythos_draw_pending`
suspension fields:

```rust
hand_size_discard_pending: Option<HandSizeDiscard>
```

```rust
struct HandSizeDiscard {
    /// Over-cap investigators in player order, front = currently
    /// prompted. Precomputed once at step 4.5.
    remaining: Vec<InvestigatorId>,
}
```

The queue is precomputed once when step 4.5 fires: discarding only ever
shrinks the discarding investigator's own hand, so no other
investigator's over-cap status can change mid-resolution and no
recomputation is needed.

## Control flow

`check_hand_size(cx)` — today a stub at `phases.rs:489` carrying
`TODO(#111)`, called from `upkeep_resume` between 4.4
(`upkeep_draw_and_resource`) and 4.6 (`upkeep_phase_end`) — becomes:

1. Scan active investigators in turn order
   (`active_investigators_in_turn_order`) for `hand.len() > 8`.
2. **No over-cap investigators:** fall through to `upkeep_phase_end`
   exactly as today. Fully synchronous — no behavior change for the
   common case.
3. **Some over-cap investigators:** set `hand_size_discard_pending`
   with the player-order queue and surface `AwaitingInput` with a
   discard prompt for `remaining[0]`. Do **not** run
   `upkeep_phase_end` yet — that runs when the queue drains.

### Suspension plumbing (approach P2)

The discard runs inside the `upkeep_resume` window continuation, which
is currently `-> ()`, and no window continuation suspends today. To
surface the pause as `AwaitingInput` (consistent with every other
`ResolveInput` suspension — hunter / spawn / skill-test commit all
return `AwaitingInput`):

- Change `run_window_continuation` and `open_fast_window` to return
  `EngineOutcome`.
- The upkeep path (`upkeep_phase` → `open_fast_window(UpkeepBegins)`
  auto-skip inline, and `close_reaction_window_at` → continuation when
  the window was held open for a Fast play) propagates the
  `AwaitingInput` out to the apply boundary.
- The other `open_fast_window` / `run_window_continuation` callers
  (Mythos `MythosAfterDraws`, Enemy `BeforeInvestigatorAttacked` /
  `AfterAllInvestigatorsAttacked`, Investigation `InvestigationBegins`)
  do not have a suspending continuation; they `debug_assert_eq!` the
  returned outcome is `Done`, matching the existing
  `upkeep_phase_end → step_phase` assertion style.

Keeping the prompt where it is created (rather than converting
`Done → AwaitingInput` at the apply boundary) is why P2 is preferred
over the lower-plumbing "Done + pending flag" alternative used by the
round-end Mythos pause: a boundary conversion would have to synthesize
the `InputRequest` prompt far from the logic that knows what is being
asked.

## Input routing and resume

- **`resolve_input`** (`mod.rs:299`) gains a new early branch, routed
  before the reaction-window / skill-test checks like the other
  distinct suspension modes:
  ```rust
  if cx.state.hand_size_discard_pending.is_some() {
      return resume_hand_size_discard(cx, response);
  }
  ```
  plus a `debug_assert!` that it is mutually exclusive with the other
  `*_pending` modes (they arise in different phases).
- **Apply-top guard** (`mod.rs:64–117`) gains a matching arm so any
  non-`ResolveInput` action is rejected while a discard is pending,
  mirroring the hunter / skill-test guards.
- **`resume_hand_size_discard(cx, response)`**:
  1. Validate the response is `DiscardCards { indices }` against
     `remaining[0]`'s hand. Reject (state untouched, no events) unless
     indices are unique, in-bounds, and the count is **exactly**
     `hand.len() - HAND_SIZE_LIMIT`. Rejecting an inexact count keeps
     the rule's "until 8 remaining" invariant structural.
  2. On success: remove the chosen cards, emitting
     `CardDiscarded { from: Zone::Hand, .. }` per card (reusing the
     existing event), and pop the queue front.
  3. If the queue is now empty → run `upkeep_phase_end` (4.6 +
     transition to Mythos, which itself seeds `mythos_draw_pending`).
     Otherwise re-emit `AwaitingInput` for the new `remaining[0]`.

## Events

No new event types. Each discarded card emits the existing
`CardDiscarded { from: Zone::Hand, .. }`. Window open/close and phase
boundary events are unchanged.

## Testing

Engine unit tests (`crates/game-core`, `TestGame` builder +
event-assertion macros):

- Over-cap detection: an investigator with `hand.len() > 8` at 4.5
  produces `AwaitingInput`; `<= 8` runs the step fully synchronously
  (no `hand_size_discard_pending`, proceeds to 4.6 unchanged).
- Validation rejections leave state untouched: too few, too many,
  duplicate, and out-of-bounds indices.
- Player-order sequencing: two over-cap investigators are prompted in
  turn order; each resolved by one `DiscardCards`; only after both
  drain does 4.6 run.

Integration test (`crates/cards/tests/` or
`crates/scenarios/tests/`, where a registry can be installed) covering
the issue's acceptance: end turn with 10 cards in hand and cap 8 fires
`AwaitingInput`, accepts a 2-card `DiscardCards`, lands at hand size 8,
and proceeds toward Mythos.

## Out of scope

- Per-investigator `max_hand_size` field and constant-modifier-query
  integration (deferred until a card modifies the cap).
- Any non-8 base cap.
