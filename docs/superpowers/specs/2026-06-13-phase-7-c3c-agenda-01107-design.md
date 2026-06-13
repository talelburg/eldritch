# C3c — Agenda 01107 forced abilities (design)

**Issue:** [#232](https://github.com/talelburg/eldritch/issues/232) (Phase 7, Slice 1, Group C).
**Sibling:** Act-2 (01109) round-end clue window split into [#275](https://github.com/talelburg/eldritch/issues/275) (C3d) — see "Scope split" below.
**Depends on:** C3b (Ghoul enemies, PR #272), C1a/C1b (board build), and the `Effect::Native` bridge (#276, PR #277) — the agenda's effects are authored as card-local native fns on it.

## Scope split

The issue and the Group-C decomposition spec scope C3c to agenda 01107's
two Forced abilities. The phase-7 doc's decision log additionally folded
act-2 (01109)'s round-end clue-spend objective into C3c ("build the
round-end window once; the agenda doom rides it"). On review the agenda's
round-end **doom** is a fire-and-forget forced effect that does **not**
need a suspendable player window, so the two are cleanly separable. **This
spec covers the agenda only.** The act-2 round-end window — which needs a
suspendable `AwaitingInput` window, `upkeep_phase_end` threading, and
`AdvanceAct` re-gating — is **C3d ([#275](https://github.com/talelburg/eldritch/issues/275))**,
tackled next.

## Verified card text (snapshot)

**01107 They're Getting Out!** (agenda stage 3, doom threshold 10):

> **Forced** - At the end of the enemy phase: Each unengaged [[Ghoul]] enemy moves 1 location towards the Parlor.
> **Forced** - At the end of the round: Place 1 doom on this agenda for each [[Ghoul]] enemy in the Hallway or Parlor.

## Verified map facts

The Gathering map is a star: the **Hallway (01112)** hub connects to the
**Attic (01113)**, **Cellar (01114)**, and **Parlor (01115)**; the Study
(01111) is isolated and removed when Act 1 advances. Toward the Parlor,
**every location has a unique shortest first step** (always via the
Hallway, which has a single edge to the Parlor). **Equidistant ties toward
the Parlor cannot occur in this scenario.**

## Design

Both abilities are authored as DSL `Ability`s on the agenda card and fire
through the existing forced-trigger evaluator path (`fire_forced_triggers`
→ `apply_effect`), exactly like Act 1's `act_01108.rs` on-advance board
build. The effects they invoke are board-dependent, single-use scenario
operations, so — per the #276 decision — each is a **card-local Rust fn**
dispatched via `Effect::Native { tag }`, living in `agenda_01107.rs`, not
a new shared `Effect` variant. (The agenda is a card-object, so its Forced
abilities ride the `abilities()`/registry path; `Effect::Native` is how
that path reaches card-local Rust.)

### 1. New `EventPattern::RoundEnded` (card-dsl)

Add `EventPattern::RoundEnded` (bare, no narrowing fields) mirroring
`PhaseEnded`, plus a `round_ended()` builder. Distinct from
`PhaseEnded { Upkeep }`: "end of round" is its own framework timing point
even though, per RR p.24, the round ends at the close of the upkeep phase.
Keeping them separate lets a future "end of upkeep phase" card and an "end
of round" card coexist without conflation.

### 2. New `ForcedTriggerPoint::RoundEnded` (forced_triggers.rs)

Scans the current act and agenda for `OnEvent(RoundEnded, After)`
abilities; controller = lead investigator (the board-wide doom effect
ignores it). Same `push_matching` machinery as `PhaseEnded`.

### 3. Fire `RoundEnded` in `upkeep_phase_end` (phases.rs, step 4.6)

After the existing `PhaseEnded { Upkeep }` forced dispatch, fire
`ForcedTriggerPoint::RoundEnded`. Both resolve to `Done` in slice-1 scope
(the doom effect just increments a counter — no suspension), so the
existing `debug_assert!(matches!(forced, Done))` guard pattern is
preserved for the new dispatch too. `upkeep_phase_end` keeps its `()`
return in C3c; C3d threads it to `EngineOutcome`.

### 4. Two card-local native fns (`agenda_01107.rs`)

Both are `NativeEffectFn`s (`fn(&mut Cx, &EvalContext) -> EngineOutcome`),
dispatched by tag through the registry's `native_effect_for`. They read
enemy traits / locations and the corpus directly via the now-public
`game-core` surface (`Cx`, plus `shortest_first_steps` — promoted to `pub`
by this PR, the one helper C3c needs beyond #276's). The Ghoul trait,
Parlor (01115), and Hallway (01112) codes live inline in the card module.

- **`"01107:move-ghouls"`** — for each enemy that is **unengaged**
  (`engaged_with.is_none()`), has a `current_location`, and whose `traits`
  contains `"Ghoul"`: compute `shortest_first_steps(state, loc, parlor)`
  and move it one step, setting `current_location` and emitting
  `Event::EnemyMoved`. The Parlor `LocationId` is resolved from `"01115"`
  via `location_id_by_code` at call time. **Tie-break:** lowest
  `LocationId` among the returned first steps, documented as unreachable on
  this map (RR p.12: the controlling player chooses on a tie — deferred
  until a map with ties lands; deterministic here avoids a reject in a
  fire-and-forget forced path). **Engagement on arrival is not modeled**
  for this forced move (the card text is positional only; these Ghouls move
  *toward* the Parlor, away from where investigators typically are) —
  noted, revisit if a consumer needs it. Iteration over enemies is in
  `EnemyId` order for a deterministic event stream.

- **`"01107:round-end-doom"`** — count enemies whose `traits` contains
  `"Ghoul"` and whose `current_location` is the Hallway (01112) or Parlor
  (01115) (**not** filtered by engagement, per card text), then add that
  count to `state.agenda_doom`. **No threshold check** here — RR p.24
  checks doom in Mythos step 1.3; round-end placement only accumulates.

Both no-op cleanly (0 movers / 0 doom) when no matching enemy is in play.
The move fn rejects loudly only if the Parlor (01115) is not in play (a
malformed board); the doom fn counts only the locations actually in play.

### 5. Agenda card impl

`crates/cards/src/impls/agenda_01107.rs` exposing `CODE = "01107"`, the two
native fns above, a `native_effect_for(tag)` resolver, and two `on_event`
abilities:

```rust
on_event(EventPattern::PhaseEnded { phase: Phase::Enemy }, After,
         native("01107:move-ghouls"))      // → the Parlor
on_event(EventPattern::RoundEnded, After,
         native("01107:round-end-doom"))    // doom per Ghoul in Hallway/Parlor
```

Registered in `crates/cards/src/impls/mod.rs` (`abilities_for` match arm,
`native_effect_for` delegation, + `pub mod`).

## Testing

- **Card tests** (`agenda_01107.rs`): the two abilities carry the expected
  triggers + native tags; `native_effect_for` resolves exactly those two
  tags. The native fns' behavior is exercised against a built `GameState`:
  - move fn: unengaged Ghoul steps toward the Parlor; engaged Ghoul does
    **not** move; non-Ghoul does not move; a Ghoul already at the Parlor
    does not move; deterministic single-step.
  - doom fn: `agenda_doom` increments by the count of Ghouls in Hallway +
    Parlor (incl. engaged ones); enemies elsewhere don't count.
- **Engine unit tests** (`forced_triggers.rs` / `phases.rs`):
  `ForcedTriggerPoint::RoundEnded` collects agenda `RoundEnded` abilities,
  and `upkeep_phase_end` fires it (mock-registry integration test, like the
  existing `native_effect.rs` harness).
- **Integration** (`crates/cards/tests/`): a round-end cycle on a built
  Gathering board moves a Ghoul one step toward the Parlor at enemy-phase
  end and places the right doom at round end; no agenda advance occurs at
  round end (threshold check stays in Mythos).

## Out of scope (→ C3d, #275)

Act-2 (01109)'s round-end clue-spend window, `upkeep_phase_end`
`EngineOutcome` threading, and `AdvanceAct` re-gating.
