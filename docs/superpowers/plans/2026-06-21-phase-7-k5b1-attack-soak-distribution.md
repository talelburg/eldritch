# K5b-1 — Interactive Soak Distribution (attack path) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When an enemy attack deals damage/horror and the defending investigator controls a soaker with capacity, let the player distribute each point across themselves and eligible soakers (RR p.7), one point at a time, replacing the soak-first auto-assignment — for the **attack path** (the contained, highest-value case).

**Architecture:** Build the shared interactive-distribution machinery — a `Continuation::DamageAssignment` frame that accumulates an `Assignment` via per-point `PickSingle` prompts (K4's substrate), gated to prompt only when a real choice exists — and wire it into the enemy-attack dealing path. Separate "decide the assignment" (may suspend) from "finish dealing the attacker given a placed assignment" (deterministic: place → queue soak windows → exhaust → continue the loop). The effect/non-attack path reuses this machinery in the follow-on plan K5b-2.

**Tech Stack:** Rust, `game-core` engine crate + `cards` integration tests. No new dependencies. Engine-only.

## Global Constraints

- Match CI's strict flags before declaring a task done: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`, `cargo build -p web --target wasm32-unknown-unknown`, `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- Validate-first / mutate-second; suspensions are serializable `Continuation` frames resumed via `ResolveInput` (top-frame dispatch in `resolve_input`).
- RR p.7 (verified, in the design spec): the defending player *assigns* each point of damage/horror, then all assigned tokens are *placed simultaneously*, then defeat is checked. So per-point assignment builds the `Assignment`; a single `place_assignment` at the end preserves simultaneity. `place_assignment` is unchanged.
- Invalid `ResolveInput` rejects and **leaves the frame** for retry (the K4 / HunterMove contract).
- Gate: prompt only when ≥1 soaker is eligible for the point being assigned; otherwise assign deterministically (no `AwaitingInput`). The no-soaker case is byte-identical to today.
- Commit subjects: `scope: description` (scope = `engine`). Feature branch `engine/soak-interactive` (already created; the K5 design is on `main`).
- This plan is **K5b-1 (attack path) only.** K5b-2 (effect path) is a follow-on plan; #44 stays open until it lands.

## File structure

- `crates/game-core/src/state/game_state.rs` — `Continuation::DamageAssignment` variant + `DamageSource` enum; `Assignment` gains `Clone + Serialize + Deserialize` (it moves onto a frame).
- `crates/game-core/src/engine/dispatch/combat.rs` — the distribution machinery (`DistributionTarget`, `eligible_targets`, `advance_distribution`, `suspend_or_finish_assignment`, `resume_damage_assignment`, `finish_attacker_after_assignment`) and the attack-dealing restructure (`process_attacker_dealing` / `deal_head_and_maybe_park` split into decide → finish).
- `crates/game-core/src/engine/dispatch/mod.rs` — `resolve_input` routes `DamageAssignment` → `resume_damage_assignment`.
- `crates/cards/tests/soak_distribution.rs` — registry-backed attack-path distribution tests.

---

### Task 1: State — `DamageAssignment` frame + `DamageSource`, and make `Assignment` serializable

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (the `Continuation` enum; serde test module)
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (the `Assignment` struct derives)

**Interfaces:**
- Produces: `Continuation::DamageAssignment { investigator: InvestigatorId, remaining_damage: u8, remaining_horror: u8, assignment: Assignment, source: DamageSource }` and `enum DamageSource { EnemyAttack { enemy: EnemyId, remaining_attackers: Vec<EnemyId>, attack_source: EnemyAttackSource }, Effect }`. Consumed by Tasks 2-4.

- [ ] **Step 1: Make `Assignment` serializable + clonable**

In `combat.rs`, the `Assignment` struct (currently `#[derive(Debug, Default, PartialEq, Eq)]`) moves onto a continuation frame, so add `Clone, Serialize, Deserialize`:

```rust
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Assignment {
```

(Change visibility `pub(super)` → `pub(crate)` so `game_state.rs` can name it in the `Continuation` variant. Its fields stay `pub`.)

- [ ] **Step 2: Add the `DamageSource` enum + `DamageAssignment` variant**

In `game_state.rs`, near the other attack-loop types (after `AttackLoopStage`), add:

```rust
/// How a [`Continuation::DamageAssignment`] resumes once the player has
/// finished distributing the harm (#44/K5b).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DamageSource {
    /// An enemy attack: after placement, queue soak reaction windows for the
    /// damaged survivors, exhaust the attacker (enemy phase), and continue the
    /// attack loop over `remaining_attackers`.
    EnemyAttack {
        /// The attacking enemy (for the soak window + exhaust).
        enemy: EnemyId,
        /// Attackers not yet resolved, in resolution order (head already removed).
        remaining_attackers: Vec<EnemyId>,
        /// Which loop drives this attack.
        attack_source: EnemyAttackSource,
    },
    /// A card/treachery `Effect::Deal` (K5b-2): after placement, return `Done`
    /// so the effect walk continues. Reserved here; wired in K5b-2.
    Effect,
}
```

and the `Continuation` variant (after `AttackLoop`):

```rust
    /// An in-progress player distribution of an attack's / effect's damage +
    /// horror across eligible soakers and the investigator (#44/K5b, RR p.7).
    /// Accumulates `assignment` via per-point `PickSingle` prompts; when both
    /// `remaining_*` reach 0, the assignment is placed once (simultaneous) and
    /// the loop resumes by `source`. The top frame while prompting (it *is* the
    /// prompt); resumed via `ResolveInput` by `resume_damage_assignment`.
    DamageAssignment {
        /// The investigator taking the harm.
        investigator: InvestigatorId,
        /// Damage points still to assign.
        remaining_damage: u8,
        /// Horror points still to assign.
        remaining_horror: u8,
        /// Accumulating assignment (placed when both counters hit 0).
        assignment: crate::engine::dispatch::combat::Assignment,
        /// How to resume after placement.
        source: DamageSource,
    },
```

(Confirm the import path for `Assignment` — it is `crate::engine::dispatch::combat::Assignment` once `pub(crate)`. Add `EnemyAttackSource` / `EnemyId` to scope if not already imported in this module — they are, used by `AttackLoop`.)

- [ ] **Step 3: Add a serde round-trip test**

In `game_state.rs`'s continuation serde test module:

```rust
#[test]
fn damage_assignment_frame_round_trips_through_serde() {
    use crate::engine::dispatch::combat::Assignment;
    use crate::state::{Continuation, DamageSource, EnemyAttackSource, EnemyId};
    let mut state = GameStateBuilder::new().build();
    state.continuations.push(Continuation::DamageAssignment {
        investigator: InvestigatorId(1),
        remaining_damage: 2,
        remaining_horror: 0,
        assignment: Assignment::default(),
        source: DamageSource::EnemyAttack {
            enemy: EnemyId(5),
            remaining_attackers: vec![EnemyId(6)],
            attack_source: EnemyAttackSource::EnemyPhase,
        },
    });
    let json = serde_json::to_string(&state).expect("serialize");
    let back: GameState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.continuations, state.continuations);
}
```

- [ ] **Step 4: Build + run**

Run: `cargo test -p game-core --lib damage_assignment_frame_round_trips_through_serde`
Expected: PASS. (If `resolve_input`/other matches on `Continuation` now fail to compile for non-exhaustive arms, that's expected — Task 4 adds the `DamageAssignment` arm; for now add a temporary `Continuation::DamageAssignment { .. } => unreachable!("wired in K5b-1 Task 4")` to any match that breaks, and a `DamageSource::Effect` arm where needed, to keep this task compiling. Note each so Task 4 replaces them.)

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/combat.rs
git commit -m "engine: add DamageAssignment frame + DamageSource for soak distribution (K5b-1 of #44)"
```

---

### Task 2: Refactor the attack-dealing tail into `finish_attacker_after_assignment` (behaviour-preserving)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`process_attacker_dealing`, `deal_head_and_maybe_park`)
- Test: existing combat/phases attack-loop suite (regression guard)

**Interfaces:**
- Produces: `fn finish_attacker_after_assignment(cx, investigator, enemy_id, attack_source, remaining_attackers, assignment) -> Option<EngineOutcome>` — given a *computed* `Assignment`, place it, queue soak windows for survivors, exhaust the attacker (enemy phase), then either park on a soak window (`Some(AwaitingInput)`) or continue the loop over `remaining_attackers` (`Some(outcome)` from `drive_attack_loop`+tail, or `None` is not used — see below). Consumed by Task 3 (both the no-prompt synchronous path and the resume).

This is a pure refactor: split `process_attacker_dealing` so the deterministic "place + queue + exhaust" tail is callable given an already-built `Assignment`, while the assignment is still built soak-first synchronously here.

- [ ] **Step 1: Extract the place/queue/exhaust tail**

Replace the body of `process_attacker_dealing` so it builds the soak-first assignment, then delegates placement+windows+exhaust to a new helper. Add `finish_attacker_after_assignment` and refactor `process_attacker_dealing` to call it:

```rust
/// Place a computed `assignment` for one attacker, queue a soak reaction window
/// per damaged survivor, and exhaust the attacker (enemy phase only). The
/// deterministic tail shared by the no-prompt synchronous path and the
/// interactive `resume_damage_assignment` (#44/K5b). `assignment` is already
/// built (soak-first or player-chosen); this never prompts.
fn place_queue_exhaust(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
    attack_source: EnemyAttackSource,
    assignment: &Assignment,
) {
    let damaged_survivors = place_assignment(cx, investigator, assignment);
    for asset in damaged_survivors {
        let _ = super::emit::emit_event(
            cx,
            &super::emit::TimingEvent::EnemyAttackDamagedSelf {
                asset,
                enemy: enemy_id,
                controller: investigator,
            },
        );
    }
    if attack_source == EnemyAttackSource::EnemyPhase {
        let enemy = cx.state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
            unreachable!(
                "place_queue_exhaust: snapshotted enemy {enemy_id:?} is gone from \
                 state.enemies; state-corruption invariant violation"
            )
        });
        enemy.exhausted = true;
        cx.events.push(Event::EnemyExhausted { enemy: enemy_id });
    }
}
```

Then rewrite `process_attacker_dealing` to build the soak-first assignment and call it (cancelled → empty assignment so nothing is placed, but the exhaust still runs):

```rust
fn process_attacker_dealing(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
    source: EnemyAttackSource,
    cancelled: bool,
) {
    let assignment = if cancelled {
        // Cancelled (Dodge 01023, RR p.6): no damage/horror dealt; the attack
        // is still "made", so the attacker still exhausts below.
        Assignment::default()
    } else {
        let enemy = cx.state.enemies.get(&enemy_id).unwrap_or_else(|| {
            unreachable!("process_attacker_dealing: enemy {enemy_id:?} gone; invariant")
        });
        let (damage, horror) = (enemy.attack_damage, enemy.attack_horror);
        let soakers = build_soakers(cx.state, investigator);
        assign_attack(&soakers, damage, horror)
    };
    place_queue_exhaust(cx, investigator, enemy_id, source, &assignment);
}
```

(Note: `enemy_attack` is no longer the path the enemy phase uses to read damage/horror — `process_attacker_dealing` now reads them directly. Keep `enemy_attack` only if other callers exist; grep — if `drive_aoo`/`drive_retaliate` rely on `process_attacker_dealing` (they do, via `deal_head_and_maybe_park`), `enemy_attack` may become unused. If unused after this task, delete it and its doc; if still used, leave it. Resolve by grep before committing.)

- [ ] **Step 2: Run the attack-loop suite**

Run: `cargo test -p game-core --lib engine::dispatch::combat && cargo test -p game-core --lib engine::dispatch::phases::enemy_phase`
Expected: PASS — soak-first assignment unchanged, placement/exhaust order preserved (the `EnemyAttackDamagedSelf` window still precedes `EnemyExhausted`).

- [ ] **Step 3: Run the registry-backed soak integration suite**

Run: `cargo test -p cards --test guard_dog_soak && cargo test -p cards --test non_attack_soak`
Expected: PASS (the C5b + K5a behaviour is intact).

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs
git commit -m "engine: extract place_queue_exhaust attacker tail (K5b-1 of #44)"
```

---

### Task 3: The interactive distribution machinery + attack-path wiring

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (the distribution functions + `deal_head_and_maybe_park`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resolve_input` routing)

**Interfaces:**
- Consumes: `DamageAssignment`/`DamageSource` (Task 1), `place_queue_exhaust` (Task 2), `finish_attack_loop`/`park_attack_loop_beneath_window`/`drive_attack_loop` (existing), `super::hunters::candidate_options` (existing).
- Produces: `pub(super) fn resume_damage_assignment(cx, response) -> EngineOutcome`.

**Design invariant (read first).** The `DamageAssignment` frame's `remaining_damage`/`remaining_horror` are the **authoritative count of points still to assign**, kept in lockstep with `assignment` by `advance_distribution` taking them `&mut` and decrementing as it auto-assigns the deterministic tail. The synchronous (no-prompt) attack path stays **inside** `drive_attack_loop` (it must NOT re-drive the remaining attackers); only the **suspended** path parks the frame (taking `remaining_attackers` into it) and its resume — running *outside* `drive_attack_loop` — re-drives. So: don't `mem::take(attackers)` until the suspend branch.

- [ ] **Step 1: Write the failing gate test**

Add to `combat.rs`'s `combat_tests`:

```rust
#[test]
fn advance_distribution_drains_without_soakers_and_prompts_with_one() {
    // No soaker → fully deterministic: all damage to the investigator, drained.
    let mut asg = Assignment::default();
    let (mut d, mut h) = (2u8, 0u8);
    assert!(advance_distribution(&[], &mut d, &mut h, &mut asg).is_some());
    assert_eq!((d, h, asg.investigator_damage), (0, 0, 2));

    // A soaker with capacity → a damage point is contested → prompt (None),
    // and the counters still show the un-assigned points.
    let soaker = Soaker { instance: crate::state::CardInstanceId(1), remaining_health: 3, remaining_sanity: 0 };
    let mut asg2 = Assignment::default();
    let (mut d2, mut h2) = (2u8, 0u8);
    assert!(advance_distribution(&[soaker], &mut d2, &mut h2, &mut asg2).is_none());
    assert_eq!((d2, h2), (2, 0), "nothing auto-assigned while a soaker can take the point");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core --lib distribution_suspends_with_a_soaker_and_finishes_without`
Expected: FAIL — `advance_distribution` not defined.

- [ ] **Step 3: Implement the per-point machinery**

Add to `combat.rs`:

```rust
/// A target for one point of soak distribution (#44/K5b): the investigator
/// itself, or a controlled soaker asset instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DistributionTarget {
    Investigator,
    Asset(CardInstanceId),
}

/// The eligible targets for one point of `harm` (damage or horror), given the
/// soakers and the assignment-so-far: always the investigator, plus each soaker
/// with remaining capacity for that harm type (printed remaining − already
/// assigned in `assignment`).
fn eligible_targets(
    soakers: &[Soaker],
    assignment: &Assignment,
    damage_point: bool,
) -> Vec<DistributionTarget> {
    let mut targets = vec![DistributionTarget::Investigator];
    for s in soakers {
        let assigned = if damage_point {
            assignment.asset_damage.get(&s.instance).copied().unwrap_or(0)
        } else {
            assignment.asset_horror.get(&s.instance).copied().unwrap_or(0)
        };
        let cap = if damage_point { s.remaining_health } else { s.remaining_sanity };
        if cap.saturating_sub(assigned) > 0 {
            targets.push(DistributionTarget::Asset(s.instance));
        }
    }
    targets
}

/// Advance the distribution deterministically as far as possible, keeping the
/// `remaining_*` counters and `assignment` in lockstep (decrementing a counter
/// as it auto-assigns that point). Returns `Some(())` when both counters drain
/// with no choice left, or `None` the moment a point has a soaker option (2+
/// eligible targets) — the caller then prompts. Damage points first, then
/// horror; a point with only the investigator eligible is auto-assigned to the
/// investigator (no soaker can take it), no prompt.
fn advance_distribution(
    soakers: &[Soaker],
    remaining_damage: &mut u8,
    remaining_horror: &mut u8,
    assignment: &mut Assignment,
) -> Option<()> {
    while *remaining_damage > 0 {
        if eligible_targets(soakers, assignment, true).len() > 1 {
            return None; // a damage point has a soaker option → prompt
        }
        assignment.investigator_damage =
            assignment.investigator_damage.saturating_add(*remaining_damage);
        *remaining_damage = 0;
    }
    while *remaining_horror > 0 {
        if eligible_targets(soakers, assignment, false).len() > 1 {
            return None; // a horror point has a soaker option → prompt
        }
        assignment.investigator_horror =
            assignment.investigator_horror.saturating_add(*remaining_horror);
        *remaining_horror = 0;
    }
    Some(())
}

/// Credit one assigned point of `damage_point` (else horror) to `target`.
fn credit_point(assignment: &mut Assignment, target: DistributionTarget, damage_point: bool) {
    match (target, damage_point) {
        (DistributionTarget::Investigator, true) => assignment.investigator_damage += 1,
        (DistributionTarget::Investigator, false) => assignment.investigator_horror += 1,
        (DistributionTarget::Asset(id), true) => *assignment.asset_damage.entry(id).or_insert(0) += 1,
        (DistributionTarget::Asset(id), false) => *assignment.asset_horror.entry(id).or_insert(0) += 1,
    }
}

/// Build the `PickSingle` over the eligible targets for the next point (the top
/// `DamageAssignment` frame must already be in place). Damage points precede horror.
fn prompt_current_point(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let Some(Continuation::DamageAssignment { remaining_damage, remaining_horror, assignment, .. }) =
        cx.state.continuations.last()
    else {
        unreachable!("prompt_current_point: top frame is not DamageAssignment");
    };
    let (rd, rh) = (*remaining_damage, *remaining_horror);
    let assignment = assignment.clone();
    let soakers = build_soakers(cx.state, investigator);
    let damage_point = rd > 0;
    let targets = eligible_targets(&soakers, &assignment, damage_point);
    let kind = if damage_point { "damage" } else { "horror" };
    let prompt = format!(
        "Investigator {investigator:?}: assign 1 {kind} to which target? ({rd} damage / {rh} horror left)"
    );
    EngineOutcome::AwaitingInput {
        request: InputRequest::choice(prompt, super::hunters::candidate_options(&targets)),
        resume_token: ResumeToken(0),
    }
}
```

- [ ] **Step 4: Implement `resume_damage_assignment`**

The resume runs **outside** `drive_attack_loop`, so on completion it must re-drive the remaining attackers itself (contrast the synchronous attack path in Step 5, which stays *inside* the loop). Add:

```rust
/// Resume a soak distribution with the player's `PickSingle`: credit one point
/// to the chosen target, decrement that counter, then advance — re-prompt if a
/// point is still contested, else place once (simultaneous) and resume by
/// source. Invalid pick → reject, keep the frame (the HunterMove contract).
pub(super) fn resume_damage_assignment(
    cx: &mut Cx,
    response: &crate::action::InputResponse,
) -> EngineOutcome {
    use crate::state::{Continuation, DamageSource};
    let Some(Continuation::DamageAssignment {
        investigator,
        mut remaining_damage,
        mut remaining_horror,
        mut assignment,
        source,
    }) = cx.state.continuations.last().cloned()
    else {
        unreachable!("resume_damage_assignment: top frame is not DamageAssignment");
    };
    let crate::action::InputResponse::PickSingle(OptionId(i)) = response else {
        return EngineOutcome::Rejected {
            reason: format!("ResolveInput: damage distribution expects PickSingle, got {response:?}")
                .into(),
        };
    };
    let damage_point = remaining_damage > 0;
    let soakers = build_soakers(cx.state, investigator);
    let targets = eligible_targets(&soakers, &assignment, damage_point);
    let Some(target) = targets.get(*i as usize).copied() else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: distribution option {i} out of range (0..{})",
                targets.len()
            )
            .into(),
        };
    };
    // Valid: pop the frame we validated against, credit the point, advance.
    cx.state.continuations.pop();
    credit_point(&mut assignment, target, damage_point);
    if damage_point {
        remaining_damage -= 1;
    } else {
        remaining_horror -= 1;
    }
    if advance_distribution(&soakers, &mut remaining_damage, &mut remaining_horror, &mut assignment)
        .is_none()
    {
        // Still contested: re-park with the updated counters/assignment, re-prompt.
        cx.state.continuations.push(Continuation::DamageAssignment {
            investigator,
            remaining_damage,
            remaining_horror,
            assignment,
            source,
        });
        return prompt_current_point(cx, investigator);
    }
    // Drained → place once, then resume by source (re-driving the loop here,
    // since we are outside `drive_attack_loop`).
    match source {
        DamageSource::EnemyAttack { enemy, remaining_attackers, attack_source } => {
            place_queue_exhaust(cx, investigator, enemy, attack_source, &assignment);
            if cx.state.open_windows().is_empty() {
                let out = drive_attack_loop(cx, investigator, remaining_attackers, attack_source);
                if matches!(out, EngineOutcome::AwaitingInput { .. }) {
                    return out;
                }
                finish_attack_loop(cx, attack_source, investigator)
            } else {
                park_attack_loop_beneath_window(
                    cx,
                    investigator,
                    remaining_attackers,
                    attack_source,
                    AttackLoopStage::AfterSoak,
                );
                super::reaction_windows::open_queued_reaction_window(cx)
            }
        }
        // Reserved for K5b-2 (effect path): place and let the effect walk continue.
        DamageSource::Effect => {
            let _ = place_assignment(cx, investigator, &assignment);
            EngineOutcome::Done
        }
    }
}
```

- [ ] **Step 5: Wire the attack path to suspend (synchronous stays inside the loop)**

Restructure `deal_head_and_maybe_park` so the **suspend** branch parks the frame (taking `remaining_attackers`) and the **synchronous** branch finishes inline and falls through to the existing window-check with `attackers` intact — never re-driving. `place_queue_exhaust` (Task 2) is the shared deterministic tail.

```rust
fn deal_head_and_maybe_park(
    cx: &mut Cx,
    investigator: InvestigatorId,
    attackers: &mut Vec<EnemyId>,
    source: EnemyAttackSource,
    cancelled: bool,
) -> Option<EngineOutcome> {
    let enemy_id = attackers.remove(0);

    // Build the assignment. Cancelled → empty (no harm, still exhausts). Else
    // soak-first deterministically as far as possible; if a point is contested,
    // suspend on a distribution prompt (parking the rest of the loop on the frame).
    let mut assignment = Assignment::default();
    if !cancelled {
        let enemy = cx.state.enemies.get(&enemy_id).unwrap_or_else(|| {
            unreachable!("deal_head_and_maybe_park: enemy {enemy_id:?} gone; invariant")
        });
        let (mut rd, mut rh) = (enemy.attack_damage, enemy.attack_horror);
        let soakers = build_soakers(cx.state, investigator);
        if advance_distribution(&soakers, &mut rd, &mut rh, &mut assignment).is_none() {
            // Contested → park the frame (remaining_attackers into it) and prompt.
            cx.state.continuations.push(Continuation::DamageAssignment {
                investigator,
                remaining_damage: rd,
                remaining_horror: rh,
                assignment,
                source: crate::state::DamageSource::EnemyAttack {
                    enemy: enemy_id,
                    remaining_attackers: std::mem::take(attackers),
                    attack_source: source,
                },
            });
            return Some(prompt_current_point(cx, investigator));
        }
        // Not contested: `assignment` is the complete soak-first assignment.
    }

    // Synchronous (no prompt): place + queue windows + exhaust, then the caller's
    // existing window-check (`attackers` left intact for the outer loop).
    place_queue_exhaust(cx, investigator, enemy_id, source, &assignment);
    if cx.state.open_windows().is_empty() {
        None
    } else {
        Some(park_on_soak_window(cx, investigator, std::mem::take(attackers), source))
    }
}
```

This keeps the no-soaker attack path byte-identical to today (build the soak-first assignment, place, queue, exhaust, window-check) and threads the prompt only when contested.

- [ ] **Step 6: Route `DamageAssignment` in `resolve_input`**

In `dispatch/mod.rs`, add an arm (near the `AttackLoop` arm):

```rust
        Some(Continuation::DamageAssignment { .. }) => combat::resume_damage_assignment(cx, response),
```

Replace the temporary `unreachable!` stub from Task 1 Step 4.

- [ ] **Step 7: Run the gate test + full lib suite**

Run: `cargo test -p game-core --lib`
Expected: PASS — the gate test green; the no-soaker attack path unchanged (every existing attack-loop test stays green because without a registry no point is ever contested).

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: interactive per-point soak distribution for attacks (K5b-1 of #44)"
```

---

### Task 4: Registry-backed attack-distribution tests

**Files:**
- Create: `crates/cards/tests/soak_distribution.rs`

**Interfaces:**
- Consumes: the full distribution machinery via `apply` (enemy attack against an investigator controlling a soaker).

Model on `crates/cards/tests/guard_dog_soak.rs` (registry install + `soak_state` + `EndTurn` to drive the enemy phase; or an AoO via a `Move`). Drive a 2-damage attack with Guard Dog (health 3) in play and resolve the per-point `PickSingle`s.

- [ ] **Step 1: Distribution prompt + split across soaker and investigator**

```rust
#[test]
fn two_damage_attack_distributes_one_to_guard_dog_one_to_self() {
    // Enemy deals 2 damage; investigator controls Guard Dog (health 3).
    // First point → Guard Dog, second point → investigator.
    // Assert: Guard Dog.accumulated_damage == 1, investigator.damage == 1,
    // and (since damage landed on Guard Dog) its retaliate window then opens.
    // ... soak_state with a 2-damage engaged attacker + Guard Dog; EndTurn;
    //     two ResolveInput(PickSingle(...)) picking the target by label
    //     ("Asset(CardInstanceId(..))" then "Investigator") ...
}
```

Build the per-point picks using the option labels (`format!("{target:?}")` — `DistributionTarget::Investigator` / `DistributionTarget::Asset(CardInstanceId(n))`). Verify the exact label format against the `DistributionTarget` Debug derive before asserting.

- [ ] **Step 2: Player declines to soak (all to investigator)**

```rust
#[test]
fn player_may_decline_to_soak_taking_all_damage() {
    // 2-damage attack, Guard Dog in play; assign both points to the investigator.
    // Assert investigator.damage == 2, Guard Dog.accumulated_damage == 0
    // (untouched), no retaliate window (no asset damaged).
}
```

- [ ] **Step 3: Capacity exhaustion drops a soaker from later prompts**

```rust
#[test]
fn soaker_drops_out_once_full() {
    // A soaker with 1 remaining health: first damage point may go to it; the
    // second point's prompt offers only the investigator (soaker full).
    // Use a Guard Dog pre-damaged to accumulated 2 (1 remaining), 2-damage attack.
}
```

- [ ] **Step 4: Run them**

Run: `cargo test -p cards --test soak_distribution`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cards/tests/soak_distribution.rs
git commit -m "test: interactive attack soak distribution (K5b-1 of #44)"
```

---

### Task 5: Update the soak-first `TODO(#44)` + full gauntlet

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (the `assign_attack` `TODO(#44)` doc-comment — narrow it to "K5b-2: the effect path still uses soak-first; attacks are interactive")

- [ ] **Step 1: Narrow the `assign_attack` TODO**

`assign_attack` is now the deterministic helper still used by the no-prompt path and (until K5b-2) the effect path. Update its `TODO(#44)` to:

```rust
/// Soak-first is the deterministic default used when no point is contested
/// (no soaker with capacity) and by the non-attack/effect path until K5b-2.
/// The attack path's interactive per-point distribution (#44/K5b-1) is in
/// `suspend_or_finish_assignment`; `TODO(#44)`: route the effect path through it
/// too (K5b-2).
```

- [ ] **Step 2: Full native gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green.

- [ ] **Step 3: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs
git commit -m "engine: narrow the assign_attack soak-first TODO to the effect path (K5b-1 of #44)"
```

---

## Post-implementation (PR procedure, not TDD tasks)

- Pre-push review pass before pushing.
- Push `engine/soak-interactive`, open the PR (do **not** `Closes #44` — K5b-2 completes it; reference "K5b-1 of #44"). Design-decisions paragraph: per-point `PickSingle` distribution (RR p.7 simultaneity preserved via a single end-of-distribution `place_assignment`); gated to prompt only when contested; attack path only (effect path is K5b-2); `DamageSource::Effect` reserved.
- Watch CI; update `docs/phases/phase-7-the-gathering.md` after green (K5b-1 shipped: attack distribution; K5b-2 remains: effect path). Don't close #44.
- Merge after explicit user approval. K5b-2 is the next plan.

## Self-review notes

- **Spec coverage (K5b attack portion):** per-point `PickSingle` (Task 3) ✓; gate (Task 3 `advance_distribution`) ✓; `DamageAssignment` frame + simultaneity via single `place_assignment` (Tasks 1, 3 `finish_distribution`) ✓; attack-path resume routing the `AttackLoop` continuation (Task 3 `finish_distribution` EnemyAttack arm) ✓; invalid-pick reject + frame retained (Task 3 `resume_damage_assignment`) ✓; behaviour-preserving no-soaker case (Tasks 2, 3 Step 7) ✓. Effect path + the multi-window drain are out of K5b-1 (K5b-2 / deferred).
- **Two design subtleties, resolved in the plan:** (a) the counter/`assignment` lockstep — `advance_distribution` takes `remaining_*` by `&mut` and decrements them as it auto-assigns, so the frame counters and `assignment` never drift; (b) the synchronous-vs-resumed asymmetry — the synchronous (no-prompt) attack path stays *inside* `drive_attack_loop` (Step 5 falls through to the window-check with `attackers` intact, never re-driving), while only the suspended branch parks the frame and its resume (Step 4, running outside the loop) re-drives. `mem::take(attackers)` happens only on the suspend branch.
- **Type consistency:** `Assignment` (now `pub(crate)`, `Clone+Serialize`), `DamageSource`, `DistributionTarget`, `advance_distribution`/`suspend_or_finish_assignment`/`finish_distribution`/`resume_damage_assignment`/`place_queue_exhaust` names are used consistently across tasks.
