# Victory-Point Enemy Display Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Place a defeated enemy that has a printed Victory value into the victory display (Rules Reference p.21).

**Architecture:** Carry `victory: Option<u8>` on the `Enemy` struct (set at spawn from `CardKind::Enemy`), and in `damage_enemy`'s defeat branch push the code into `state.victory_display` + emit the existing `Event::EnteredVictoryDisplay` — mirroring the location placement, but capturing at defeat since the enemy is removed immediately.

**Tech Stack:** Rust — `game-core` only (no corpus/pipeline involvement).

**Spec:** `docs/superpowers/specs/2026-06-13-victory-point-enemy-display-design.md`
**Issue:** #273 · **Branch:** `engine/victory-enemy-display` (already created)

---

## Task 1: Add `victory` to the `Enemy` struct + read it at spawn

Adding a field to `Enemy` touches its struct def and every literal (4 sites, all in game-core); they move together to compile. TDD via a `spawn_enemy` test that asserts the spawned enemy reflects the metadata's victory.

**Files:**
- Modify: `crates/game-core/src/state/enemy.rs` (struct def ~line 83; test literal ~line 92)
- Modify: `crates/game-core/src/test_support/fixtures.rs` (`test_enemy` ~line 97)
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs` (`spawn_enemy` destructure ~line 253, `Enemy` literal ~line 334; `enemy_metadata` test helper + a new test in `mod spawn_enemy_tests`)

- [ ] **Step 1: Write the failing test**

In `crates/game-core/src/engine/dispatch/encounter.rs`, in `mod spawn_enemy_tests`, add a `victory` parameter to the `enemy_metadata` helper. Change its signature and the `CardKind::Enemy` literal so `victory` is a parameter instead of hardcoded `None`:

```rust
    #[allow(clippy::too_many_arguments)]
    fn enemy_metadata(
        spawn: Option<Spawn>,
        health: HealthValue,
        hunter: bool,
        retaliate: bool,
        prey: Prey,
        fight: u8,
        evade: u8,
        damage: u8,
        horror: u8,
        victory: Option<u8>,
    ) -> CardMetadata {
        CardMetadata {
            code: "_synth_enemy".into(),
            name: "Synth Enemy".into(),
            text: None,
            traits: Vec::new(),
            pack_code: "_synth".into(),
            kind: CardKind::Enemy {
                fight,
                evade,
                damage,
                horror,
                health: Some(health),
                victory,
                spawn,
                surge: false,
                peril: false,
                hunter,
                retaliate,
                prey,
                quantity: 1,
            },
        }
    }
```

Update the `synth_enemy_metadata` wrapper to pass `None` for the new arg:

```rust
    fn synth_enemy_metadata(spawn: Option<Spawn>) -> CardMetadata {
        enemy_metadata(
            spawn,
            HealthValue::Fixed(1),
            false,
            false,
            Prey::Default,
            1,
            1,
            0,
            0,
            None,
        )
    }
```

The two existing tests that call `enemy_metadata` directly (`spawn_enemy_reads_combat_stats_and_keywords_from_metadata`, `spawn_enemy_scales_per_investigator_health_by_investigator_count`) each need the new trailing `None` arg added to their `enemy_metadata(...)` call.

Then add the new test:

```rust
    #[test]
    fn spawn_enemy_reads_victory_from_metadata() {
        let mut loc = test_location(10, "Loc");
        loc.code = CardCode("_l".into());
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(loc)
            .with_turn_order([InvestigatorId(1)])
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(10));

        let metadata = enemy_metadata(
            None,
            HealthValue::Fixed(5),
            false,
            false,
            Prey::Default,
            4,
            4,
            2,
            2,
            Some(2),
        );
        let mut events = Vec::new();
        spawn_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );

        let enemy = state.enemies.values().next().expect("enemy spawned");
        assert_eq!(enemy.victory, Some(2));
    }
```

- [ ] **Step 2: Run the test to verify it fails to compile**

Run: `cargo test -p game-core spawn_enemy_reads_victory_from_metadata`
Expected: FAIL — `Enemy` has no field `victory` (and the literal in `spawn_enemy` doesn't set it).

- [ ] **Step 3: Add the field to `Enemy`**

In `crates/game-core/src/state/enemy.rs`, after the `retaliate: bool,` field (~line 83):

```rust
    pub retaliate: bool,
    /// Printed Victory value (Rules Reference p.21). `Some(n)` places the
    /// enemy in the victory display when it is defeated; `None` for enemies
    /// that award no victory points.
    pub victory: Option<u8>,
}
```

- [ ] **Step 4: Update the other `Enemy` literals**

`crates/game-core/src/state/enemy.rs` test literal (~line 92, `enemy_carries_hunter_and_prey`) — add `victory: Some(2),` (Ghoul Priest's printed value):

```rust
            retaliate: true,
            code: crate::CardCode::new("01116"),
            victory: Some(2),
        };
```

`crates/game-core/src/test_support/fixtures.rs` `test_enemy` (~line 113) — add `victory: None,`:

```rust
        hunter: false,
        prey: Prey::Default,
        retaliate: false,
        victory: None,
    }
```

- [ ] **Step 5: Set `victory` at spawn**

In `crates/game-core/src/engine/dispatch/encounter.rs`, add `victory` to the `spawn_enemy` destructure (~line 253):

```rust
    let CardKind::Enemy {
        spawn,
        health,
        fight,
        evade,
        damage,
        horror,
        hunter,
        retaliate,
        prey,
        victory,
        ..
    } = &metadata.kind
    else {
```

And set it on the minted `Enemy` literal (~line 334), after `retaliate: *retaliate,`:

```rust
        hunter: *hunter,
        prey,
        retaliate: *retaliate,
        victory: *victory,
    };
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p game-core spawn_enemy`
Expected: PASS (all spawn_enemy tests, incl. the new one).

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/state/enemy.rs crates/game-core/src/test_support/fixtures.rs crates/game-core/src/engine/dispatch/encounter.rs
git commit -m "$(cat <<'EOF'
engine: carry enemy victory value on Enemy, set at spawn (#273)

Enemy gains `victory: Option<u8>`, populated by spawn_enemy from
CardKind::Enemy. Consumed by the defeat handler in the next commit.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Place defeated victory-point enemies in the victory display

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`damage_enemy` defeat branch ~line 27; tests in `mod combat_tests` ~line 308)

- [ ] **Step 1: Write the failing tests**

In `crates/game-core/src/engine/dispatch/combat.rs`, extend the `mod combat_tests` imports and add two tests. First widen the import line (currently `use crate::test_support::{test_enemy, GameStateBuilder};`):

```rust
    use super::super::Cx;
    use crate::event::Event;
    use crate::state::{EnemyId, InvestigatorId};
    use crate::test_support::{test_enemy, GameStateBuilder};
    use crate::{assert_event, assert_no_event};
```

Then add:

```rust
    #[test]
    fn defeating_victory_enemy_places_it_in_the_victory_display() {
        let eid = EnemyId(1);
        let mut enemy = test_enemy(1, "Ghoul Priest");
        enemy.code = crate::CardCode::new("01116");
        enemy.max_health = 1;
        enemy.victory = Some(2);
        let mut state = GameStateBuilder::new().build();
        state.enemies.insert(eid, enemy);
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        super::damage_enemy(&mut cx, eid, 1, Some(InvestigatorId(1)));

        assert_eq!(state.victory_display, vec![crate::CardCode::new("01116")]);
        assert_event!(
            events,
            Event::EnteredVictoryDisplay {
                code,
                victory: 2,
            } if code.as_str() == "01116"
        );
    }

    #[test]
    fn defeating_non_victory_enemy_places_nothing() {
        let eid = EnemyId(1);
        let mut enemy = test_enemy(1, "Ghoul");
        enemy.max_health = 1;
        enemy.victory = None;
        let mut state = GameStateBuilder::new().build();
        state.enemies.insert(eid, enemy);
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        super::damage_enemy(&mut cx, eid, 1, Some(InvestigatorId(1)));

        assert!(state.victory_display.is_empty());
        assert_no_event!(events, Event::EnteredVictoryDisplay { .. });
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p game-core defeating_victory_enemy defeating_non_victory_enemy`
Expected: the victory test FAILS (`victory_display` empty / no event); the non-victory test passes vacuously.

- [ ] **Step 3: Implement the placement**

In `crates/game-core/src/engine/dispatch/combat.rs`, in `damage_enemy`'s defeat branch, capture victory before removal and place it after the `EnemyDefeated` event + removal, before the reaction window. Replace:

```rust
    if new_damage >= enemy.max_health {
        let defeated_code = enemy.code.clone(); // capture before the enemy is removed
        cx.events.push(Event::EnemyDefeated {
            enemy: enemy_id,
            by,
        });
        cx.state.enemies.remove(&enemy_id);
```

with:

```rust
    if new_damage >= enemy.max_health {
        let defeated_code = enemy.code.clone(); // capture before the enemy is removed
        let defeated_victory = enemy.victory; // ditto
        cx.events.push(Event::EnemyDefeated {
            enemy: enemy_id,
            by,
        });
        cx.state.enemies.remove(&enemy_id);
        // RR p.21: a defeated enemy with a Victory value enters the victory
        // display. Captured here (not scanned at scenario resolution like
        // victory locations) because the enemy is removed above.
        if let Some(victory) = defeated_victory.filter(|v| *v > 0) {
            cx.state.victory_display.push(defeated_code.clone());
            cx.events.push(Event::EnteredVictoryDisplay {
                code: defeated_code.clone(),
                victory,
            });
        }
```

(`defeated_code` is cloned for the victory event; the original is still moved into the `fire_forced_triggers` call below.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p game-core defeating_victory_enemy defeating_non_victory_enemy`
Expected: PASS.

- [ ] **Step 5: Run the full combat + spawn suites to confirm no regressions**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/combat.rs
git commit -m "$(cat <<'EOF'
engine: place defeated victory-point enemies in the victory display (#273)

In damage_enemy's defeat branch, a defeated enemy with victory > 0 is
pushed to victory_display with an EnteredVictoryDisplay event — mirroring
the location placement but captured at defeat (the enemy is removed).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Full gauntlet + PR + phase doc

- [ ] **Step 1: Run the full local gauntlet**

```bash
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
                            cargo fmt --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown
                            cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all green. (`assert_event!`'s `if`-guard form is used in other modules, so the macro supports it; if clippy flags the `.filter(|v| *v > 0)` closure, no change is expected — it is idiomatic.)

- [ ] **Step 2: Push and open the PR**

```bash
git push -u origin engine/victory-enemy-display
gh pr create --title "engine: place defeated victory-point enemies in the victory display (#273)" --fill
```
PR body (repo template): summary, the design decision (victory carried on `Enemy` and captured at defeat, vs the location path's resolution-time scan), test notes, and `Closes #273.`

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch`
Fix any failures with follow-up commits to the same branch.

- [ ] **Step 4: Update the phase doc — ONLY after CI is green**

In `docs/phases/phase-7-the-gathering.md`, the C2 **Decisions made** entry promised "The victory-point **enemy** path (place as defeated) plugs into the same zone in **C3**." Update that clause to record it landed: e.g. "…the victory-point **enemy** path lands in PR #<n> (#273): enemies carry `victory` on the `Enemy` struct and `damage_enemy` places defeated victory enemies into `victory_display`." Commit as the final commit on the branch. Do **not** merge — stop for user approval.

---

## Self-review notes

- **Spec coverage:** `Enemy.victory` field (T1) · set at spawn (T1) · placement in `damage_enemy` (T2) · construction sites updated (T1) · positive + negative unit tests (T2) · spawn-reads-victory test (T1). Covered.
- **Type consistency:** `victory: Option<u8>` on `Enemy`; `Event::EnteredVictoryDisplay { code: CardCode, victory: u8 }` (existing); `enemy_metadata`'s new trailing `victory: Option<u8>` param matches all four call sites (the wrapper + two existing tests + the new test).
- **Reuse:** no new event (`EnteredVictoryDisplay` exists); no registry coupling (victory read from `Enemy`); mirrors the location placement shape in `engine/mod.rs`.
- **YAGNI:** non-damage defeat paths aren't handled — none exist; `damage_enemy` is the sole defeat/removal path.
