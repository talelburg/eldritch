# Legal-Action Enumerator — combat/engage (slice 2a-ii-2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend `legal_actions(state)` to enumerate the combat/engage actions — Fight, Evade (per engaged enemy), and Engage (per co-located enemy not already engaged with the active investigator).

**Architecture:** Continues slice 2a-ii-1's pattern: a new `push_combat_engage_actions` helper in `engine/enumerate.rs` that mirrors the `fight`/`evade`/`engage` handlers' acceptance conditions, reusing the already-`pub(crate)` `validate_basic_action` + `action_cost` predicates and reading enemy fields directly. Read-only; nothing routes through it (2b). Each task extends the cross-check with a combat board.

**Tech Stack:** Rust, `game-core` kernel crate. No new deps.

## Global Constraints

- **Build + expose, defer routing** (slice decision). Read-only enumerator; no handler rewired. "Accepted iff offered" holds by construction (shared predicates + direct field reads matching the handlers).
- **Mirror current handler behaviour, not the rules, where they diverge.** **Fight requires engagement** in the current handler (`validate_engaged_action`); the rules allow fighting any co-located enemy (RR p.12). This slice enumerates Fight over **engaged** enemies to match the handler — tracked in **#401**; when #401 lands, the Fight domain widens to co-located and the cross-check enforces the match. **Engage** *does* follow the rule (RR p.11): co-located enemies where `engaged_with != Some(active)`, including enemies engaged with *another* investigator.
- **Behaviour-preserving:** no handler changes. Full host gauntlet green every task: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`.
- **Design of record:** umbrella spec §E; builds on 2a-ii-1 (PR #402, merged).
- **Commit footer** (every commit), verbatim:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
  ```
- **Branch:** `engine/enumerator-combat`. One commit per task.

## Reference: current handler legality (what the enumerator mirrors)

- **Fight** (`actions.rs` `fight`): `validate_engaged_action` (= `validate_basic_action` + enemy-in-state + `enemy.engaged_with == Some(investigator)`) + `enemy.fight >= 0` + `charge_action(Fight)` (affordable: `action_cost(Fight) <= actions_remaining`).
- **Evade** (`actions.rs` `evade`): same shape with `enemy.evade >= 0` + `action_cost(Evade)`.
- **Engage** (`actions.rs` `engage`): `validate_basic_action` + `inv.current_location` is `Some(loc)` + enemy-in-state + `enemy.current_location == Some(loc)` + `enemy.engaged_with != Some(investigator)`, then `spend_one_action` (cost 1, already covered by `validate_basic_action`'s `actions_remaining >= 1`).

`PlayerAction` constructors: `Fight { investigator, enemy }`, `Evade { investigator, enemy }`, `Engage { investigator, enemy }` (`enemy: EnemyId`). `Enemy` fields: `fight: i8`, `evade: i8`, `engaged_with: Option<InvestigatorId>`, `current_location: Option<LocationId>`. `state.enemies` is a `BTreeMap<EnemyId, Enemy>` (deterministic key-order iteration → stable option order).

---

### Task 1: Fight + Evade over engaged enemies

**Files:**
- Modify: `crates/game-core/src/engine/enumerate.rs` — add `push_combat_engage_actions` (Fight/Evade half) + call it from `legal_actions`; extend the cross-check.

**Interfaces:**
- Consumes: `legal_actions` (2a-ii-1), `validate_basic_action`, `action_cost` (both `pub(crate)`, 2a-ii-1).

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `enumerate.rs`. Helper to place an engaged enemy:

```rust
    /// An enemy engaged with investigator 1 at `loc`, ready.
    fn engaged_enemy(id: u32, loc: crate::state::LocationId) -> crate::state::Enemy {
        let mut e = crate::test_support::test_enemy(id, "Ghoul");
        e.engaged_with = Some(InvestigatorId(1));
        e.current_location = Some(loc);
        e
    }

    #[test]
    fn fight_and_evade_offered_for_each_engaged_enemy() {
        let mut state = open_turn_state();
        let loc = crate::test_support::test_location(10, "Study");
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(loc_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;
        let e = engaged_enemy(7, loc_id);
        state.enemies.insert(e.id, e);

        let actions = legal_actions(&state);
        assert!(actions.contains(&PlayerAction::Fight {
            investigator: InvestigatorId(1),
            enemy: crate::state::EnemyId(7),
        }));
        assert!(actions.contains(&PlayerAction::Evade {
            investigator: InvestigatorId(1),
            enemy: crate::state::EnemyId(7),
        }));
    }

    #[test]
    fn no_fight_or_evade_for_an_unengaged_enemy() {
        let mut state = open_turn_state();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;
        // Enemy present but engaged with nobody → not a Fight/Evade target.
        let e = crate::test_support::test_enemy(7, "Ghoul");
        state.enemies.insert(e.id, e);
        assert!(!legal_actions(&state)
            .iter()
            .any(|a| matches!(a, PlayerAction::Fight { .. } | PlayerAction::Evade { .. })));
    }

    #[test]
    fn negative_fight_value_offers_evade_only() {
        let mut state = open_turn_state();
        let loc = crate::test_support::test_location(10, "Study");
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(loc_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;
        let mut e = engaged_enemy(7, loc_id);
        e.fight = -1; // malformed-but-handled: handler rejects Fight, allows Evade
        state.enemies.insert(e.id, e);

        let actions = legal_actions(&state);
        assert!(!actions.contains(&PlayerAction::Fight {
            investigator: InvestigatorId(1),
            enemy: crate::state::EnemyId(7),
        }));
        assert!(actions.contains(&PlayerAction::Evade {
            investigator: InvestigatorId(1),
            enemy: crate::state::EnemyId(7),
        }));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p game-core enumerate::tests`
Expected: FAIL — Fight/Evade never offered.

- [ ] **Step 3: Implement the Fight/Evade half**

In `enumerate.rs`, add the call in `legal_actions` (after `push_basic_actions`):

```rust
    push_basic_actions(state, investigator, &mut actions);
    push_combat_engage_actions(state, investigator, &mut actions);
    actions
```

Add the helper (Engage half added in Task 2):

```rust
/// Append the combat / engage actions legal for `investigator`, mirroring the
/// `fight`/`evade`/`engage` handlers (slice 2a-ii-2, #393). Fight/Evade target
/// enemies *engaged with* the investigator — matching the current handler
/// (`validate_engaged_action`); the rules allow Fight against any co-located
/// enemy (RR p.12), tracked in #401, which will widen this domain in lockstep.
fn push_combat_engage_actions(
    state: &GameState,
    investigator: InvestigatorId,
    out: &mut Vec<PlayerAction>,
) {
    use crate::engine::dispatch::actions::{action_cost, validate_basic_action};

    // The shared basic-action prologue gates Fight/Evade/Engage alike; if it
    // fails (wrong phase / not active / no action), none are legal.
    let Ok(inv) = validate_basic_action(state, "enumerate", investigator) else {
        return;
    };
    let actions_remaining = inv.actions_remaining;
    let fight_affordable =
        action_cost(state, investigator, crate::dsl::ActionClass::Fight) <= actions_remaining;
    let evade_affordable =
        action_cost(state, investigator, crate::dsl::ActionClass::Evade) <= actions_remaining;

    // Fight / Evade: one option per enemy engaged with the investigator, gated
    // on a non-negative difficulty (the handler rejects a negative one).
    for (&enemy_id, enemy) in &state.enemies {
        if enemy.engaged_with != Some(investigator) {
            continue;
        }
        if fight_affordable && enemy.fight >= 0 {
            out.push(PlayerAction::Fight {
                investigator,
                enemy: enemy_id,
            });
        }
        if evade_affordable && enemy.evade >= 0 {
            out.push(PlayerAction::Evade {
                investigator,
                enemy: enemy_id,
            });
        }
    }
}
```

- [ ] **Step 4: Extend the cross-check**

In `every_enumerated_action_is_accepted_by_its_handler`, after placing the investigator on location `a_id` with 3 actions, add an engaged enemy so a Fight + Evade are enumerated and applied:

```rust
        let mut foe = crate::test_support::test_enemy(7, "Ghoul");
        foe.engaged_with = Some(InvestigatorId(1));
        foe.current_location = Some(a_id);
        state.enemies.insert(foe.id, foe);
```

(Place this before the `for action in legal_actions(&state)` loop. Fight/Evade suspend into skill tests → `AwaitingInput`, not `Rejected`, so the `!Rejected` assertion holds.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p game-core enumerate::tests`
Expected: PASS.

- [ ] **Step 6: Run the gauntlet + commit**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
git add -A
git commit -m "engine: enumerate Fight/Evade over engaged enemies (slice 2a-ii-2 of #393)

legal_actions offers Fight and Evade for each enemy engaged with the active
investigator (non-negative difficulty + affordable), mirroring the current
engaged-only handlers. Fight's rules-correct co-located domain is tracked in
#401, which will widen this in lockstep. Cross-check extended with an engaged
enemy.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

### Task 2: Engage over co-located enemies

Engage follows the rule (RR p.11): a co-located enemy is engageable iff it is not already engaged with the active investigator — **including** an enemy engaged with a *different* investigator (engaging pulls it across).

**Files:**
- Modify: `crates/game-core/src/engine/enumerate.rs` — extend `push_combat_engage_actions` + the cross-check.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module:

```rust
    #[test]
    fn engage_offered_for_co_located_enemy_engaged_with_another() {
        let mut state = open_turn_state();
        // Two investigators so an enemy can be engaged with the *other* one.
        state
            .investigators
            .insert(InvestigatorId(2), test_investigator(2));
        let loc = crate::test_support::test_location(10, "Study");
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(loc_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;
        // Enemy at my location, engaged with investigator 2 → I may engage it.
        let mut e = crate::test_support::test_enemy(7, "Ghoul");
        e.current_location = Some(loc_id);
        e.engaged_with = Some(InvestigatorId(2));
        state.enemies.insert(e.id, e);

        assert!(legal_actions(&state).contains(&PlayerAction::Engage {
            investigator: InvestigatorId(1),
            enemy: crate::state::EnemyId(7),
        }));
    }

    #[test]
    fn no_engage_for_an_enemy_already_engaged_with_me_or_elsewhere() {
        let mut state = open_turn_state();
        let loc = crate::test_support::test_location(10, "Study");
        let other = crate::test_support::test_location(11, "Hall");
        let (loc_id, other_id) = (loc.id, other.id);
        state.locations.insert(loc_id, loc);
        state.locations.insert(other_id, other);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(loc_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;
        // Already engaged with me → not engageable.
        let mut mine = engaged_enemy(7, loc_id);
        mine.current_location = Some(loc_id);
        state.enemies.insert(mine.id, mine);
        // At a different location → not engageable.
        let mut away = crate::test_support::test_enemy(8, "Rat");
        away.current_location = Some(other_id);
        state.enemies.insert(away.id, away);

        let engages: Vec<_> = legal_actions(&state)
            .into_iter()
            .filter(|a| matches!(a, PlayerAction::Engage { .. }))
            .collect();
        assert!(engages.is_empty(), "no Engage offered, got {engages:?}");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p game-core enumerate::tests`
Expected: FAIL — `engage_offered_for_co_located_enemy_engaged_with_another` fails (Engage never offered).

- [ ] **Step 3: Implement the Engage half**

Append to `push_combat_engage_actions` (after the Fight/Evade loop, before the closing brace):

```rust
    // Engage: one option per enemy at the investigator's location not already
    // engaged with them — including an enemy engaged with *another* investigator
    // (engaging pulls it across; RR p.11). Engage costs 1 action, already gated
    // by the `validate_basic_action` prologue above.
    if let Some(loc) = inv.current_location {
        for (&enemy_id, enemy) in &state.enemies {
            if enemy.current_location == Some(loc) && enemy.engaged_with != Some(investigator) {
                out.push(PlayerAction::Engage {
                    investigator,
                    enemy: enemy_id,
                });
            }
        }
    }
```

- [ ] **Step 4: Extend the cross-check**

In `every_enumerated_action_is_accepted_by_its_handler`, the engaged enemy from Task 1 (`engaged_with == Some(1)`) is *not* Engage-eligible (already engaged with me). Add a second, unengaged co-located enemy so an Engage is enumerated and applied:

```rust
        let mut engageable = crate::test_support::test_enemy(8, "Rat");
        engageable.current_location = Some(a_id);
        state.enemies.insert(engageable.id, engageable);
```

(Engage fires AoO from the *other* engaged enemy (#7) — that is `enemy_attack`, which returns `Done`/`AwaitingInput`, never `Rejected`, so the cross-check holds.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p game-core enumerate::tests`
Expected: PASS.

- [ ] **Step 6: Run the gauntlet + commit**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
git add -A
git commit -m "engine: enumerate Engage over co-located enemies (slice 2a-ii-2 of #393)

legal_actions offers Engage for each enemy at the active investigator's location
not already engaged with them — including enemies engaged with another
investigator (RR p.11: engaging pulls it across). Cross-check extended with an
engageable enemy.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

## After the tasks

- **PR** against `main` (template); design-decisions paragraph: Fight engaged-only-to-match-handler (#401 widens later), Engage's rules-correct cross-investigator domain, build+expose+defer-routing with the cross-check as the safety net. Refs #393, #401.
- **Phase/spec doc** (final commit once CI green): tick 2a-ii-2 in spec §E sequencing.
- **Next:** 2a-ii-3 (PlayCard, ActivateAbility), 2a-ii-4 (AdvanceAct + sweep).

## Self-review notes

- **Spec coverage:** §E enumerator over the combat/engage action group → Task 1 (Fight/Evade), Task 2 (Engage). Mirrors handlers; routing still deferred. ✅
- **Placeholder scan:** none.
- **Type consistency:** `Fight/Evade/Engage { investigator, enemy: EnemyId }`; `action_cost(&GameState, InvestigatorId, ActionClass) -> u8`; `Enemy.fight/evade: i8`, `engaged_with/current_location: Option<…>`. `state.enemies: BTreeMap` (stable order). All match `action.rs` / fixtures.
- **Behaviour-preservation:** no handler touched; cross-check applies every enumerated combat/engage action without `Rejected` on a realistic board (engaged enemy for Fight/Evade, co-located unengaged enemy for Engage, non-empty chaos bag from `open_turn_state`).
- **Implementer caveats:** confirm `Enemy` field names (`fight`, `evade`, `engaged_with`, `current_location`) and that `state.enemies` iterates deterministically (BTreeMap) before relying on option order; the `engaged_enemy` helper (Task 1) is reused in Task 2 — keep it.
