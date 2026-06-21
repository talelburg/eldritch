# Keystone K1 — AoO mid-action park/resume — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make attacks of opportunity open their cancel (Dodge 01023) and soak (Guard Dog 01021) reaction windows by running each AoO-provoking action as a suspendable `ActionResolution` frame, so the action's primary effect resumes after the window closes (#293).

**Architecture:** A new transient `Continuation::ActionResolution { investigator, resume: ActionResume }` frame sits above `InvestigatorTurn`. The five AoO-firing basic-action handlers restructure to `validate → spend action → push ActionResolution → drive_aoo`. `drive_aoo` routes attacks of opportunity through the existing `drive_attack_loop` (so they queue cancel/soak windows and suspend) instead of the window-dropping `fire_attacks_of_opportunity`. When the AoO loop pops, the uniform `drive` loop resumes the `ActionResolution` frame, which re-validates (actor still `Active` + the primary's own precondition) and runs the action's primary effect — aborting cleanly (keep spent action + AoO/window effects, suppress primary) on failure.

**Tech Stack:** Rust, `game-core` kernel crate (no_std-friendly, wasm target), `cards` content crate for registry-backed integration tests. Event-sourced `apply(state, action) -> ApplyResult`; serializable `Continuation` enum stack.

## Global Constraints

- **Match CI's strict flags before pushing:** `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`.
- **Handler contract — validate-first / mutate-second:** check every precondition and return `EngineOutcome::Rejected { reason }` with state + events unchanged before any mutation.
- **`game-core` never depends on `cards`:** card-data lookups go through `card_registry::current()`; registry-free unit tests get the no-registry fallback. Tests needing real card abilities live in `crates/cards/tests/` (each file is its own process and may `install(cards::REGISTRY)`).
- **AoO never exhausts the attacker (RR p.7):** *"An enemy does not exhaust while making an attack of opportunity."* The enemy-phase loop always exhausts; the AoO path must not.
- **Card-text / rules citations are verified, never paraphrased from memory.** Dodge 01023 and Guard Dog 01021 text: confirm against `https://arkhamdb.com/card/01023` / `/01021` (incl. FAQ) before asserting behaviour in tests or comments.
- **Event-assertion macros:** use `assert_event!` / `assert_no_event!` / `assert_event_count!` / `assert_event_sequence!` (order-insensitive by default) over raw slice indexing.
- **`Continuation` derives** `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` — every field of a new variant must too (the K1 fields — `InvestigatorId`, `LocationId`, `EnemyId`, an enum of those — already do).

---

### Task 1: Add the `ActionResolution` frame + `ActionResume` enum (type plumbing)

Introduce the frame type and update every exhaustive `Continuation` match so the crate compiles with the variant present but unused. No behaviour change yet.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (the `Continuation` enum near line 508; `awaits_input` line 581; `as_resolution` line 597; `as_resolution_mut` line 620)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resolve_input` match, line 361)
- Test: `crates/game-core/src/state/game_state.rs` (`#[cfg(test)]` module)

**Interfaces:**
- Produces: `Continuation::ActionResolution { investigator: InvestigatorId, resume: ActionResume }`; `enum ActionResume { Move { destination: LocationId }, Investigate, Resource, Engage { enemy: EnemyId }, Draw }`. `ActionResolution::awaits_input()` is `false`; `is_phase_anchor()` is `false`.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` module in `game_state.rs`:

```rust
#[test]
fn action_resolution_frame_never_awaits_input_and_is_not_a_phase_anchor() {
    let f = Continuation::ActionResolution {
        investigator: InvestigatorId(1),
        resume: ActionResume::Resource,
    };
    assert!(!f.awaits_input(), "a mid-action frame is internal, never a prompt");
    assert!(!f.is_phase_anchor(), "a mid-action frame is not a phase anchor");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core action_resolution_frame_never_awaits_input -v`
Expected: FAIL to compile — `no variant named ActionResolution` / `ActionResume`.

- [ ] **Step 3: Add the enum + variant**

In `game_state.rs`, add above `impl Continuation` (near the other frame payload types):

```rust
/// Which action's primary effect a parked [`Continuation::ActionResolution`]
/// frame runs once its attack-of-opportunity loop completes (#293). Carries
/// only the action's *parameters*; board-dependent values (Investigate
/// difficulty, enemy presence) are re-derived live on resume so a mid-action
/// board change is reflected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionResume {
    /// Relocate the investigator (and engaged enemies) to `destination`.
    Move { destination: LocationId },
    /// Begin the Investigate skill test on the investigator's location.
    Investigate,
    /// Gain 1 resource.
    Resource,
    /// Engage `enemy`.
    Engage { enemy: EnemyId },
    /// Draw 1 card (with the empty-deck penalty path).
    Draw,
}
```

Add the variant to the `Continuation` enum (after `AttackLoop`):

```rust
    /// An action paused over its attack-of-opportunity loop (#293, keystone of
    /// #393). Pushed above [`InvestigatorTurn`] when an AoO-provoking action is
    /// taken; the AoO [`AttackLoop`] is its child. On the loop's pop the
    /// `drive` loop resumes this frame: it re-validates (actor still active +
    /// the primary's precondition) and runs the primary effect, then pops.
    /// Transient — it persists across an `apply()` boundary only while a window
    /// suspends the loop. Never awaits input itself.
    ActionResolution {
        /// The acting investigator.
        investigator: InvestigatorId,
        /// Which primary effect to run when the AoO loop completes.
        resume: ActionResume,
    },
```

- [ ] **Step 4: Update the exhaustive matches**

In `awaits_input` (line ~590), add `ActionResolution` to the explicit-`false` arm (otherwise the `other => !other.is_phase_anchor()` fallback wrongly returns `true`):

```rust
            Continuation::InvestigatorTurn { .. }
            | Continuation::AttackLoop { .. }
            | Continuation::ActionResolution { .. } => false,
```

In `as_resolution` and `as_resolution_mut`, add `| Continuation::ActionResolution { .. }` to the `None`-returning list in each.

In `resolve_input` (`dispatch/mod.rs`, alongside the `AttackLoop`/`InvestigatorTurn` defensive arms), add:

```rust
        // A mid-action ActionResolution frame never awaits input — it is only
        // momentarily top inside `drive`. A ResolveInput here is spurious.
        Some(Continuation::ActionResolution { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (a mid-action resolution \
                     frame is top)"
                .into(),
        },
```

(`is_phase_anchor` uses `matches!`, so an unlisted variant is already `false` — no change. `builder.rs::with_phase_anchor` is also `matches!` — no change.)

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p game-core action_resolution_frame_never_awaits_input -v`
Expected: PASS.

- [ ] **Step 6: Verify the whole crate still compiles clean**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core` then `cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: PASS (no non-exhaustive-match errors).

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: add ActionResolution frame + ActionResume enum (K1 of #293)"
```

---

### Task 2: Route AoO through the attack loop with non-exhaust (`drive_aoo`)

Add the source-aware non-exhaust branch to the shared attack loop and a `drive_aoo` entry that fires attacks of opportunity through `drive_attack_loop` (so they queue cancel/soak windows). Not yet called by any handler — proven by a direct unit test.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`process_attacker_dealing` ~line 653; `deal_head_and_maybe_park` ~line 777; new `drive_aoo`)
- Test: `crates/game-core/src/engine/dispatch/combat.rs` (`#[cfg(test)] mod combat_tests`)

**Interfaces:**
- Consumes: `drive_attack_loop(cx, investigator, attackers, source) -> EngineOutcome`; `EnemyAttackSource::{EnemyPhase, AttackOfOpportunity}`.
- Produces: `pub(super) fn drive_aoo(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome`. `process_attacker_dealing` gains a `source: EnemyAttackSource` parameter and only exhausts when `source == EnemyPhase`.

- [ ] **Step 1: Write the failing test**

Add to `combat_tests`:

```rust
#[test]
fn drive_aoo_deals_damage_but_does_not_exhaust_the_attacker() {
    // RR p.7: an enemy does not exhaust while making an attack of opportunity.
    let inv_id = InvestigatorId(1);
    let mut enemy = test_enemy(100, "Ghoul");
    enemy.engaged_with = Some(inv_id);
    enemy.attack_damage = 1;
    enemy.attack_horror = 0;
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_enemy(enemy)
        .build();
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };

    let outcome = super::drive_aoo(&mut cx, inv_id);

    assert!(matches!(outcome, crate::engine::EngineOutcome::Done));
    assert!(
        !cx.state.enemies[&EnemyId(100)].exhausted,
        "AoO must not exhaust the attacker (RR p.7)"
    );
    assert_event!(events, Event::EnemyAttacked { .. });
    assert_no_event!(events, Event::EnemyExhausted { .. });
}
```

(Match the exact `Cx` construction and `EnemyAttacked` event name used by the sibling tests in `combat_tests`; adjust field names to the real `Event` variants if they differ.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core drive_aoo_deals_damage_but_does_not_exhaust -v`
Expected: FAIL — `drive_aoo` not found.

- [ ] **Step 3: Thread `source` into the exhaust decision**

Change `process_attacker_dealing`'s signature to take `source: EnemyAttackSource` and guard the exhaust block (currently unconditional, ~line 688):

```rust
fn process_attacker_dealing(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
    source: EnemyAttackSource,
    cancelled: bool,
) {
    if !cancelled {
        // ... unchanged damaged_survivors + soak-window queueing ...
    }

    // Exhaust only on the enemy phase. AoO never exhausts (RR p.7).
    if source == EnemyAttackSource::EnemyPhase {
        let enemy = cx.state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
            unreachable!(
                "process_attacker_dealing: snapshotted enemy {enemy_id:?} is gone from \
                 state.enemies; this is a state-corruption invariant violation"
            )
        });
        enemy.exhausted = true;
        cx.events.push(Event::EnemyExhausted { enemy: enemy_id });
    }
}
```

In `deal_head_and_maybe_park` (which already has `source`), pass it through:

```rust
    process_attacker_dealing(cx, investigator, enemy_id, source, cancelled);
```

- [ ] **Step 4: Add `drive_aoo`**

Add near `resolve_attacks_for_investigator` in `combat.rs`:

```rust
/// Fire attacks of opportunity from every ready enemy engaged with
/// `investigator`, driving them through the shared attack loop (#293) so each
/// AoO opens its before-attack cancel window (Dodge 01023) and per-soaked-asset
/// reaction window (Guard Dog 01021). Returns [`EngineOutcome::AwaitingInput`]
/// if a window suspends the loop, [`EngineOutcome::Done`] otherwise. Attackers
/// resolve in deterministic [`EnemyId`] order (player-pick is #143/K4). AoO
/// attackers never exhaust (RR p.7) — honored by
/// [`EnemyAttackSource::AttackOfOpportunity`].
pub(super) fn drive_aoo(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let attackers: Vec<EnemyId> = cx
        .state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator) && !e.exhausted)
        .map(|(id, _)| *id)
        .collect();
    drive_attack_loop(cx, investigator, attackers, EnemyAttackSource::AttackOfOpportunity)
}
```

- [ ] **Step 5: Run the test (and the enemy-phase regression) to verify pass**

Run: `cargo test -p game-core drive_aoo_deals_damage_but_does_not_exhaust -v`
Expected: PASS.
Run: `cargo test -p game-core resume_enemy_attack_drains_remaining_attackers_and_advances_cursor -v`
Expected: PASS (enemy-phase loop still exhausts — regression intact).

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs
git commit -m "engine: drive_aoo routes attacks of opportunity through the attack loop, non-exhausting (K1 of #293)"
```

---

### Task 3: The `drive` extension + `resume_action_resolution` + convert Move

The mechanism, proven end-to-end on Move (the most complex primary effect). After this, an AoO with no available reaction is behaviour-preserving, an AoO that defeats the actor suppresses the move, and (covered by Task 9's registry tests) a Dodge/Guard Dog window suspends and resumes the move.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/actions.rs` (`move_action` ~line 246; extract `move_primary_effect`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`drive` ~line 163; add `resume_action_resolution`)
- Test: `crates/game-core/src/engine/dispatch/actions.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: `combat::drive_aoo`; `Continuation::ActionResolution`; `ActionResume::Move`.
- Produces: `pub(super) fn move_primary_effect(cx, investigator, destination) -> EngineOutcome` (the relocation + reveal + Left/Entered emits). `resume_action_resolution(cx) -> EngineOutcome` (central re-validation + per-`resume` dispatch). `drive` resumes `ActionResolution` frames.

- [ ] **Step 1: Write the failing tests**

Add to `actions.rs` tests (use the existing test helpers; an AoO with no registry opens no window, so these cover the no-window + defeat paths):

```rust
#[test]
fn move_with_lethal_aoo_suppresses_relocation_but_keeps_spent_action() {
    // An engaged enemy whose AoO defeats the investigator: the move is
    // suppressed (still at origin), the action point + AoO damage persist.
    // ... build investigator at L1 (connected to L2), 3 actions, 1 health;
    //     engaged enemy with attack_damage >= investigator health ...
    // ... apply PlayerAction::Move { investigator, destination: L2 } ...
    assert_eq!(/* investigator.current_location */, Some(L1), "move suppressed");
    assert_eq!(/* investigator.actions_remaining */, 2, "action still spent");
    assert!(/* investigator not Active */);
}

#[test]
fn move_with_nonlethal_aoo_relocates_after_the_attack() {
    // Engaged enemy, 1 damage, investigator survives: AoO deals damage, then
    // the move resolves (no reaction available, so no window opens).
    // ... assert current_location == Some(L2), engaged enemy moved with it,
    //     investigator damage == 1 ...
}
```

Fill the `...` using the patterns in the existing `move_action` tests in this file (same builder + fixtures).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p game-core move_with_lethal_aoo move_with_nonlethal_aoo -v`
Expected: FAIL (assertions / behaviour mismatch — today the synchronous path already suppresses on defeat, so `move_with_lethal_aoo` may pass; `move_with_nonlethal_aoo` must pass too. The point of these is to *pin* behaviour across the refactor — if both pass pre-refactor, that is the behaviour-preservation baseline; keep them and proceed.)

- [ ] **Step 3: Extract `move_primary_effect`**

Move the post-AoO body of `move_action` (current lines ~353–409: capture engaged set, relocate investigator + engaged enemies, emit `InvestigatorMoved`, `reveal_location`, the `LeftLocation` then `EnteredLocation` emits) into:

```rust
/// The relocation half of a Move, run after its attack-of-opportunity loop
/// completes (#293). Re-derives `from` from the live `current_location` (the AoO
/// never moves the actor) and re-checks the destination is still connected —
/// the §D primary-precondition re-check — suppressing the move (returns `Done`)
/// if it no longer holds. Engaged enemies move with the investigator; the
/// entered location's Forced on-enter abilities become the move's outcome.
pub(super) fn move_primary_effect(
    cx: &mut Cx,
    investigator: InvestigatorId,
    destination: LocationId,
) -> EngineOutcome {
    let Some(from) = cx
        .state
        .investigators
        .get(&investigator)
        .and_then(|inv| inv.current_location)
    else {
        return EngineOutcome::Done; // actor gone/locationless: suppress
    };
    let still_connected = cx
        .state
        .locations
        .get(&from)
        .is_some_and(|l| l.connections.contains(&destination))
        && cx.state.locations.contains_key(&destination);
    if !still_connected {
        return EngineOutcome::Done; // precondition lapsed: suppress
    }
    // ... the verbatim relocation + reveal + LeftLocation/EnteredLocation body
    //     from the original move_action (lines ~357–409), using `from` above ...
}
```

- [ ] **Step 4: Restructure `move_action`**

Replace `move_action`'s tail (from the `fire_attacks_of_opportunity` call through the original relocation body) with: push the frame, drive the AoO loop, and let `drive` resume the frame.

```rust
    // Mutate-second. Charge the action (base 1 + surcharge) last.
    if let Err(rejected) = charge_action(cx, investigator, crate::dsl::ActionClass::Move, "Move") {
        return rejected;
    }

    // Park the move over its attack-of-opportunity loop (#293): push the
    // resume frame, then drive the AoO. If a cancel/soak window opens the loop
    // suspends here; otherwise `drive` resumes the frame and relocates.
    cx.state.continuations.push(crate::state::Continuation::ActionResolution {
        investigator,
        resume: crate::state::ActionResume::Move { destination },
    });
    super::combat::drive_aoo(cx, investigator)
```

Delete the now-extracted relocation body and the old `inv_after_aoo` defeat check (the re-validation gate in `resume_action_resolution` replaces it).

- [ ] **Step 5: Add `resume_action_resolution` + extend `drive`**

In `dispatch/mod.rs`, add:

```rust
/// Resume a parked [`ActionResolution`](crate::state::Continuation::ActionResolution)
/// frame (#293): pop it, run the §D re-validation gate, then dispatch to the
/// action's primary effect. The gate suppresses the primary (returns `Done`,
/// leaving the spent action + AoO/window effects in place) if the actor was
/// defeated mid-action; each primary effect additionally re-checks its own
/// target precondition. Called only by [`drive`] with such a frame on top.
fn resume_action_resolution(cx: &mut Cx) -> EngineOutcome {
    use crate::state::{ActionResume, Continuation};
    let Some(Continuation::ActionResolution { investigator, resume }) =
        cx.state.continuations.pop()
    else {
        unreachable!("resume_action_resolution: top frame is not an ActionResolution");
    };
    // §D re-validation: actor still Active? If not, suppress the primary.
    let active = cx
        .state
        .investigators
        .get(&investigator)
        .is_some_and(|inv| inv.status == crate::state::Status::Active);
    if !active {
        return EngineOutcome::Done;
    }
    match resume {
        ActionResume::Move { destination } => {
            actions::move_primary_effect(cx, investigator, destination)
        }
        // Investigate/Resource/Engage/Draw arms land in Tasks 4–7.
        other => unreachable!("resume_action_resolution: {other:?} not yet wired (K1 tasks 4-7)"),
    }
}
```

Extend `drive`'s loop `match` with an `ActionResolution` arm (before the `_ => return Done` catch-all):

```rust
            Some(Continuation::ActionResolution { .. }) => {
                match resume_action_resolution(cx) {
                    EngineOutcome::Done => {
                        // Primary ran (or was suppressed) + frame popped; loop
                        // on — the InvestigatorTurn frame beneath is now top.
                    }
                    other => return other, // primary effect suspended (e.g. skill test)
                }
            }
```

(Add `use crate::state::Continuation;` to `drive` if not already in scope.)

- [ ] **Step 6: Run tests to verify pass**

Run: `cargo test -p game-core move_with_lethal_aoo move_with_nonlethal_aoo -v`
Expected: PASS.
Run: `RUSTFLAGS="-D warnings" cargo test -p game-core` (full crate — pins the no-AoO move path + every other suite still green).
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch/actions.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: Move runs as an ActionResolution frame over its AoO loop (K1 of #293)"
```

---

### Task 4: Convert Investigate

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/actions.rs` (`investigate` ~line 30; extract `investigate_primary_effect`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resume_action_resolution` Investigate arm)
- Test: `crates/game-core/src/engine/dispatch/actions.rs`

**Interfaces:**
- Produces: `pub(super) fn investigate_primary_effect(cx, investigator) -> EngineOutcome` — re-derives the location + effective shroud and starts the Investigate skill test; re-checks the location is still revealed (suppress on failure).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn investigate_with_nonlethal_aoo_starts_the_test_after_the_attack() {
    // Engaged enemy (1 damage), revealed location: AoO deals damage, then the
    // Investigate skill test begins (outcome is AwaitingInput at the commit
    // window). Assert the EnemyAttacked event precedes the test, and the
    // investigator took 1 damage and is still Active.
}

#[test]
fn investigate_with_lethal_aoo_suppresses_the_test() {
    // AoO defeats the investigator: no skill test starts (outcome Done), action
    // spent, investigator not Active.
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p game-core investigate_with_nonlethal_aoo investigate_with_lethal_aoo -v`
Expected: FAIL.

- [ ] **Step 3: Extract `investigate_primary_effect`**

Move `investigate`'s post-AoO body (re-read location, compute effective shroud + difficulty, `start_skill_test(...)`) into:

```rust
/// The skill-test half of an Investigate, run after its AoO loop (#293).
/// Re-reads the location + effective shroud live and re-checks it is still
/// revealed (the §D precondition re-check); suppresses (returns `Done`) if not.
pub(super) fn investigate_primary_effect(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    let Some(location_id) = cx
        .state
        .investigators
        .get(&investigator)
        .and_then(|inv| inv.current_location)
    else {
        return EngineOutcome::Done;
    };
    let Some(location) = cx.state.locations.get(&location_id) else {
        return EngineOutcome::Done;
    };
    if !location.revealed {
        return EngineOutcome::Done; // precondition lapsed
    }
    // ... verbatim effective-shroud + difficulty + start_skill_test body from
    //     the original investigate (lines ~62–104) ...
}
```

Restructure `investigate`'s tail (after `spend_one_action`) to push the frame + `drive_aoo`, mirroring Task 3:

```rust
    spend_one_action(cx, investigator);
    cx.state.continuations.push(crate::state::Continuation::ActionResolution {
        investigator,
        resume: crate::state::ActionResume::Investigate,
    });
    super::combat::drive_aoo(cx, investigator)
```

Delete the old `inv_after_aoo` defeat check.

- [ ] **Step 4: Wire the resume arm**

In `resume_action_resolution`, replace the Investigate part of the catch-all with:

```rust
        ActionResume::Investigate => actions::investigate_primary_effect(cx, investigator),
```

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo test -p game-core investigate_with_nonlethal_aoo investigate_with_lethal_aoo -v` then `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/actions.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: Investigate runs as an ActionResolution frame (K1 of #293)"
```

---

### Task 5: Convert Resource

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/actions.rs` (`resource_action` ~line 114; extract `resource_primary_effect`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (Resource arm)
- Test: `crates/game-core/src/engine/dispatch/actions.rs`

**Interfaces:**
- Produces: `pub(super) fn resource_primary_effect(cx, investigator) -> EngineOutcome` — gains 1 resource (no target precondition beyond actor-Active, handled centrally).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn resource_with_lethal_aoo_suppresses_the_gain() {
    // AoO defeats the investigator: no ResourcesGained, action spent.
}

#[test]
fn resource_with_no_engaged_enemy_gains_normally() {
    // No engaged enemy: behaviour-preserving — resources +1, Done.
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p game-core resource_with_lethal_aoo resource_with_no_engaged_enemy -v`
Expected: FAIL (compile — no `resource_primary_effect`).

- [ ] **Step 3: Extract + restructure**

Extract the gain body (the `inv_mut.resources = ... saturating_add(1)` + `ResourcesGained` event) into `resource_primary_effect(cx, investigator) -> EngineOutcome` returning `Done`. Restructure `resource_action`'s tail to push `ActionResume::Resource` + `drive_aoo`; delete the old defeat check.

- [ ] **Step 4: Wire the resume arm**

```rust
        ActionResume::Resource => actions::resource_primary_effect(cx, investigator),
```

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo test -p game-core resource_with_lethal_aoo resource_with_no_engaged_enemy -v` then `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/actions.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: Resource runs as an ActionResolution frame (K1 of #293)"
```

---

### Task 6: Convert Engage

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/actions.rs` (`engage` ~line 164; extract `engage_primary_effect`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (Engage arm)
- Test: `crates/game-core/src/engine/dispatch/actions.rs`

**Interfaces:**
- Produces: `pub(super) fn engage_primary_effect(cx, investigator, enemy) -> EngineOutcome` — re-checks the enemy still exists, is co-located, and is not already engaged with the investigator (suppress on failure), then sets `engaged_with` + emits `EnemyEngaged`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn engage_with_nonlethal_aoo_engages_after_the_attack() {
    // A second engaged enemy AoOs (1 damage); the target (co-located, not yet
    // engaged) is then engaged. Assert EnemyAttacked precedes EnemyEngaged, the
    // target's engaged_with == Some(investigator), investigator survived.
}

#[test]
fn engage_with_lethal_aoo_suppresses_the_engagement() {
    // The other engaged enemy's AoO defeats the investigator: no EnemyEngaged.
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p game-core engage_with_nonlethal_aoo engage_with_lethal_aoo -v`
Expected: FAIL.

- [ ] **Step 3: Extract + restructure**

Extract the engagement body into `engage_primary_effect`, re-checking the target enemy (its existence + co-location + not-already-engaged) live and returning `Done` if the precondition lapsed:

```rust
pub(super) fn engage_primary_effect(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    let inv_location = cx
        .state
        .investigators
        .get(&investigator)
        .and_then(|inv| inv.current_location);
    let Some(enemy) = cx.state.enemies.get(&enemy_id) else {
        return EngineOutcome::Done; // target gone
    };
    if enemy.engaged_with == Some(investigator) || enemy.current_location != inv_location {
        return EngineOutcome::Done; // precondition lapsed
    }
    let enemy_mut = cx.state.enemies.get_mut(&enemy_id).expect("checked");
    enemy_mut.engaged_with = Some(investigator);
    cx.events.push(Event::EnemyEngaged { enemy: enemy_id, investigator });
    EngineOutcome::Done
}
```

Restructure `engage`'s tail to push `ActionResume::Engage { enemy: enemy_id }` + `drive_aoo`; delete the old defeat check.

- [ ] **Step 4: Wire the resume arm**

```rust
        ActionResume::Engage { enemy } => actions::engage_primary_effect(cx, investigator, enemy),
```

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo test -p game-core engage_with_nonlethal_aoo engage_with_lethal_aoo -v` then `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/actions.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: Engage runs as an ActionResolution frame (K1 of #293)"
```

---

### Task 7: Convert Draw

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/cards.rs` (`draw` ~line 247; extract `draw_primary_effect`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (Draw arm; remove the `unreachable!` catch-all — now total)
- Test: `crates/game-core/src/engine/dispatch/cards.rs`

**Interfaces:**
- Produces: `pub(super) fn draw_primary_effect(cx, investigator) -> EngineOutcome` — wraps `draw_one_with_deckout` (no target precondition beyond actor-Active). After this task `resume_action_resolution`'s match is exhaustive over `ActionResume`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn draw_with_lethal_aoo_suppresses_the_draw() {
    // AoO defeats the investigator: hand unchanged, action spent.
}

#[test]
fn draw_with_no_engaged_enemy_draws_normally() {
    // Behaviour-preserving — one card drawn, Done.
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p game-core draw_with_lethal_aoo draw_with_no_engaged_enemy -v`
Expected: FAIL.

- [ ] **Step 3: Extract + restructure**

Extract the draw body into `draw_primary_effect(cx, investigator) -> EngineOutcome` (calls `draw_one_with_deckout(cx, investigator)`; returns `Done`). Restructure `draw`'s tail to push `ActionResume::Draw` + `super::combat::drive_aoo(cx, investigator)`; delete the old `still_active` defeat check.

- [ ] **Step 4: Wire the resume arm + make the match total**

```rust
        ActionResume::Draw => cards::draw_primary_effect(cx, investigator),
```

Remove the `other => unreachable!(...)` arm — all five `ActionResume` variants are now handled.

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo test -p game-core draw_with_lethal_aoo draw_with_no_engaged_enemy -v` then `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/cards.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: Draw runs as an ActionResolution frame; resume match now total (K1 of #293)"
```

---

### Task 8: Delete `fire_attacks_of_opportunity` + refresh combat docs

No handler calls the window-dropping AoO path anymore. Remove it and the now-stale `TODO(#293)` doc comments.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (delete `fire_attacks_of_opportunity` ~line 546; update `enemy_attack` doc ~line 459 and `resume_enemy_attack` doc ~line 869)

- [ ] **Step 1: Confirm there are no remaining callers**

Run: `grep -rn "fire_attacks_of_opportunity" crates/`
Expected: only the definition + doc-comment references remain.

- [ ] **Step 2: Delete the function and update docs**

Delete `fire_attacks_of_opportunity`. In `enemy_attack`'s doc, replace the paragraph describing the AoO caller "deliberately dropping the list" with: the AoO caller now drives the loop (`drive_aoo`), so both callers queue soak windows; the survivor list is never dropped (#293). In `resume_enemy_attack`'s doc, mark the `AttackOfOpportunity` arm **reachable** (the mid-action park, #293) rather than "currently unreachable."

- [ ] **Step 3: Verify the build + docs**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core` and `RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features`
Expected: PASS (no dead-code warning, no broken intra-doc links to the deleted fn).

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs
git commit -m "engine: remove the window-dropping fire_attacks_of_opportunity; AoO now drives the loop (K1 of #293)"
```

---

### Task 9: Integration — Dodge cancels an AoO; Guard Dog retaliates against an AoO

The window-suspend/resume path needs real card abilities, so it lives in `crates/cards/tests/`. This is the #293 acceptance + the registry-backed slice of the §D keystone matrix.

**Files:**
- Modify/Create: `crates/cards/tests/guard_dog_soak.rs` (extend the existing AoO case — currently asserts only "no window stranded")
- Create (or extend): `crates/cards/tests/dodge_aoo.rs`

**Interfaces:**
- Consumes: `cards::REGISTRY` (installed per test process); `PlayerAction::Move`/`Investigate`; `InputResponse` for window resolution. Follow the existing `crates/cards/tests/play_card.rs` / `guard_dog_soak.rs` harness pattern.

- [ ] **Step 1: Verify the card text before asserting**

WebFetch `https://arkhamdb.com/card/01023` (Dodge) and `https://arkhamdb.com/card/01021` (Guard Dog), **including FAQ**. Confirm: Dodge — *"Cancel all damage and horror dealt by [an] attacking enemy"* as a reaction to an enemy attacking an investigator at your location; Guard Dog — the retaliate reaction when it takes damage. Record the verified text in each test's header comment.

- [ ] **Step 2: Write the failing test — Guard Dog retaliates against an AoO**

In `guard_dog_soak.rs`, add a test: investigator controls Guard Dog (in play), engaged with a ready enemy, takes a basic action (Move) that provokes the AoO; the AoO damage soaks onto Guard Dog, opening the `AfterEnemyAttackDamagedAsset` window; resolve the window so Guard Dog's retaliate deals damage back to the attacker. Assert: the attacker took retaliate damage, the move completed after the window closed, the attacker did **not** exhaust (RR p.7).

- [ ] **Step 3: Write the failing test — Dodge cancels an AoO**

In `dodge_aoo.rs`: investigator with Dodge in hand, engaged with a ready enemy, takes Move; the AoO opens the `BeforeEnemyAttack` window; play Dodge to cancel; assert no damage/horror dealt by the AoO, the move resolved, the attacker did not exhaust.

- [ ] **Step 4: Run to verify they fail (pre-implementation baseline)**

Run: `cargo test -p cards --test guard_dog_soak` and `cargo test -p cards --test dodge_aoo`
Expected: these are the acceptance tests; they should PASS now that Tasks 1–8 wired the mechanism. If either FAILs, debug the suspend/resume chain (window opens → `AwaitingInput`; `ResolveInput` → `resume_enemy_attack` pops `AttackLoop` → `drive` resumes `ActionResolution` → primary effect). Do not weaken the assertions to pass.

- [ ] **Step 5: Full CI gauntlet**

Run all strict-flag jobs from Global Constraints:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add crates/cards/tests/guard_dog_soak.rs crates/cards/tests/dodge_aoo.rs
git commit -m "test: Dodge cancels + Guard Dog retaliates against attacks of opportunity (closes #293)"
```

---

## Self-Review

- **Spec coverage (K1 section of the design):** ✅ `ActionResolution` frame + `ActionResume` enum (Task 1); restructure the five basic-action handlers (Tasks 3–7); AoO drives the loop / stops dropping survivors (Task 2 + 8); RR p.7 non-exhaust preserved (Task 2); `resume_enemy_attack` AoO arm + `drive` extension + re-validation gate (Task 3); defensive `resolve_input` arm (Task 1); Dodge-cancel + Guard-Dog-retaliate windows on AoO (Task 9). The §D matrix's defeat-mid-action + no-window paths are unit-tested (Tasks 3–7); the window paths are integration-tested (Task 9).
- **Deferred-by-design (not K1):** retaliate windows (#379/K2), play-card/activate AoO (#361/#378/K3), player attack-order + enemy-phase frame extension (#143/K4), damage distribution (#44/K5). Not gaps — sequenced sub-slices.
- **Placeholder scan:** the `...` markers in Tasks 3–4 are explicit "reproduce the verbatim existing body" instructions with line references, not under-specified logic; every new function has a complete signature + body or a faithful-move instruction.
- **Type consistency:** `drive_aoo`, `move_primary_effect`/`investigate_primary_effect`/`resource_primary_effect`/`engage_primary_effect`/`draw_primary_effect`, and `resume_action_resolution` names + signatures are used identically across the tasks that define and call them; `ActionResume` variant names (`Move{destination}`, `Investigate`, `Resource`, `Engage{enemy}`, `Draw`) match between Task 1 and Tasks 3–7.

## Risks & notes for the implementer

- **The riskiest seam** is the resume chain: an AoO window closing must unwind `resume_enemy_attack (pops AttackLoop, returns Done) → resolve_input → apply_player_action → drive → resume_action_resolution`. If a converted action's primary effect never runs after a window, trace that chain — `drive` must see `ActionResolution` on top after the `AttackLoop` pops.
- **Event names in tests** (`EnemyAttacked`, `EnemyExhausted`, `ResourcesGained`, `EnemyEngaged`, `InvestigatorMoved`) — confirm the exact variant + field names against `crates/game-core/src/event.rs`; the plan uses the names visible in the current handlers.
- **`move_primary_effect`'s `EnteredLocation` emit can itself suspend** (2+ simultaneous forced, #213) — it returns that outcome, which `drive` surfaces. This is preserved from the original `move_action` tail; don't swallow it.
- **Keep assertions honest** (verification-before-completion): if Task 9's acceptance tests fail, fix the engine, never the assertion.
