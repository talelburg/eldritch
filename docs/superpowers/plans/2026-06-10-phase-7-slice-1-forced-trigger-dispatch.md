# Phase 7 Slice 1 — Forced-Trigger Dispatch (Plan A2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fire Forced (`Trigger::OnEvent`) abilities printed on scenario-structure cards (locations, acts, agendas) at the right timing windows, via a separate immediate path that does not touch the player-reaction machinery.

**Architecture:** A new `fire_forced_triggers(cx, point)` chokepoint, invoked explicitly at two emission sites (after `InvestigatorMoved`; at `enemy_phase_end` / `upkeep_phase_end`). It scans only the relevant scenario-structure card(s), collects matching forced abilities, and — for the single-trigger case the slice's content produces — resolves immediately via the existing `apply_effect` evaluator. If 2+ are ever simultaneously pending it **rejects loudly** (the iterative-ordering loop is #213, the `emit_event` unification is #212) rather than silently choosing an order. This is a clean, forward-compatible subset of #212.

**Tech Stack:** Rust workspace (`card-dsl`, `game-core`); strict CI flags from `CLAUDE.md`.

**Prerequisite:** Plan A1 must be merged (this plan uses `Effect::DealHorror`/`DealDamage`, `EventPattern::EnteredLocation`, and `Act`/`Agenda.code`).

**Scope (explicit):**
- IN: forced-on-enter (location) + forced phase-end (act/agenda) dispatch; single-trigger resolution; the 2+ loud-reject guard; `EventPattern::PhaseEnded`.
- OUT: the iterative ordering loop (#213); the universal `emit_event` chokepoint (#212); player-optional ("may") scenario-card reactions (none exist in the slice; player reactions stay on the existing reaction-window path); Roland's reaction (unchanged — already on `AfterEnemyDefeated`).

---

## File Structure

- `crates/card-dsl/src/dsl.rs` — add `EventPattern::PhaseEnded { phase }` variant. (`Phase` is `game_core`'s type; `card-dsl` must not depend on `game-core`. Mirror the phase as a `card-dsl`-local enum if `card-dsl` has no `Phase`, OR carry the discriminant some `card-dsl`-safe way — see Task 1 for the resolved approach.)
- `crates/game-core/src/engine/dispatch/forced_triggers.rs` — **new**. `ForcedTriggerPoint` enum + `fire_forced_triggers` + the scan/collect/resolve/guard logic.
- `crates/game-core/src/engine/dispatch/mod.rs` — declare the new module; no dispatch-arm changes (forced firing is called from handlers, not from the top-level action match).
- `crates/game-core/src/engine/dispatch/actions.rs:271` — wire `fire_forced_triggers` after the `InvestigatorMoved` emit in `move_action`.
- `crates/game-core/src/engine/dispatch/phases.rs:389,464` — wire `fire_forced_triggers` into `enemy_phase_end` and `upkeep_phase_end`.
- `crates/game-core/src/engine/dispatch/reaction_windows.rs` — add the inert `EventPattern::PhaseEnded` arm to `trigger_matches` (keeps the exhaustive match compiling; forced patterns never match reaction windows).

---

## Task 0 (decision, no code): where does `Phase` live for `EventPattern::PhaseEnded`?

`card-dsl` must not depend on `game-core` (layering). `Phase` (`Mythos`/`Investigation`/`Enemy`/`Upkeep`) lives in `game-core::state::phase`. Options:

- **(A, chosen)** Add a `card-dsl`-local `Phase` mirror enum in `card-dsl` and have `EventPattern::PhaseEnded { phase: card_dsl::Phase }`. `game-core` already re-exports `card_dsl` types; `forced_triggers` maps `card_dsl::Phase` ↔ `state::Phase` at the boundary (a 4-arm match). Keeps layering clean.
- (B) Make `EventPattern::PhaseEnded` carry no phase and have `fire_forced_triggers` infer the phase from the call site. Rejected: a card author can't then say "at the end of the *enemy* phase" declaratively — the discriminant belongs on the pattern.

**Resolution:** Option A. Task 1 adds `card_dsl::Phase` and the boundary map.

(If `card-dsl` already has a `Phase`-like type, reuse it and skip the new enum — confirm with `grep -n "enum Phase" crates/card-dsl/src/`.)

---

## Task 1: `EventPattern::PhaseEnded { phase }` (DSL, inert in `trigger_matches`)

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (`EventPattern` enum; new `Phase` mirror if absent)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`trigger_matches` inert arm)
- Test: inline `#[cfg(test)]` in `crates/card-dsl/src/dsl.rs`

- [ ] **Step 1: Confirm whether `card-dsl` already has a `Phase`**

Run: `grep -n "enum Phase" crates/card-dsl/src/dsl.rs crates/card-dsl/src/lib.rs`
Expected: no match → add the mirror in Step 3. (If it matches, reuse it and adjust Step 3.)

- [ ] **Step 2: Write the failing round-trip test**

In `crates/card-dsl/src/dsl.rs` `#[cfg(test)] mod tests`:

```rust
#[test]
fn phase_ended_pattern_round_trips() {
    let p = EventPattern::PhaseEnded { phase: Phase::Enemy };
    let json = serde_json::to_string(&p).unwrap();
    let back: EventPattern = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo test -p card-dsl phase_ended_pattern_round_trips 2>&1 | tail -10`
Expected: compile error — `EventPattern::PhaseEnded` and/or `Phase` do not exist.

- [ ] **Step 4: Add the `card-dsl` `Phase` mirror (if absent) and the variant**

In `crates/card-dsl/src/dsl.rs`, add the mirror enum near the other DSL data types:

```rust
/// The four game phases, mirrored in `card-dsl` so `EventPattern` can
/// name a phase without `card-dsl` depending on `game-core` (layering).
/// `game-core` maps this to its own `state::Phase` at the dispatch
/// boundary (see `engine::dispatch::forced_triggers`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Phase {
    Mythos,
    Investigation,
    Enemy,
    Upkeep,
}
```

Then add to `pub enum EventPattern`, after `EnteredLocation` (added in A1):

```rust
    /// A game phase ended. Forced agenda/act effects keyed to a phase
    /// boundary listen here: agenda `01107` moves Ghouls at
    /// `PhaseEnded { phase: Enemy }` and places doom at the end of the
    /// round (`PhaseEnded { phase: Upkeep }`).
    ///
    /// Matched only by the forced dispatch path
    /// (`engine::dispatch::forced_triggers`), never by player reaction
    /// windows — `trigger_matches` returns `false` for it.
    PhaseEnded { phase: Phase },
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p card-dsl phase_ended_pattern_round_trips 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 6: Keep `game-core` compiling — inert `trigger_matches` arm**

Run: `cargo build -p game-core 2>&1 | tail -15`
Expected: non-exhaustive-match error in `trigger_matches` (`reaction_windows.rs`).

Add `EventPattern::PhaseEnded { .. }` to the existing inert tuple arm in `trigger_matches` (the one that already lists `EnemyDefeated | CardRevealed | EnemySpawned | EnteredLocation` returning `false`):

```rust
        (
            WindowKind::PlayerWindow(_) | WindowKind::AfterEnemyDefeated { .. },
            EventPattern::EnemyDefeated { .. }
            | EventPattern::CardRevealed { .. }
            | EventPattern::EnemySpawned
            | EventPattern::EnteredLocation
            | EventPattern::PhaseEnded { .. },
        ) => false,
```

Run: `cargo build -p game-core 2>&1 | tail -5` → clean.

- [ ] **Step 7: Strict gauntlet + commit**

```sh
RUSTFLAGS="-D warnings" cargo test -p card-dsl -p game-core --all-features 2>&1 | tail -10
cargo clippy -p card-dsl -p game-core --all-targets --all-features -- -D warnings 2>&1 | tail -10
cargo fmt --check
```
```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "dsl: EventPattern::PhaseEnded { phase } (inert in reaction windows)

Mirror Phase into card-dsl to keep the layering boundary; the variant
is matched only by the forced dispatch path (Task 2+), never by player
reaction windows. Needed by agenda 01107's phase-end forced effects.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: `forced_triggers` module — `ForcedTriggerPoint` + `fire_forced_triggers` skeleton

**Files:**
- Create: `crates/game-core/src/engine/dispatch/forced_triggers.rs`
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (add `mod forced_triggers;`)
- Test: inline `#[cfg(test)]` in `forced_triggers.rs`

This task builds the scan/collect/resolve/guard core against the **EnteredLocation** point (the simplest), with a synthetic location carrying a forced `DealHorror`-on-enter ability. Task 4 adds the PhaseEnded point.

- [ ] **Step 1: Declare the module**

In `crates/game-core/src/engine/dispatch/mod.rs`, add alongside the other `mod` declarations:

```rust
mod forced_triggers;
```

- [ ] **Step 2: Write the failing test (single forced-on-enter resolves immediately)**

Create `crates/game-core/src/engine/dispatch/forced_triggers.rs` with only this test module to start (the test references items defined in Step 4):

```rust
//! Forced-trigger dispatch: fires `Trigger::OnEvent` abilities printed
//! on scenario-structure cards (locations, acts, agendas) at framework
//! timing points, via an immediate path separate from the player
//! reaction-window machinery. Single-trigger only in this slice; 2+
//! simultaneous pending triggers reject loudly (#213 adds the ordering
//! loop, #212 the universal emit_event chokepoint).

use crate::card_data::CardType; // adjust imports to what the impl needs
use crate::dsl::Phase as DslPhase;
use crate::engine::dispatch::Cx;
use crate::engine::outcome::EngineOutcome;
use crate::state::{InvestigatorId, LocationId, Phase};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card_registry;
    use crate::dsl::{ability_on_event, deal_horror, EventPattern, EventTiming, InvestigatorTarget};
    use crate::event::Event;
    use crate::state::CardCode;
    use crate::test_support::{test_investigator, test_location, TestGame};

    // A registry whose only card (the test location code) carries a
    // forced "after you enter: take 1 horror" ability.
    fn forced_horror_abilities(code: &CardCode) -> Option<Vec<crate::dsl::Ability>> {
        if code == &CardCode::new("test-attic") {
            Some(vec![ability_on_event(
                EventPattern::EnteredLocation,
                EventTiming::After,
                deal_horror(InvestigatorTarget::Controller, 1),
            )])
        } else {
            None
        }
    }

    fn no_metadata(_: &CardCode) -> Option<&'static crate::card_data::CardMetadata> {
        None
    }

    #[test]
    fn forced_on_enter_resolves_immediately() {
        let _ = card_registry::install(card_registry::CardRegistry {
            metadata_for: no_metadata,
            abilities_for: forced_horror_abilities,
        });

        let mut loc = test_location(10, "Attic");
        loc.code = CardCode::new("test-attic");
        let mut state = TestGame::new()
            .with_investigator_at(test_investigator(1), LocationId(10))
            .with_location(loc)
            .with_active_investigator(InvestigatorId(1))
            .build();
        let mut events = Vec::new();
        let mut cx = Cx { state: &mut state, events: &mut events };

        let outcome = fire_forced_triggers(
            &mut cx,
            ForcedTriggerPoint::EnteredLocation {
                investigator: InvestigatorId(1),
                location: LocationId(10),
            },
        );

        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&InvestigatorId(1)].horror, 1);
        assert!(events.iter().any(|e| matches!(e, Event::HorrorTaken { amount: 1, .. })));
    }
}
```

> NOTE for the implementer: confirm exact names before finalising — the registry struct/field names (`card_registry::CardRegistry`, `metadata_for`, `abilities_for`), the `ability_on_event` builder (it exists per `dsl.rs` ~line 623 — `ability_on_event(pattern, timing, effect)`), and `TestGame` helpers — by reading `crates/cards/tests/play_card.rs` and `reaction_windows.rs`'s own tests. Because the process-global registry is a `OnceLock`, run this test in a file that does not also install a different registry (a dedicated integration test in `crates/cards/tests/` may be cleaner than an in-crate test if collisions arise — mirror the existing reaction tests' choice).

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo test -p game-core forced_on_enter_resolves_immediately 2>&1 | tail -20`
Expected: compile error — `fire_forced_triggers` / `ForcedTriggerPoint` undefined.

- [ ] **Step 4: Implement `ForcedTriggerPoint` + `fire_forced_triggers` (EnteredLocation arm)**

In `forced_triggers.rs`, above the test module:

```rust
/// A framework timing point at which Forced (`Trigger::OnEvent`)
/// abilities on scenario-structure cards may fire. Each variant carries
/// the binding context the fired effect needs (the "you", the phase).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ForcedTriggerPoint {
    /// An investigator entered a location. Scans that location's card
    /// for `EventPattern::EnteredLocation` forced abilities; binds
    /// controller = the entering investigator.
    EnteredLocation { investigator: InvestigatorId, location: LocationId },
    /// A phase ended. Scans the current act and agenda for
    /// `EventPattern::PhaseEnded { phase }` forced abilities; binds
    /// controller = the lead investigator (board-wide effects ignore it).
    PhaseEnded { phase: Phase },
}

/// One forced ability to resolve: the source card's code, the ability
/// index within that card, and the controller to evaluate it as.
struct ForcedHit {
    code: crate::state::CardCode,
    ability_index: usize,
    controller: InvestigatorId,
}

/// Fire Forced abilities matching `point`. Single-trigger path for this
/// slice: 0 → `Done`; 1 → resolve via `apply_effect`; 2+ → reject loudly
/// (no silently-chosen order — #213 adds the ordering loop). Reaction
/// ("may") scenario abilities do not exist in-slice and are not handled
/// here; player reactions stay on the reaction-window path.
pub(super) fn fire_forced_triggers(cx: &mut Cx, point: ForcedTriggerPoint) -> EngineOutcome {
    let hits = collect_forced_hits(cx.state, point);
    match hits.len() {
        0 => EngineOutcome::Done,
        1 => resolve_one(cx, &hits[0]),
        n => EngineOutcome::Rejected {
            reason: format!(
                "fire_forced_triggers: {n} simultaneous forced triggers at {point:?}; \
                 ordering not yet implemented (see #213). Slice-1 content never produces \
                 this — investigate the source."
            )
            .into(),
        },
    }
}

/// Collect the forced abilities matching `point` from the relevant
/// scenario-structure source(s), in source order. Returns an empty vec
/// when the registry isn't installed.
fn collect_forced_hits(state: &crate::state::GameState, point: ForcedTriggerPoint) -> Vec<ForcedHit> {
    let Some(reg) = crate::card_registry::current() else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    match point {
        ForcedTriggerPoint::EnteredLocation { investigator, location } => {
            let Some(loc) = state.locations.get(&location) else {
                return hits;
            };
            push_matching(&reg, &loc.code, investigator, &mut hits, |p| {
                matches!(p, crate::dsl::EventPattern::EnteredLocation)
            });
        }
        ForcedTriggerPoint::PhaseEnded { phase } => {
            // Task 4 fills this in (act + agenda scan). Left empty here
            // so Task 2's EnteredLocation test passes in isolation.
            let _ = phase;
        }
    }
    hits
}

/// Push every Forced `OnEvent` ability on `code` whose pattern passes
/// `want` (and whose timing is `After`) as a `ForcedHit`.
fn push_matching(
    reg: &crate::card_registry::CardRegistry,
    code: &crate::state::CardCode,
    controller: InvestigatorId,
    out: &mut Vec<ForcedHit>,
    want: impl Fn(crate::dsl::EventPattern) -> bool,
) {
    let Some(abilities) = (reg.abilities_for)(code) else {
        return;
    };
    for (idx, ability) in abilities.iter().enumerate() {
        if let crate::dsl::Trigger::OnEvent { pattern, timing } = ability.trigger {
            if timing == crate::dsl::EventTiming::After && want(pattern) {
                out.push(ForcedHit { code: code.clone(), ability_index: idx, controller });
            }
        }
    }
}

/// Resolve a single forced ability via the evaluator.
fn resolve_one(cx: &mut Cx, hit: &ForcedHit) -> EngineOutcome {
    let Some(reg) = crate::card_registry::current() else {
        return EngineOutcome::Rejected {
            reason: "fire_forced_triggers: registry vanished between collect and resolve".into(),
        };
    };
    let Some(abilities) = (reg.abilities_for)(&hit.code) else {
        return EngineOutcome::Rejected {
            reason: format!("fire_forced_triggers: {} has no abilities at resolve time", hit.code).into(),
        };
    };
    let effect = abilities[hit.ability_index].effect.clone();
    crate::engine::evaluator::apply_effect(
        cx,
        &effect,
        crate::engine::evaluator::EvalContext::for_controller(hit.controller),
    )
}
```

Adjust the top-of-file `use` lines to exactly what the implementation references (drop the placeholder `CardType`/`DslPhase` imports if unused; `cargo clippy` will flag them).

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p game-core forced_on_enter_resolves_immediately 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 6: Strict gauntlet + commit**

```sh
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features 2>&1 | tail -12
cargo clippy -p game-core --all-targets --all-features -- -D warnings 2>&1 | tail -12
cargo fmt --check
```
```bash
git add crates/game-core/src/engine/dispatch/forced_triggers.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: forced_triggers module — fire_forced_triggers (EnteredLocation)

Separate immediate path for Forced OnEvent abilities on scenario-structure
cards. Single-trigger resolution via the evaluator; 2+ simultaneous reject
loudly (ordering loop is #213). PhaseEnded arm stubbed for Task 4.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: wire `fire_forced_triggers` into `move_action`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/actions.rs:271` (`move_action`)
- Test: inline `#[cfg(test)]` in `actions.rs` (or extend an existing move test)

- [ ] **Step 1: Write the failing test (moving into a forced location applies its effect)**

Add to `actions.rs` tests (mirror the registry-install pattern from Task 2 — if `OnceLock` collisions arise with other `actions.rs` tests, move this to a dedicated `crates/cards/tests/forced_on_enter.rs` integration test instead):

```rust
#[test]
fn moving_into_forced_location_fires_its_effect() {
    // install a registry where "test-attic" has forced DealHorror-on-enter
    // (same helper shape as forced_triggers::tests::forced_horror_abilities)
    // ... build a two-location board: investigator at 10, connected to 11
    //     where 11.code == "test-attic"; then:
    let result = apply(state, Action::Player(PlayerAction::Move {
        investigator: InvestigatorId(1),
        destination: LocationId(11),
    }));
    assert!(matches!(result.outcome, EngineOutcome::Done));
    assert_eq!(result.state.investigators[&InvestigatorId(1)].horror, 1);
}
```

> Flesh out the board setup against an existing `move_action` test in this file (connections, phase, active investigator). Use the full `apply(...)` entry point so the wiring is exercised end-to-end.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core moving_into_forced_location_fires_its_effect 2>&1 | tail -15`
Expected: FAIL — horror is 0 (the move succeeds but no forced effect fires).

- [ ] **Step 3: Wire the call**

In `crates/game-core/src/engine/dispatch/actions.rs`, replace `move_action`'s terminal `EngineOutcome::Done` (right after the `InvestigatorMoved` push, ~line 274) with:

```rust
    super::forced_triggers::fire_forced_triggers(
        cx,
        super::forced_triggers::ForcedTriggerPoint::EnteredLocation {
            investigator,
            location: destination,
        },
    )
```

(`super::forced_triggers` because `actions` and `forced_triggers` are sibling modules under `dispatch`. Make `ForcedTriggerPoint`/`fire_forced_triggers` `pub(super)` — already specified in Task 2.)

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p game-core moving_into_forced_location_fires_its_effect 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 5: Run the full move/action test set (regression — Move now does more)**

Run: `cargo test -p game-core move_ 2>&1 | tail -20`
Expected: all existing move tests still PASS (a plain move to a location with no forced ability returns `Done` exactly as before — `fire_forced_triggers` returns `Done` on 0 hits).

- [ ] **Step 6: Strict gauntlet + commit**

```sh
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features 2>&1 | tail -12
cargo clippy -p game-core --all-targets --all-features -- -D warnings 2>&1 | tail -10
cargo fmt --check
```
```bash
git add crates/game-core/src/engine/dispatch/actions.rs
git commit -m "engine: fire forced-on-enter effects from move_action

Move now fires the entered location's Forced OnEvent abilities via
fire_forced_triggers. No-op (Done) for locations without one, so existing
move behavior is unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: PhaseEnded dispatch (act + agenda scan) + wire into phase-end

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (fill the `PhaseEnded` arm + `card_dsl::Phase` ↔ `state::Phase` map)
- Modify: `crates/game-core/src/engine/dispatch/phases.rs:389` (`enemy_phase_end`), `:464` (`upkeep_phase_end`)
- Test: inline `#[cfg(test)]` in `forced_triggers.rs`

- [ ] **Step 1: Write the failing test (forced agenda effect fires at enemy-phase end)**

In `forced_triggers.rs` tests, add a test that installs a registry where an agenda code (e.g. `"test-agenda"`) carries a forced ability with `EventPattern::PhaseEnded { phase: DslPhase::Enemy }` whose effect is observable (e.g. `deal_horror` to the lead investigator — board-wide effects are Rust impls, but a DSL effect keeps the test simple and exercises the path). Build a state whose `agenda_deck[agenda_index].code == "test-agenda"` and one investigator (the lead), then call:

```rust
let outcome = fire_forced_triggers(&mut cx, ForcedTriggerPoint::PhaseEnded { phase: Phase::Enemy });
assert_eq!(outcome, EngineOutcome::Done);
assert_eq!(state.investigators[&InvestigatorId(1)].horror, 1);
```

(And a negative assertion: `PhaseEnded { phase: Phase::Mythos }` fires nothing → horror stays 0.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core -- forced_triggers 2>&1 | tail -20`
Expected: FAIL — the `PhaseEnded` arm is empty (Task 2 stub), horror stays 0.

- [ ] **Step 3: Implement the `PhaseEnded` arm + phase map**

In `forced_triggers.rs`, add the boundary map and fill the arm in `collect_forced_hits`:

```rust
/// Map the engine's `state::Phase` to the `card-dsl` mirror so a
/// `PhaseEnded` pattern can be compared. (4-arm total — see Task 0.)
fn dsl_phase(phase: Phase) -> crate::dsl::Phase {
    match phase {
        Phase::Mythos => crate::dsl::Phase::Mythos,
        Phase::Investigation => crate::dsl::Phase::Investigation,
        Phase::Enemy => crate::dsl::Phase::Enemy,
        Phase::Upkeep => crate::dsl::Phase::Upkeep,
    }
}
```

Replace the stubbed `PhaseEnded` arm in `collect_forced_hits`:

```rust
        ForcedTriggerPoint::PhaseEnded { phase } => {
            let want_phase = dsl_phase(phase);
            // Lead investigator binds the controller for board-wide
            // effects (which ignore it). First of turn_order is the lead.
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            // Current act, then current agenda (source order).
            if let Some(act) = state.act_deck.get(state.act_index) {
                push_matching(&reg, &act.code, lead, &mut hits, |p| {
                    matches!(p, crate::dsl::EventPattern::PhaseEnded { phase } if phase == want_phase)
                });
            }
            if let Some(agenda) = state.agenda_deck.get(state.agenda_index) {
                push_matching(&reg, &agenda.code, lead, &mut hits, |p| {
                    matches!(p, crate::dsl::EventPattern::PhaseEnded { phase } if phase == want_phase)
                });
            }
        }
```

> Confirm the cursor field names (`act_index`, `agenda_index`) against `game_state.rs` (Task A1 worked near them). If `turn_order` can be empty at phase-end in valid states, `lead` falls back to no-hits, which is correct (no investigators ⇒ no forced firing).

- [ ] **Step 4: Run the forced-trigger tests to verify they pass**

Run: `cargo test -p game-core -- forced_triggers 2>&1 | tail -20`
Expected: PASS (both the Enemy-phase positive and the Mythos negative).

- [ ] **Step 5: Wire into `enemy_phase_end` and `upkeep_phase_end`**

In `crates/game-core/src/engine/dispatch/phases.rs`:

`enemy_phase_end` (returns `EngineOutcome`) — after its `PhaseEnded { phase: Enemy }` emit and before its return, thread the forced outcome. If `enemy_phase_end` currently ends with `step_phase(cx)` or a `Done`, fire first and short-circuit on non-`Done`:

```rust
    let forced = super::forced_triggers::fire_forced_triggers(
        cx,
        super::forced_triggers::ForcedTriggerPoint::PhaseEnded { phase: Phase::Enemy },
    );
    if !matches!(forced, EngineOutcome::Done) {
        return forced; // 2+-trigger loud reject (unreachable in-slice); propagate
    }
    // ... existing transition (e.g. step_phase(cx) / Done) ...
```

`upkeep_phase_end` (returns `()`) — it cannot propagate an outcome. Fire and assert `Done` in debug (the 2+ reject is unreachable in-slice; surfacing it loudly in debug is the honest guard):

```rust
    let forced = super::forced_triggers::fire_forced_triggers(
        cx,
        super::forced_triggers::ForcedTriggerPoint::PhaseEnded { phase: Phase::Upkeep },
    );
    debug_assert!(
        matches!(forced, EngineOutcome::Done),
        "upkeep_phase_end forced trigger did not resolve to Done: {forced:?} \
         (2+ simultaneous forced at round end needs #213)"
    );
```

> Read both functions' exact tails first (`phases.rs:389` and `:464`) and splice the call after the `PhaseEnded` emit. The `()` return on `upkeep_phase_end` is a known limitation the `emit_event` restructure (#212) resolves by centralising suspension/propagation — note it in a code comment.

- [ ] **Step 6: Run the phase + round-flow tests (regression)**

Run: `cargo test -p game-core phase 2>&1 | tail -20`
Run: `cargo test -p game-core upkeep 2>&1 | tail -10`
Expected: all PASS — phases with no forced act/agenda abilities are unchanged (`fire_forced_triggers` returns `Done` on 0 hits).

- [ ] **Step 7: Strict gauntlet + commit**

```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features 2>&1 | tail -15
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -10
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features 2>&1 | tail -10
cargo fmt --check
```
```bash
git add crates/game-core/src/engine/dispatch/forced_triggers.rs crates/game-core/src/engine/dispatch/phases.rs
git commit -m "engine: fire forced act/agenda effects at phase-end

fire_forced_triggers now scans the current act + agenda for
EventPattern::PhaseEnded forced abilities, wired into enemy_phase_end and
upkeep_phase_end. upkeep_phase_end's () return can't propagate the 2+
reject (debug_assert guards it); #212 resolves that by centralising
suspension. Needed by agenda 01107.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: the 2+-simultaneous loud-reject guard (explicit test)

**Files:**
- Test: inline `#[cfg(test)]` in `forced_triggers.rs`

The guard already exists (Task 2's `n =>` arm). This task pins it with a test so a future refactor can't silently turn it into "pick whatever order."

- [ ] **Step 1: Write the test**

In `forced_triggers.rs` tests, install a registry where one location code carries **two** forced `EnteredLocation` abilities (e.g. two `deal_horror`), build a board with the investigator entering it, and assert the reject:

```rust
#[test]
fn two_simultaneous_forced_triggers_reject_loudly() {
    // registry: "test-double" has TWO forced DealHorror-on-enter abilities
    // ... build board, investigator at the double-forced location ...
    let outcome = fire_forced_triggers(
        &mut cx,
        ForcedTriggerPoint::EnteredLocation { investigator: InvestigatorId(1), location: LocationId(10) },
    );
    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    // and: no horror applied (rejected before resolving either) —
    // confirm against the validate-first contract (apply() restores state
    // on Rejected; here we call the helper directly, so assert the helper
    // itself did not resolve an effect before rejecting).
    assert_eq!(cx.state.investigators[&InvestigatorId(1)].horror, 0);
}
```

> The helper rejects *before* `resolve_one` (it counts hits first), so no effect is applied — assert horror stayed 0. If you route this through full `apply(...)` instead, the transactional restore also guarantees it.

- [ ] **Step 2: Run it to verify it passes**

Run: `cargo test -p game-core two_simultaneous_forced_triggers_reject_loudly 2>&1 | tail -10`
Expected: PASS (the guard is already implemented).

- [ ] **Step 3: Commit**

```bash
git add crates/game-core/src/engine/dispatch/forced_triggers.rs
git commit -m "test: pin the 2+-simultaneous forced-trigger loud reject (#213)

Guards against a future refactor silently choosing an order instead of
rejecting. The ordering loop lands in #213.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

- **Spec coverage:** A2 covers spec Group A items 3–5 (widen the trigger scan to scenario-structure sources via the separate forced path; the forced auto-fire; the new `EventPattern` + trigger-point wiring at `InvestigatorMoved` / `PhaseEnded(Enemy)` / round-end). The reaction-window path and Roland's reaction are untouched, as the spec requires. The 2+-ordering deferral points explicitly at #213; the `emit_event` unification at #212 — both filed.
- **Placeholder scan:** the only deliberately-deferred body is Task 2's `PhaseEnded` arm, filled in Task 4 (not a placeholder — it's a documented two-step build with a passing test at each step). No TODO/TBD left at plan end.
- **Type consistency:** `ForcedTriggerPoint` variants (`EnteredLocation { investigator, location }`, `PhaseEnded { phase }`) match across the enum, `collect_forced_hits`, and all three call sites. `fire_forced_triggers`/`ForcedTriggerPoint` are `pub(super)` and called as `super::forced_triggers::…` from sibling `actions`/`phases`. `card_dsl::Phase` ↔ `state::Phase` is bridged by `dsl_phase` (Task 4). `EventPattern::PhaseEnded { phase }` is inert in `trigger_matches` (Task 1) and matched only in `collect_forced_hits`.
- **Verification points flagged inline:** registry struct/field names, the `ability_on_event` builder, `TestGame` helpers, `act_index`/`agenda_index` cursor names, and the `OnceLock` install-collision risk (with the "move to `crates/cards/tests/`" escape hatch) — each step says to confirm against a named existing example.
- **Regression coverage:** Tasks 3 and 4 each run the existing move/phase test sets to prove no-forced-ability paths are unchanged (`Done` on 0 hits).
