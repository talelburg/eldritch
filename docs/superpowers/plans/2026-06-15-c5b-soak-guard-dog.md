# C5b — Enemy-Attack Damage Soak + Guard Dog Reaction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the core enemy-attack damage/horror soak mechanic so Guard Dog 01021's faithful "damage to Guard Dog" reaction fires and deals 1 damage to the attacking enemy.

**Architecture:** The enemy-attack pipeline becomes `assign → place simultaneously → defeat-check → reaction window` (RR p.7). Assignment is a swappable step: deterministic soak-first now, interactive distribution deferred to a reframed #44. A new reaction window (`AfterEnemyAttackDamagedAsset`) fires the soaked asset's `EnemyAttackDamagedSelf` ability; the attack loop is made suspend/resumable via `pending_enemy_attack`, the persistence `combat.rs:265` already predicted.

**Tech Stack:** Rust workspace (`card-dsl`, `game-core`, `cards`). Event-sourced engine; validate-first/mutate-second handlers; reaction-window pipeline in `crates/game-core/src/engine/dispatch/reaction_windows.rs`.

**Spec:** `docs/superpowers/specs/2026-06-15-phase-7-slice-1-c5b-soak-guard-dog-design.md`

**CI gauntlet (run before every push):**
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `crates/card-dsl/src/dsl.rs` | `EventPattern::EnemyAttackDamagedSelf` variant | Modify |
| `crates/game-core/src/state/window.rs` (or wherever `WindowKind` lives) | `WindowKind::AfterEnemyAttackDamagedAsset` | Modify |
| `crates/game-core/src/state/game_state.rs` | `pending_enemy_attack`, `PendingEnemyAttack`, `EnemyAttackSource` | Modify |
| `crates/game-core/src/engine/evaluator.rs` | `EvalContext.attacking_enemy` field + constructors | Modify |
| `crates/game-core/src/engine/dispatch/combat.rs` | `assign_attack`, placement, asset defeat, rewritten `enemy_attack`, resumable `resolve_attacks_for_investigator` | Modify |
| `crates/game-core/src/engine/dispatch/reaction_windows.rs` | `trigger_matches` arm, `attacking_enemy` threading, `run_window_continuation` arm | Modify |
| `crates/game-core/src/engine/dispatch/mod.rs` | `resolve_input` routing for resume (if needed beyond window-close path) | Modify |
| `crates/cards/src/impls/guard_dog.rs` | Guard Dog ability + native retaliate effect + card test | Create |
| `crates/cards/src/impls/mod.rs` + native registry | register `guard_dog` + native tag | Modify |
| `crates/cards/tests/guard_dog_soak.rs` | end-to-end integration test | Create |

Locate `WindowKind`'s definition first:
```sh
grep -rn "pub enum WindowKind" crates/game-core/src/
```

---

## Task 1: Add `EventPattern::EnemyAttackDamagedSelf`

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (the `EventPattern` enum, ~line 190; serde test block ~line 1456)

- [ ] **Step 1: Write the failing serde round-trip test**

In the `#[cfg(test)]` block near the existing `WouldDiscoverClues` / `GameEnd` round-trip (search `for p in [EventPattern::WouldDiscoverClues`):

```rust
#[test]
fn enemy_attack_damaged_self_round_trips() {
    let p = EventPattern::EnemyAttackDamagedSelf;
    let json = serde_json::to_string(&p).expect("serialize");
    let back: EventPattern = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(p, back);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p card-dsl enemy_attack_damaged_self_round_trips`
Expected: FAIL — `no variant named EnemyAttackDamagedSelf`.

- [ ] **Step 3: Add the variant**

After the `GameEnd` variant (end of the `EventPattern` enum, ~line 309), add:

```rust
    /// An enemy attack dealt damage to the asset this ability is printed
    /// on (the soaked ally). Bare — the engine binds *self* = the soaked
    /// asset instance from the firing window context, the way
    /// [`EnteredLocation`](Self::EnteredLocation) / [`EndOfTurn`](Self::EndOfTurn)
    /// bind theirs. Matched **only** by
    /// `WindowKind::AfterEnemyAttackDamagedAsset` in the reaction
    /// pipeline; `trigger_matches` binds the attacking enemy into the
    /// `EvalContext`. First (and only) consumer: Guard Dog 01021's
    /// "[reaction] When an enemy attack deals damage to Guard Dog: Deal 1
    /// damage to the attacking enemy." (C5b #237.)
    EnemyAttackDamagedSelf,
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p card-dsl enemy_attack_damaged_self_round_trips`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/card-dsl/src/dsl.rs
git commit -m "card-dsl: add EventPattern::EnemyAttackDamagedSelf (C5b)"
```

---

## Task 2: Add `WindowKind::AfterEnemyAttackDamagedAsset`

**Files:**
- Modify: the `WindowKind` enum (path from the grep above)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` — `trigger_matches` exhaustive false-arm (~line 189) so the crate still compiles

- [ ] **Step 1: Add the variant**

Next to `AfterEnemyDefeated { enemy, by }`:

```rust
    /// An enemy attack placed damage on a controlled asset (soak). Opens
    /// after placement so the soaked asset's `EnemyAttackDamagedSelf`
    /// reaction (Guard Dog 01021) can fire. `asset` is the soaked
    /// instance, `enemy` the attacker (threaded into the reaction's
    /// `EvalContext.attacking_enemy`), `controller` the asset's owner.
    /// (C5b #237.)
    AfterEnemyAttackDamagedAsset {
        asset: CardInstanceId,
        enemy: EnemyId,
        controller: InvestigatorId,
    },
```

Ensure `CardInstanceId`, `EnemyId`, `InvestigatorId` are imported in that module.

- [ ] **Step 2: Run to verify the non-exhaustive match fails**

Run: `cargo build -p game-core 2>&1 | head -20`
Expected: FAIL — `trigger_matches` (and any other exhaustive `match kind`) now non-exhaustive.

- [ ] **Step 3: Add the false-arm to the catch-all in `trigger_matches`**

In `reaction_windows.rs` extend the existing `(WindowKind::PlayerWindow(_) | WindowKind::AfterEnemyDefeated { .. }, …) => false` arm to also cover the new kind paired with non-matching patterns. The cleanest edit: add `| WindowKind::AfterEnemyAttackDamagedAsset { .. }` to that arm's kind list, and the real match arm comes in Task 9. For now this makes every `(new kind, pattern)` pair return `false`:

```rust
        (
            WindowKind::PlayerWindow(_)
            | WindowKind::AfterEnemyDefeated { .. }
            | WindowKind::AfterEnemyAttackDamagedAsset { .. },
            EventPattern::EnemyDefeated { .. }
            | EventPattern::CardRevealed { .. }
            | EventPattern::EnemySpawned
            | EventPattern::EnteredLocation
            | EventPattern::PhaseEnded { .. }
            | EventPattern::ActAdvanced
            | EventPattern::AgendaAdvanced
            | EventPattern::RoundEnded
            | EventPattern::EndOfTurn
            | EventPattern::AfterLocationInvestigated
            | EventPattern::WouldDiscoverClues
            | EventPattern::GameEnd
            | EventPattern::EnemyAttackDamagedSelf,
        ) => false,
```

Also fix any other exhaustive `match kind` over `WindowKind` the build flags (e.g. `run_window_continuation` ~line 686 — add a temporary `WindowKind::AfterEnemyAttackDamagedAsset { .. } => EngineOutcome::Done,` arm; replaced in Task 12).

- [ ] **Step 4: Run to verify it builds**

Run: `cargo build -p game-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "game-core: add WindowKind::AfterEnemyAttackDamagedAsset (C5b)"
```

---

## Task 3: Add `EvalContext.attacking_enemy`

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (`EvalContext` struct + every constructor: `for_controller`, `for_controller_with_source`, and any others — grep `impl EvalContext`)

- [ ] **Step 1: Add the field**

After `clue_discovery_count` in the struct:

```rust
    /// The attacking enemy bound while resolving an
    /// `EnemyAttackDamagedSelf` reaction, so the card-local
    /// `Effect::Native` retaliate can name it. `None` outside that
    /// window. Mirrors `failed_by` / `clue_discovery_count`. (C5b #237.)
    pub attacking_enemy: Option<crate::state::EnemyId>,
```

- [ ] **Step 2: Run to verify constructors fail**

Run: `cargo build -p game-core 2>&1 | head -20`
Expected: FAIL — missing field in `EvalContext { .. }` literals.

- [ ] **Step 3: Default it in every constructor**

In each `EvalContext` constructor body add `attacking_enemy: None,`. (Grep `EvalContext {` within `evaluator.rs` to find them all, including the test-support ones.)

- [ ] **Step 4: Run to verify it builds**

Run: `cargo build -p game-core --all-features`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/evaluator.rs
git commit -m "game-core: thread EvalContext.attacking_enemy (C5b)"
```

---

## Task 4: Add `pending_enemy_attack` state

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (new fields/types near `pending_end_turn` ~line 224; default in the constructor/`Default`)

- [ ] **Step 1: Add the types and field**

Near `SpawnEngagePending` / `ActRoundEndPending`:

```rust
/// Which driver to resume after a mid-attack reaction window closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnemyAttackSource {
    /// Enemy-phase step 3.3 (`resolve_attacks_for_investigator`).
    EnemyPhase,
    /// Attack of opportunity (`fire_attacks_of_opportunity`).
    AttackOfOpportunity,
}

/// A parked enemy-attack loop, suspended because an attack's damage
/// soaked onto an asset and opened an `AfterEnemyAttackDamagedAsset`
/// reaction window. Resumed by `resume_enemy_attack` once the window
/// closes — the same suspend/resume shape as [`GameState::pending_end_turn`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingEnemyAttack {
    /// The investigator whose engaged enemies are attacking.
    pub investigator: InvestigatorId,
    /// Attackers not yet resolved (the current attacker already
    /// resolved before the window opened), in resolution order.
    pub remaining_attackers: Vec<EnemyId>,
    /// Which loop to re-enter.
    pub source: EnemyAttackSource,
}
```

Add the field to `GameState`:

```rust
    /// `Some` while an enemy-attack loop is suspended on a soak reaction
    /// window (C5b #237). Mirror of [`pending_end_turn`](Self::pending_end_turn).
    pub pending_enemy_attack: Option<PendingEnemyAttack>,
```

- [ ] **Step 2: Run to verify default fails**

Run: `cargo build -p game-core 2>&1 | head -20`
Expected: FAIL — missing field in `GameState` construction.

- [ ] **Step 3: Default it**

In `GameState`'s constructor/`Default`, add `pending_enemy_attack: None,`.

- [ ] **Step 4: Run to verify it builds + existing tests pass**

Run: `cargo build -p game-core --all-features && cargo test -p game-core --lib`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/game_state.rs
git commit -m "game-core: add pending_enemy_attack suspend state (C5b)"
```

---

## Task 5: Deterministic `assign_attack`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (new pure helper + unit test in the module's `#[cfg(test)]`)

The function reads printed health/sanity from the registry metadata. Pattern for reading metadata: `card_registry::current()` then `(reg.metadata_for)(&code)` → match `CardKind::Asset { health, sanity, .. }`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn assign_attack_fills_soaker_before_investigator() {
    // 1 ally with health 3, attack deals 2 damage / 0 horror →
    // all 2 damage soaks onto the ally, none on the investigator.
    // (Build via TestGame with Guard Dog in cards_in_play; see existing
    // combat.rs tests for the builder pattern + REGISTRY install.)
    let assignment = /* call assign_attack(state, enemy, inv) */;
    assert_eq!(assignment.investigator_damage, 0);
    assert_eq!(assignment.asset_damage.get(&ally_instance), Some(&2));
}
```

Use the existing combat-test scaffolding (grep `mod tests` in `combat.rs`); install `cards::REGISTRY` is **not** available from `game-core` — instead use a test-local registry or move this assignment unit test to where a registry exists. **If `game-core` can't install a real registry, test `assign_attack` against a synthetic metadata source** the same way other `game-core` tests fake card data, OR make `assign_attack` take the metadata lookup as a parameter (`fn assign_attack(eligible: &[(CardInstanceId, u8 /*remaining health*/, u8 /*remaining sanity*/)], inv: InvestigatorId, damage: u8, horror: u8) -> Assignment`) so it's a pure function unit-testable without the registry. **Prefer the parameterized pure form** — it isolates the soak-ordering logic and defers registry coupling to the caller (Task 8).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core assign_attack_fills_soaker_before_investigator`
Expected: FAIL — `assign_attack` not defined.

- [ ] **Step 3: Implement the pure assignment**

```rust
/// A computed damage/horror distribution for one enemy attack.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Assignment {
    pub investigator_damage: u8,
    pub investigator_horror: u8,
    /// instance → damage soaked onto it.
    pub asset_damage: std::collections::BTreeMap<CardInstanceId, u8>,
    /// instance → horror soaked onto it.
    pub asset_horror: std::collections::BTreeMap<CardInstanceId, u8>,
}

/// One eligible soaker: remaining damage/horror capacity (printed stat
/// minus already-accumulated). Caller derives these from registry
/// metadata + `CardInPlay.accumulated_*`.
pub(super) struct Soaker {
    pub instance: CardInstanceId,
    pub remaining_health: u8,
    pub remaining_sanity: u8,
}

/// Deterministic soak-first assignment (TODO(#44): replace with an
/// interactive distribution prompt). Fills `soakers` (already ordered by
/// CardInstanceId by the caller) up to remaining capacity, then the
/// investigator absorbs the rest. Damage and horror are assigned
/// independently.
pub(super) fn assign_attack(soakers: &[Soaker], mut damage: u8, mut horror: u8) -> Assignment {
    let mut a = Assignment::default();
    for s in soakers {
        let d = damage.min(s.remaining_health);
        if d > 0 {
            a.asset_damage.insert(s.instance, d);
            damage -= d;
        }
        let h = horror.min(s.remaining_sanity);
        if h > 0 {
            a.asset_horror.insert(s.instance, h);
            horror -= h;
        }
    }
    a.investigator_damage = damage;
    a.investigator_horror = horror;
    a
}
```

Adjust the Step-1 test to build `&[Soaker { instance: ally_instance, remaining_health: 3, remaining_sanity: 1 }]` and call `assign_attack(&soakers, 2, 0)`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p game-core assign_attack`
Expected: PASS. Add a second test: `assign_attack(&[Soaker{health:1,sanity:0,..}], 2, 0)` → asset_damage 1, investigator_damage 1 (overflow).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs
git commit -m "game-core: deterministic soak-first assign_attack (C5b)"
```

---

## Task 6: Placement + asset defeat-on-overflow

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs`
- Look at: `crates/game-core/src/engine/dispatch/elimination.rs` for the asset-discard event shape (`Event::CardDiscarded`)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn place_assignment_soaks_and_defeats_overflowed_asset() {
    // ally health 1, assign 2 damage → accumulated reaches 1 >= health,
    // ally is discarded from cards_in_play; CardDiscarded emitted.
    // (Builder + REGISTRY-bearing context as in other combat tests; if
    // metadata is needed, route through the integration crate instead —
    // see note in Task 8.)
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core place_assignment_soaks_and_defeats_overflowed_asset`
Expected: FAIL.

- [ ] **Step 3: Implement placement + asset defeat**

```rust
/// Place a computed [`Assignment`] simultaneously (RR p.7), then check
/// defeats. Returns the list of assets that took damage (for the soak
/// reaction window) and whether the investigator crossed a lethal
/// threshold. Asset defeat (accumulated >= printed stat) discards the
/// instance from `cards_in_play` with `Event::CardDiscarded`.
pub(super) fn place_assignment(
    cx: &mut Cx,
    investigator: InvestigatorId,
    a: &Assignment,
) -> Vec<CardInstanceId> {
    // 1. Place on assets (accumulate).
    for (inst, dmg) in &a.asset_damage {
        if let Some(card) = find_controlled_mut(cx.state, investigator, *inst) {
            card.accumulated_damage = card.accumulated_damage.saturating_add(*dmg);
        }
    }
    for (inst, hor) in &a.asset_horror {
        if let Some(card) = find_controlled_mut(cx.state, investigator, *inst) {
            card.accumulated_horror = card.accumulated_horror.saturating_add(*hor);
        }
    }
    // 2. Place on investigator (existing numeric helpers; they emit
    //    DamageTaken / HorrorTaken and return lethality).
    let dmg_lethal = apply_damage_numeric(cx, investigator, a.investigator_damage);
    let hor_lethal = apply_horror_numeric(cx, investigator, a.investigator_horror);
    if dmg_lethal || hor_lethal {
        let cause = if dmg_lethal { DefeatCause::Damage } else { DefeatCause::Horror };
        super::elimination::apply_investigator_defeat(cx, investigator, cause);
    }
    // 3. Defeat overflowed assets (discard). Returns the damaged-asset
    //    list for the window caller BEFORE discarding, so a defeated
    //    soaker still gets its reaction window (Guard Dog "deals 1 damage"
    //    even on the attack that kills it — RR: reaction triggers on the
    //    damage event).
    let damaged: Vec<CardInstanceId> = a.asset_damage.keys().copied().collect();
    defeat_overflowed_assets(cx, investigator);
    damaged
}
```

Implement `find_controlled_mut` (look up the instance in `inv.cards_in_play`) and `defeat_overflowed_assets` (for each in-play asset, read printed health/sanity from `(reg.metadata_for)(&code)`; if `accumulated_damage >= health` or `accumulated_horror >= sanity`, remove from `cards_in_play`, push `Event::CardDiscarded { code, from: Zone::PlayArea /* match existing variant */ }`). Mirror the discard event used elsewhere — grep `Event::CardDiscarded` for the exact field shape.

**Ordering note:** return the damaged list, queue windows (Task 10) for *living* soakers, and let defeat happen — but the window must reference the asset instance which may be discarded. Decision: queue the window for a damaged asset **only if it survives** (if Guard Dog is defeated by the same attack it does not retaliate, since it has left play before the reaction would resolve). Filter `damaged` to instances still in `cards_in_play` after `defeat_overflowed_assets`. Document this as the chosen reading.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p game-core place_assignment`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs
git commit -m "game-core: simultaneous placement + asset defeat-on-overflow (C5b)"
```

---

## Task 7: Rewrite `enemy_attack` over the new pipeline

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`enemy_attack`, ~line 186)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn enemy_attack_with_no_soakers_matches_old_behavior() {
    // Attack 2/0 against an investigator with no assets → inv.damage == 2,
    // DamageTaken emitted, no asset interaction. (Regression guard that
    // the rewrite is behavior-preserving with zero soakers.)
}
```

- [ ] **Step 2: Run to verify it fails or passes**

Run: `cargo test -p game-core enemy_attack_with_no_soakers_matches_old_behavior`
Expected: PASS already (old behavior) — keep it as a regression guard through the rewrite. If the builder differs, adjust until green against current code first.

- [ ] **Step 3: Rewrite `enemy_attack`**

Replace the body that calls `apply_damage_numeric` / `apply_horror_numeric` directly with:

```rust
pub(super) fn enemy_attack(cx: &mut Cx, enemy_id: EnemyId, investigator: InvestigatorId) {
    let enemy = cx.state.enemies.get(&enemy_id).unwrap_or_else(|| {
        unreachable!(
            "enemy_attack: enemy {enemy_id:?} is not in state.enemies; \
             this is a state-corruption invariant violation"
        )
    });
    let damage = enemy.attack_damage;
    let horror = enemy.attack_horror;

    // Build eligible soakers (controlled assets with remaining capacity),
    // ordered by CardInstanceId. Registry metadata gives printed
    // health/sanity; CardInPlay.accumulated_* gives what's used.
    let soakers = build_soakers(cx.state, investigator);
    let assignment = assign_attack(&soakers, damage, horror);
    let damaged_survivors = place_assignment(cx, investigator, &assignment);

    // Queue a soak reaction window per surviving damaged asset (Task 10).
    for asset in damaged_survivors {
        super::reaction_windows::queue_reaction_window(
            cx,
            WindowKind::AfterEnemyAttackDamagedAsset { asset, enemy: enemy_id, controller: investigator },
        );
    }
}
```

Implement `build_soakers(state, investigator) -> Vec<Soaker>`: iterate `inv.cards_in_play` in order, look up `(reg.metadata_for)(&code)`, and for `CardKind::Asset { health, sanity, .. }` with any remaining capacity push a `Soaker { instance, remaining_health: health.unwrap_or(0).saturating_sub(accumulated_damage), remaining_sanity: sanity.unwrap_or(0).saturating_sub(accumulated_horror) }`. Skip assets with both capacities 0. Returns empty when the registry is absent (tests that don't install it keep old behavior).

- [ ] **Step 4: Run to verify**

Run: `cargo test -p game-core --lib combat` and the existing AoO/enemy-phase tests.
Expected: PASS (no-soaker regression guard green; existing tests unaffected because no soakers are installed).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs
git commit -m "game-core: route enemy_attack through assign/place/window pipeline (C5b)"
```

---

## Task 8: `trigger_matches` arm for the soak window

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`trigger_matches`, ~line 151)

- [ ] **Step 1: Add the real match arm**

Before the catch-all false arm, add:

```rust
        (
            WindowKind::AfterEnemyAttackDamagedAsset { .. },
            EventPattern::EnemyAttackDamagedSelf,
        ) => true,
```

And **remove** `EnemyAttackDamagedSelf` from being matched-false for `AfterEnemyAttackDamagedAsset` — i.e. the catch-all still returns false for that kind paired with *other* patterns. (The scan in `scan_pending_triggers` already restricts to the soaked asset's own abilities only if you filter by instance; see Step 2.)

- [ ] **Step 2: Scope the scan to the soaked asset instance**

`scan_pending_triggers` scans every controlled instance. For `AfterEnemyAttackDamagedAsset { asset, .. }`, only the `asset` instance should match (self-binding). Add an instance filter: when `kind` is `AfterEnemyAttackDamagedAsset { asset, .. }`, skip every `card.instance_id != asset`. Implement by threading the kind into the per-card loop (it's already in scope) and `continue`-ing on mismatch.

- [ ] **Step 3: Unit test the matcher**

```rust
#[test]
fn soak_window_matches_only_self_instance() {
    // Two allies in play, attack soaks onto ally A. A window keyed to A
    // produces exactly one pending trigger (A's reaction), not B's.
}
```

Run: `cargo test -p game-core soak_window_matches_only_self_instance`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "game-core: soak window matches the self asset's EnemyAttackDamagedSelf (C5b)"
```

---

## Task 9: Thread `attacking_enemy` into the reaction `EvalContext`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`fire_pending_trigger`, ~line 363)

- [ ] **Step 1: Bind the enemy from the window kind**

The window being driven is `cx.state.open_windows[window_idx]`. Read its `kind`; if `AfterEnemyAttackDamagedAsset { enemy, .. }`, set the field on the context:

```rust
    let mut eval_ctx =
        EvalContext::for_controller_with_source(trigger.controller, trigger.instance_id);
    if let WindowKind::AfterEnemyAttackDamagedAsset { enemy, .. } =
        cx.state.open_windows[window_idx].kind
    {
        eval_ctx.attacking_enemy = Some(enemy);
    }
```

(`eval_ctx` becomes `mut`.)

- [ ] **Step 2: Build + lib tests**

Run: `cargo test -p game-core --lib reaction`
Expected: PASS (no behavior change yet — no card reads `attacking_enemy`).

- [ ] **Step 3: Commit**

```bash
git add crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "game-core: bind attacking_enemy into soak reaction EvalContext (C5b)"
```

---

## Task 10: Resumable `resolve_attacks_for_investigator` + continuation

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`resolve_attacks_for_investigator`, ~line 281)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`run_window_continuation`, the `AfterEnemyAttackDamagedAsset` arm + the `BeforeInvestigatorAttacked` arm at ~line 586)

This is the suspend/resume `combat.rs:265` predicted. The key change: the loop persists its remaining attackers and returns `EngineOutcome::AwaitingInput` when a soak window opens; the enemy-phase continuation propagates it; on window close, `resume_enemy_attack` re-enters the loop.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn enemy_phase_suspends_on_guard_dog_and_resumes_remaining_attackers() {
    // Investigator engaged with two attackers, controlling Guard Dog.
    // First attacker soaks onto Guard Dog → AwaitingInput (soak window).
    // Resolve the window (Skip) → loop resumes, second attacker resolves.
    // Assert both attackers exhausted at the end.
}
```

(Integration-level — likely belongs in `crates/cards/tests/` with a real registry. If so, write it there and mark this lib test as a smaller suspension-state unit check, e.g. asserting `pending_enemy_attack` is set on suspend.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core enemy_phase_suspends_on_guard_dog`
Expected: FAIL.

- [ ] **Step 3: Make the loop return `EngineOutcome` and persist remaining attackers**

Change `resolve_attacks_for_investigator` to:

```rust
pub(super) fn resolve_attacks_for_investigator(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    let attackers: Vec<EnemyId> = /* unchanged snapshot */;
    drive_attack_loop(cx, investigator, attackers, EnemyAttackSource::EnemyPhase)
}

/// Shared loop body: resolve each attacker, suspending if an attack
/// opens a reaction window (parking the rest in `pending_enemy_attack`).
fn drive_attack_loop(
    cx: &mut Cx,
    investigator: InvestigatorId,
    mut attackers: Vec<EnemyId>,
    source: EnemyAttackSource,
) -> EngineOutcome {
    while let Some(enemy_id) = (!attackers.is_empty()).then(|| attackers.remove(0)) {
        let active = cx.state.investigators.get(&investigator)
            .is_some_and(|inv| inv.status == Status::Active);
        if !active { break; }

        enemy_attack(cx, enemy_id, investigator);

        // Exhaust the attacker post-resolution (unchanged event/flag).
        if let Some(enemy) = cx.state.enemies.get_mut(&enemy_id) {
            if !enemy.exhausted {
                enemy.exhausted = true;
                cx.events.push(Event::EnemyExhausted { enemy: enemy_id });
            }
        }

        // If the attack opened a soak window, suspend: park the rest and
        // return AwaitingInput for the queued window.
        if !cx.state.open_windows.is_empty() {
            cx.state.pending_enemy_attack = Some(PendingEnemyAttack {
                investigator,
                remaining_attackers: attackers,
                source,
            });
            return super::reaction_windows::open_queued_reaction_window(cx);
        }
    }
    EngineOutcome::Done
}
```

Preserve the exact exhaust event the current code emits — copy its shape from the existing loop (it may capture defeated-target details). Verify the early-break-on-defeat semantics match the original doc comment.

- [ ] **Step 4: Add `resume_enemy_attack` + wire the continuation**

In `combat.rs`:

```rust
/// Re-enter a suspended enemy-attack loop after its soak reaction window
/// closed. Mirror of `resume_end_turn` / `resume_spawn_engage`.
pub(super) fn resume_enemy_attack(cx: &mut Cx) -> EngineOutcome {
    let pending = cx.state.pending_enemy_attack.take().unwrap_or_else(|| {
        unreachable!("resume_enemy_attack: no pending_enemy_attack")
    });
    let outcome = drive_attack_loop(
        cx, pending.investigator, pending.remaining_attackers, pending.source,
    );
    if !matches!(outcome, EngineOutcome::Done) {
        return outcome; // suspended again on a later attacker
    }
    // Loop finished. For the enemy phase, advance the cursor + open the
    // next window (the logic moved out of the BeforeInvestigatorAttacked
    // continuation); for AoO, hand back to the move/action driver.
    match pending.source {
        EnemyAttackSource::EnemyPhase => super::reaction_windows::after_enemy_phase_attacks(cx, pending.investigator),
        EnemyAttackSource::AttackOfOpportunity => EngineOutcome::Done,
    }
}
```

In `reaction_windows.rs`:
- Replace the temporary `AfterEnemyAttackDamagedAsset { .. } => EngineOutcome::Done` arm in `run_window_continuation` with `=> super::combat::resume_enemy_attack(cx),`.
- Extract the cursor-advance + next-window logic from the `BeforeInvestigatorAttacked` arm (lines 621–641) into a shared `pub(super) fn after_enemy_phase_attacks(cx, investigator) -> EngineOutcome` that runs the `next_active_investigator_after` + `open_fast_window` block. Call it from both the `BeforeInvestigatorAttacked` arm (after `resolve_attacks_for_investigator` returns `Done`) **and** `resume_enemy_attack`. In the `BeforeInvestigatorAttacked` arm, if `resolve_attacks_for_investigator` returns `AwaitingInput`, return it immediately **without** advancing the cursor (the cursor advances later via `resume_enemy_attack` → `after_enemy_phase_attacks`).

- [ ] **Step 5: Run + commit**

Run: `cargo test -p game-core --lib`
Expected: PASS (existing enemy-phase tests still green; suspension-state unit check passes).

```bash
git add -A
git commit -m "game-core: resumable enemy-phase attack loop on soak window (C5b)"
```

---

## Task 11: Guard Dog card impl + native retaliate

**Files:**
- Create: `crates/cards/src/impls/guard_dog.rs`
- Modify: `crates/cards/src/impls/mod.rs` (register the module)
- Modify: the `cards` native-effect registry function (grep `native_effect_for` / the `NATIVE_EFFECTS` map in `crates/cards/src/`)

- [ ] **Step 1: Write the card + native + failing card test**

```rust
//! Guard Dog (01021) — Guardian ally. "[reaction] When an enemy attack
//! deals damage to Guard Dog: Deal 1 damage to the attacking enemy."

use game_core::card_registry::Cx;
use game_core::dsl::{on_event, native, Ability, EventPattern, EventTiming};
use game_core::engine::evaluator::EvalContext; // adjust to real export path
use game_core::engine::EngineOutcome;

pub const CODE: &str = "01021";

pub fn abilities() -> Vec<Ability> {
    vec![on_event(
        EventPattern::EnemyAttackDamagedSelf,
        EventTiming::After,
        native("01021:retaliate"),
    )]
}

/// "Deal 1 damage to the attacking enemy." The attacker is bound on
/// `EvalContext.attacking_enemy` by the soak reaction window.
pub fn retaliate(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let enemy = ctx.attacking_enemy.expect(
        "01021:retaliate fired without attacking_enemy bound — only the \
         AfterEnemyAttackDamagedAsset window fires this ability",
    );
    game_core::engine::deal_damage_to_enemy(cx, enemy, 1, Some(ctx.controller));
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    // Card test: an enemy attack soaks onto Guard Dog, the reaction deals
    // 1 damage to the attacker. Use the cards-crate test harness that
    // installs REGISTRY (see other impls' tests).
}
```

Resolve the real public paths: `Cx` and the enemy-damage helper need to be reachable from `cards`. `combat::damage_enemy` is `pub(super)` — **expose a public engine entry point** `game_core::engine::deal_damage_to_enemy(cx, enemy, amount, by)` (thin pub wrapper over `combat::damage_enemy`) for native effects, mirroring how other natives reach engine internals (grep how Crypt Chill's / Ancient Evils' natives call in). Add that wrapper in `game-core` as its own tiny step if it doesn't exist.

- [ ] **Step 2: Register the native + module**

In `crates/cards/src/impls/mod.rs` add `pub mod guard_dog;`. In the native-effect registry, map `"01021:retaliate" => guard_dog::retaliate`. In the abilities/metadata registry, ensure `01021` routes to `guard_dog::abilities()` (grep how other impls are wired into `REGISTRY` — likely a generated match the impls plug into).

- [ ] **Step 3: Run the card test**

Run: `cargo test -p cards guard_dog`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "cards: Guard Dog 01021 reaction + native retaliate (C5b)"
```

---

## Task 12: End-to-end integration test

**Files:**
- Create: `crates/cards/tests/guard_dog_soak.rs`

- [ ] **Step 1: Write the integration test**

```rust
//! End-to-end: an enemy attacks an investigator controlling Guard Dog;
//! damage soaks onto Guard Dog; the reaction window fires; the attacker
//! takes 1 damage.

// install(cards::REGISTRY); build a game with Guard Dog in play and an
// engaged enemy whose attack deals 1 damage; drive the enemy phase (or an
// AoO); resolve the soak reaction window by firing Guard Dog's trigger;
// assert Event::EnemyDamaged { enemy, amount: 1, .. } and that Guard Dog
// has accumulated_damage 1.
```

Follow `crates/cards/tests/play_card.rs` for the harness + `install` pattern.

- [ ] **Step 2: Run**

Run: `cargo test -p cards --test guard_dog_soak`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/cards/tests/guard_dog_soak.rs
git commit -m "cards: end-to-end Guard Dog soak + retaliate integration test (C5b)"
```

---

## Task 13: Attacks-of-opportunity path (separable)

> If the PR is bloating, this task can split to a fast-follow; the enemy-phase path (Tasks 1–12) is independently shippable. Plan both per the spec.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`fire_attacks_of_opportunity`, ~line 218)
- Modify: `crates/game-core/src/engine/dispatch/actions.rs` (the two call sites, lines 112, 244)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn aoo_suspends_on_guard_dog_and_resumes() {
    // Investigator with Guard Dog moves away from an engaged enemy →
    // AoO soaks onto Guard Dog → AwaitingInput → resolve → action completes.
}
```

- [ ] **Step 2: Make `fire_attacks_of_opportunity` return `EngineOutcome`**

Route it through `drive_attack_loop(cx, investigator, attackers, EnemyAttackSource::AttackOfOpportunity)`. The two `actions.rs` call sites must propagate `AwaitingInput`: if the returned outcome is `AwaitingInput`, the surrounding action handler returns it (parking is already in `pending_enemy_attack`; `resume_enemy_attack`'s `AttackOfOpportunity` arm returns `Done`, letting the action's *remaining* work… )

**Open sub-decision for the implementer:** the AoO fires mid-action (before the action's own effect completes, e.g. the move). Suspending there means the move's completion must also resume. If that second continuation is non-trivial, **scope AoO to a fast-follow** and land Tasks 1–12 first. Confirm with the reviewer before expanding `actions.rs`. Document whichever path is taken.

- [ ] **Step 3: Run + commit**

Run: `cargo test -p game-core aoo_suspends_on_guard_dog_and_resumes`
Expected: PASS.

```bash
git add -A
git commit -m "game-core: resumable attacks-of-opportunity on soak window (C5b)"
```

---

## Task 14: Full gauntlet + push + PR

- [ ] **Step 1: Run the full CI gauntlet**

```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all green.

- [ ] **Step 2: Push the branch and open the PR**

```bash
git push -u origin engine/guard-dog-soak
gh pr create --fill
```
PR body: design-decisions paragraph (soak-first deterministic stand-in, symmetric horror, Native retaliate, resumable attack loop), `Closes #237.`

- [ ] **Step 3: Watch CI**

```bash
gh pr checks <PR#> --watch
```

---

## Task 15: Phase doc + #44 reframe (final commit, on merge-readiness)

- [ ] **Step 1: Update `docs/phases/phase-7-the-gathering.md`**

Move C5b (#237) to the Closed table / flip its Group-C row to `✅ PR #<n>`; add the spec's "Decisions made" entries (soak-first deterministic assignment, symmetric horror, Native retaliate, resumable attack loop). Update the Status line's "Shipped:" list and "Next:" pointer (C5c → C7).

- [ ] **Step 2: Reframe #44**

```bash
gh issue edit 44 --body "<updated: core soak mechanic shipped in C5b PR #<n>; \
remaining scope = interactive damage/horror distribution choice \
(replace the soak-first deterministic assign_attack with a parked \
window surfacing eligible soakers + a player-chosen {target → points} \
InputResponse variant).>"
```

- [ ] **Step 3: Commit the phase doc (after CI is green)**

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: phase-7 — C5b shipped (enemy-attack soak + Guard Dog)"
git push
```

---

## Self-review notes

- **Spec coverage:** assignment (T5), placement+defeat (T6), enemy_attack rewrite (T7), window+pattern+EvalContext (T1–T3, T8–T9), resumable loop (T4, T10, T13), Guard Dog impl (T11), tests (T6/T7/T10/T11/T12/T13), #44 reframe (T15). All spec sections mapped.
- **Type consistency:** `Assignment` / `Soaker` (T5) reused in T6–T7; `PendingEnemyAttack` / `EnemyAttackSource` (T4) reused in T10/T13; `attacking_enemy` (T3) read in T9/T11; `drive_attack_loop` / `after_enemy_phase_attacks` / `resume_enemy_attack` named consistently across T10/T13.
- **Known soft spots flagged for the implementer:** (a) whether `assign_attack`/placement unit tests live in `game-core` (no registry) vs `cards` (real metadata) — resolved by the pure-function form in T5; (b) the AoO mid-action resume (T13) may exceed scope — explicit fast-follow off-ramp; (c) exact `Event::CardDiscarded`/`Zone` variant shapes must be copied from existing code, not guessed.
