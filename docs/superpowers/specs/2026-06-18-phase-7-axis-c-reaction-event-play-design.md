# Phase 7 — Axis C: reaction-event-play (Evidence! 01022)

**Status:** design approved 2026-06-18, ready for implementation plan.
**Issues:** #335 (Axis C tracker), #304 (Evidence! 01022).
**Umbrella:** [trigger-dispatch rework](2026-06-16-trigger-dispatch-rework-umbrella-design.md) §4 Axis C.
**Predecessors shipped:** Axis B (#212/#213, `emit_event` + the `Continuation::Resolution`
stack) and Axis A (#334/#349, `PickSingle(OptionId)` + `ChoiceOption` + `InputRequest::choice`).

## Why this exists

The choice-cluster completion sub-slice closed every Gathering card reachable on
Axes A+B. The remaining carved cards need new dispatch machinery. **Axis C** is
the smallest of those: it admits a **Fast event played from hand** as an option
inside a reaction window. It unblocks **Evidence! 01022** (this spec's only card)
and is a hard prerequisite for Dodge 01023 (Axis D).

Evidence! 01022, verbatim from `data/arkhamdb-snapshot/pack/core/core.json`:

> Fast. Play after you defeat an enemy.
> Discover 1 clue at your location.

(`type_code: event`, `traits: Insight.`, `cost: 1`.)

## The core insight: Evidence! is Roland's reaction, sourced from hand

Roland Banks 01001's investigator reaction (`crates/cards/src/impls/roland_banks.rs`)
is, verbatim: "After you defeat an enemy: Discover 1 clue at your location."
It compiles to:

```rust
reaction_on_event(
    EventPattern::EnemyDefeated { by_controller: true, code: None },
    EventTiming::After,
    discover_clue(LocationTarget::YourLocation, 1),
)
```

Evidence!'s play instruction ("Play after you defeat an enemy") and effect
("Discover 1 clue at your location") are the **identical declaration** — Evidence!
merely omits Roland's once-per-round `UsageLimit`. Per RR p.11, a Fast event with
a when/after instruction plays "as if the described timing point were a triggering
condition." So Evidence! *is* the same `OnEvent` reaction; the only difference is
the **source zone**: Roland's ability rides a card in play (scanned today by
`scan_pending_triggers` over `controlled_card_instances()`); Evidence!'s rides an
Event in hand and resolves by **playing the card**.

**Consequence: no new play-timing predicate language.** The predicate is the
existing `trigger_matches(pattern, kind)` in
`crates/game-core/src/engine/dispatch/reaction_windows.rs`. Axis C extends *where*
the engine scans for matches (hand, not just play) and *how* a hand match resolves
(play the event vs. fire an in-play ability).

## Current architecture (what the rework changes)

A reaction window today (`reaction_windows.rs`):

1. `emit_event` (`dispatch/emit.rs`) calls `queue_reaction_window(cx, kind)` for a
   reaction-capable `TimingEvent` (e.g. `EnemyDefeated` → `WindowKind::AfterEnemyDefeated`).
2. `queue_reaction_window` runs `scan_pending_triggers` over every investigator's
   in-play cards. **If the result is empty, it returns without opening a window**
   (`reaction_windows.rs:52`). Otherwise it pushes a `Continuation::Resolution`
   frame holding the matched in-play candidates and `fast_actors: FastActorScope::Any`.
3. `open_queued_reaction_window` emits `AwaitingInput` with a **prompt-only**
   `InputRequest::prompt(...)` (empty `options`), expecting `InputResponse::PickIndex(i)`
   / `Skip`.
4. `resume_reaction_window` accepts `PickIndex(i)` (fire in-play trigger `i` via
   `fire_pending_trigger`) or `Skip`; **all other variants reject**.

Two gaps for Evidence!:

- **The window never opens.** If Evidence! is in hand but no in-play card reacts
  (Roland not in play, or already used his limit), step 2 finds zero in-play
  candidates and skips the window — Evidence! is never offered.
- **No way to play it.** Even with the window open (Roland in play too), the
  engine is paused; `apply` (`engine/mod.rs:100`) rejects every non-`ResolveInput`
  action, so a parallel `PlayCard` is impossible, and `resume_reaction_window`
  rejects everything but `PickIndex`/`Skip`. The Fast event play must arrive as a
  **structured option** in the window's `ResolveInput` set.

(Note: the framework `open_fast_window` path returns `Done` and does *not* pause —
the active player plays Fast cards there via an ordinary `PlayCard`. That is a
separate interaction model; see Scope boundary.)

## The design

### 1. Open the window when a hand Fast-event matches (not just an in-play trigger)

`queue_reaction_window` gains a second source. After `scan_pending_triggers`
(in-play matches), scan each **window-eligible** investigator's hand for Fast
**Events** carrying an `OnEvent` ability whose `(pattern, timing, kind: Reaction)`
matches the window's `WindowKind`/event, reusing `trigger_matches`. The window
opens/suspends when **either** source is non-empty. The empty-bail at
`reaction_windows.rs:52` becomes "bail only when *both* sources are empty."

The hand scan is controller-scoped by the pattern itself: `EnemyDefeated {
by_controller: true }` matches only the investigator credited with the defeat
(the window's `by`), so Evidence! is offered to exactly that investigator. (Solo
Slice-1: the active investigator. The scan order mirrors `scan_pending_triggers`:
active investigator first, then turn order.)

### 2. Unified `OptionId` option list

The window's offered options become the union, in a stable order:

```
options = [ in-play pending triggers ... ] ++ [ matching hand Fast-event plays ... ]
```

Each becomes a `ChoiceOption { id: OptionId(i), label }` (the Axis-A type in
`engine/outcome.rs`). `OptionId(i)` is the index into the offered list (the
existing Axis-A convention). The window frame must record enough to map each
`OptionId` back to its origin on resume — which in-play candidate, or which
investigator + hand index. The exact frame representation (extend
`ResolutionFrame`, or carry a parallel offered-options vector mirroring
`ChoiceFrame.offered`) is settled in the plan against the borrow constraints;
the contract is: **resume validates `OptionId` membership against the offered set
and dispatches by origin.**

### 3. Resume contract migration: `PickIndex` → `PickSingle(OptionId)`

- `open_queued_reaction_window` emits `InputRequest::choice(prompt, options)`
  instead of `InputRequest::prompt(...)`.
- `resume_reaction_window` accepts `InputResponse::PickSingle(OptionId)` instead
  of `PickIndex(u32)`:
  - id resolves to an **in-play trigger** → existing `fire_pending_trigger` (by its
    index within the trigger sub-list);
  - id resolves to a **hand Fast-event** → **play it** (see §4);
  - out-of-membership id → reject, window stays open (mirrors today's
    out-of-bounds `PickIndex` behavior).
- `InputResponse::Skip` is unchanged (close the window; still rejected while a
  forced run is open).

This retires the legacy `PickIndex`-while-paused reaction-window contract in
favor of the structured single-selection one, narrowing the input-contract split
flagged in #347/#348. `PickIndex` itself is not deleted here (other callers /
wire-format stability); only the reaction-window path moves off it.

### 4. Resolving a hand Fast-event option = playing the card

A picked hand-event option routes through the existing **play path** rather than a
parallel implementation. Reuse `play_card`'s body
(`dispatch/cards.rs`): emit `Event::CardPlayed`, stash the event in
`pending_played_event` (RR Appendix I step 3 — leaves hand at play-start), run the
event's effect through the evaluator, and let the apply loop flush
`pending_played_event` to discard on completion (RR Appendix I step 4 — the path
Dynamite Blast 01024 already uses for a suspending event).

The effect run is the **matched `OnEvent` ability's effect** (Evidence!'s
`discover_clue(YourLocation, 1)`). Evidence! has no separate `Trigger::OnPlay`
ability; its sole ability is the reaction. The plan decides whether the
window-play path runs "the matched reaction ability's effect" specifically or
"all of the event's play-resolved abilities" — for Evidence! (one ability) the two
coincide; the former is the precise reading of "the timing point is the triggering
condition" and is preferred unless reuse of `play_card`'s loop makes the latter
cleaner. `EvalContext` for the effect binds `controller = the playing investigator`
(the defeat's `by`), so `LocationTarget::YourLocation` resolves to that
investigator's location.

### 5. `fast_actors` scope

`FastActorScope::Any` (and `WindowBinding.fast_actors`) stays as the coarse
actor-eligibility gate used by `check_play_card` / `check_activate_ability`. The
umbrella's "tighten the `Any` blanket" concern is satisfied structurally: a Fast
event is now **offered** only when its `OnEvent` pattern matches the window (a
per-card, controller-scoped predicate), so the blanket is no longer the thing
deciding whether Evidence! is playable here. We document this and do not remove
`fast_actors` (it remains the multiplayer-relevant "who may act" gate).

## Scope boundary

- **Framework `open_fast_window` windows are out of scope.** They return `Done`
  (engine not paused) and admit Fast plays via an ordinary non-paused `PlayCard`
  (`check_play_card`'s permissive-window branch) — a different interaction model
  from the paused, structured after-event reaction window. Axis C migrates only
  the after-event reaction-window resume path that Evidence!/Dodge need. Unifying
  the framework fast-window path onto the same `OptionId` surface is a follow-up,
  not required by any Slice-1 card. (Filed as a follow-up at plan time.)
- **Fast assets / Fast 0-action abilities as window options are out of scope.**
  Evidence! is a Fast *event*; the hand scan is Event-only. `any_fast_play_eligible`
  already covers the broader set for the framework path; widening the structured
  option list to assets/abilities waits for a card that needs it.
- **Axis D (cancellation) is out of scope.** Dodge 01023 needs both Axis C and a
  Before-timing cancel/replacement signal (#336); only the reaction-event-play
  half lands here.
- **No new DSL primitive.** Evidence! reuses `EventPattern::EnemyDefeated`,
  `discover_clue`, `LocationTarget::YourLocation`, and `reaction_on_event` verbatim
  from the existing surface.

## Evidence! 01022 card

`crates/cards/src/impls/evidence_01022.rs`:

```rust
pub const CODE: &str = "01022";

pub fn abilities() -> Vec<Ability> {
    vec![reaction_on_event(
        EventPattern::EnemyDefeated { by_controller: true, code: None },
        EventTiming::After,
        discover_clue(LocationTarget::YourLocation, 1),
    )]
}
```

(No `UsageLimit` — Evidence! has no per-round limit; it is a one-shot event
discarded on play.) Wire into the registry (`impls::abilities_for`) and the module
list. The metadata (Fast, cost 1, Insight, event) comes from the generated corpus —
not hand-typed.

## Testing

1. **Card test** (`evidence_01022.rs`): `abilities()` is one `OnEvent` reaction with
   the `EnemyDefeated { by_controller: true, code: None }` / `After` / `Reaction`
   trigger and `discover_clue(YourLocation, 1)` effect; registry dispatches `CODE`.
2. **Engine unit tests** (`reaction_windows.rs`):
   - The after-defeat window **opens** when a matching hand Fast-event exists and
     *no* in-play trigger does (the empty-in-play-but-hand-match case).
   - The offered option list is the union (in-play triggers ++ hand events) with
     stable `OptionId`s; `PickSingle` of an out-of-membership id rejects and leaves
     the window open.
   - `PickSingle` of the hand-event option plays it (asserts `CardPlayed`,
     the effect's events, `CardDiscarded { from: Hand }`), then closes the window.
   - The non-credited investigator is **not** offered Evidence! (pattern scoping).
3. **Integration test** (`crates/cards/tests/`, real registry): solo Roland (or a
   fixture with Evidence! in hand) defeats an enemy → the after-defeat window
   offers Evidence! → `PickSingle` it → 1 clue discovered at the location, Evidence!
   in discard, window closed. Cover the both-sources case (Roland in play *and*
   Evidence! in hand → two options).
4. **Regression:** existing reaction-window tests (Roland 01001, Dr. Milan 01033,
   Guard Dog soak) migrate from `PickIndex` to `PickSingle(OptionId)` and stay green.

## Decisions made (to fold into the phase doc when the PR lands)

- **Evidence!'s play-timing predicate is the existing `OnEvent`/`trigger_matches`
  match, not a new "play window" field — a Fast reaction event is its in-play-
  reaction twin sourced from hand (RR p.11).** Evidence! reuses Roland 01001's exact
  declaration minus the usage limit.
- **Reaction windows move from the prompt-only `PickIndex` contract to the
  structured `PickSingle(OptionId)` contract (Axis-A's `ChoiceOption` surface);
  options are `{in-play triggers} ∪ {matching hand Fast-events}`.** A hand-event
  option resolves through the existing `play_card` path (`pending_played_event`
  discard). The legacy `PickIndex` reaction-window path is retired; `PickIndex` the
  variant survives for other callers.
- **A reaction window opens when *either* an in-play trigger or a hand Fast-event
  matches** — `queue_reaction_window`'s empty-bail now requires both sources empty.
- **`fast_actors` is not tightened/removed; offering is pattern-gated instead.**
  The blanket `Any` no longer decides Fast-event playability in a window.
- **Framework `open_fast_window` fast-play unification is deferred** (separate
  non-paused `PlayCard` model; no Slice-1 card needs it).
</content>
</invoke>
