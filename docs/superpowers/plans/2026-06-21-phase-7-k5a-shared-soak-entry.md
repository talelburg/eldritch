# K5a — Shared Soak Entry (non-attack damage/horror soaks) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route non-attack damage/horror (treachery/card effects) through the same soak pipeline as enemy attacks, so a controlled soaker asset (Guard Dog, Beat Cop) absorbs treachery harm per RR p.7 — closing the gap where `take_damage`/`take_horror` apply straight to the investigator.

**Architecture:** Extract the existing `enemy_attack` body (`build_soakers → assign_attack → place_assignment`) into a shared `soak_and_place(cx, investigator, damage, horror) -> Vec<survivors>`, then reroute `enemy_attack`, `take_damage`, and `take_horror` through it. K5a keeps the soak-first deterministic default (no player choice yet — that's K5b), so it is non-suspending and behaviour-additive: attacks are byte-identical, non-attack harm now soaks, and the no-soaker case is unchanged.

**Tech Stack:** Rust, `game-core` engine crate + `cards` integration tests. No new dependencies. Engine-only.

## Global Constraints

- Match CI's strict flags before declaring a task done: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`, plus `cargo build -p web --target wasm32-unknown-unknown` and `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- Validate-first / mutate-second handler contract; behaviour-additive only (no behaviour change for attacks or the no-soaker case).
- RR p.7 (verified, in the design spec): damage/horror is assigned, placed simultaneously, then defeat is checked. `place_assignment` already encodes this; K5a does not change it.
- Soak eligibility needs the card registry (printed health/sanity). Without a registry `build_soakers` returns empty, so soak behaviour is only observable in registry-backed `crates/cards/tests/` integration tests — lib-level tests only confirm the routing is behaviour-preserving.
- Commit subjects: `scope: description` (scope = `engine`). Feature branch `engine/soak-distribution` (already created; the design doc is already committed there).
- This plan is **K5a only.** K5b (interactive per-point distribution) gets its own plan after K5a merges — its attack-path restructure is planned against the `soak_and_place` seam this PR creates.

---

### Task 1: Extract `soak_and_place` and reroute all three callers

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`enemy_attack` ~lines 471-493; add `soak_and_place`)
- Modify: `crates/game-core/src/engine/dispatch/elimination.rs` (`take_horror` ~lines 184-188, `take_damage` ~lines 198-202)
- Test: existing `game-core` lib suite (behaviour-preserving guard — no new lib test; the additive soak behaviour is proven by Task 2's integration tests, which need a registry)

**Interfaces:**
- Produces: `pub(super) fn soak_and_place(cx: &mut Cx, investigator: InvestigatorId, damage: u8, horror: u8) -> Vec<CardInstanceId>` — runs `build_soakers → assign_attack → place_assignment`; returns the damaged surviving soakers (for attack callers to queue windows). Consumed by `enemy_attack` (this task) and `take_damage`/`take_horror` (this task), and by K5b's interactive replacement later.

- [ ] **Step 1: Add `soak_and_place` and make `enemy_attack` a thin wrapper**

In `combat.rs`, replace the body of `enemy_attack` (the soak-first block at ~lines 485-492) and add the shared helper just above `enemy_attack`:

```rust
/// Distribute `damage` + `horror` to `investigator` across eligible soakers
/// then self (soak-first, RR p.7), place simultaneously, and defeat overflowed
/// assets — the shared soak entry for **both** enemy attacks and non-attack
/// card/treachery harm (#44/K5a). Returns the damaged surviving soaker assets
/// (the [`place_assignment`] survivor list) so an attack caller can queue one
/// [`WindowKind::AfterEnemyAttackDamagedAsset`] reaction window per survivor;
/// non-attack callers pass one of `damage`/`horror` as 0 and ignore the return
/// (treachery harm opens no soak reaction window — Guard Dog 01021 retaliates
/// only to enemy *attacks*). `build_soakers` returns empty with no registry or
/// no soak-bearing asset, so the assignment then drops everything on the
/// investigator — behaviour-identical to the pre-soak direct-apply path.
pub(super) fn soak_and_place(
    cx: &mut Cx,
    investigator: InvestigatorId,
    damage: u8,
    horror: u8,
) -> Vec<CardInstanceId> {
    let soakers = build_soakers(cx.state, investigator);
    let assignment = assign_attack(&soakers, damage, horror);
    place_assignment(cx, investigator, &assignment)
}
```

Then in `enemy_attack`, replace the trailing soak-first block:

```rust
    let damage = enemy.attack_damage;
    let horror = enemy.attack_horror;

    // Soak-first assignment → simultaneous placement → defeat check
    // (RR p.7; C5b #237). `build_soakers` returns empty when no registry
    // is installed or the investigator controls no soak-bearing assets,
    // so the assignment drops all damage/horror on the investigator —
    // behavior-identical to the pre-soak direct-apply path.
    let soakers = build_soakers(cx.state, investigator);
    let assignment = assign_attack(&soakers, damage, horror);
    place_assignment(cx, investigator, &assignment)
```

with:

```rust
    let damage = enemy.attack_damage;
    let horror = enemy.attack_horror;
    soak_and_place(cx, investigator, damage, horror)
```

- [ ] **Step 2: Run the combat suite to confirm the attack path is unchanged**

Run: `cargo test -p game-core --lib engine::dispatch::combat`
Expected: PASS — `enemy_attack` / soak / Guard Dog tests green (pure extraction).

- [ ] **Step 3: Reroute `take_damage` and `take_horror` through `soak_and_place`**

In `elimination.rs`, replace the two wrapper bodies. `take_horror` (~lines 184-188):

```rust
pub(crate) fn take_horror(cx: &mut Cx, investigator: InvestigatorId, amount: u8) {
    // Route through the shared soak entry (#44/K5a) so a controlled sanity-bearing
    // asset (Beat Cop, Holy Rosary) absorbs non-attack horror; `place_assignment`
    // applies investigator defeat (cause Horror) when the investigator's share is
    // lethal, preserving this wrapper's prior behaviour. No soak window (Effect
    // source, not an enemy attack), so the survivor list is dropped.
    let _ = super::combat::soak_and_place(cx, investigator, 0, amount);
}
```

`take_damage` (~lines 198-202):

```rust
pub fn take_damage(cx: &mut Cx, investigator: InvestigatorId, amount: u8) {
    // Route through the shared soak entry (#44/K5a) so a controlled health-bearing
    // asset (Guard Dog, Beat Cop) absorbs non-attack damage; `place_assignment`
    // applies investigator defeat (cause Damage) when the investigator's share is
    // lethal, preserving this wrapper's prior behaviour. No soak window.
    let _ = super::combat::soak_and_place(cx, investigator, amount, 0);
}
```

(Leave the existing doc-comments above each fn; update only the bodies + the inline comment. The doc-comment reference to "compose the lower-level `apply_damage_numeric` + … triple" is still accurate for the *attack* case it describes, so no edit needed there.)

- [ ] **Step 4: Run the full game-core lib suite (behaviour-preserving without a registry)**

Run: `cargo test -p game-core --lib`
Expected: PASS — every lib test stays green. Without a registry `build_soakers` is empty, so `take_damage`/`take_horror` still drop all harm on the investigator with identical events (`DamageTaken`/`HorrorTaken` + defeat-if-lethal), so the elimination/defeat tests don't change.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs crates/game-core/src/engine/dispatch/elimination.rs
git commit -m "engine: route non-attack damage/horror through the shared soak entry (K5a of #44)"
```

---

### Task 2: Integration tests — non-attack harm soaks (registry-backed, via `apply`)

**Files:**
- Create: `crates/cards/tests/non_attack_soak.rs`

**Interfaces:**
- Consumes: nothing new — drives real Gathering treacheries (Grasping Hands 01162 damage, Rotting Remains 01163 horror) through the public `apply` revelation path, exercising `take_damage`/`take_horror` → `soak_and_place` end-to-end.

Soak is only observable with the registry installed, and integration tests reach the engine **only through `apply`** (there is no public `Cx` constructor). So the proof is a real treachery revealed against an investigator who controls a soaker. Model the file on `crates/cards/tests/revelation_treacheries.rs` (its `install_registry` / `reveal_top` / `board_with` helpers, copied — each test file is its own binary). Verified facts: `test_investigator` has all skills = 3, max_health 8, max_sanity 8; Guard Dog 01021 = health 3 / sanity 1; Beat Cop 01018 = health 2 / sanity 2; Grasping Hands 01162 tests Agility 3 (1 damage per point failed); Rotting Remains 01163 tests Willpower 3 (1 horror per point failed).

- [ ] **Step 1: Write the test file scaffold + the damage-soak test**

Create `crates/cards/tests/non_attack_soak.rs`:

```rust
//! K5a (#44): non-attack damage/horror from card/treachery effects soaks onto
//! controlled assets via the shared soak entry, like enemy attacks already did.
//! Driven through the real `apply` revelation path against the corpus registry.

use std::sync::Once;

use game_core::action::EngineRecord;
use game_core::state::{CardCode, CardInPlay, CardInstanceId, ChaosToken, InvestigatorId, LocationId};
use game_core::test_support::{drive, test_investigator, test_location, GameStateBuilder, ScriptedResolver};
use game_core::{Action, EngineOutcome};

static INSTALL: Once = Once::new();
fn install_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Reveal the top encounter card for investigator 1, committing no cards at the
/// revelation skill-test window.
fn reveal_top(state: game_core::GameState) -> game_core::ApplyResult {
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed { investigator: InvestigatorId(1) }),
        resolver,
    )
}

/// Investigator 1 at a location, controlling `soaker` (instance 1), with
/// `treachery` on top of the encounter deck and one rigged chaos token.
fn board_with_soaker(treachery: &str, soaker: &str, token: ChaosToken) -> game_core::GameState {
    let mut inv = test_investigator(1);
    inv.cards_in_play = vec![CardInPlay::enter_play(CardCode::new(soaker), CardInstanceId(1))];
    let mut state = GameStateBuilder::new()
        .with_investigator_at(inv, LocationId(20))
        .with_location(test_location(20, "Here"))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.chaos_bag.tokens = vec![token];
    state.encounter_deck.push_back(CardCode::new(treachery));
    state
}

#[test]
fn grasping_hands_damage_soaks_onto_guard_dog() {
    install_registry();
    // Agility 3 + Numeric(-2) = 1 vs difficulty 3 → fail by 2 → 2 damage.
    // Guard Dog (health 3) soaks both and survives; no soak reaction window
    // (Effect source — Guard Dog retaliates only to enemy *attacks*).
    let result = reveal_top(board_with_soaker("01162", "01021", ChaosToken::Numeric(-2)));
    assert_eq!(result.outcome, EngineOutcome::Done, "no soak reaction window for treachery harm");
    let inv = &result.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.damage, 0, "damage soaked, investigator took none");
    let dog = inv.cards_in_play.iter().find(|c| c.instance_id == CardInstanceId(1));
    assert_eq!(dog.map(|c| c.accumulated_damage), Some(2), "2 damage soaked onto Guard Dog");
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p cards --test non_attack_soak grasping_hands_damage_soaks_onto_guard_dog`
Expected: PASS (Task 1's reroute makes treachery damage soak). If it FAILS with `inv.damage == 2`, Task 1's `take_damage` reroute is incomplete.

- [ ] **Step 3: Add the horror-soak test**

```rust
#[test]
fn rotting_remains_horror_soaks_onto_beat_cop() {
    install_registry();
    // Willpower 3 + Numeric(-1) = 2 vs difficulty 3 → fail by 1 → 1 horror.
    // Beat Cop (sanity 2) soaks it and survives (accumulated 1 < 2).
    let result = reveal_top(board_with_soaker("01163", "01018", ChaosToken::Numeric(-1)));
    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = &result.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.horror, 0, "horror soaked, investigator took none");
    let cop = inv.cards_in_play.iter().find(|c| c.instance_id == CardInstanceId(1));
    assert_eq!(cop.map(|c| c.accumulated_horror), Some(1), "1 horror soaked onto Beat Cop");
}
```

- [ ] **Step 4: Run both**

Run: `cargo test -p cards --test non_attack_soak`
Expected: PASS. (If a margin assertion is off, re-derive it from the verified stats above — all skills 3, difficulties 3 — and the chosen token; do not change the soaker without rechecking it survives the dealt harm.)

- [ ] **Step 5: Commit**

```bash
git add crates/cards/tests/non_attack_soak.rs
git commit -m "test: non-attack treachery damage/horror soaks onto controlled assets (K5a of #44)"
```

---

### Task 3: Full gauntlet

**Files:** none (verification).

- [ ] **Step 1: Run the full native CI gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green. Pay attention to any **registry-backed** existing test that deals `take_damage`/`take_horror` to an investigator who *also* controls a soaker — its expected investigator damage/horror would now (correctly) be soaked. If one fails for that reason, update it to assert the soaked-onto-asset placement (and note it in the commit). The most likely candidates are self-damage card tests (e.g. Crypt Chill, Dynamite Blast) — confirm whether their scenarios put a soaker in play.

- [ ] **Step 2: Commit any test adjustments**

```bash
git add -A
git commit -m "test: adjust registry-backed harm tests for non-attack soak (K5a of #44)"
```

(Skip if the gauntlet was clean.)

---

## Post-implementation (PR procedure, not TDD tasks)

- Pre-push review pass (superpowers final-reviewer) before pushing, per the project workflow.
- Push `engine/soak-distribution`, open the PR (template; do **not** write `Closes #44` — #44 stays open for K5b; reference it as "K5a of #44"). Design-decisions paragraph: shared `soak_and_place` entry; non-attack harm now soaks via the soak-first default; multi-window drain deferred (unconstructible — single Ally-slot reactor).
- Watch CI.
- **Only after CI is green**, update `docs/phases/phase-7-the-gathering.md`: note K5a shipped (the non-attack-soak half of #44) and that interactive distribution (K5b) + the deferred multi-window drain remain. Do not close #44.
- Merge only after explicit user approval. K5b is the next plan.

## Self-review notes

- **Spec coverage (K5a portion):** shared `soak_and_place` entry (Task 1) ✓; non-attack damage soaks (Task 2) ✓; non-attack horror soaks (Task 2) ✓; defeat behaviour preserved via `place_assignment` (Task 1, lib suite green) ✓; attack path byte-identical (Task 1 Step 2) ✓; multi-window drain untouched/deferred (no task — `park_on_soak_window` guard unchanged) ✓. K5b (interactive distribution) is explicitly out of this plan.
- **Type consistency:** `soak_and_place(cx, investigator, damage, horror) -> Vec<CardInstanceId>` is used identically by `enemy_attack`, `take_damage`, `take_horror`. `build_soakers`/`assign_attack`/`place_assignment` signatures unchanged.
- **Resolved during planning:** integration tests have no public `Cx`; they reach the engine only via `apply`, so Task 2 proves soak through real treacheries (Grasping Hands / Rotting Remains) on the revelation path, modelled on `revelation_treacheries.rs`. Margins are derived from verified stats (all skills 3, difficulties 3) and soaker health/sanity (Guard Dog 3/1, Beat Cop 2/2).
- **Open risk flagged inline:** a registry-backed existing test that deals `take_*` harm to an investigator who also controls a soaker would now (correctly) soak — Task 3 Step 1 watches for it; likely none (Crypt Chill's no-asset branch has no soaker; its with-asset branch discards rather than soaks).
