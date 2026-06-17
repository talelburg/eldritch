# `Cost::DiscardSelf` + Enemy Choice + `DealDamageToEnemy` (#301) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Beat Cop's engine prereqs — a discard-the-source-asset activation cost, the keystone's enemy choice variety, and a typed deal-damage-to-a-chosen-enemy effect with a pre-cost target check.

**Architecture:** `Cost::DiscardSelf` removes the source from `cards_in_play` during cost payment (reusing the `defeat_overflowed_assets` discard pattern). `EnemyTarget::Chosen(Choose<EntityScope>)` reuses the merged keystone's choice machinery; a shared `combat::enemies_in_scope` enumerator serves both the evaluator's grounding and an activation-layer pre-cost check (mirroring the existing `effect_initiates_fight` guard). `Effect::DealDamageToEnemy` grounds the enemy then calls the existing `combat::deal_damage_to_enemy`.

**Tech Stack:** Rust workspace; `card-dsl` (DSL types), `game-core` (evaluator, dispatch). serde-derive on DSL types.

## Global Constraints

- Match CI's strict flags before every commit: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`.
- `card-dsl` is **below** `game-core`: no `game-core` types (`EnemyId`, etc.) may appear in `card-dsl`. `EnemyTarget` references only `Choose`/`EntityScope` (card-dsl types from the keystone).
- New DSL types/variants derive `Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize` (the target enums and `Effect`/`Cost` are `Copy`/`Clone` respectively — `EnemyTarget` must be `Copy` like its siblings; `Effect::DealDamageToEnemy` only needs `Clone` like the rest of `Effect`).
- Validate-first / mutate-second. The pre-cost check must reject **before** `pay_activation_costs` runs.
- `Cost::DiscardSelf` is the **sole source-referencing cost** on an ability and is paid last; pairing it with `Cost::Exhaust` / `Cost::SpendUses` is rejected at the check layer (loud, `TODO` until a card needs the combo).
- Scope: enemy variety ships only `EnemyTarget::Chosen` (no `Engaged`); `EntityScope::At` is reused verbatim from the keystone (no new spatial terms). Beat Cop / Knife *cards* are out of scope (PR-4 / PR-5).

**Branch:** `engine/discard-self-enemy-damage` (issue #301). The design spec `docs/superpowers/specs/2026-06-17-phase-7-301-discard-self-enemy-damage-design.md` + this plan are committed as the branch's first commit before Task 1.

---

### Task 1: `Cost::DiscardSelf`

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (add the `Cost` variant)
- Modify: `crates/game-core/src/engine/dispatch/abilities.rs` (`check_cost_payable`, `pay_activation_costs`)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`check_activate_ability` — the combo guard)
- Test: `crates/card-dsl/src/dsl.rs` (serde), `crates/game-core/tests/discard_self.rs` (new integration binary)

**Interfaces:**
- Produces: `Cost::DiscardSelf` (no fields).

- [ ] **Step 1: Failing serde test** (append to `crates/card-dsl/src/dsl.rs` test module)

```rust
#[test]
fn discard_self_cost_serde_round_trips() {
    let c = Cost::DiscardSelf;
    let json = serde_json::to_string(&c).unwrap();
    assert_eq!(serde_json::from_str::<Cost>(&json).unwrap(), c);
}
```

- [ ] **Step 2: Run — fails to compile**

Run: `cargo test -p card-dsl discard_self_cost_serde_round_trips`
Expected: FAIL — `no variant named DiscardSelf`.

- [ ] **Step 3: Add the variant** (`crates/card-dsl/src/dsl.rs`, in `enum Cost`, after `SpendUses`)

```rust
    /// Discard the source asset *in play* to pay for its own ability
    /// (Beat Cop 01018, Knife 01086). Distinct from
    /// [`Effect::DiscardSelf`], which removes a treachery from a threat
    /// area / location. Must be the only source-referencing cost on an
    /// ability (it removes the source); paid last.
    DiscardSelf,
```

Run: `cargo test -p card-dsl discard_self_cost_serde_round_trips` → PASS.

- [ ] **Step 4: Validate as payable** (`crates/game-core/src/engine/dispatch/abilities.rs`, `check_cost_payable` match)

Add before the `Cost::DiscardCardFromHand` arm:

```rust
        // Source is in play by the activation precondition (check_activate_ability
        // located it in cards_in_play), so it is always payable.
        Cost::DiscardSelf => Ok(()),
```

- [ ] **Step 5: Pay it — remove the source from play** (`crates/game-core/src/engine/dispatch/abilities.rs`, `pay_activation_costs` match)

Add before the `Cost::DiscardCardFromHand` arm:

```rust
            Cost::DiscardSelf => {
                let inv_mut = cx
                    .state
                    .investigators
                    .get_mut(&investigator)
                    .expect("validated above");
                // Look up by instance_id (robust if an earlier cost shifted positions).
                if let Some(pos) = inv_mut
                    .cards_in_play
                    .iter()
                    .position(|c| c.instance_id == instance_id)
                {
                    let card = inv_mut.cards_in_play.remove(pos);
                    inv_mut.discard.push(card.code.clone());
                    cx.events.push(Event::CardDiscarded {
                        investigator,
                        code: card.code,
                        from: crate::state::Zone::InPlay,
                    });
                }
            }
```

- [ ] **Step 6: Combo guard** (`crates/game-core/src/engine/dispatch/reaction_windows.rs`, in `check_activate_ability`, right after `costs` is available and before the Fight check)

```rust
    // DiscardSelf removes the source, invalidating any other source-referencing
    // cost. It must be the sole such cost (Beat Cop / Knife list only it).
    if costs.iter().any(|c| matches!(c, Cost::DiscardSelf))
        && costs
            .iter()
            .any(|c| matches!(c, Cost::Exhaust | Cost::SpendUses { .. }))
    {
        return Err(
            "ActivateAbility: Cost::DiscardSelf cannot combine with Exhaust/SpendUses on the \
             same ability (it removes the source); TODO(#301) lift if a card needs the combo"
                .into(),
        );
    }
```

- [ ] **Step 7: Integration test** (create `crates/game-core/tests/discard_self.rs`)

```rust
//! `Cost::DiscardSelf`: an activated ability discards its own in-play asset
//! as a cost. Mock registry in its own integration binary (own process +
//! `OnceLock<CardRegistry>`), mirroring `weapon_fight.rs`.

use std::sync::OnceLock;

use game_core::card_data::{CardKind, CardMetadata, Class, SkillIcons, Slot};
use game_core::dsl::{activated, gain_resources, Ability, Cost, InvestigatorTarget};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{CardCode, CardInPlay, CardInstanceId, InvestigatorId, Phase};
use game_core::test_support::{apply_no_commits, test_investigator, GameStateBuilder};
use game_core::{assert_event, Action, PlayerAction};

const TRINKET: &str = "TRNK1";

fn trinket_metadata() -> CardMetadata {
    CardMetadata {
        code: TRINKET.to_owned(),
        name: "Mock Trinket".to_owned(),
        traits: vec!["Item".to_owned()],
        text: Some("[fast] Discard Mock Trinket: gain 1 resource.".to_owned()),
        pack_code: "_mock".to_owned(),
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            slots: vec![Slot::Hand],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 1,
            uses: None,
        },
    }
}

fn trinket_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(trinket_metadata)
}

fn mock_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    (code.as_str() == TRINKET).then(trinket_static)
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        // [fast] Discard Mock Trinket: gain 1 resource.
        TRINKET => Some(vec![activated(
            0,
            vec![Cost::DiscardSelf],
            gain_resources(InvestigatorTarget::You, 1),
        )]),
        _ => None,
    }
}

fn install_mock_registry() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = game_core::card_registry::install(game_core::card_registry::CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
            native_effect_for: |_| None,
        });
    });
}

#[test]
fn discard_self_removes_source_from_play_and_runs_the_effect() {
    install_mock_registry();
    let id = InvestigatorId(1);
    let inst = CardInstanceId(0);
    let mut inv = test_investigator(1);
    let before = inv.resources;
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(TRINKET), inst));
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_investigator(inv)
        .build();

    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id: inst,
            ability_index: 0,
        }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv_after = &result.state.investigators[&id];
    assert!(
        inv_after.cards_in_play.is_empty(),
        "source asset left play",
    );
    assert_eq!(inv_after.discard, vec![CardCode::new(TRINKET)]);
    assert_eq!(inv_after.resources, before + 1, "the effect still ran");
    assert_event!(
        result.events,
        Event::CardDiscarded { from: game_core::state::Zone::InPlay, .. }
    );
}
```

- [ ] **Step 8: Run, gauntlet, commit**

Run: `cargo test -p game-core --test discard_self` → PASS. Then the four strict commands. Then:

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/dispatch/abilities.rs crates/game-core/src/engine/dispatch/reaction_windows.rs crates/game-core/tests/discard_self.rs
git commit -m "engine: Cost::DiscardSelf — discard the source asset in play as a cost (#301)"
```

---

### Task 2: `EnemyTarget` + `Effect::DealDamageToEnemy` + enemy choice

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (`EnemyTarget`, builder, `Effect::DealDamageToEnemy`)
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`enemies_in_scope`)
- Modify: `crates/game-core/src/engine/evaluator.rs` (`EvalContext.chosen_enemy`; `ground_chosen_targets` arm; `ground_enemy_choice`; `resolve_enemy_target`; the `DealDamageToEnemy` handler)
- Test: `crates/card-dsl/src/dsl.rs` (serde), `crates/game-core/src/engine/evaluator.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: `Choose<EntityScope>`, `EntityScope::At`, `LocationSet` (keystone); `combat::deal_damage_to_enemy(cx, EnemyId, u8, Option<InvestigatorId>)`.
- Produces: `enum EnemyTarget { Chosen(Choose<EntityScope>) }` + `EnemyTarget::chosen_at_your_location()`; `Effect::DealDamageToEnemy { target: EnemyTarget, amount: u8 }` + `deal_damage_to_enemy(target, amount) -> Effect`; `combat::enemies_in_scope(&GameState, InvestigatorId, EntityScope) -> Vec<EnemyId>`; `EvalContext.chosen_enemy: Option<EnemyId>`.

- [ ] **Step 1: Failing serde test** (append to `crates/card-dsl/src/dsl.rs` test module)

```rust
#[test]
fn deal_damage_to_enemy_serde_round_trips() {
    let e = deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1);
    assert_eq!(
        e,
        Effect::DealDamageToEnemy {
            target: EnemyTarget::Chosen(Choose {
                scope: EntityScope::At(LocationSet::Here)
            }),
            amount: 1,
        }
    );
    let json = serde_json::to_string(&e).unwrap();
    assert_eq!(serde_json::from_str::<Effect>(&json).unwrap(), e);
}
```

- [ ] **Step 2: Run — fails to compile**

Run: `cargo test -p card-dsl deal_damage_to_enemy_serde_round_trips`
Expected: FAIL — `EnemyTarget` / `Effect::DealDamageToEnemy` / builder absent.

- [ ] **Step 3: Add the DSL surface** (`crates/card-dsl/src/dsl.rs`)

After `impl LocationTarget { … }` (the targets section):

```rust
/// Single-enemy target spec. One variant today (`Chosen`); a non-chosen
/// form (`Engaged`, a specific spawned enemy) lands with its first consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EnemyTarget {
    /// The chooser picks one enemy from the [`Choose`]'s scope. Bound by
    /// `ground_chosen_targets` before the handler runs.
    Chosen(Choose<EntityScope>),
}

impl EnemyTarget {
    /// "Choose an enemy at your location."
    #[must_use]
    pub fn chosen_at_your_location() -> Self {
        EnemyTarget::Chosen(Choose {
            scope: EntityScope::At(LocationSet::Here),
        })
    }
}
```

In `enum Effect` (after `DealHorror`):

```rust
    /// Deal `amount` direct (non-test) damage to the resolved enemy
    /// `target`, applying the defeat cascade (Beat Cop 01018). Typed (not
    /// `Native`) so the activation pre-cost check can verify ≥1 candidate
    /// before any cost is paid. `amount == 0` is a no-op.
    DealDamageToEnemy { target: EnemyTarget, amount: u8 },
```

Builder, after `deal_damage`:

```rust
/// Build [`Effect::DealDamageToEnemy`].
#[must_use]
pub fn deal_damage_to_enemy(target: EnemyTarget, amount: u8) -> Effect {
    Effect::DealDamageToEnemy { target, amount }
}
```

Run: `cargo test -p card-dsl deal_damage_to_enemy_serde_round_trips` → PASS.

- [ ] **Step 4: `enemies_in_scope`** (`crates/game-core/src/engine/dispatch/combat.rs`)

```rust
/// Enemies matching an [`EntityScope`](crate::dsl::EntityScope), in `BTreeMap`
/// (id) order so the `OptionId` index replays deterministically. Shared by the
/// evaluator's grounding and the activation pre-cost target check.
pub(super) fn enemies_in_scope(
    state: &GameState,
    controller: InvestigatorId,
    scope: crate::dsl::EntityScope,
) -> Vec<EnemyId> {
    use crate::dsl::{EntityScope, LocationSet};
    let EntityScope::At(set) = scope;
    match set {
        LocationSet::Anywhere => state.enemies.keys().copied().collect(),
        LocationSet::Here => match state
            .investigators
            .get(&controller)
            .and_then(|i| i.current_location)
        {
            Some(here) => state
                .enemies
                .iter()
                .filter(|(_, e)| e.current_location == Some(here))
                .map(|(id, _)| *id)
                .collect(),
            None => Vec::new(),
        },
    }
}
```

(Add `use` for `GameState`, `InvestigatorId`, `EnemyId` if not already imported in combat.rs.)

- [ ] **Step 5: `EvalContext.chosen_enemy`** (`crates/game-core/src/engine/evaluator.rs`)

Add the field after `chosen_location` (line ~115):

```rust
    /// The enemy a controller picked for an `EnemyTarget::Chosen`. The enemy
    /// counterpart of `chosen_investigator` / `chosen_location`.
    pub chosen_enemy: Option<crate::state::EnemyId>,
```

Add `chosen_enemy: None,` to **both** `EvalContext` constructors (`for_controller` and `for_controller_with_source`, alongside the existing `chosen_investigator: None,` lines).

- [ ] **Step 6: Ground + resolve + handle** (`crates/game-core/src/engine/evaluator.rs`)

In `ground_chosen_targets`, after the `DiscoverClue` location block:

```rust
    if let Effect::DealDamageToEnemy {
        target: EnemyTarget::Chosen(choose),
        ..
    } = effect
    {
        if eval_ctx.chosen_enemy.is_none() {
            return ground_enemy_choice(cx, eval_ctx, cursor, choose.scope);
        }
    }
```

Add `ground_enemy_choice` (mirror of `ground_investigator_choice`, binding `chosen_enemy`, candidates from `combat::enemies_in_scope`):

```rust
fn ground_enemy_choice(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    cursor: &mut DecisionCursor<'_>,
    scope: crate::dsl::EntityScope,
) -> Result<EvalContext, EngineOutcome> {
    use crate::engine::dispatch::choice::{
        resolve_choice_count, suspend_for_choice, ChoiceResolution,
    };
    let candidates =
        crate::engine::dispatch::combat::enemies_in_scope(cx.state, eval_ctx.controller, scope);
    let bind = |id| {
        let mut ctx = eval_ctx;
        ctx.chosen_enemy = Some(id);
        Ok(ctx)
    };
    match resolve_choice_count(candidates.len()) {
        ChoiceResolution::Empty => Err(EngineOutcome::Rejected {
            reason: "Chosen enemy: no candidate in scope".into(),
        }),
        ChoiceResolution::Auto(i) => bind(candidates[i]),
        ChoiceResolution::Suspend => {
            if let Some(crate::engine::OptionId(i)) = cursor.take() {
                bind(candidates[i as usize])
            } else {
                let labels = candidates.iter().map(|id| format!("{id:?}")).collect();
                Err(suspend_for_choice(
                    cx,
                    "Choose an enemy",
                    labels,
                    cursor.recorded_so_far(),
                    cursor.root(),
                    eval_ctx,
                ))
            }
        }
    }
}
```

Add `resolve_enemy_target` (next to `resolve_location_target`):

```rust
fn resolve_enemy_target(
    ctx: EvalContext,
    target: EnemyTarget,
) -> Result<crate::state::EnemyId, &'static str> {
    match target {
        EnemyTarget::Chosen(_) => ctx.chosen_enemy.ok_or(
            "EnemyTarget::Chosen resolved before target-grounding bound it \
             (ground_chosen_targets should run first)",
        ),
    }
}
```

In the `apply_effect_inner` effect match (next to `Effect::DealDamage`), add the handler:

```rust
        Effect::DealDamageToEnemy { target, amount } => {
            if *amount == 0 {
                return EngineOutcome::Done;
            }
            let enemy = match resolve_enemy_target(eval_ctx, *target) {
                Ok(e) => e,
                Err(reason) => return EngineOutcome::Rejected { reason: reason.into() },
            };
            crate::engine::dispatch::combat::deal_damage_to_enemy(
                cx,
                enemy,
                *amount,
                Some(eval_ctx.controller),
            );
            EngineOutcome::Done
        }
```

(Import `EnemyTarget` in the evaluator's `use crate::dsl::{…}` block.)

- [ ] **Step 7: Failing behavior test** (append to evaluator `#[cfg(test)]` module; add `EnemyTarget` + `deal_damage_to_enemy` to the test-module `use crate::dsl::{…}`, and `test_enemy` to the `test_support` import)

```rust
#[test]
fn deal_damage_to_chosen_enemy_at_your_location_auto_binds_and_damages() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_location(test_location(1, "A"))
        .with_location(test_location(2, "B"))
        .with_enemy({
            let mut e = test_enemy(100, "Ghoul");
            e.max_health = 3;
            e.current_location = Some(LocationId(1));
            e
        })
        .with_enemy({
            let mut e = test_enemy(101, "Faraway");
            e.max_health = 3;
            e.current_location = Some(LocationId(2));
            e
        })
        .build();
    state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location = Some(LocationId(1));
    let mut events = Vec::new();
    let outcome = apply_effect(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.enemies[&EnemyId(100)].damage, 1, "co-located enemy damaged");
    assert_eq!(state.enemies[&EnemyId(101)].damage, 0, "faraway enemy untouched");
    assert!(state.continuations.is_empty(), "sole co-located candidate auto-binds");
}
```

(Add `EnemyId` to the test-module `use crate::state::{…}`.)

- [ ] **Step 8: Run → PASS** (the handler + grounding now exist)

Run: `cargo test -p game-core --lib deal_damage_to_chosen_enemy_at_your_location_auto_binds_and_damages` → PASS.

- [ ] **Step 9: Suspend + reject coverage** (append)

```rust
#[test]
fn deal_damage_to_chosen_enemy_suspends_when_two_are_co_located() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_location(test_location(1, "A"))
        .with_enemy({ let mut e = test_enemy(100, "G1"); e.current_location = Some(LocationId(1)); e })
        .with_enemy({ let mut e = test_enemy(101, "G2"); e.current_location = Some(LocationId(1)); e })
        .build();
    state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location = Some(LocationId(1));
    let mut events = Vec::new();
    let outcome = apply_effect(
        &mut Cx { state: &mut state, events: &mut events },
        &deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    match state.continuations.last() {
        Some(crate::state::Continuation::Choice(frame)) => {
            assert_eq!(frame.offered.len(), 2, "two co-located enemies offered");
        }
        other => panic!("expected a Choice frame, got {other:?}"),
    }
}

#[test]
fn deal_damage_to_chosen_enemy_rejects_when_none_co_located() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_location(test_location(1, "A"))
        .build();
    state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location = Some(LocationId(1));
    let mut events = Vec::new();
    let outcome = apply_effect(
        &mut Cx { state: &mut state, events: &mut events },
        &deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    assert!(state.continuations.is_empty());
}
```

Run: `cargo test -p game-core --lib deal_damage_to_chosen_enemy` → 3 PASS.

- [ ] **Step 10: Gauntlet + commit**

Run the four strict commands. Then:

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/dispatch/combat.rs crates/game-core/src/engine/evaluator.rs
git commit -m "engine: EnemyTarget + Effect::DealDamageToEnemy + enemy choice (#301)"
```

---

### Task 3: pre-cost target check + end-to-end (Beat Cop shape)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`check_activate_ability` — the `DealDamageToEnemy` pre-cost check)
- Test: `crates/game-core/tests/discard_self.rs` (extend with the Beat-Cop-shaped ability)

**Interfaces:**
- Consumes: `combat::enemies_in_scope`, `Effect::DealDamageToEnemy`, `EnemyTarget` (Task 2); `Cost::DiscardSelf` (Task 1).

- [ ] **Step 1: Add the pre-cost check** (`check_activate_ability`, next to the `effect_initiates_fight` block)

```rust
    // A DealDamageToEnemy ability needs ≥1 enemy in scope, validated here so the
    // activation rejects *before* any cost is paid (you can't discard the source
    // for no legal target). ≥1 proceeds; 2+ suspends via the Choose resolver.
    if let crate::dsl::Effect::DealDamageToEnemy {
        target: crate::dsl::EnemyTarget::Chosen(choose),
        ..
    } = &effect
    {
        if super::combat::enemies_in_scope(state, investigator, choose.scope).is_empty() {
            return Err(
                "ActivateAbility: a 'deal damage to an enemy at your location' ability \
                 needs at least one enemy at your location"
                    .into(),
            );
        }
    }
```

- [ ] **Step 2: Failing end-to-end test** (extend `crates/game-core/tests/discard_self.rs`)

Add a second mock card and abilities arm. Update `mock_metadata_for` to also map `COP`, add `COP` to `mock_abilities_for`, and add the card constant + metadata (mirror `TRINKET`, name "Mock Cop"):

```rust
const COP: &str = "MCOP1";
// (add COP metadata mirroring trinket_metadata with code/name COP, and a
//  matching `cop_static()` + extend `mock_metadata_for` to map COP.)

// in mock_abilities_for:
//   COP => Some(vec![activated(
//       0,
//       vec![Cost::DiscardSelf],
//       game_core::dsl::deal_damage_to_enemy(
//           game_core::dsl::EnemyTarget::chosen_at_your_location(), 1),
//   )]),
```

Then the tests:

```rust
fn board_with_cop(enemy_at_loc: bool) -> (game_core::GameState, InvestigatorId, CardInstanceId) {
    install_mock_registry();
    let id = InvestigatorId(1);
    let inst = CardInstanceId(0);
    let mut inv = test_investigator(1);
    inv.current_location = Some(game_core::state::LocationId(1));
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(COP), inst));
    let mut builder = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_location(game_core::test_support::test_location(1, "A"));
    if enemy_at_loc {
        let mut e = game_core::test_support::test_enemy(100, "Ghoul");
        e.max_health = 3;
        e.current_location = Some(game_core::state::LocationId(1));
        builder = builder.with_enemy(e);
    }
    let state = builder.with_investigator(inv).build();
    (state, id, inst)
}

#[test]
fn discard_self_deal_damage_rejects_with_no_enemy_and_keeps_source_in_play() {
    let (state, id, inst) = board_with_cop(false);
    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id: inst,
            ability_index: 0,
        }),
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(
        result.state.investigators[&id].cards_in_play.len(),
        1,
        "rejected before paying ⇒ source NOT discarded",
    );
}

#[test]
fn discard_self_deal_damage_discards_source_and_damages_the_enemy() {
    let (state, id, inst) = board_with_cop(true);
    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id: inst,
            ability_index: 0,
        }),
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert!(result.state.investigators[&id].cards_in_play.is_empty(), "source discarded");
    assert_eq!(result.state.enemies[&game_core::state::EnemyId(100)].damage, 1);
}
```

(Add `test_enemy`, `test_location` to the test's `test_support` import, or fully-qualify as above.)

- [ ] **Step 3: Run → PASS, gauntlet, commit**

Run: `cargo test -p game-core --test discard_self` → all PASS. Then the four strict commands. Then:

```bash
git add crates/game-core/src/engine/dispatch/reaction_windows.rs crates/game-core/tests/discard_self.rs
git commit -m "engine: pre-cost enemy-target check for DealDamageToEnemy + end-to-end (#301)"
```

---

### Task 4: phase-7 doc (final commit, after CI is green)

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md`

- [ ] **Step 1: Update the doc**

Add a Decisions entry (apply the "would a future PR-author choose differently without this?" test):

> **`Cost::DiscardSelf` discards the source asset in play (sole source-cost, paid last); `Effect::DealDamageToEnemy` is typed for a pre-cost target check (#301, PR #NN).** `DiscardSelf` removes the source from `cards_in_play` → owner's discard (`CardDiscarded { InPlay }`), reusing the defeat-discard path; combining it with `Exhaust`/`SpendUses` is a loud reject. The enemy variety ships as `EnemyTarget::Chosen(Choose<EntityScope>)`, reusing the keystone's `EntityScope::At`; `chosen_enemy` binds it; `combat::enemies_in_scope` is shared by the evaluator and the activation pre-cost check, which rejects 0-enemies-in-scope **before** paying (mirroring `effect_initiates_fight`) — the reason `DealDamageToEnemy` is typed, not `Native`. 2+ suspends via the Choose resolver. Beat Cop's content (PR-4 #239) and Knife's discard-self cost (PR-5 #312) are now unblocked.

Mark PR-2 done in the choice-cluster sub-slice note (it links the decomposition spec; update #301's row/status per `docs/phases/README.md`).

- [ ] **Step 2: Commit**

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: phase-7 — Cost::DiscardSelf + DealDamageToEnemy (#301)"
```

---

## Self-Review

**Spec coverage:** ① `Cost::DiscardSelf` (check/pay/combo-guard) → Task 1; ② enemy variety (`EnemyTarget`, `chosen_enemy`, `enemies_in_scope`, ground/resolve) → Task 2; ③ `Effect::DealDamageToEnemy` + handler → Task 2; pre-cost target check → Task 3; testing (serde, payment, enumeration auto/suspend/reject, end-to-end reject-before-pay) → Tasks 1–3; phase doc → Task 4. Deferred items (Engaged, combo, cards) are explicitly out of scope.

**Placeholder scan:** none — all code shown; the only `#NN` is the PR number filled at doc time. Task 3 Step 2's COP metadata says "mirror trinket_metadata" with the exact field list already given in Task 1 Step 7 — repeated structure, not a vague directive.

**Type consistency:** `EnemyTarget::Chosen(Choose<EntityScope>)`, `EnemyTarget::chosen_at_your_location()`, `Effect::DealDamageToEnemy { target, amount }`, `deal_damage_to_enemy(target, amount)`, `combat::enemies_in_scope(&GameState, InvestigatorId, EntityScope) -> Vec<EnemyId>`, `EvalContext.chosen_enemy: Option<EnemyId>`, `resolve_enemy_target(EvalContext, EnemyTarget) -> Result<EnemyId, &'static str>`, `combat::deal_damage_to_enemy(cx, EnemyId, u8, Option<InvestigatorId>)` — consistent across tasks. Enemy `current_location: Option<LocationId>` matches `state/enemy.rs`.
