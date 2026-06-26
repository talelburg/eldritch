//! End-to-end Dodge 01023 + Guard Dog 01021 against attacks of opportunity
//! (`AoO`), driven through the public [`apply`] API with the real card corpus
//! installed (#293 acceptance — K1 keystone integration test).
//!
//! These are the registry-backed proofs that the mid-action park / resume
//! mechanism works end-to-end: when a basic action (Move) fires an `AoO`, the
//! `AoO` runs through `drive_attack_loop` so it opens the before-attack cancel
//! window (Dodge) and the per-soaked-asset reaction window (Guard Dog), the
//! engine suspends with `AwaitingInput`, and the action's primary effect
//! (the move) resumes correctly after `ResolveInput` closes the window.
//!
//! `game-core`'s unit tests can't install `cards::REGISTRY` (the engine crate
//! can't depend on `cards`), so this lives in `crates/cards/tests/`.
//!
//! ## Verified card text (`ArkhamDB`, 2026-06-21)
//!
//! **Dodge (01023):** "Fast. Play when an enemy attacks an investigator at
//! your location. Cancel that attack."
//! FAQ:
//!   - Dodge works against any attack type: Enemy phase attacks, attacks of
//!     opportunity, and Retaliate abilities.
//!   - When an attack is cancelled in the Enemy phase, the attacking enemy
//!     still exhausts. (For `AoO`: `AoO` attackers never exhaust per RR p.7,
//!     so Dodge + `AoO` = no damage, no exhaust — both rules apply.)
//!   - Against Massive enemies, Dodge cancels only a single attack.
//!
//! **Guard Dog (01021):** "[reaction] When an enemy attack deals damage to
//! Guard Dog: Deal 1 damage to the attacking enemy." Health 3, Sanity 1.
//! FAQ: "You can use Guard Dog's ability when you assign lethal damage/
//! horror to it." The trigger is 'when an enemy attack deals damage' with
//! no `AoO` carve-out — Guard Dog retaliates against `AoO` attacks.
#![allow(clippy::too_many_lines)]

use game_core::engine::{apply, EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, Enemy, EnemyId, InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{take_turn_action, test_enemy, test_investigator, test_location};
use game_core::{Action, InputResponse, PlayerAction, TurnAction};

/// Dodge (01023): Neutral Tactic, Fast, before-attack cancel reaction.
const DODGE: &str = "01023";

/// Guard Dog (01021): Guardian Ally, health 3 / sanity 1, damage-retaliate.
const GUARD_DOG: &str = "01021";

#[ctor::ctor]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// The soak-distribution `PickSingle` `OptionId` for the soaker asset (#44/K5b —
/// an `AoO` against an investigator with a soaker prompts for the damage
/// distribution before placing it).
fn pick_soaker(outcome: &EngineOutcome) -> OptionId {
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("expected a distribution prompt, got {outcome:?}");
    };
    request
        .options
        .iter()
        .find(|o| o.label.contains("Asset"))
        .unwrap_or_else(|| panic!("no soaker option in {:?}", request.options))
        .id
}

/// An engaged ready enemy at `loc` dealing `damage` / 0 horror with `max_health`.
/// `AoO` attackers are ready (not exhausted) and engaged; `max_health` lets
/// callers ensure the attacker survives a Guard Dog retaliation.
fn engaged_attacker(
    id: u32,
    inv: InvestigatorId,
    loc: LocationId,
    damage: u8,
    max_health: u8,
) -> Enemy {
    let mut e = test_enemy(id, format!("Attacker {id}"));
    e.attack_damage = damage;
    e.attack_horror = 0;
    e.max_health = max_health;
    e.current_location = Some(loc);
    e.engaged_with = Some(inv);
    e
}

// -----------------------------------------------------------------------
// Test 1 — Dodge cancels an AoO; the move completes; attacker not exhausted
// -----------------------------------------------------------------------

/// An investigator with Dodge in hand, engaged by a ready enemy, takes a
/// Move action. The `AoO` opens the `BeforeEnemyAttack` cancel window; the
/// player plays Dodge to cancel; the attack deals no damage/horror; the
/// move then completes (`ActionResolution` frame resumes).
///
/// Verifies the end-to-end suspend/resume chain:
///   Move → push `ActionResolution` → `drive_aoo` → `drive_attack_loop` →
///   `BeforeEnemyAttack` window opens → `AwaitingInput` →
///   `ResolveInput{PickSingle(0)}` → plays Dodge → cancel →
///   `resume_enemy_attack` (`BeforeAttack` arm, `cancelled=true`) → no damage →
///   loop `Done` (`AoO` source) → `drive` → `ActionResolution` on top →
///   `resume_action_resolution` → `move_primary_effect` → `Done`.
#[test]
fn dodge_cancels_attack_of_opportunity_no_damage_move_completes_attacker_not_exhausted() {
    let inv_id = InvestigatorId(1);
    let from = LocationId(101);
    let dest = LocationId(102);
    let enemy_id = EnemyId(7);

    let mut study = test_location(101, "Study");
    study.connections = vec![dest];
    let mut hallway = test_location(102, "Hallway");
    hallway.connections = vec![from];

    let mut investigator = test_investigator(1);
    investigator.current_location = Some(from);
    // Use a real investigator code so max_health()/max_sanity() can read from
    // the installed cards registry (#448 cp2a). Skids O'Toole (01003, 8/6).
    investigator.investigator_card.code = CardCode::new("01003");
    investigator.hand = vec![CardCode::new(DODGE)];

    let attacker = engaged_attacker(7, inv_id, from, 2, 3);

    let state = game_core::test_support::GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(study)
        .with_location(hallway)
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_enemy(attacker)
        .build();

    // Step 1: take the Move — AoO fires; Dodge is in hand so the
    // BeforeEnemyAttack cancel window opens and suspends.
    let result = take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: dest,
        },
    );
    let mut state = result.state;

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "BeforeEnemyAttack window must suspend the AoO loop: {:?}",
        result.outcome
    );
    // No damage yet; move not yet completed.
    assert_eq!(state.investigators[&inv_id].damage(), 0);
    assert_eq!(
        state.investigators[&inv_id].current_location,
        Some(from),
        "move not yet resolved while the cancel window is open"
    );
    // Dodge still in hand; not yet played.
    assert!(
        state.investigators[&inv_id]
            .hand
            .contains(&CardCode::new(DODGE)),
        "Dodge is still in hand before the window is resolved"
    );

    // Step 2: play Dodge (the single offered candidate) — cancel the AoO.
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    state = result.state;

    // The AoO was cancelled: no damage/horror dealt.
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "the cancelled AoO dealt no damage"
    );
    assert_eq!(
        state.investigators[&inv_id].horror(),
        0,
        "the cancelled AoO dealt no horror"
    );
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, Event::DamageTaken { .. })),
        "a cancelled attack deals no damage: {:?}",
        result.events
    );

    // The attacker did NOT exhaust (AoO: RR p.7 — Dodge FAQ says the enemy
    // still exhausts for an Enemy-phase cancel, but AoO attackers are exempt
    // from exhaustion by RR p.7 regardless of whether the attack was cancelled).
    assert!(
        !state.enemies[&enemy_id].exhausted,
        "AoO attacker never exhausts, even after a Dodge cancel (RR p.7)"
    );

    // The move completed after the cancel window closed: the ActionResolution
    // frame resumed and ran move_primary_effect.
    assert_eq!(
        state.investigators[&inv_id].current_location,
        Some(dest),
        "move resolved: investigator reached the destination after Dodge cancelled the AoO"
    );
    assert_eq!(
        state.enemies[&enemy_id].current_location,
        Some(dest),
        "engaged enemy moved with the investigator to the destination"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::InvestigatorMoved { investigator, to, .. }
                if *investigator == inv_id && *to == dest
        )),
        "InvestigatorMoved emitted after the cancel window closed: {:?}",
        result.events
    );

    // Dodge left hand and went to discard (a played Fast event).
    assert!(
        !state.investigators[&inv_id]
            .hand
            .contains(&CardCode::new(DODGE)),
        "Dodge left hand after being played"
    );
    assert!(
        state.investigators[&inv_id]
            .discard
            .contains(&CardCode::new(DODGE)),
        "Dodge is in the discard pile after being played"
    );

    // No windows stranded after the full cycle.
    assert!(
        state.open_windows().is_empty(),
        "no windows stranded after Dodge cancel + move resume: {:?}",
        state.open_windows()
    );
}

// -----------------------------------------------------------------------
// Test 2 — Skip the cancel window: AoO lands, move still completes
// -----------------------------------------------------------------------

/// Confirms the resume path works even when the before-attack window is
/// explicitly skipped (not played): the attack lands normally, and the move
/// still completes after the `AoO` loop finishes.
///
/// This is NOT the primary #293 case (no Guard Dog, no soak window here since
/// the investigator has no soaking asset), but it verifies that a
/// `ResolveInput{Skip}` on a `BeforeEnemyAttack` window correctly resumes the
/// `ActionResolution` frame.
#[test]
fn skipping_before_attack_window_lets_aoo_land_and_move_still_completes() {
    let inv_id = InvestigatorId(1);
    let from = LocationId(101);
    let dest = LocationId(102);
    let enemy_id = EnemyId(7);

    let mut study = test_location(101, "Study");
    study.connections = vec![dest];
    let mut hallway = test_location(102, "Hallway");
    hallway.connections = vec![from];

    let mut investigator = test_investigator(1);
    investigator.current_location = Some(from);
    // Use a real investigator code so max_health()/max_sanity() can read from
    // the installed cards registry (#448 cp2a). Skids O'Toole (01003, 8/6).
    investigator.investigator_card.code = CardCode::new("01003");
    investigator.hand = vec![CardCode::new(DODGE)];

    let attacker = engaged_attacker(7, inv_id, from, 2, 5);

    let state = game_core::test_support::GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(study)
        .with_location(hallway)
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_enemy(attacker)
        .build();

    // Step 1: Move → AoO → BeforeEnemyAttack window.
    let result = take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: dest,
        },
    );
    let mut state = result.state;
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    // Step 2: skip the cancel window → AoO lands → move completes.
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );
    state = result.state;

    // The AoO landed: investigator took 2 damage.
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::DamageTaken { investigator, amount: 2 } if *investigator == inv_id
        )),
        "the un-cancelled AoO dealt its 2 damage: {:?}",
        result.events
    );
    assert_eq!(state.investigators[&inv_id].damage(), 2);

    // Attacker never exhausts (AoO rule, RR p.7).
    assert!(
        !state.enemies[&enemy_id].exhausted,
        "AoO attacker never exhausts (RR p.7)"
    );

    // Move still completed.
    assert_eq!(
        state.investigators[&inv_id].current_location,
        Some(dest),
        "move resolved after the skipped AoO cancel window"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::InvestigatorMoved { investigator, to, .. }
                if *investigator == inv_id && *to == dest
        )),
        "InvestigatorMoved emitted: {:?}",
        result.events
    );
}

// -----------------------------------------------------------------------
// Test 3 — Guard Dog retaliates against an AoO; move completes after resume
// -----------------------------------------------------------------------

/// An investigator controlling Guard Dog in play, engaged by a ready enemy,
/// takes a Move action. The `AoO` has no before-attack cancel reaction (no Dodge
/// in hand), so the `AoO` damage resolves directly, soaking onto Guard Dog.
/// The soak window opens; the player fires Guard Dog's reaction; the attacker
/// takes 1 damage; the `ActionResolution` frame then resumes and the move
/// completes.
///
/// This is the canonical K1/#293 acceptance test: a soak window opened by an
/// `AoO` drives the full suspend/resume cycle, confirming that the
/// `ActionResolution` frame beneath the `AttackLoop` is correctly resumed by
/// `drive` once the window closes.
#[test]
fn guard_dog_retaliates_against_aoo_and_move_completes() {
    let dog = CardInstanceId(1);
    let inv_id = InvestigatorId(1);
    let from = LocationId(101);
    let dest = LocationId(102);
    let enemy_id = EnemyId(7);

    let mut study = test_location(101, "Study");
    study.connections = vec![dest];
    let mut hallway = test_location(102, "Hallway");
    hallway.connections = vec![from];

    let mut investigator = test_investigator(1);
    investigator.current_location = Some(from);
    // Guard Dog in play, no Dodge in hand — so no before-attack cancel window.
    investigator.cards_in_play = vec![CardInPlay::enter_play(CardCode::new(GUARD_DOG), dog)];

    // Attacker deals 2 damage; Guard Dog (health 3) survives (2 < 3) and
    // retaliates. Max health 5 ensures the attacker survives the 1 retaliate.
    let attacker = engaged_attacker(7, inv_id, from, 2, 5);

    let state = game_core::test_support::GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(study)
        .with_location(hallway)
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_enemy(attacker)
        .build();

    // Step 1: Move → AoO → distribution prompt (Guard Dog has capacity, #44/K5b).
    // Assign both AoO damage points onto Guard Dog → soak window opens.
    let result = take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: dest,
        },
    );
    let result = apply(
        result.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(pick_soaker(&result.outcome)),
        }),
    );
    let result = apply(
        result.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(pick_soaker(&result.outcome)),
        }),
    );
    let mut state = result.state;

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "Guard Dog soak window must suspend the AoO loop: {:?}",
        result.outcome
    );
    // Guard Dog soaked the damage; investigator took none.
    let dog_in_play = state.investigators[&inv_id]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == dog)
        .expect("Guard Dog still in play");
    assert_eq!(
        dog_in_play.accumulated_damage, 2,
        "AoO damage soaked onto Guard Dog"
    );
    assert_eq!(state.investigators[&inv_id].damage(), 0);
    // Move not yet resolved (parked on ActionResolution).
    assert_eq!(
        state.investigators[&inv_id].current_location,
        Some(from),
        "move not yet resolved while soak window is open"
    );
    // No retaliate damage yet.
    assert_eq!(state.enemies[&enemy_id].damage, 0);

    // Step 2: fire Guard Dog's reaction (the single pending trigger).
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    state = result.state;

    // Guard Dog dealt 1 retaliate damage to the AoO attacker.
    assert_eq!(
        state.enemies[&enemy_id].damage, 1,
        "Guard Dog's reaction dealt 1 damage to the AoO attacker"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == enemy_id
        )),
        "EnemyDamaged {{ amount: 1 }} emitted: {:?}",
        result.events
    );

    // AoO attacker never exhausts (RR p.7).
    assert!(
        !state.enemies[&enemy_id].exhausted,
        "AoO attacker does not exhaust even after Guard Dog retaliates (RR p.7)"
    );

    // Move completed: ActionResolution frame resumed after the window closed.
    assert_eq!(
        state.investigators[&inv_id].current_location,
        Some(dest),
        "move resolved: investigator reached the destination after Guard Dog retaliated"
    );
    assert_eq!(
        state.enemies[&enemy_id].current_location,
        Some(dest),
        "engaged enemy moved with the investigator"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::InvestigatorMoved { investigator, to, .. }
                if *investigator == inv_id && *to == dest
        )),
        "InvestigatorMoved emitted after the soak window closed: {:?}",
        result.events
    );

    // No windows remain after the full cycle.
    assert!(
        state.open_windows().is_empty(),
        "no windows stranded after Guard Dog reaction + move resume: {:?}",
        state.open_windows()
    );
}
