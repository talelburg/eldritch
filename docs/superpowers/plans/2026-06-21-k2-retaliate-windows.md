# K2 — Retaliate cancel/soak windows — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a failed-Fight Retaliate attack open its cancel (Dodge 01023) and soak (Guard Dog 01021) reaction windows by routing it through the shared attack loop, resuming the Fight's skill-test teardown after the window closes (#379).

**Architecture:** `fire_retaliate_if_any` stops calling `combat::enemy_attack` directly and instead drives the single retaliate attacker through the existing `drive_attack_loop` under a new `EnemyAttackSource::Retaliate` (reusing K1's `BeforeEnemyAttack`/`AfterEnemyAttackDamagedAsset` windows + `AttackLoopStage` two-stage cursor, and K1's `EnemyPhase`-gated non-exhaust). The retaliate fires from the `PostRetaliate` stage of the skill-test follow-up, which runs on the existing `SkillTest` continuation frame; on the window's close, `resume_enemy_attack`'s new `Retaliate` arm re-enters `drive_skill_test` so the follow-up finishes teardown.

**Tech Stack:** Rust, `game-core` kernel crate (compiles to wasm via the `web` crate), `cards` content crate for registry-backed integration tests. Event-sourced `apply`; serializable `Continuation` stack.

## Global Constraints

- **Match CI's strict flags before pushing:** `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`. (Per-task: run at least `fmt --check`, `clippy -p game-core`, and the doc build for the crate you touched — accumulated fmt/doc drift bit the K1 slice when only `test`+`clippy` were run per task.)
- **Handler contract — validate-first / mutate-second** stays intact; K2 changes a *route*, not when retaliate fires or its damage.
- **`game-core` never depends on `cards`:** card-data via `card_registry::current()`; registry-free unit tests get the no-registry fallback (no reaction windows open). Tests needing real card abilities live in `crates/cards/tests/`.
- **RR p.18 (verified, quoted in the existing `fire_retaliate_if_any` doc):** *"Each time an investigator fails a skill test while attacking a ready enemy with the retaliate keyword, after applying all results for that skill test, that enemy performs an attack against the attacking investigator. An enemy does not exhaust after performing a retaliate attack."* — trigger is **failed Fight only**; **no exhaust** (already satisfied: exhaust is gated to `EnemyAttackSource::EnemyPhase` in `process_attacker_dealing`).
- **Card text verified, never paraphrased:** confirm Dodge 01023 / Guard Dog 01021 text + FAQ at `https://arkhamdb.com/card/01023` / `/01021` before asserting in K2b.
- **Event-assertion macros** (`assert_event!` / `assert_no_event!` / `assert_event_sequence!`) over raw slice indexing.

---

### Task 1 (K2a): Route retaliate through the attack loop

The whole engine change as one reviewable unit: the new source variant, the `drive_retaliate` helper, the suspendable `fire_retaliate_if_any`, the `PostRetaliate` suspension handling, and the resume arm. The no-window path is behaviour-preserving (the existing `failed_fight_*` tests stay green); the window-suspend/resume path is first exercised by Task 2's registry-backed tests (mirroring K1, where the suspend path lived in the integration task).

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (the `EnemyAttackSource` enum, ~line 317)
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (add `drive_retaliate`; `resume_enemy_attack`'s `match source` tail, ~line 922)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (`fire_retaliate_if_any` ~line 854; the `PostRetaliate` arm of `drive_skill_test` ~line 442)
- Test: `crates/game-core/src/engine/dispatch/combat.rs` (`#[cfg(test)] mod combat_tests`) + the existing `engine/mod.rs` retaliate tests stay green.

**Interfaces:**
- Consumes: `drive_attack_loop(cx, investigator, attackers: Vec<EnemyId>, source) -> EngineOutcome`; `skill_test::drive_skill_test(cx) -> EngineOutcome` (pub(super)); `Continuation::SkillTest` carrying the `FinishContinuation` cursor.
- Produces: `EnemyAttackSource::Retaliate`; `pub(super) fn drive_retaliate(cx: &mut Cx, enemy: EnemyId, investigator: InvestigatorId) -> EngineOutcome`; `fire_retaliate_if_any(cx, investigator, succeeded) -> EngineOutcome` (was `()`).

- [ ] **Step 1: Write the failing test — `drive_retaliate` deals damage and does not exhaust**

Add to `combat_tests` (mirror the existing `drive_aoo_deals_damage_but_does_not_exhaust_the_attacker` test for the harness idiom — same `Cx { state, events }` construction and real `Event` names):

```rust
#[test]
fn drive_retaliate_deals_damage_but_does_not_exhaust_the_attacker() {
    // RR p.18: a retaliate attack does not exhaust the attacker.
    let inv_id = InvestigatorId(1);
    let mut enemy = test_enemy(100, "Retaliator");
    enemy.retaliate = true;
    enemy.attack_damage = 1;
    enemy.attack_horror = 0;
    // Not engaged: a retaliate fires regardless of engagement, driven by enemy id.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_enemy(enemy)
        .build();
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };

    let outcome = super::drive_retaliate(&mut cx, EnemyId(100), inv_id);

    assert!(matches!(outcome, crate::engine::EngineOutcome::Done));
    assert!(!cx.state.enemies[&EnemyId(100)].exhausted, "retaliate must not exhaust (RR p.18)");
    assert_eq!(cx.state.investigators[&inv_id].damage, 1, "retaliate dealt 1 damage");
    assert_no_event!(events, Event::EnemyExhausted { .. });
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core --lib drive_retaliate_deals_damage`
Expected: FAIL — `drive_retaliate` not found.

- [ ] **Step 3: Add the `Retaliate` source variant**

In `game_state.rs`, extend `EnemyAttackSource`:

```rust
pub enum EnemyAttackSource {
    /// Enemy-phase step 3.3 (`resolve_attacks_for_investigator`).
    EnemyPhase,
    /// Attack of opportunity (`drive_aoo`).
    AttackOfOpportunity,
    /// Retaliate attack from a failed Fight (`drive_retaliate`, RR p.18).
    Retaliate,
}
```

- [ ] **Step 4: Add `drive_retaliate`**

In `combat.rs`, next to `drive_aoo`:

```rust
/// Fire a single Retaliate attack from `enemy` against `investigator`, driving it
/// through the shared attack loop (#379) so it opens the before-attack cancel
/// window (Dodge 01023) and the per-soaked-asset reaction window (Guard Dog 01021).
/// A retaliate is one enemy attacking once, so the attacker list is a singleton;
/// the two sequential suspension points are tracked by [`AttackLoopStage`]. Returns
/// [`AwaitingInput`] if a window suspends, [`Done`] otherwise. Non-exhausting
/// (RR p.18) — honored by [`EnemyAttackSource::Retaliate`] (exhaust is
/// `EnemyPhase`-gated). Caller (`fire_retaliate_if_any`) has already confirmed the
/// enemy is ready + has the retaliate keyword.
///
/// [`AwaitingInput`]: crate::engine::EngineOutcome::AwaitingInput
/// [`Done`]: crate::engine::EngineOutcome::Done
pub(super) fn drive_retaliate(
    cx: &mut Cx,
    enemy: EnemyId,
    investigator: InvestigatorId,
) -> EngineOutcome {
    drive_attack_loop(cx, investigator, vec![enemy], EnemyAttackSource::Retaliate)
}
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test -p game-core --lib drive_retaliate_deals_damage`
Expected: PASS.
Run: `cargo build -p game-core 2>&1 | grep -i "non-exhaustive\|match"` — expect the `resume_enemy_attack` `match source` to now be non-exhaustive (compile error). That is the next step's failing signal:

Run: `RUSTFLAGS="-D warnings" cargo build -p game-core`
Expected: FAIL — `match source` in `resume_enemy_attack` doesn't cover `Retaliate`.

- [ ] **Step 6: Wire the resume arm**

In `combat.rs`, `resume_enemy_attack`'s `match source` tail, add the `Retaliate` arm:

```rust
    match source {
        EnemyAttackSource::EnemyPhase => {
            super::reaction_windows::after_enemy_phase_attacks(cx, investigator)
        }
        EnemyAttackSource::AttackOfOpportunity => EngineOutcome::Done,
        // The retaliate's window closed; the loop drained. Hand control back to the
        // Fight's skill-test follow-up (its `SkillTest` frame is now top, cursor at
        // `PostOnResolution`) so teardown finishes (#379).
        EnemyAttackSource::Retaliate => super::skill_test::drive_skill_test(cx),
    }
```

Run: `RUSTFLAGS="-D warnings" cargo build -p game-core`
Expected: PASS (match is total again).

- [ ] **Step 7: Write the failing test — retaliate still fires + the new route keeps the existing behaviour**

The existing retaliate tests (`engine/mod.rs`: `failed_fight_against_ready_retaliate_enemy_triggers_attack`, `successful_fight_against_retaliate_enemy_does_not_trigger_attack`, `failed_fight_against_exhausted_retaliate_enemy_does_not_trigger_attack`) are the behaviour-preservation pins; they must stay green after Step 8. They already assert the investigator takes retaliate damage on a failed Fight and not otherwise. Run them now to confirm the baseline:

Run: `cargo test -p game-core --lib retaliate`
Expected: PASS (pre-change baseline).

Add one new pin for the non-exhaust route in `engine/mod.rs` next to those tests (use that module's existing builder idiom for a failed Fight; copy the setup from `failed_fight_against_ready_retaliate_enemy_triggers_attack`):

```rust
#[test]
fn retaliate_via_loop_does_not_exhaust_the_enemy() {
    // After K2 routes retaliate through drive_attack_loop, a failed Fight against a
    // ready retaliate enemy still deals the retaliate damage AND leaves the enemy
    // ready (RR p.18) — the loop path must not exhaust it.
    // ... build a failed-Fight scenario vs a ready retaliate enemy (copy the
    //     setup from failed_fight_against_ready_retaliate_enemy_triggers_attack),
    //     drive it to completion, then assert:
    //   - the investigator took the enemy's attack_damage (retaliate landed), and
    //   - state.enemies[&enemy].exhausted == false.
}
```

- [ ] **Step 8: Make `fire_retaliate_if_any` suspendable + route through `drive_retaliate`; handle the `PostRetaliate` suspension**

In `skill_test.rs`, change `fire_retaliate_if_any` to return `EngineOutcome` and route through `drive_retaliate`:

```rust
fn fire_retaliate_if_any(
    cx: &mut Cx,
    investigator: InvestigatorId,
    succeeded: bool,
) -> EngineOutcome {
    if succeeded {
        return EngineOutcome::Done;
    }
    let follow_up = cx.state.current_skill_test().map(|t| t.follow_up);
    let Some(SkillTestFollowUp::Fight { enemy, .. }) = follow_up else {
        return EngineOutcome::Done;
    };
    let retaliates = cx
        .state
        .enemies
        .get(&enemy)
        .is_some_and(|e| e.retaliate && !e.exhausted);
    if retaliates {
        // Route through the attack loop (#379) so the retaliate opens its cancel
        // (Dodge) and soak (Guard Dog) windows; non-exhausting (RR p.18).
        super::combat::drive_retaliate(cx, enemy, investigator)
    } else {
        EngineOutcome::Done
    }
}
```

Update the `PostRetaliate` arm of `drive_skill_test` (advance the cursor **before** firing, so a suspended retaliate resumes at `PostOnResolution`; propagate a suspension):

```rust
            FinishContinuation::PostRetaliate { succeeded } => {
                // Advance the cursor first: a retaliate that suspends on its
                // cancel/soak window resumes here at PostOnResolution (the retaliate
                // already happened; only its window is being resolved).
                cx.state
                    .current_skill_test_mut()
                    .expect("the SkillTest frame must persist across driver steps")
                    .continuation = FinishContinuation::PostOnResolution { succeeded };
                let outcome = fire_retaliate_if_any(cx, investigator, succeeded);
                if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
                    return outcome; // parked on the retaliate's window; resume via drive_skill_test
                }
            }
```

Also update `fire_retaliate_if_any`'s doc-comment: it now routes through `drive_retaliate` (not `enemy_attack`) and opens the cancel/soak windows; drop the "future home of the reaction window" line (now realized for the attack windows; the separate after-resolution window is still #64).

- [ ] **Step 9: Run the retaliate suite + the new pin**

Run: `cargo test -p game-core --lib retaliate`
Expected: PASS (existing pins green + `retaliate_via_loop_does_not_exhaust_the_enemy`).
Run: `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS (full crate — the no-window route is behaviour-preserving everywhere).

- [ ] **Step 10: Per-task gauntlet + commit**

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings`, `RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features`, `cargo fmt` then `cargo fmt --check`.
Expected: all clean.

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/combat.rs crates/game-core/src/engine/dispatch/skill_test.rs crates/game-core/src/engine/mod.rs
git commit -m "engine: route Retaliate through the attack loop so it opens cancel/soak windows (K2a of #379)"
```

---

### Task 2 (K2b): Integration — Dodge cancels + Guard Dog retaliates against a retaliate

The registry-backed proof of the suspend→`ResolveInput`→resume cycle and the #379 acceptance. This is where the `Retaliate` resume arm (Task 1, Step 6) is first exercised end-to-end.

**Files:**
- Create: `crates/cards/tests/retaliate_windows.rs`

**Interfaces:**
- Consumes: `cards::REGISTRY` (installed per test process); the Fight action + `InputResponse` for window resolution; a `retaliate`-keyword enemy. Follow the harness pattern in `crates/cards/tests/dodge_aoo.rs` and `guard_dog_soak.rs` (K1).

- [ ] **Step 1: Verify card text before asserting**

WebFetch `https://arkhamdb.com/card/01023` (Dodge) and `https://arkhamdb.com/card/01021` (Guard Dog), **including FAQ**. Confirm Dodge cancels an enemy attack (a retaliate is an enemy attack) and Guard Dog's reaction deals 1 to the attacking enemy when it soaks damage. Record the verified text in the test file's header comment. If WebFetch is unavailable, fall back to `data/arkhamdb-snapshot/pack/` and say so in the report.

- [ ] **Step 2: Write the failing test — Guard Dog retaliates against a retaliate**

In `retaliate_windows.rs` (install `cards::REGISTRY` per the existing tests' pattern): set up an investigator controlling Guard Dog in play, at a ready `retaliate`-keyword enemy (a `test_enemy` with `retaliate = true` is fine — the registry is for Dodge/Guard Dog, not the enemy). Force a **failed Fight** (low combat vs high enemy fight + a deterministic chaos token so the total misses). The failed Fight fires the retaliate; its damage soaks onto Guard Dog, opening the `AfterEnemyAttackDamagedAsset` window; resolve it (`ResolveInput`) so Guard Dog deals 1 to the retaliating enemy. Assert: the enemy took 1 (Guard Dog's reaction), the enemy did **not** exhaust (RR p.18), and the Fight's skill test completed afterward (`Event::SkillTestEnded` fired; no `SkillTest` frame remains on `state.continuations`).

- [ ] **Step 3: Write the failing test — Dodge cancels a retaliate**

Same setup but Dodge in hand instead (or alongside): the failed Fight fires the retaliate; the `BeforeEnemyAttack` window opens; play Dodge (`ResolveInput`) to cancel. Assert: no damage/horror dealt by the retaliate (investigator damage unchanged from pre-retaliate), the enemy did not exhaust, and the skill test completed (`SkillTestEnded`, no `SkillTest` frame left).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p cards --test retaliate_windows`
Expected: PASS — Tasks 1+2 together wire the full cycle. If a test FAILs (e.g. the skill test never tears down after the window, or the retaliate window never opens), debug the resume chain (`drive_retaliate` → window → `resume_enemy_attack` Retaliate arm → `drive_skill_test` → teardown). Do **not** weaken assertions to pass.

- [ ] **Step 5: Full CI gauntlet**

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
git add crates/cards/tests/retaliate_windows.rs
git commit -m "test: Dodge cancels + Guard Dog retaliates against a retaliate attack (closes #379)"
```

---

## Self-Review

- **Spec coverage:** ✅ new `EnemyAttackSource::Retaliate` + `drive_retaliate` (Task 1 Steps 3–4); `fire_retaliate_if_any` returns `EngineOutcome` + routes through the loop (Step 8); `PostRetaliate` advances the cursor before firing + propagates the suspension (Step 8); `resume_enemy_attack` Retaliate arm re-enters `drive_skill_test` (Step 6); non-exhaust inherited (covered by the Step-1 and Step-7 tests); failed-Fight-only scope unchanged (the gate in `fire_retaliate_if_any` is untouched); Dodge-cancel + Guard-Dog-retaliate windows (Task 2). The "after an enemy attacks" window (#64) is explicitly out of scope and no task adds it.
- **Placeholder scan:** the two `// ...` markers (Task 1 Step 7 test body, Task 2 test bodies) are explicit "copy the setup from named existing test" instructions with concrete assertions enumerated, not vague logic — the registry-backed Fight-setup idiom is non-trivial and lives in the named sibling tests, so pointing at them is correct over transcribing a guessed setup.
- **Type consistency:** `drive_retaliate(cx, enemy, investigator) -> EngineOutcome`, `fire_retaliate_if_any(...) -> EngineOutcome`, and `EnemyAttackSource::Retaliate` are named identically across the tasks that define and call them; the `resume_enemy_attack` arm and the `PostRetaliate` handler both rely on `drive_skill_test(cx) -> EngineOutcome` (the real pub(super) signature).

## Risks & notes for the implementer

- **The resume chain is the seam:** a retaliate window closing must unwind `resume_enemy_attack` (pops the `AttackLoop`, drains) → its `Retaliate` arm → `drive_skill_test` → reads cursor `PostOnResolution` → teardown (`SkillTestEnded`, pop `SkillTest`). If Task 2's tests hang at the window or never emit `SkillTestEnded`, trace that chain — the cursor must be `PostOnResolution` at park time (Task 1 Step 8 advances it before firing).
- **`run_window_continuation` needs no change:** it routes the soak/cancel window-close to `resume_enemy_attack` by *window kind*, and `resume_enemy_attack` reads the source from the `AttackLoop` frame — so a `Retaliate`-source loop resumes through the same path as AoO/enemy-phase.
- **Event/field names** (`EnemyExhausted`, `DamageTaken`/the real attack-damage event, `SkillTestEnded`) — confirm against `crates/game-core/src/event.rs`; `Event::EnemyAttacked` does **not** exist (the attack emits `DamageTaken`).
- **Keep assertions honest** (verification-before-completion): if Task 2 fails, fix the engine, never the assertion.
