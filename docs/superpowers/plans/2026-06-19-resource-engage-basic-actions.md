# Resource + Engage Basic Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the two missing basic actions — Resource (gain 1 resource) and Engage (engage an enemy at your location) — both firing attacks of opportunity per the Rules Reference, and fix the pre-existing Draw AoO gap.

**Architecture:** Two new `PlayerAction` variants dispatched to handlers in `engine/dispatch/actions.rs`, each following the established validate-first / mutate-second basic-action shape (the `investigate` handler is the canonical model): validate every precondition, then `spend_one_action` → `combat::fire_attacks_of_opportunity` → if the investigator survived the AoO, apply the effect. Draw gets the same `fire_attacks_of_opportunity` call it currently lacks.

**Tech Stack:** Rust, `game-core` kernel crate. Tests are `#[cfg(test)]` engine unit tests using the `TestGame`/`GameStateBuilder` builder + `test_investigator`/`test_enemy`/`test_location` fixtures + the `assert_event!` / `assert_no_event!` macros.

## Global Constraints

- **Validate-first / mutate-second:** every handler checks all preconditions and returns `EngineOutcome::Rejected { reason }` with state + events unchanged before any mutation.
- **AoO-exempt actions are fight / evade / parley / resign ONLY** (RR p.5). Draw, Resource, Move, Investigate, Engage all provoke an AoO.
- **AoO can eliminate the investigator;** after `fire_attacks_of_opportunity`, re-read status and suppress the action's primary effect (return `EngineOutcome::Done`) if `status != Status::Active` — exactly as `investigate` does (`actions.rs:118-130`).
- **State-corruption invariants panic** via `unreachable!` (active investigator missing from map); user-facing precondition failures return `Rejected`.
- **CI gauntlet (run before the final commit / push):**
  ```sh
  cargo fmt --check
  cargo clippy --all-targets --all-features -- -D warnings
  RUSTFLAGS="-D warnings" cargo test --all --all-features
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
  ```
- Tests live in `crates/game-core/src/engine/mod.rs` alongside the existing basic-action tests (`move_with_ready_engaged_enemy_fires_aoo_and_enemy_follows` et al.). Fixtures: `test_investigator(id)` → `resources: 5, actions_remaining: 3, status: Active, current_location: None`; `test_enemy(id, name)` → `attack_damage: 1, attack_horror: 0, current_location: None, exhausted: false, engaged_with: None`.

---

### Task 1: Resource action (#141)

**Files:**
- Modify: `crates/game-core/src/action.rs` (add `Resource` variant to `PlayerAction`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (add dispatch arm)
- Modify: `crates/game-core/src/engine/dispatch/actions.rs` (add `resource_action` handler)
- Test: `crates/game-core/src/engine/mod.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub(super) fn resource_action(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome`
- Consumes (existing): `spend_one_action`, `combat::fire_attacks_of_opportunity`, `Event::ResourcesGained { investigator, amount }`.

- [ ] **Step 1: Write the failing happy-path + AoO tests**

In `crates/game-core/src/engine/mod.rs` test module, add:

```rust
#[test]
fn resource_action_spends_action_and_gains_one_resource() {
    let inv_id = InvestigatorId(1);
    let loc = crate::state::LocationId(10);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(10, "Study"))
        .with_investigator({
            let mut i = test_investigator(1);
            i.current_location = Some(loc);
            i.resources = 5;
            i
        })
        .with_active_investigator(inv_id)
        .build();

    let result = apply(
        state,
        Action::Player(PlayerAction::Resource { investigator: inv_id }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(result.state.investigators[&inv_id].resources, 6);
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
    assert_event!(
        result.events,
        Event::ResourcesGained { investigator, amount: 1 } if *investigator == inv_id
    );
}

#[test]
fn resource_action_fires_aoo_from_ready_engaged_enemy() {
    let inv_id = InvestigatorId(1);
    let loc = crate::state::LocationId(10);
    let mut enemy = test_enemy(200, "Engaged Ghoul");
    enemy.current_location = Some(loc);
    enemy.engaged_with = Some(inv_id);
    enemy.attack_damage = 1;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(10, "Study"))
        .with_investigator({
            let mut i = test_investigator(1);
            i.current_location = Some(loc);
            i
        })
        .with_active_investigator(inv_id)
        .with_enemy(enemy)
        .build();

    let result = apply(
        state,
        Action::Player(PlayerAction::Resource { investigator: inv_id }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    // AoO fired: investigator took 1 damage, but the resource is still gained.
    assert_eq!(result.state.investigators[&inv_id].damage, 1);
    assert_eq!(result.state.investigators[&inv_id].resources, 6);
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p game-core resource_action 2>&1 | tail -20`
Expected: FAIL — `no variant named Resource found for enum PlayerAction` (compile error).

- [ ] **Step 3: Add the `Resource` variant to `PlayerAction`**

In `crates/game-core/src/action.rs`, add to the `PlayerAction` enum (after `Investigate`):

```rust
    /// Gain 1 resource (the basic "Resource" action, Rules Reference
    /// Investigation step 2.2.1). Spends 1 action.
    ///
    /// Validate: Investigation phase, investigator is active and
    /// `Status::Active`, `actions_remaining >= 1`.
    ///
    /// Resource is NOT on the AoO-exempt list (only Fight, Evade,
    /// Parley, Resign are), so each ready engaged enemy makes an attack
    /// of opportunity before the resource is gained; an AoO that
    /// eliminates the investigator suppresses the gain.
    Resource {
        /// Investigator taking the action. Must be the active
        /// investigator during the Investigation phase.
        investigator: InvestigatorId,
    },
```

- [ ] **Step 4: Add the dispatch arm**

In `crates/game-core/src/engine/dispatch/mod.rs`, in the `apply_player_action` match (after the `Investigate` arm):

```rust
        PlayerAction::Resource { investigator } => actions::resource_action(cx, *investigator),
```

- [ ] **Step 5: Implement the `resource_action` handler**

In `crates/game-core/src/engine/dispatch/actions.rs`, add:

```rust
/// Handler for [`PlayerAction::Resource`]. The basic "gain 1 resource"
/// action (Rules Reference, Investigation step 2.2.1).
///
/// Validate-first: Investigation phase, `investigator` is active and
/// `Status::Active`, `actions_remaining >= 1`. Mutate-second: spend 1
/// action, fire attacks of opportunity (Resource is NOT AoO-exempt),
/// then — if the investigator survived the AoO — gain 1 resource.
pub(super) fn resource_action(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    if cx.state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "Resource is only valid during the Investigation phase (was {:?})",
                cx.state.phase
            )
            .into(),
        };
    }
    if cx.state.active_investigator != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Resource: {investigator:?} is not the active investigator ({:?})",
                cx.state.active_investigator,
            )
            .into(),
        };
    }
    let inv = cx.state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "Resource: active_investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
        )
    });
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!("Resource: {investigator:?} is not Active (status {:?})", inv.status)
                .into(),
        };
    }
    if inv.actions_remaining < 1 {
        return EngineOutcome::Rejected {
            reason: "Resource requires at least 1 action point".into(),
        };
    }

    // Mutate-second: spend the action, fire AoO, then gain the resource.
    spend_one_action(cx, investigator);
    super::combat::fire_attacks_of_opportunity(cx, investigator);

    // If AoO eliminated the investigator, the gain is suppressed; the
    // spent action + AoO events stay (mirrors `investigate`).
    let inv_after = cx.state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "Resource: investigator {investigator:?} disappeared between AoO and gain; \
             this is a state-corruption invariant violation"
        )
    });
    if inv_after.status != Status::Active {
        return EngineOutcome::Done;
    }

    let inv_mut = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("investigator existence checked above");
    inv_mut.resources = inv_mut.resources.saturating_add(1);
    cx.events.push(Event::ResourcesGained {
        investigator,
        amount: 1,
    });
    EngineOutcome::Done
}
```

- [ ] **Step 6: Add the rejection tests**

In the same test module:

```rust
#[test]
fn resource_action_rejects_wrong_phase() {
    let inv_id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Mythos)
        .with_investigator(test_investigator(1))
        .with_active_investigator(inv_id)
        .build();
    let result = apply(state, Action::Player(PlayerAction::Resource { investigator: inv_id }));
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.investigators[&inv_id].resources, 5);
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 3);
}

#[test]
fn resource_action_rejects_when_not_active_investigator() {
    let inv_id = InvestigatorId(1);
    let other = InvestigatorId(2);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(test_investigator(1))
        .with_investigator(test_investigator(2))
        .with_active_investigator(other)
        .build();
    let result = apply(state, Action::Player(PlayerAction::Resource { investigator: inv_id }));
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
}

#[test]
fn resource_action_rejects_with_no_actions_remaining() {
    let inv_id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator({
            let mut i = test_investigator(1);
            i.actions_remaining = 0;
            i
        })
        .with_active_investigator(inv_id)
        .build();
    let result = apply(state, Action::Player(PlayerAction::Resource { investigator: inv_id }));
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.investigators[&inv_id].resources, 5);
}
```

- [ ] **Step 7: Run all Resource tests to verify they pass**

Run: `cargo test -p game-core resource_action 2>&1 | tail -20`
Expected: PASS (5 tests).

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/action.rs crates/game-core/src/engine/dispatch/mod.rs crates/game-core/src/engine/dispatch/actions.rs crates/game-core/src/engine/mod.rs
git commit -m "engine: Resource basic action (gain 1 resource, fires AoO)

Adds PlayerAction::Resource — the basic gain-1-resource action
(Investigation step 2.2.1). Fires attacks of opportunity (not on the
RR p.5 exempt list of fight/evade/parley/resign); an AoO that
eliminates the investigator suppresses the gain. Part of #141.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Engage action (#77)

**Files:**
- Modify: `crates/game-core/src/action.rs` (add `Engage` variant)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (add dispatch arm)
- Modify: `crates/game-core/src/engine/dispatch/actions.rs` (add `engage` handler)
- Test: `crates/game-core/src/engine/mod.rs`

**Interfaces:**
- Produces: `pub(super) fn engage(cx: &mut Cx, investigator: InvestigatorId, enemy_id: EnemyId) -> EngineOutcome`
- Consumes (existing): `spend_one_action`, `combat::fire_attacks_of_opportunity`, `Event::EnemyEngaged { enemy, investigator }`, `Enemy.{current_location, engaged_with}`.

- [ ] **Step 1: Write the failing happy-path + AoO tests**

```rust
#[test]
fn engage_action_engages_unengaged_enemy_at_location() {
    let inv_id = InvestigatorId(1);
    let loc = crate::state::LocationId(10);
    let enemy_id = EnemyId(300);
    let mut enemy = test_enemy(300, "Aloof Ghoul");
    enemy.current_location = Some(loc);
    enemy.engaged_with = None;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(10, "Study"))
        .with_investigator({
            let mut i = test_investigator(1);
            i.current_location = Some(loc);
            i
        })
        .with_active_investigator(inv_id)
        .with_enemy(enemy)
        .build();

    let result = apply(
        state,
        Action::Player(PlayerAction::Engage { investigator: inv_id, enemy: enemy_id }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(result.state.enemies[&enemy_id].engaged_with, Some(inv_id));
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
    assert_event!(
        result.events,
        Event::EnemyEngaged { enemy, investigator }
            if *enemy == enemy_id && *investigator == inv_id
    );
}

#[test]
fn engage_action_provokes_aoo_from_other_engaged_enemy_not_the_target() {
    let inv_id = InvestigatorId(1);
    let loc = crate::state::LocationId(10);
    let target_id = EnemyId(300);
    let other_id = EnemyId(301);
    let mut target = test_enemy(300, "Target Ghoul"); // not engaged yet
    target.current_location = Some(loc);
    let mut other = test_enemy(301, "Already-Engaged Ghoul");
    other.current_location = Some(loc);
    other.engaged_with = Some(inv_id);
    other.attack_damage = 1;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(10, "Study"))
        .with_investigator({
            let mut i = test_investigator(1);
            i.current_location = Some(loc);
            i
        })
        .with_active_investigator(inv_id)
        .with_enemy(target)
        .with_enemy(other)
        .build();

    let result = apply(
        state,
        Action::Player(PlayerAction::Engage { investigator: inv_id, enemy: target_id }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    // The OTHER engaged enemy made the AoO; the target (not engaged at
    // AoO time) did not. Target ends engaged; investigator took 1 damage.
    assert_eq!(result.state.investigators[&inv_id].damage, 1);
    assert_eq!(result.state.enemies[&target_id].engaged_with, Some(inv_id));
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p game-core engage_action 2>&1 | tail -20`
Expected: FAIL — `no variant named Engage found for enum PlayerAction`.

- [ ] **Step 3: Add the `Engage` variant to `PlayerAction`**

In `crates/game-core/src/action.rs`, after the `Resource` variant:

```rust
    /// Engage an enemy at the active investigator's location that the
    /// investigator is not already engaged with (Rules Reference p.4).
    /// Spends 1 action; the enemy becomes engaged with the investigator.
    ///
    /// Validate: Investigation phase, investigator is active and
    /// `Status::Active`, `actions_remaining >= 1`, the enemy exists, is
    /// at the investigator's `current_location`, and is not already
    /// engaged with the investigator.
    ///
    /// Engage is NOT on the AoO-exempt list, so OTHER ready engaged
    /// enemies make attacks of opportunity before the engagement
    /// resolves (the target is not engaged at that point, so it does
    /// not). The multiplayer "engage an enemy engaged with another
    /// investigator" clause is latent (single `engaged_with` field).
    Engage {
        /// Investigator performing the action. Must be the active
        /// investigator during the Investigation phase.
        investigator: InvestigatorId,
        /// The enemy to engage. Must be at the investigator's location
        /// and not already engaged with the investigator.
        enemy: EnemyId,
    },
```

- [ ] **Step 4: Add the dispatch arm**

In `crates/game-core/src/engine/dispatch/mod.rs`, after the `Resource` arm:

```rust
        PlayerAction::Engage {
            investigator,
            enemy,
        } => actions::engage(cx, *investigator, *enemy),
```

- [ ] **Step 5: Implement the `engage` handler**

In `crates/game-core/src/engine/dispatch/actions.rs`:

```rust
/// Handler for [`PlayerAction::Engage`]. Engage an enemy at the
/// investigator's location that they are not already engaged with
/// (Rules Reference p.4) — it becomes engaged with the investigator.
///
/// Validate-first: Investigation phase, active + `Status::Active`,
/// `actions_remaining >= 1`, enemy in state, enemy at the investigator's
/// `current_location`, not already engaged with the investigator.
/// Mutate-second: spend 1 action, fire attacks of opportunity (Engage is
/// NOT AoO-exempt — the target is not engaged yet so it cannot AoO; only
/// OTHER engaged ready enemies do), then — if the investigator survived —
/// engage the enemy.
pub(super) fn engage(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    if cx.state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "Engage is only valid during the Investigation phase (was {:?})",
                cx.state.phase
            )
            .into(),
        };
    }
    if cx.state.active_investigator != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Engage: {investigator:?} is not the active investigator ({:?})",
                cx.state.active_investigator,
            )
            .into(),
        };
    }
    let inv = cx.state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "Engage: active_investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
        )
    });
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!("Engage: {investigator:?} is not Active (status {:?})", inv.status)
                .into(),
        };
    }
    if inv.actions_remaining < 1 {
        return EngineOutcome::Rejected {
            reason: "Engage requires at least 1 action point".into(),
        };
    }
    let inv_location = inv.current_location;
    let Some(enemy) = cx.state.enemies.get(&enemy_id) else {
        return EngineOutcome::Rejected {
            reason: format!("Engage: enemy {enemy_id:?} is not in state").into(),
        };
    };
    if enemy.engaged_with == Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!("Engage: {investigator:?} is already engaged with {enemy_id:?}").into(),
        };
    }
    if enemy.current_location != inv_location {
        return EngineOutcome::Rejected {
            reason: format!(
                "Engage: enemy {enemy_id:?} (at {:?}) is not at {investigator:?}'s location ({:?})",
                enemy.current_location, inv_location,
            )
            .into(),
        };
    }

    // Mutate-second.
    spend_one_action(cx, investigator);
    super::combat::fire_attacks_of_opportunity(cx, investigator);

    let inv_after = cx.state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "Engage: investigator {investigator:?} disappeared between AoO and engagement; \
             this is a state-corruption invariant violation"
        )
    });
    if inv_after.status != Status::Active {
        return EngineOutcome::Done;
    }

    let enemy_mut = cx.state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
        unreachable!(
            "Engage: enemy {enemy_id:?} disappeared between validation and engagement; \
             this is a state-corruption invariant violation"
        )
    });
    enemy_mut.engaged_with = Some(investigator);
    cx.events.push(Event::EnemyEngaged {
        enemy: enemy_id,
        investigator,
    });
    EngineOutcome::Done
}
```

- [ ] **Step 6: Add the rejection tests**

```rust
#[test]
fn engage_action_rejects_enemy_not_at_location() {
    let inv_id = InvestigatorId(1);
    let here = crate::state::LocationId(10);
    let there = crate::state::LocationId(11);
    let enemy_id = EnemyId(300);
    let mut enemy = test_enemy(300, "Distant Ghoul");
    enemy.current_location = Some(there);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(10, "Study"))
        .with_location(test_location(11, "Hallway"))
        .with_investigator({
            let mut i = test_investigator(1);
            i.current_location = Some(here);
            i
        })
        .with_active_investigator(inv_id)
        .with_enemy(enemy)
        .build();
    let result = apply(state, Action::Player(PlayerAction::Engage { investigator: inv_id, enemy: enemy_id }));
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.enemies[&enemy_id].engaged_with, None);
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 3);
}

#[test]
fn engage_action_rejects_already_engaged_enemy() {
    let inv_id = InvestigatorId(1);
    let loc = crate::state::LocationId(10);
    let enemy_id = EnemyId(300);
    let mut enemy = test_enemy(300, "Engaged Ghoul");
    enemy.current_location = Some(loc);
    enemy.engaged_with = Some(inv_id);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(10, "Study"))
        .with_investigator({
            let mut i = test_investigator(1);
            i.current_location = Some(loc);
            i
        })
        .with_active_investigator(inv_id)
        .with_enemy(enemy)
        .build();
    let result = apply(state, Action::Player(PlayerAction::Engage { investigator: inv_id, enemy: enemy_id }));
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 3);
}

#[test]
fn engage_action_rejects_unknown_enemy() {
    let inv_id = InvestigatorId(1);
    let loc = crate::state::LocationId(10);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(10, "Study"))
        .with_investigator({
            let mut i = test_investigator(1);
            i.current_location = Some(loc);
            i
        })
        .with_active_investigator(inv_id)
        .build();
    let result = apply(state, Action::Player(PlayerAction::Engage { investigator: inv_id, enemy: EnemyId(999) }));
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
}

#[test]
fn engage_action_rejects_no_actions_remaining() {
    let inv_id = InvestigatorId(1);
    let loc = crate::state::LocationId(10);
    let enemy_id = EnemyId(300);
    let mut enemy = test_enemy(300, "Ghoul");
    enemy.current_location = Some(loc);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(10, "Study"))
        .with_investigator({
            let mut i = test_investigator(1);
            i.current_location = Some(loc);
            i.actions_remaining = 0;
            i
        })
        .with_active_investigator(inv_id)
        .with_enemy(enemy)
        .build();
    let result = apply(state, Action::Player(PlayerAction::Engage { investigator: inv_id, enemy: enemy_id }));
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.enemies[&enemy_id].engaged_with, None);
}
```

- [ ] **Step 7: Run all Engage tests to verify they pass**

Run: `cargo test -p game-core engage_action 2>&1 | tail -20`
Expected: PASS (6 tests).

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/action.rs crates/game-core/src/engine/dispatch/mod.rs crates/game-core/src/engine/dispatch/actions.rs crates/game-core/src/engine/mod.rs
git commit -m "engine: Engage basic action (engage enemy at your location)

Adds PlayerAction::Engage (RR p.4): engage an enemy at your location not
already engaged with you; it becomes engaged. Fires attacks of
opportunity from OTHER ready engaged enemies (the target isn't engaged
yet). Multiplayer 'engage an enemy engaged with another investigator'
clause is latent. Part of #77.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Draw AoO fix + exempt-list comment fix

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/cards.rs` (`draw` handler — add AoO)
- Modify: `crates/game-core/src/engine/dispatch/actions.rs:108` (fix the exempt-list comment)
- Test: `crates/game-core/src/engine/mod.rs`

**Interfaces:**
- Consumes (existing): `combat::fire_attacks_of_opportunity`, the `draw` handler's existing structure.

- [ ] **Step 1: Write the failing Draw-AoO test**

```rust
#[test]
fn draw_action_fires_aoo_from_ready_engaged_enemy() {
    let inv_id = InvestigatorId(1);
    let loc = crate::state::LocationId(10);
    let mut enemy = test_enemy(200, "Engaged Ghoul");
    enemy.current_location = Some(loc);
    enemy.engaged_with = Some(inv_id);
    enemy.attack_damage = 1;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(10, "Study"))
        .with_investigator({
            let mut i = test_investigator(1);
            i.current_location = Some(loc);
            // Give the deck a card so the draw itself succeeds without
            // the empty-deck horror path muddying the AoO assertion.
            i.deck = vec![CardCode::new("_test_card_1")];
            i
        })
        .with_active_investigator(inv_id)
        .with_enemy(enemy)
        .build();

    let result = apply(state, Action::Player(PlayerAction::Draw { investigator: inv_id }));

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(result.state.investigators[&inv_id].damage, 1);
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
    );
}
```

> Note: confirm the `CardCode` constructor used elsewhere in this test module (`CardCode::new(...)` vs `CardCode("...".into())`) and match it; both appear in the codebase — use whichever the surrounding tests use.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p game-core draw_action_fires_aoo 2>&1 | tail -20`
Expected: FAIL — `investigators[&inv_id].damage` is `0` (no AoO fired) / `DamageTaken event missing`.

- [ ] **Step 3: Add the AoO call to the `draw` handler**

In `crates/game-core/src/engine/dispatch/cards.rs`, in `draw`, immediately after `super::actions::spend_one_action(cx, investigator);` and before the deck-draw logic, insert:

```rust
    // Draw is NOT on the AoO-exempt list (only Fight, Evade, Parley,
    // Resign are), so each ready engaged enemy attacks before the card
    // is drawn (RR p.5).
    super::combat::fire_attacks_of_opportunity(cx, investigator);

    // If the AoO eliminated the investigator, suppress the draw (the
    // spent action + AoO events stay), mirroring `investigate`.
    if cx
        .state
        .investigators
        .get(&investigator)
        .is_none_or(|inv| inv.status != Status::Active)
    {
        return EngineOutcome::Done;
    }
```

> If `Status` / `EngineOutcome` are not already imported in `cards.rs`, add the imports the surrounding code uses (the file already references `Status` for the `Draw` validation, so it is in scope). Verify `is_none_or` is acceptable under the crate's MSRV; if clippy/compile rejects it, use `.map_or(true, |inv| inv.status != Status::Active)`.

- [ ] **Step 4: Fix the exempt-list comment**

In `crates/game-core/src/engine/dispatch/actions.rs` (the `investigate` handler, ~line 108), change:

```rust
    // test. Investigate is NOT on the AoO-exempt list (only Fight,
    // Evade, Parley, Engage, Resign are), so each ready engaged
    // enemy attacks before the test resolves.
```

to:

```rust
    // test. Investigate is NOT on the AoO-exempt list (only Fight,
    // Evade, Parley, Resign are), so each ready engaged enemy attacks
    // before the test resolves.
```

- [ ] **Step 5: Run the Draw-AoO test + the full draw test group to verify**

Run: `cargo test -p game-core draw 2>&1 | tail -20`
Expected: PASS — the new AoO test plus all existing `draw` tests still green (the empty-deck-horror tests use no engaged enemy, so AoO is a no-op there).

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/cards.rs crates/game-core/src/engine/dispatch/actions.rs crates/game-core/src/engine/mod.rs
git commit -m "engine: Draw action fires attacks of opportunity

Draw is not on the RR p.5 AoO-exempt list (fight/evade/parley/resign),
but the handler never fired AoO — fixed, with the same elimination guard
the other basic actions use. Also corrects the actions.rs exempt-list
comment that wrongly included Engage.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Final gauntlet + phase-doc update

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md` (mark #141/#77 progress)

- [ ] **Step 1: Run the full CI gauntlet**

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
RUSTFLAGS="-D warnings" cargo test --all --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: all green. Fix any clippy/fmt/doc issues before proceeding.

- [ ] **Step 2: Update the phase doc**

In `docs/phases/phase-7-the-gathering.md`, Tier-1 group A: note #141 (Resource) and #77 (Engage's basic-action half) as shipped (cite the PR once open), and record the discovered/fixed Draw-AoO gap + the corrected exempt-list. Add a Decisions/Architecture note only if load-bearing for future work (the "all non-exempt actions fire `fire_attacks_of_opportunity`; the keystone upgrades them to open windows" point is worth one line). Defer this commit until the PR is otherwise ready (per the phase-doc workflow).

- [ ] **Step 3: Commit the doc update**

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: phase-7 — Resource + Engage basic actions shipped

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-review

**Spec coverage:**
- Resource action (#141) → Task 1. ✓
- Engage action (#77) → Task 2. ✓
- Both fire AoO (not exempt), with elimination guard → Tasks 1 & 2 mutate steps. ✓
- Draw AoO fix (folded in, option b) → Task 3. ✓
- Exempt-list comment fix → Task 3 Step 4. ✓
- Validate-first/mutate-second, panics for state-corruption → handler code follows `investigate`. ✓
- Out of scope (AoO windows #293/#379, attack-order #143, Engage multiplayer clause, Parley/Resign #258) → not implemented; noted in variant doc-comments. ✓
- Testing per action (happy / rejections / AoO) → Tasks 1, 2, 3 test steps. ✓

**Placeholder scan:** No "TBD"/"add validation"/"write tests for the above" — every handler and test step carries complete code. Two flagged verification notes (CardCode constructor form, `is_none_or` MSRV) are explicit fallbacks, not placeholders. ✓

**Type consistency:** `resource_action(cx, investigator)`, `engage(cx, investigator, enemy_id)` match their dispatch arms; `Event::ResourcesGained { investigator, amount }` and `Event::EnemyEngaged { enemy, investigator }` match the `event.rs` definitions; `Enemy.{current_location, engaged_with}`, `Status::Active`, `spend_one_action`, `combat::fire_attacks_of_opportunity` all verified against source. ✓
