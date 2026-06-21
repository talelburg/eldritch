# K4 — Player-Chosen Attack Order (#143) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When an investigator is engaged with 2+ ready enemies, let the player pick which attacks next (RR p.25 step 3.3), at both the enemy-phase and attack-of-opportunity sites.

**Architecture:** Add the order choice at the top of the shared `drive_attack_loop`: with 2+ attackers remaining, suspend on a `PickSingle` over the remaining enemies, parking the existing `Continuation::AttackLoop` frame with a new `AttackLoopStage::PickOrder` stage. On `ResolveInput`, move the chosen enemy to the head, resolve that one attack (its existing before-cancel/deal/soak sequence), then loop — re-prompting if 2+ still remain (interleaved picking). Two small refactors first (extract the per-attacker body and the source-keyed post-loop tail) so the driver and the new resume share them.

**Tech Stack:** Rust, `game-core` engine crate. No new dependencies. Engine-only (no web/wasm changes).

## Global Constraints

- Match CI's strict flags before declaring a task done: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`.
- Validate-first / mutate-second handler contract; behaviour-preserving except the documented new player agency.
- Card/rules text is verified, never paraphrased — already done in the design spec (`docs/superpowers/specs/2026-06-21-phase-7-k4-attack-order-design.md`). RR p.25 step 3.3: *"If an investigator is engaged with multiple enemies, resolve their attacks in the order of the attacked investigator's choosing."*
- Commit subjects: `scope: description` (scope = `engine`). One feature branch (`engine/attack-order`, already created).
- All work is in the `game-core` crate; no registry-backed card is required for the order mechanism (synthetic test enemies suffice).

---

### Task 1: Add the `AttackLoopStage::PickOrder` variant

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (the `AttackLoopStage` enum, ~line 329; the serde test near line 2057)
- Test: same file (`#[cfg(test)]` module)

**Interfaces:**
- Produces: `AttackLoopStage::PickOrder` — the third stage variant, consumed by Tasks 4/5 in `combat.rs` and `dispatch/mod.rs`.

- [ ] **Step 1: Write the failing serde round-trip test**

Add to the existing `#[cfg(test)]` module in `game_state.rs` (next to `enemy_phase_anchor_attacking_round_trips_through_serde`):

```rust
#[test]
fn attack_loop_pick_order_stage_round_trips_through_serde() {
    use crate::state::{AttackLoopStage, Continuation, EnemyAttackSource, EnemyId};
    let mut state = GameState::default();
    state.continuations.push(Continuation::AttackLoop {
        investigator: InvestigatorId(1),
        remaining_attackers: vec![EnemyId(2), EnemyId(3)],
        source: EnemyAttackSource::EnemyPhase,
        stage: AttackLoopStage::PickOrder,
    });
    let json = serde_json::to_string(&state).expect("serialize");
    let back: GameState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(state, back);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core attack_loop_pick_order_stage_round_trips_through_serde`
Expected: FAIL to compile — `no variant named PickOrder found for enum AttackLoopStage`.

- [ ] **Step 3: Add the variant**

In the `AttackLoopStage` enum (after the `AfterSoak` variant, ~line 337), add:

```rust
    /// Suspended on the player's attack-order `PickSingle` (#143/K4): 2+
    /// attackers remain and none has dealt this iteration. The `AttackLoop`
    /// frame is the **top** frame (no reaction window above it) and *is* the
    /// prompt. Resume reorders `remaining_attackers` to put the picked enemy at
    /// the head, deals it, then continues. Unlike the window stages — which park
    /// *beneath* a reaction window and resume on window-close via
    /// [`resume_enemy_attack`](crate::engine) — this stage resumes on
    /// `ResolveInput` via `resume_attack_order_pick`.
    PickOrder,
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p game-core attack_loop_pick_order_stage_round_trips_through_serde`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/game_state.rs
git commit -m "engine: add AttackLoopStage::PickOrder for player attack-order (#143)"
```

---

### Task 2: Extract `process_head_attacker` (behaviour-preserving refactor)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`drive_attack_loop`, ~lines 804-854)
- Test: existing `combat.rs` / `phases.rs` attack-loop tests (must stay green)

**Interfaces:**
- Produces: `fn process_head_attacker(cx: &mut Cx, investigator: InvestigatorId, attackers: &mut Vec<EnemyId>, source: EnemyAttackSource) -> Option<EngineOutcome>` — resolves the head attacker (before-cancel window → park-`BeforeAttack`, else deal + maybe park-`AfterSoak`). `Some(outcome)` = suspended; `None` = continue. Consumed by `drive_attack_loop` (this task) and `resume_attack_order_pick` (Task 4).

This is a pure refactor: lift the loop body out of `drive_attack_loop` with no behaviour change. The existing tests are the regression guard, so no new test is written.

- [ ] **Step 1: Add the `process_head_attacker` helper**

Insert above `drive_attack_loop` (after `deal_head_and_maybe_park`, ~line 802):

```rust
/// Resolve the head attacker: open its `BeforeEnemyAttack` cancel window (park
/// the loop as [`AttackLoopStage::BeforeAttack`] and suspend if a cancel
/// reaction is available, Axis D #336), otherwise deal it + maybe park on its
/// `AfterEnemyAttackDamagedAsset` soak window (C5b #237). `Some(outcome)` =
/// suspended (the loop is parked beneath the queued window); `None` = continue
/// to the next attacker. Caller guarantees `attackers` is non-empty. Shared by
/// [`drive_attack_loop`] and the order-pick resume (`resume_attack_order_pick`,
/// #143).
fn process_head_attacker(
    cx: &mut Cx,
    investigator: InvestigatorId,
    attackers: &mut Vec<EnemyId>,
    source: EnemyAttackSource,
) -> Option<EngineOutcome> {
    let enemy_id = *attackers
        .first()
        .expect("process_head_attacker called with an empty attacker list");

    // Before-attack cancel window (Axis D #336): suspend BEFORE dealing damage,
    // keeping the head at the front so the `BeforeAttack` resume processes it.
    let _ = super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::EnemyAttacks {
            enemy: enemy_id,
            investigator,
        },
    );
    if !cx.state.open_windows().is_empty() {
        park_attack_loop_beneath_window(
            cx,
            investigator,
            std::mem::take(attackers),
            source,
            AttackLoopStage::BeforeAttack,
        );
        return Some(super::reaction_windows::open_queued_reaction_window(cx));
    }

    // No cancel reaction: deal this (un-cancelled) attacker, suspending if it
    // opens a soak window.
    deal_head_and_maybe_park(cx, investigator, attackers, source, false)
}
```

- [ ] **Step 2: Rewrite `drive_attack_loop` to call it**

Replace the current `drive_attack_loop` body (lines ~804-854) with:

```rust
fn drive_attack_loop(
    cx: &mut Cx,
    investigator: InvestigatorId,
    mut attackers: Vec<EnemyId>,
    source: EnemyAttackSource,
) -> EngineOutcome {
    while !attackers.is_empty() {
        // Early-break on defeat. See fn doc step 1.
        let active = cx
            .state
            .investigators
            .get(&investigator)
            .is_some_and(|inv| inv.status == Status::Active);
        if !active {
            break;
        }

        if let Some(suspended) =
            process_head_attacker(cx, investigator, &mut attackers, source)
        {
            return suspended;
        }
    }
    EngineOutcome::Done
}
```

(The order-pick gate is added in Task 4; this step is the body extraction only.)

- [ ] **Step 3: Run the attack-loop tests to verify no behaviour change**

Run: `cargo test -p game-core --lib combat:: && cargo test -p game-core --lib phases::`
Expected: PASS — all existing `drive_attack_loop` / `resolve_attacks_for_investigator` / `resume_enemy_attack` tests green (this is a pure refactor).

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs
git commit -m "engine: extract process_head_attacker from drive_attack_loop (#143)"
```

---

### Task 3: Extract `finish_attack_loop` (behaviour-preserving refactor)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`resume_enemy_attack`, the source `match` at ~lines 927-936)
- Test: existing `resume_enemy_attack` tests (must stay green)

**Interfaces:**
- Produces: `fn finish_attack_loop(cx: &mut Cx, source: EnemyAttackSource, investigator: InvestigatorId) -> EngineOutcome` — the source-keyed post-loop tail (`EnemyPhase → after_enemy_phase_attacks` · `AttackOfOpportunity → Done` · `Retaliate → drive_skill_test`). Consumed by `resume_enemy_attack` (this task) and `resume_attack_order_pick` (Task 4).

- [ ] **Step 1: Add the `finish_attack_loop` helper**

Insert before `resume_enemy_attack` (~line 856):

```rust
/// The source-keyed step that runs once an attack loop drains to
/// [`EngineOutcome::Done`]: enemy phase advances its per-investigator cursor and
/// opens the next window; an AoO returns control to the parked
/// `ActionResolution` frame (`Done`, the `drive` loop resumes it); a retaliate
/// re-enters the Fight's skill-test follow-up. Shared by [`resume_enemy_attack`]
/// (window-close drain) and `resume_attack_order_pick` (order-pick drain, #143).
fn finish_attack_loop(
    cx: &mut Cx,
    source: EnemyAttackSource,
    investigator: InvestigatorId,
) -> EngineOutcome {
    match source {
        EnemyAttackSource::EnemyPhase => {
            super::reaction_windows::after_enemy_phase_attacks(cx, investigator)
        }
        EnemyAttackSource::AttackOfOpportunity => EngineOutcome::Done,
        // The retaliate's window closed; hand control back to the Fight's
        // skill-test follow-up (its `SkillTest` frame is now top) so teardown
        // finishes (#379).
        EnemyAttackSource::Retaliate => super::skill_test::drive_skill_test(cx),
    }
}
```

- [ ] **Step 2: Replace the inline match in `resume_enemy_attack`**

In `resume_enemy_attack`, replace the trailing `match source { … }` (lines ~927-936) with:

```rust
    finish_attack_loop(cx, source, investigator)
```

(Keep the preceding `debug_assert!` that the outcome is `Done`.)

- [ ] **Step 3: Run the resume tests to verify no behaviour change**

Run: `cargo test -p game-core --lib combat::`
Expected: PASS — `resume_enemy_attack_drains_remaining_attackers_and_advances_cursor`, `drive_retaliate_*`, `drive_aoo_*` all green.

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs
git commit -m "engine: extract finish_attack_loop source tail from resume_enemy_attack (#143)"
```

---

### Task 4: The order-pick mechanism + migrate the affected tests

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (imports; the order-pick gate in `drive_attack_loop`; add `suspend_order_pick` + `resume_attack_order_pick`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resolve_input` `AttackLoop` arm, ~lines 437-444)
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (migrate two existing tests, ~lines 3279, 3325)
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (migrate `resume_enemy_attack_drains_remaining_attackers_and_advances_cursor`, ~line 1162)
- Test: new enemy-phase order-pick test in `phases.rs`

**Interfaces:**
- Consumes: `process_head_attacker`, `finish_attack_loop` (Tasks 2/3); `AttackLoopStage::PickOrder` (Task 1); `super::hunters::candidate_options` (existing, `pub(super)`); `InputRequest::choice`, `OptionId`, `ResumeToken` (existing in `crate::engine::outcome`).
- Produces: `pub(super) fn resume_attack_order_pick(cx: &mut Cx, response: &crate::action::InputResponse) -> EngineOutcome` — routed from `resolve_input`.

- [ ] **Step 1: Write the failing enemy-phase order-pick test**

Add to `phases.rs`'s test module (near the other `resolve_attacks_for_investigator_*` tests). It sets up the `EnemyPhase` anchor + `turn_order` (like `resume_enemy_attack_drains_…`) so the post-loop tail resolves cleanly, then drives through `resolve_input`:

```rust
#[test]
fn resolve_attacks_for_investigator_pick_overrides_enemy_id_order() {
    use crate::action::InputResponse;
    use crate::engine::{EngineOutcome, OptionId};
    use crate::state::EnemyId;

    let inv_id = InvestigatorId(1);
    let mut e_lower = test_enemy(2, "Lower id"); // EnemyId(2), dmg 1
    e_lower.engaged_with = Some(inv_id);
    e_lower.attack_damage = 1;
    let mut e_higher = test_enemy(10, "Higher id"); // EnemyId(10), dmg 2
    e_higher.engaged_with = Some(inv_id);
    e_higher.attack_damage = 2;

    let mut state = GameStateBuilder::default()
        .with_investigator({
            let mut inv = test_investigator(1);
            inv.max_health = 100; // survive both attacks
            inv
        })
        .with_turn_order([inv_id])
        .with_enemy(e_higher) // inserted in non-id order: BTreeMap still snapshots 2 then 10
        .with_enemy(e_lower)
        .with_phase_anchor(crate::state::Continuation::EnemyPhase {
            resume: crate::state::EnemyResume::BeforeInvestigatorAttacked,
            attacking: Some(inv_id),
        })
        .build();
    let mut events = Vec::new();

    // 2 ready engaged enemies → suspend on the order pick (#143), not EnemyId order.
    let outcome = super::super::combat::resolve_attacks_for_investigator(
        &mut super::super::Cx { state: &mut state, events: &mut events },
        inv_id,
    );
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("expected an attack-order prompt, got {outcome:?}");
    };
    // Options are the snapshotted attackers in EnemyId order: option 0 = EnemyId(2),
    // option 1 = EnemyId(10). Pick the higher-id enemy (dmg 2) to strike FIRST.
    let pick = request
        .options
        .iter()
        .find(|o| o.label == format!("{:?}", EnemyId(10)))
        .expect("EnemyId(10) offered")
        .id;
    assert_eq!(pick, OptionId(1), "EnemyId(10) is option 1 in EnemyId order");

    let resumed = super::super::resolve_input(
        &mut super::super::Cx { state: &mut state, events: &mut events },
        &InputResponse::PickSingle(pick),
    );
    // Both attacks resolved; the chosen (EnemyId 10, dmg 2) struck first.
    assert!(!matches!(resumed, EngineOutcome::AwaitingInput { .. }), "loop drained");
    let damages: Vec<u8> = events
        .iter()
        .filter_map(|e| match e {
            Event::DamageTaken { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect();
    assert_eq!(damages, vec![2, 1], "chosen EnemyId(10) (dmg 2) attacked before EnemyId(2) (dmg 1)");
    assert!(state.enemies[&EnemyId(2)].exhausted && state.enemies[&EnemyId(10)].exhausted);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core resolve_attacks_for_investigator_pick_overrides_enemy_id_order`
Expected: FAIL — `resolve_attacks_for_investigator` still resolves synchronously (returns `Done` after dealing in EnemyId order), so the `AwaitingInput` `let-else` panics.

- [ ] **Step 3: Add the imports to `combat.rs`**

Extend the `use` at the top of `combat.rs` (line 5) so the suspension can build its request:

```rust
use crate::engine::outcome::{InputRequest, OptionId, ResumeToken};
use crate::engine::EngineOutcome;
```

(Leave the existing `use crate::engine::EngineOutcome;` if already present — merge, don't duplicate.)

- [ ] **Step 4: Add the order-pick gate to `drive_attack_loop`**

In `drive_attack_loop` (from Task 2), insert the gate between the active check and the `process_head_attacker` call:

```rust
        if !active {
            break;
        }

        // Player-chosen attack order (#143, RR p.25 step 3.3): with 2+ ready
        // attackers remaining, suspend for the order pick before resolving the
        // head. Covers the enemy phase, AoO, and (vacuously, 1-element) retaliate
        // — all three route through here. Single-attacker lists skip this and
        // resolve inline, preserving prior behaviour.
        if attackers.len() >= 2 {
            return suspend_order_pick(cx, investigator, attackers, source);
        }

        if let Some(suspended) =
            process_head_attacker(cx, investigator, &mut attackers, source)
        {
            return suspended;
        }
```

- [ ] **Step 5: Add `suspend_order_pick`**

Insert before `drive_attack_loop`:

```rust
/// Park the loop on its order-pick `PickSingle` (#143): push the `AttackLoop`
/// frame as the **top** frame (no window above — it *is* the prompt) at
/// [`AttackLoopStage::PickOrder`], and return `AwaitingInput` offering the
/// remaining attackers (option `i` = `remaining_attackers[i]`, EnemyId order).
/// `resume_attack_order_pick` resolves the `PickSingle` back. Called only with
/// `attackers.len() >= 2`.
fn suspend_order_pick(
    cx: &mut Cx,
    investigator: InvestigatorId,
    attackers: Vec<EnemyId>,
    source: EnemyAttackSource,
) -> EngineOutcome {
    let prompt = format!(
        "Investigator {investigator:?} is engaged with {} enemies: pick which attacks \
         next (RR p.25 step 3.3)",
        attackers.len()
    );
    let options = super::hunters::candidate_options(&attackers);
    cx.state.continuations.push(Continuation::AttackLoop {
        investigator,
        remaining_attackers: attackers,
        source,
        stage: AttackLoopStage::PickOrder,
    });
    EngineOutcome::AwaitingInput {
        request: InputRequest::choice(prompt, options),
        resume_token: ResumeToken(0),
    }
}
```

- [ ] **Step 6: Add `resume_attack_order_pick`**

Insert after `resume_enemy_attack`:

```rust
/// Resume a loop suspended on its order-pick `PickSingle` (#143). The
/// `AttackLoop{stage: PickOrder}` frame is the top frame (no window above it),
/// so `resolve_input` routes here directly (not via window-close). Validate the
/// `PickSingle` against the stored `remaining_attackers`; on an invalid pick,
/// reject and **leave the frame** so the client can retry (mirrors
/// `resume_hunter_choice`). On a valid pick, move the chosen enemy to the head,
/// resolve it via [`process_head_attacker`] (which may re-suspend on its own
/// cancel/soak window), then drive the rest — re-prompting if 2+ still remain —
/// and run the source-keyed tail ([`finish_attack_loop`]) on completion.
pub(super) fn resume_attack_order_pick(
    cx: &mut Cx,
    response: &crate::action::InputResponse,
) -> EngineOutcome {
    let Some(Continuation::AttackLoop {
        investigator,
        remaining_attackers,
        source,
        stage: AttackLoopStage::PickOrder,
    }) = cx.state.continuations.last().cloned()
    else {
        unreachable!(
            "resume_attack_order_pick: top frame is not an AttackLoop{{PickOrder}}; \
             resolve_input only routes here when it is — state-corruption invariant \
             violation"
        )
    };
    let crate::action::InputResponse::PickSingle(OptionId(i)) = response else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: attack-order pick expects InputResponse::PickSingle, got {response:?}"
            )
            .into(),
        };
    };
    let i = *i as usize;
    if i >= remaining_attackers.len() {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: attack-order option {i} out of range (0..{})",
                remaining_attackers.len()
            )
            .into(),
        };
    }

    // Valid pick: pop the frame we validated against, then move the chosen enemy
    // to the head (preserving the others' relative order for the next prompt).
    cx.state.continuations.pop();
    let mut attackers = remaining_attackers;
    let chosen = attackers.remove(i);
    attackers.insert(0, chosen);

    if let Some(suspended) = process_head_attacker(cx, investigator, &mut attackers, source) {
        return suspended; // the chosen head opened its own cancel/soak window
    }
    let outcome = drive_attack_loop(cx, investigator, attackers, source);
    if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
        return outcome; // next order pick, or a later attacker's window
    }
    debug_assert!(
        matches!(outcome, EngineOutcome::Done),
        "drive_attack_loop returned unexpected {outcome:?}"
    );
    finish_attack_loop(cx, source, investigator)
}
```

- [ ] **Step 7: Route the `PickOrder` frame in `resolve_input`**

In `dispatch/mod.rs`, replace the single `AttackLoop` arm (~lines 441-444) with a stage-split. Ensure `AttackLoopStage` is in scope (the function already has `use crate::state::Continuation;` near line 415 — add `AttackLoopStage` to it):

```rust
        // An order-pick suspension parks the `AttackLoop` frame as the top frame
        // (it *is* the prompt) — route its `PickSingle` to the order resume
        // (#143). Every other `AttackLoop` stage sits beneath a reaction window
        // (the window is the prompt) and never legitimately awaits input here, so
        // it rejects defensively (mirrors the EncounterCard arm).
        Some(Continuation::AttackLoop {
            stage: AttackLoopStage::PickOrder,
            ..
        }) => combat::resume_attack_order_pick(cx, response),
        Some(Continuation::AttackLoop { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (a parked attack loop is top)"
                .into(),
        },
```

- [ ] **Step 8: Run the new test to verify it passes**

Run: `cargo test -p game-core resolve_attacks_for_investigator_pick_overrides_enemy_id_order`
Expected: PASS.

- [ ] **Step 9: Migrate the two `phases.rs` tests broken by the new pause**

`resolve_attacks_for_investigator_iterates_attackers_in_enemy_id_order` (the old "deterministic EnemyId order" premise is now replaced by player-pick) — **delete it** (its successor is the Step-1 test). Then update `resolve_attacks_for_investigator_early_breaks_when_target_defeated_mid_loop` to resolve the order pick first, picking the killer (`EnemyId(1)`) to strike first:

```rust
#[test]
fn resolve_attacks_for_investigator_early_breaks_when_target_defeated_mid_loop() {
    use crate::action::InputResponse;
    use crate::engine::{EngineOutcome, OptionId};

    let inv_id = InvestigatorId(1);
    let mut e1 = test_enemy(1, "Killer"); // dmg 1, defeats the 1-health inv
    e1.engaged_with = Some(inv_id);
    e1.attack_damage = 1;
    let mut e2 = test_enemy(2, "Bystander"); // must NOT attack (early-break)
    e2.engaged_with = Some(inv_id);
    e2.attack_damage = 5;

    let mut state = GameStateBuilder::default()
        .with_investigator({
            let mut inv = test_investigator(1);
            inv.max_health = 1; // e1's attack defeats
            inv
        })
        .with_turn_order([inv_id])
        .with_enemy(e1)
        .with_enemy(e2)
        .with_phase_anchor(crate::state::Continuation::EnemyPhase {
            resume: crate::state::EnemyResume::BeforeInvestigatorAttacked,
            attacking: Some(inv_id),
        })
        .build();
    let mut events = Vec::new();

    // 2 engaged → order pick first.
    let outcome = super::super::combat::resolve_attacks_for_investigator(
        &mut super::super::Cx { state: &mut state, events: &mut events },
        inv_id,
    );
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("expected an order pick, got {outcome:?}");
    };
    let pick = request
        .options
        .iter()
        .find(|o| o.label == format!("{:?}", EnemyId(1)))
        .expect("EnemyId(1) offered")
        .id;
    assert_eq!(pick, OptionId(0));

    let _ = super::super::resolve_input(
        &mut super::super::Cx { state: &mut state, events: &mut events },
        &InputResponse::PickSingle(pick),
    );

    // e1 attacked + exhausted; e2 did NOT attack and did NOT exhaust (early-break
    // after e1 defeated the investigator — no re-prompt, the active check precedes
    // the order gate).
    assert!(state.enemies[&EnemyId(1)].exhausted, "e1 attacked, must exhaust");
    assert!(!state.enemies[&EnemyId(2)].exhausted, "e2 must not exhaust (early-break)");
}
```

(Add `use crate::state::EnemyId;` to the test if not already imported in that module scope.)

- [ ] **Step 10: Migrate the `combat.rs` drains test broken by the re-prompt**

`resume_enemy_attack_drains_remaining_attackers_and_advances_cursor` pre-parks `[second, third]` at `AfterSoak`; on resume, the drain now re-prompts (2 remain). Update it to a single remaining attacker so the drain stays synchronous (the multi-attacker drain-with-pick is covered by Task 5's 3-enemy test). Change the parked list and the assertions to one attacker:

```rust
        let second = EnemyId(2);
        let mut e2 = test_enemy(2, "Second Attacker");
        e2.engaged_with = Some(inv_id);

        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([inv_id])
            .with_enemy(e2)
            .with_phase_anchor(crate::state::Continuation::EnemyPhase {
                resume: crate::state::EnemyResume::BeforeInvestigatorAttacked,
                attacking: Some(inv_id),
            })
            .build();
        state.continuations.push(Continuation::AttackLoop {
            investigator: inv_id,
            remaining_attackers: vec![second], // one remaining: drains without a re-prompt
            source: EnemyAttackSource::EnemyPhase,
            stage: AttackLoopStage::AfterSoak,
        });
```

and drop the `third` enemy + its `exhausted`/`EnemyExhausted` assertions, keeping the `second`-exhausted, frame-popped, and cursor-advanced assertions. Rename to `resume_enemy_attack_drains_remaining_attacker_and_advances_cursor` (singular).

- [ ] **Step 11: Run the full game-core suite to verify all green**

Run: `cargo test -p game-core --lib`
Expected: PASS — new test, both migrated tests, and the untouched suite all green.

- [ ] **Step 12: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs crates/game-core/src/engine/dispatch/mod.rs crates/game-core/src/engine/dispatch/phases.rs
git commit -m "engine: player picks engaged-enemy attack order (#143)"
```

---

### Task 5: Additional coverage — AoO pick, 3-enemy partial order, invalid pick, frame-spanning

**Files:**
- Test: `crates/game-core/src/engine/dispatch/combat.rs` (`combat_tests` module)

**Interfaces:**
- Consumes: `drive_aoo`, `resolve_attacks_for_investigator`, `resume_attack_order_pick` (via `resolve_input`), `AttackLoopStage::PickOrder`.

All four tests exercise Task 4's code; no implementation changes.

- [ ] **Step 1: AoO order pick (two engaged enemies)**

```rust
#[test]
fn drive_aoo_offers_order_pick_for_two_engaged_enemies() {
    use crate::engine::{EngineOutcome, OptionId};
    use crate::state::{AttackLoopStage, Continuation};

    let inv_id = InvestigatorId(1);
    let mut e_a = test_enemy(5, "A"); // EnemyId(5), dmg 1
    e_a.engaged_with = Some(inv_id);
    e_a.attack_damage = 1;
    let mut e_b = test_enemy(6, "B"); // EnemyId(6), dmg 2
    e_b.engaged_with = Some(inv_id);
    e_b.attack_damage = 2;

    let mut state = GameStateBuilder::new()
        .with_investigator({
            let mut inv = test_investigator(1);
            inv.max_health = 100;
            inv
        })
        .with_enemy(e_a)
        .with_enemy(e_b)
        .build();
    let mut events = Vec::new();

    let outcome = super::drive_aoo(
        &mut Cx { state: &mut state, events: &mut events },
        inv_id,
    );
    assert!(
        matches!(outcome, EngineOutcome::AwaitingInput { .. }),
        "2 engaged ready enemies → AoO order pick (#143)"
    );
    // The parked frame carries the AoO source and PickOrder stage.
    assert!(matches!(
        state.continuations.last(),
        Some(Continuation::AttackLoop {
            source: crate::state::EnemyAttackSource::AttackOfOpportunity,
            stage: AttackLoopStage::PickOrder,
            ..
        })
    ));

    // Pick EnemyId(6) (dmg 2) first; neither AoO attacker exhausts (RR p.7).
    let resumed = super::super::resolve_input(
        &mut Cx { state: &mut state, events: &mut events },
        &crate::action::InputResponse::PickSingle(OptionId(1)),
    );
    assert!(matches!(resumed, EngineOutcome::Done), "AoO loop drained");
    let damages: Vec<u8> = events
        .iter()
        .filter_map(|e| match e {
            Event::DamageTaken { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect();
    assert_eq!(damages, vec![2, 1], "chosen EnemyId(6) struck first");
    assert!(!state.enemies[&EnemyId(5)].exhausted && !state.enemies[&EnemyId(6)].exhausted,
        "AoO attackers never exhaust (RR p.7)");
}
```

- [ ] **Step 2: Three-enemy partial ordering (two prompts, three attacks)**

```rust
#[test]
fn drive_aoo_three_enemies_prompts_twice_resolving_in_chosen_order() {
    use crate::engine::{EngineOutcome, OptionId};

    let inv_id = InvestigatorId(1);
    let mk = |id: u32, dmg: u8| {
        let mut e = test_enemy(id, "E");
        e.engaged_with = Some(inv_id);
        e.attack_damage = dmg;
        e
    };
    let mut state = GameStateBuilder::new()
        .with_investigator({
            let mut inv = test_investigator(1);
            inv.max_health = 100;
            inv
        })
        .with_enemy(mk(7, 1))
        .with_enemy(mk(8, 2))
        .with_enemy(mk(9, 3))
        .build();
    let mut events = Vec::new();

    // First prompt over [7,8,9]: pick EnemyId(9) (dmg 3) → option 2.
    let o1 = super::drive_aoo(&mut Cx { state: &mut state, events: &mut events }, inv_id);
    assert!(matches!(o1, EngineOutcome::AwaitingInput { .. }));
    let o2 = super::super::resolve_input(
        &mut Cx { state: &mut state, events: &mut events },
        &crate::action::InputResponse::PickSingle(OptionId(2)),
    );
    // After resolving EnemyId(9), [7,8] remain → second prompt. Pick EnemyId(8)
    // (dmg 2) → option 1 (the remaining list is [7,8] in EnemyId order).
    assert!(matches!(o2, EngineOutcome::AwaitingInput { .. }), "second order prompt");
    let o3 = super::super::resolve_input(
        &mut Cx { state: &mut state, events: &mut events },
        &crate::action::InputResponse::PickSingle(OptionId(1)),
    );
    assert!(matches!(o3, EngineOutcome::Done), "third attacker is forced, loop drains");

    let damages: Vec<u8> = events
        .iter()
        .filter_map(|e| match e {
            Event::DamageTaken { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect();
    assert_eq!(damages, vec![3, 2, 1], "chosen order: 9 (dmg3), 8 (dmg2), 7 (dmg1)");
}
```

- [ ] **Step 3: Invalid pick rejects, frame retained**

```rust
#[test]
fn resume_attack_order_pick_rejects_out_of_range_and_keeps_frame() {
    use crate::engine::EngineOutcome;
    use crate::state::{AttackLoopStage, Continuation};

    let inv_id = InvestigatorId(1);
    let mut e_a = test_enemy(5, "A");
    e_a.engaged_with = Some(inv_id);
    let mut e_b = test_enemy(6, "B");
    e_b.engaged_with = Some(inv_id);

    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_enemy(e_a)
        .with_enemy(e_b)
        .build();
    let mut events = Vec::new();
    let _ = super::drive_aoo(&mut Cx { state: &mut state, events: &mut events }, inv_id);

    // Out-of-range option (only 0,1 valid).
    let rejected = super::super::resolve_input(
        &mut Cx { state: &mut state, events: &mut events },
        &crate::action::InputResponse::PickSingle(crate::engine::OptionId(9)),
    );
    assert!(matches!(rejected, EngineOutcome::Rejected { .. }));
    // Wrong variant.
    let rejected2 = super::super::resolve_input(
        &mut Cx { state: &mut state, events: &mut events },
        &crate::action::InputResponse::Skip,
    );
    assert!(matches!(rejected2, EngineOutcome::Rejected { .. }));
    // The PickOrder frame survives both rejections for retry.
    assert!(matches!(
        state.continuations.last(),
        Some(Continuation::AttackLoop { stage: AttackLoopStage::PickOrder, .. })
    ));
}
```

(If `InputResponse::Skip` is not the exact variant name, use any non-`PickSingle` variant — confirm against `crate::action::InputResponse`.)

- [ ] **Step 4: Run the new tests**

Run: `cargo test -p game-core --lib combat::`
Expected: PASS (all four new tests + the untouched suite).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs
git commit -m "engine: tests for AoO/3-enemy attack-order picks + invalid-pick reject (#143)"
```

---

### Task 6: Remove the `TODO(#143)`s, refresh doc-comments, run the gauntlet

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`drive_aoo` doc ~line 538; `resolve_attacks_for_investigator` doc ~lines 585-589)

**Interfaces:** none (docs + verification).

- [ ] **Step 1: Remove the `drive_aoo` deterministic-order TODO**

In `drive_aoo`'s doc-comment, replace the sentence "Attackers resolve in deterministic [`EnemyId`] order (player-pick is #143/K4)." with:

```rust
/// With 2+ engaged ready enemies the loop suspends for the player's attack-order
/// pick (#143, RR p.25 step 3.3); a single attacker resolves inline.
```

- [ ] **Step 2: Remove the `resolve_attacks_for_investigator` order TODO**

Replace its `**Attack order:**` paragraph (the `TODO(#143)` block, ~lines 585-589) with:

```rust
/// **Attack order:** player-chosen (#143). With 2+ ready engaged enemies the
/// loop suspends on a `PickSingle` (`AttackLoopStage::PickOrder`) so the attacked
/// investigator picks which strikes next (RR p.25 step 3.3: "resolve their
/// attacks in the order of the attacked investigator's choosing"), one at a time
/// between attacks; a single attacker resolves inline. The attacker set is
/// snapshotted here in `EnemyId` order (the option order) and frozen for the
/// sequence — the pick reorders the stored list, never re-scanning state.
```

- [ ] **Step 3: Confirm no `TODO(#143)` remains**

Run: `! grep -rn "TODO(#143)\|#143" crates/game-core/src/engine/dispatch/combat.rs`
Expected: no output (exit 0 from the negation; the acceptance criterion "no remaining TODO at either call site").

- [ ] **Step 4: Run the full native CI gauntlet**

Run each, expecting PASS / clean:

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green. (No web/wasm code changed, but the wasm jobs confirm the engine change compiles to `wasm32`.)

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs
git commit -m "engine: drop the TODO(#143) attack-order deferrals (#143)"
```

---

## Post-implementation (PR procedure, not TDD tasks)

- Push `engine/attack-order`, open the PR (template; `Closes #143`; design-decisions paragraph: interleaved pick in the shared `drive_attack_loop`, frame now spans step 3.3, snapshot frozen at loop entry).
- Pre-push review pass (superpowers final-reviewer) before pushing, per the project workflow.
- Watch CI (`gh pr checks <PR#> --watch`).
- **Only after CI is green**, update `docs/phases/phase-7-the-gathering.md` as the final commit: mark K4 (#143) shipped in the Ordering step-4 arc and Tier-1 C bullet, move #143 to the Closed table, note the Shape-A carry-over resolved (enemy-phase frame now spans step 3.3) + the snapshot-timing resolution, and flag K5 (#44/#119) as the remaining keystone sub-slice. Add a Decisions-made entry only if load-bearing for a future PR (e.g. "attack order is interleaved one-at-a-time, not upfront — reuses `remaining_attackers`").
- Merge only after explicit user approval.

## Self-review notes

- **Spec coverage:** order pick at both sites (Tasks 4 enemy-phase + 5 AoO) ✓; interleaved one-at-a-time (Task 4 gate + 5 three-enemy) ✓; `AttackLoopStage::PickOrder` routing (Tasks 1, 4) ✓; frame spans step 3.3 (falls out of the gate; asserted via the AoO frame check in Task 5) ✓; snapshot frozen at loop entry (Task 6 doc) ✓; single/retaliate never prompt (Task 4 migrated early-break test + the untouched `drive_retaliate_*` suite) ✓; invalid-pick reject + frame retained (Task 5) ✓; both `TODO(#143)`s removed (Task 6) ✓.
- **Type consistency:** `process_head_attacker(&mut Vec<EnemyId>) -> Option<EngineOutcome>` and `finish_attack_loop(source, investigator) -> EngineOutcome` and `resume_attack_order_pick(&InputResponse) -> EngineOutcome` are used identically across tasks; `AttackLoopStage::PickOrder` and `Continuation::AttackLoop { investigator, remaining_attackers, source, stage }` field names match `game_state.rs`.
- **Behaviour-preserving refactors (Tasks 2, 3) precede the behaviour change (Task 4)** so each is independently green and reviewable.
