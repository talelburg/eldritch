# C3a — Prey variants + Retaliate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add two engine primitives The Gathering's encounter enemies need — the `Prey::LowestRemainingHealth` instruction (Ravenous Ghoul 01161) and the Retaliate keyword (Ghoul Priest 01116).

**Architecture:** `Prey::LowestRemainingHealth` is a new variant on the existing `#[non_exhaustive]` `Prey` enum, wired into `resolve_prey` as a min-over-remaining-health branch mirroring the existing `HighestStat` max branch. Retaliate adds a `retaliate: bool` flag to the `Enemy` state struct and a new `FinishContinuation::PostRetaliate` step in the skill-test driver that fires an enemy attack after a failed Fight test (after ST.7's OnSkillTestResolution triggers, before ST.8 teardown — RR p.26 "after applying all results"). C3a is engine-only: `spawn_enemy` keeps its hardcoded defaults; C3b ([#231](https://github.com/talelburg/eldritch/issues/231)) populates the keywords onto real enemies.

**Tech Stack:** Rust, the `game-core` kernel crate + the `card-dsl` data crate. No async, no I/O. Tests via `cargo test` with `RUSTFLAGS="-D warnings"`.

**Spec:** `docs/superpowers/specs/2026-06-13-phase-7-c3a-prey-retaliate-design.md`

---

## Background the implementer needs

- **`resolve_prey`** (`crates/game-core/src/engine/dispatch/hunters.rs:31`) narrows a candidate investigator set by a `Prey` instruction, returning `PreyResolution::One` / `Tie` / `None`. The `HighestStat` branch (lines 41-65) is the template: compute the max of a per-investigator value, then keep every candidate matching it.
- **`Prey`** enum lives in `crates/card-dsl/src/card_data.rs:178`, re-exported as `crate::card_data::Prey` inside `game-core`. It is `#[non_exhaustive]`; `resolve_prey` has a catch-all `_ => unreachable!(...)` arm (lines 66-72) — a new variant **must** be wired before that arm or it panics at runtime.
- **`Enemy`** state struct: `crates/game-core/src/state/enemy.rs:29`. The runtime enemy (distinct from `CardKind::Enemy` *metadata*). Three literal-construction sites exist in-crate and all must gain the new field: `test_enemy` fixture (`crates/game-core/src/test_support/fixtures.rs:97`), the `enemy_carries_hunter_and_prey` test (`crates/game-core/src/state/enemy.rs:86`), and `spawn_enemy` (`crates/game-core/src/engine/dispatch/encounter.rs:309`).
- **Investigator** state (`crates/game-core/src/state/investigator.rs`) has `max_health: u8` and `damage: u8`. Remaining health = `max_health − damage` (saturating).
- **Skill-test driver**: `finish_skill_test` (`crates/game-core/src/engine/dispatch/skill_test.rs:132`) resolves the chaos token, runs the success-only `apply_skill_test_follow_up`, then sets continuation `PostFollowUp { succeeded }` and calls `drive_skill_test` (line 220). `drive_skill_test`'s loop maps: `PostFollowUp` → `fire_on_skill_test_resolution` then advance; `PostOnResolution` → discard committed cards + `SkillTestEnded` + clear in-flight, return `Done`. `FinishContinuation` is defined at `crates/game-core/src/state/game_state.rs:423` (`#[non_exhaustive]`, `Copy`, serde).
- **`SkillTestFollowUp`** (`crates/game-core/src/state/game_state.rs:456`): `None | Investigate | Fight { enemy } | Evade { enemy }`. The in-flight record carries `follow_up` (`game_state.rs:357`); it persists until teardown, so later driver steps can re-read it.
- **`enemy_attack`** (`crates/game-core/src/engine/dispatch/combat.rs:175`, `pub(super) fn enemy_attack(cx, enemy_id, investigator)`) places the enemy's `attack_damage` + `attack_horror` on the investigator (simultaneously per RR p.7) and handles investigator defeat. It does **not** exhaust the attacker — exactly the Retaliate no-exhaust clause. Call it as `super::combat::enemy_attack(...)` from `skill_test.rs`.
- **Test helpers** (in `crates/game-core/src/engine/mod.rs` test module): `fight_evade_scenario()` (`:1814`) returns `(InvestigatorId(1), EnemyId(100), GameState)` with the enemy engaged, `fight=evade=3`, `max_health=2`, chaos bag `bag_only_zero()` (always draws `Numeric(0)`), phase Investigation, active investigator set. `apply_no_commits(state, action)` (`crates/game-core/src/test_support/resolver.rs:238`) drives a skill-test-initiating action through the commit window with zero commits and aggregates all events into `ApplyResult { state, events, outcome }`. `test_investigator` defaults: all skills 3, `max_health` 8, `max_sanity` 8, `damage`/`horror` 0.

### CI gauntlet (run before declaring any task done)

```sh
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
RUSTFLAGS="-D warnings" cargo test -p card-dsl --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

Single test: `cargo test -p game-core <test_fn_name>`.

---

## Task 1: `Prey::LowestRemainingHealth`

**Files:**
- Modify: `crates/card-dsl/src/card_data.rs:178` (add enum variant)
- Modify: `crates/game-core/src/engine/dispatch/hunters.rs:39` (add `resolve_prey` branch)
- Test: `crates/game-core/src/engine/dispatch/hunters.rs` (`resolve_prey_tests` module, ~line 451)

- [ ] **Step 1: Write the failing tests**

Add to the `resolve_prey_tests` module in `crates/game-core/src/engine/dispatch/hunters.rs` (alongside the existing `resolve_prey_highest_stat_*` tests):

```rust
    #[test]
    fn resolve_prey_lowest_remaining_health_picks_min() {
        // inv1: max_health 5, damage 4 → remaining 1.
        // inv2: max_health 5, damage 0 → remaining 5. inv1 is lowest.
        let mut hurt = test_investigator(1);
        hurt.max_health = 5;
        hurt.damage = 4;
        let mut healthy = test_investigator(2);
        healthy.max_health = 5;
        healthy.damage = 0;
        let state = GameStateBuilder::new()
            .with_investigator(hurt)
            .with_investigator(healthy)
            .build();
        let r = resolve_prey(
            &state,
            crate::card_data::Prey::LowestRemainingHealth,
            &[InvestigatorId(1), InvestigatorId(2)],
        );
        assert!(matches!(r, PreyResolution::One(id) if id == InvestigatorId(1)));
    }

    #[test]
    fn resolve_prey_lowest_remaining_health_tie_is_tie() {
        // inv1: 5 − 2 = 3 remaining. inv2: 4 − 1 = 3 remaining. Tie.
        let mut a = test_investigator(1);
        a.max_health = 5;
        a.damage = 2;
        let mut b = test_investigator(2);
        b.max_health = 4;
        b.damage = 1;
        let state = GameStateBuilder::new()
            .with_investigator(a)
            .with_investigator(b)
            .build();
        let r = resolve_prey(
            &state,
            crate::card_data::Prey::LowestRemainingHealth,
            &[InvestigatorId(1), InvestigatorId(2)],
        );
        assert!(matches!(r, PreyResolution::Tie(ref v) if v.len() == 2));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p game-core resolve_prey_lowest_remaining_health 2>&1 | tail -20`
Expected: compile error — `no variant named LowestRemainingHealth found for enum ... Prey`.

- [ ] **Step 3a: Add the enum variant**

In `crates/card-dsl/src/card_data.rs`, inside the `Prey` enum (after the `HighestStat(Stat)` variant, before the closing brace at ~line 186):

```rust
    /// Pursue / engage the investigator with the lowest remaining
    /// health (base health − damage; Rules Reference p.12). Ties fall to
    /// the lead investigator. Ravenous Ghoul (`01161`).
    // TODO: generalize to a `{ measure, direction }` shape (covering
    // stats *and* derived measures) when a 2nd derived-measure prey
    // lands — Lowest remaining sanity, Most clues, Fewest cards in hand.
    LowestRemainingHealth,
```

- [ ] **Step 3b: Wire the `resolve_prey` branch**

In `crates/game-core/src/engine/dispatch/hunters.rs`, inside `resolve_prey`'s `match prey` (after the `Prey::HighestStat(stat) => { ... }` arm at line 65, before the `_ => unreachable!` catch-all at line 66):

```rust
        Prey::LowestRemainingHealth => {
            let remaining = |id: &InvestigatorId| -> Option<u8> {
                state
                    .investigators
                    .get(id)
                    .map(|inv| inv.max_health.saturating_sub(inv.damage))
            };
            let min = candidates.iter().filter_map(remaining).min();
            match min {
                Some(m) => candidates
                    .iter()
                    .copied()
                    .filter(|id| remaining(id) == Some(m))
                    .collect(),
                None => Vec::new(),
            }
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p game-core resolve_prey 2>&1 | tail -20`
Expected: PASS (all `resolve_prey_*` tests, including the two new ones).

- [ ] **Step 5: Run the gauntlet + commit**

```sh
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
RUSTFLAGS="-D warnings" cargo test -p card-dsl --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

```bash
git add crates/card-dsl/src/card_data.rs crates/game-core/src/engine/dispatch/hunters.rs
git commit -m "engine: Prey::LowestRemainingHealth (Ravenous Ghoul 01161)

Adds the lowest-remaining-health prey instruction (RR p.12) as a specific
Prey variant, wired into resolve_prey as a min-over-(max_health-damage)
branch mirroring HighestStat. Generalize to a measure/direction shape on
the 2nd derived-measure consumer (TODO noted in the enum).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: `Enemy.retaliate` field

Pure state plumbing — adds the flag and fills it on every `Enemy` literal. No behavior yet (Task 3 consumes it). `spawn_enemy` keeps `retaliate: false` (C3b populates it from card data).

**Files:**
- Modify: `crates/game-core/src/state/enemy.rs:77` (field) and `:86` (test literal + assertion)
- Modify: `crates/game-core/src/test_support/fixtures.rs:97` (`test_enemy` literal)
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs:309` (`spawn_enemy` literal)

- [ ] **Step 1: Write the failing test**

In `crates/game-core/src/state/enemy.rs`, edit the existing `enemy_carries_hunter_and_prey` test (line 85): add `retaliate: true,` to the `Enemy { ... }` literal (after the `prey:` field) and add an assertion at the end of the test body:

```rust
        assert!(e.retaliate);
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core enemy_carries_hunter_and_prey 2>&1 | tail -20`
Expected: compile error — `struct Enemy has no field named retaliate`.

- [ ] **Step 3a: Add the struct field**

In `crates/game-core/src/state/enemy.rs`, add to the `Enemy` struct after the `prey` field (line 77):

```rust
    /// Whether this enemy has the Retaliate keyword (Rules Reference
    /// p.18): after an investigator fails a Fight test against this
    /// enemy while it is ready, it performs an attack against that
    /// investigator (without exhausting). `false` for enemies with no
    /// printed Retaliate line.
    pub retaliate: bool,
```

- [ ] **Step 3b: Fill the two remaining literals**

In `crates/game-core/src/test_support/fixtures.rs`, in `test_enemy` (line 97), add after `prey: Prey::Default,`:

```rust
        retaliate: false,
```

In `crates/game-core/src/engine/dispatch/encounter.rs`, in `spawn_enemy`'s `Enemy { ... }` (line 309), add after `prey: crate::card_data::Prey::Default,`:

```rust
        retaliate: false,
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p game-core enemy_carries_hunter_and_prey 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Run the gauntlet + commit**

```sh
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

```bash
git add crates/game-core/src/state/enemy.rs crates/game-core/src/test_support/fixtures.rs crates/game-core/src/engine/dispatch/encounter.rs
git commit -m "engine: add Enemy.retaliate flag (state plumbing)

Adds the retaliate keyword flag to the runtime Enemy struct, defaulted
false on every construction site (fixture, spawn_enemy). Consumed by the
retaliate-firing step in the next commit; populated from card data in C3b.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Retaliate firing (`FinishContinuation::PostRetaliate`)

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs:434` (new `FinishContinuation` variant)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs:247` (driver wiring) + new helper
- Test: `crates/game-core/src/engine/mod.rs` (test module, near the fight tests ~line 1890)

- [ ] **Step 1: Write the failing tests**

In `crates/game-core/src/engine/mod.rs`, in the test module containing `fight_evade_scenario`, add:

```rust
    #[test]
    fn failed_fight_against_ready_retaliate_enemy_triggers_attack() {
        // Combat 1 vs fight 3 → fail. Enemy retaliates 1 dmg + 1 horror.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().skills.combat = 1;
        let e = state.enemies.get_mut(&enemy_id).unwrap();
        e.retaliate = true;
        e.attack_damage = 1;
        e.attack_horror = 1;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(result.events, Event::SkillTestFailed { .. });
        // Retaliate attack lands (damage + horror, simultaneously).
        assert_event!(result.events, Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id);
        assert_event!(result.events, Event::HorrorTaken { investigator, amount: 1 } if *investigator == inv_id);
        assert_eq!(result.state.investigators[&inv_id].damage, 1);
        assert_eq!(result.state.investigators[&inv_id].horror, 1);
        // Enemy does NOT exhaust after a retaliate attack (RR p.18).
        assert!(!result.state.enemies[&enemy_id].exhausted);
        // Failed fight dealt no damage to the enemy.
        assert_no_event!(result.events, Event::EnemyDamaged { .. });
        // Skill test still tears down.
        assert_event!(result.events, Event::SkillTestEnded { .. });
    }

    #[test]
    fn successful_fight_against_retaliate_enemy_does_not_trigger_attack() {
        // Combat 3 vs fight 3 → success; retaliate must NOT fire.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().skills.combat = 3;
        let e = state.enemies.get_mut(&enemy_id).unwrap();
        e.retaliate = true;
        e.attack_damage = 1;
        e.attack_horror = 1;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_event!(result.events, Event::SkillTestSucceeded { .. });
        assert_no_event!(result.events, Event::DamageTaken { .. });
        assert_no_event!(result.events, Event::HorrorTaken { .. });
        assert_eq!(result.state.investigators[&inv_id].damage, 0);
    }

    #[test]
    fn failed_fight_against_exhausted_retaliate_enemy_does_not_trigger_attack() {
        // Retaliate requires a READY enemy (RR p.18). Exhausted → no attack.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().skills.combat = 1;
        let e = state.enemies.get_mut(&enemy_id).unwrap();
        e.retaliate = true;
        e.exhausted = true;
        e.attack_damage = 1;
        e.attack_horror = 1;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_event!(result.events, Event::SkillTestFailed { .. });
        assert_no_event!(result.events, Event::DamageTaken { .. });
        assert_eq!(result.state.investigators[&inv_id].damage, 0);
    }

    #[test]
    fn failed_fight_against_non_retaliate_enemy_does_not_trigger_attack() {
        // No retaliate flag → no attack on failure.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().skills.combat = 1;
        let e = state.enemies.get_mut(&enemy_id).unwrap();
        e.attack_damage = 1;
        e.attack_horror = 1;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_event!(result.events, Event::SkillTestFailed { .. });
        assert_no_event!(result.events, Event::DamageTaken { .. });
        assert_eq!(result.state.investigators[&inv_id].damage, 0);
    }

    #[test]
    fn failed_evade_against_retaliate_enemy_does_not_trigger_attack() {
        // Retaliate is "while attacking" — a failed Evade must NOT fire it.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().skills.agility = 1; // vs evade 3 → fail
        let e = state.enemies.get_mut(&enemy_id).unwrap();
        e.retaliate = true;
        e.attack_damage = 1;
        e.attack_horror = 1;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Evade {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_event!(result.events, Event::SkillTestFailed { .. });
        assert_no_event!(result.events, Event::DamageTaken { .. });
        assert_eq!(result.state.investigators[&inv_id].damage, 0);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p game-core retaliate 2>&1 | tail -25`
Expected: `failed_fight_against_ready_retaliate_enemy_triggers_attack` FAILS (no `DamageTaken`/`HorrorTaken` event; `damage == 0`). The four negative tests may already pass (no retaliate path exists yet) — that's fine; the positive test is the red.

- [ ] **Step 3a: Add the `FinishContinuation::PostRetaliate` variant**

In `crates/game-core/src/state/game_state.rs`, in the `FinishContinuation` enum, **between** `PostFollowUp { .. }` (ends line 434) and `PostOnResolution { .. }` (starts line 439):

```rust
    /// Step 3 (`OnSkillTestResolution`) is complete. The next driver
    /// iteration fires a Retaliate attack if the test was a failed Fight
    /// against a ready retaliate enemy (Rules Reference p.18 — "after
    /// applying all results for that skill test"), then advances to
    /// teardown.
    PostRetaliate {
        /// The chaos-token resolution's success determination — Retaliate
        /// fires only on failure.
        succeeded: bool,
    },
```

- [ ] **Step 3b: Wire the driver + add the helper**

In `crates/game-core/src/engine/dispatch/skill_test.rs`, in `drive_skill_test`'s `match continuation`:

Change the `PostFollowUp` arm's next continuation from `PostOnResolution` to `PostRetaliate` (line ~253), and add a new `PostRetaliate` arm immediately after it. The result:

```rust
            FinishContinuation::PostFollowUp { succeeded } => {
                fire_on_skill_test_resolution(cx, investigator, &indices_u8, succeeded);
                cx.state
                    .in_flight_skill_test
                    .as_mut()
                    .expect("in_flight_skill_test must persist across driver steps")
                    .continuation = FinishContinuation::PostRetaliate { succeeded };
            }
            FinishContinuation::PostRetaliate { succeeded } => {
                fire_retaliate_if_any(cx, investigator, succeeded);
                cx.state
                    .in_flight_skill_test
                    .as_mut()
                    .expect("in_flight_skill_test must persist across driver steps")
                    .continuation = FinishContinuation::PostOnResolution { succeeded };
            }
```

(The `PostOnResolution` arm is unchanged.)

Then add this helper near `apply_skill_test_follow_up` (e.g. after it, ~line 532):

```rust
/// Fire a Retaliate attack if the just-resolved test was a *failed Fight*
/// against a ready enemy with the retaliate keyword.
///
/// Rules Reference p.18: *"Each time an investigator fails a skill test
/// while attacking a ready enemy with the retaliate keyword, after
/// applying all results for that skill test, that enemy performs an
/// attack against the attacking investigator. An enemy does not exhaust
/// after performing a retaliate attack."*
///
/// Runs at the `PostRetaliate` step — after `fire_on_skill_test_resolution`
/// (the rest of ST.7) and before the `PostOnResolution` teardown (ST.8) —
/// matching "after applying all results." The attack routes through
/// [`super::combat::enemy_attack`], which does not exhaust the attacker,
/// satisfying the no-exhaust clause for free.
///
/// No-op unless every condition holds: the test failed; its follow-up was
/// `Fight`; the enemy is still in play, ready (`!exhausted`), and has
/// `retaliate`. A missing enemy is skipped quietly — a failed fight deals
/// no damage, so the target can't have been defeated mid-test; this only
/// guards against future enemy-removing commit effects. This step is also
/// the future home of the "after an enemy attacks" reaction window (Guard
/// Dog C5b, Roland's reaction).
fn fire_retaliate_if_any(cx: &mut Cx, investigator: InvestigatorId, succeeded: bool) {
    if succeeded {
        return;
    }
    let follow_up = cx.state.in_flight_skill_test.as_ref().map(|t| t.follow_up);
    let Some(SkillTestFollowUp::Fight { enemy }) = follow_up else {
        return;
    };
    let retaliates = cx
        .state
        .enemies
        .get(&enemy)
        .is_some_and(|e| e.retaliate && !e.exhausted);
    if retaliates {
        super::combat::enemy_attack(cx, enemy, investigator);
    }
}
```

`SkillTestFollowUp` is already imported in `skill_test.rs` (used by `apply_skill_test_follow_up`); `InvestigatorId` likewise. No new `use` needed.

- [ ] **Step 4: Run to verify all pass**

Run: `cargo test -p game-core retaliate 2>&1 | tail -25`
Expected: all five `*_retaliate_*` tests PASS.

- [ ] **Step 5: Run the gauntlet + commit**

```sh
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/skill_test.rs crates/game-core/src/engine/mod.rs
git commit -m "engine: Retaliate keyword (Ghoul Priest 01116)

After a failed Fight test against a ready retaliate enemy, the enemy
attacks the investigator without exhausting (RR p.18). Fires in a new
FinishContinuation::PostRetaliate step, after ST.7's OnSkillTestResolution
triggers and before ST.8 teardown (RR p.26 'after applying all results').

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Final verification (whole-workspace gauntlet)

After all three tasks, run the full CI gauntlet from the repo root before opening the PR:

```sh
RUSTFLAGS="-D warnings"    cargo test --all --all-features
                           cargo clippy --all-targets --all-features -- -D warnings
                           cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
                           cargo build -p web --target wasm32-unknown-unknown
                           cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

(`wasm-pack test` for `crates/web` is unaffected by this engine change but is part of CI.)

## Self-review notes (coverage vs. spec)

- Spec §1 `Prey::LowestRemainingHealth` → Task 1 (variant + branch + One/Tie tests).
- Spec §2 state `retaliate: bool` → Task 2 (field + all three literals).
- Spec §2 firing (Option B, `PostRetaliate` after ST.7) → Task 3 (continuation + driver + helper).
- Spec §2 edge cases (success / exhausted / non-retaliate / non-Fight follow-up) → Task 3 negative tests (success, exhausted, non-retaliate, failed-Evade). Enemy-absent graceful-skip is covered by the `.is_some_and(...)` guard (asserted indirectly — no enemy-removing path exists to test directly in C3a; documented in the helper).
- Out-of-scope (spawn_enemy population, six enemy impls) correctly left untouched: `spawn_enemy` keeps `retaliate: false`.
