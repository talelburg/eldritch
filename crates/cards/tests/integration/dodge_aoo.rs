//! End-to-end Dodge 01023 + Guard Dog 01021 against attacks of opportunity
//! (`AoO`), driven step by step through a `TestSession` against the real card
//! corpus.
//!
//! These are the registry-backed proofs that the mid-action park / resume
//! mechanism works end-to-end: when a basic action (Move) fires an `AoO`, the
//! `AoO` runs through `drive_attack_loop` so it opens the before-attack cancel
//! window (Dodge) and the per-soaked-asset reaction window (Guard Dog), the
//! engine suspends with `AwaitingInput`, and the action's primary effect
//! (the move) resumes correctly after `ResolveInput` closes the window.
//!
//! `game-core`'s unit tests can't reach `cards::REGISTRY` (the engine crate
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

use crate::common::attacker;
use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{CardCode, EnemyId, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, seat, BoardLookup as _, Seat, TestSession};
use game_core::{assert_event, assert_no_event};

/// Dodge (01023): Neutral Tactic, Fast, before-attack cancel reaction.
const DODGE: &str = "01023";

/// Guard Dog (01021): Guardian Ally, health 3 / sanity 1, damage-retaliate.
const GUARD_DOG: &str = "01021";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);
const HALLWAY: LocationId = LocationId(102);
const ATTACKER: EnemyId = EnemyId(7);

/// The Move every test takes: from the Study to the Hallway.
const MOVE_TO_HALLWAY: TurnAction = TurnAction::Move {
    investigator: INV,
    destination: HALLWAY,
};

/// Dodge as the cancel window offers it: a card in hand, by code.
fn dodge_in_hand(investigator: InvestigatorId) -> OptionTarget {
    OptionTarget::HandCardByCode {
        investigator,
        code: CardCode::new(DODGE),
    }
}

/// `investigator`'s turn at the Study, which connects to the Hallway, engaged
/// by a ready attacker with `attacker_health`.
fn move_board(investigator: Seat, attacker_health: u8) -> TestSession {
    let study = test_support::location(HERE).connections(vec![HALLWAY]);
    let hallway = test_support::location(HALLWAY).connections(vec![HERE]);

    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(study)
        .with_location(hallway)
        .with_seat(investigator.at(HERE))
        .open_turn(INV)
        .with_enemy(
            attacker(ATTACKER, 2, 0)
                .health(attacker_health)
                .engaged_with(INV),
        )
        .session()
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
///   pick Dodge → plays Dodge → cancel → the coordinator
///   abandons the attack's sequence at its resolve step → no damage → the
///   parked `AttackLoop` is re-exposed and drains (`AoO` source, so no
///   exhaust) → `drive` → `ActionResolution` on top →
///   `resume_action_resolution` → `move::perform` → the turn menu.
#[test]
fn dodge_cancels_attack_of_opportunity_no_damage_move_completes_attacker_not_exhausted() {
    // Step 1: take the Move — AoO fires; Dodge is in hand so the
    // BeforeEnemyAttack cancel window opens and suspends.
    let session = move_board(seat(1).hand([DODGE]), 3).take(&MOVE_TO_HALLWAY);
    let state = session.state();

    assert!(
        session.prompt().skippable,
        "BeforeEnemyAttack window must suspend the AoO loop: {:?}",
        session.prompt()
    );
    // No damage yet; move not yet completed.
    assert_eq!(state.investigators[&INV].damage(), 0);
    assert_eq!(
        state.investigators[&INV].current_location,
        Some(HERE),
        "move not yet resolved while the cancel window is open"
    );
    // Dodge still in hand; not yet played.
    assert!(
        state.investigators[&INV]
            .hand
            .contains(&CardCode::new(DODGE)),
        "Dodge is still in hand before the window is resolved"
    );

    // Step 2: play Dodge (the single offered candidate) — cancel the AoO.
    let session = session.pick(dodge_in_hand(INV));
    session.assert_at_turn_menu(INV);
    let result = session.finish();
    let state = result.state;

    // The AoO was cancelled: no damage/horror dealt.
    assert_eq!(
        state.investigators[&INV].damage(),
        0,
        "the cancelled AoO dealt no damage"
    );
    assert_eq!(
        state.investigators[&INV].horror(),
        0,
        "the cancelled AoO dealt no horror"
    );
    // A cancelled attack deals no damage.
    assert_no_event!(result.events, Event::DamagePlacedOnInvestigator { .. });

    // The attacker did NOT exhaust (AoO: RR p.7 — Dodge FAQ says the enemy
    // still exhausts for an Enemy-phase cancel, but AoO attackers are exempt
    // from exhaustion by RR p.7 regardless of whether the attack was cancelled).
    assert!(
        !state.enemies[&ATTACKER].exhausted,
        "AoO attacker never exhausts, even after a Dodge cancel (RR p.7)"
    );

    // The move completed after the cancel window closed: the ActionResolution
    // frame resumed and ran `move::perform`.
    assert_eq!(
        state.investigators[&INV].current_location,
        Some(HALLWAY),
        "move resolved: investigator reached the destination after Dodge cancelled the AoO"
    );
    assert_eq!(
        state.enemies[&ATTACKER].current_location,
        Some(HALLWAY),
        "engaged enemy moved with the investigator to the destination"
    );
    // InvestigatorMoved emitted after the cancel window closed.
    assert_event!(
        result.events,
        Event::InvestigatorMoved { investigator, to, .. }
            if *investigator == INV && *to == HALLWAY
    );

    // Dodge left hand and went to discard (a played Fast event).
    assert!(
        !state.investigators[&INV]
            .hand
            .contains(&CardCode::new(DODGE)),
        "Dodge left hand after being played"
    );
    assert!(
        state.investigators[&INV]
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
/// This is NOT the soak case (no Guard Dog, no soak window here since
/// the investigator has no soaking asset), but it verifies that a
/// `ResolveInput{Skip}` on a `BeforeEnemyAttack` window correctly resumes the
/// `ActionResolution` frame.
#[test]
fn skipping_before_attack_window_lets_aoo_land_and_move_still_completes() {
    // Step 1: Move → AoO → BeforeEnemyAttack window.
    let session = move_board(seat(1).hand([DODGE]), 5).take(&MOVE_TO_HALLWAY);
    assert!(session.prompt().skippable, "{:?}", session.prompt());

    // Step 2: skip the cancel window → AoO lands → move completes.
    let session = session.skip();
    session.assert_at_turn_menu(INV);
    let result = session.finish();
    let state = result.state;

    // The AoO landed: the un-cancelled AoO dealt its 2 damage.
    assert_event!(
        result.events,
        Event::DamagePlacedOnInvestigator { investigator, amount: 2 } if *investigator == INV
    );
    assert_eq!(state.investigators[&INV].damage(), 2);

    // Attacker never exhausts (AoO rule, RR p.7).
    assert!(
        !state.enemies[&ATTACKER].exhausted,
        "AoO attacker never exhausts (RR p.7)"
    );

    // Move still completed.
    assert_eq!(
        state.investigators[&INV].current_location,
        Some(HALLWAY),
        "move resolved after the skipped AoO cancel window"
    );
    assert_event!(
        result.events,
        Event::InvestigatorMoved { investigator, to, .. }
            if *investigator == INV && *to == HALLWAY
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
/// This is the canonical soak-on-`AoO` test: a soak window opened by an
/// `AoO` drives the full suspend/resume cycle, confirming that the
/// `ActionResolution` frame beneath the `AttackLoop` is correctly resumed by
/// `drive` once the window closes.
#[test]
fn guard_dog_retaliates_against_aoo_and_move_completes() {
    // Guard Dog in play, no Dodge in hand — so no before-attack cancel window.
    // Attacker deals 2 damage; Guard Dog (health 3) survives (2 < 3) and
    // retaliates. Max health 5 ensures the attacker survives the 1 retaliate.
    let session = move_board(seat(1).in_play([GUARD_DOG]), 5);
    let dog_id = session.state().in_play(INV, GUARD_DOG);
    let dog = OptionTarget::CardInstance(dog_id);

    // Step 1: Move → AoO → distribution prompt (Guard Dog has capacity).
    // Assign both AoO damage points onto Guard Dog → soak window opens.
    let session = session
        .take(&MOVE_TO_HALLWAY)
        .pick(dog.clone())
        .pick(dog.clone());
    let state = session.state();

    assert!(
        session.prompt().skippable,
        "Guard Dog's when-cell window must suspend the AoO loop: {:?}",
        session.prompt()
    );
    // The damage is assigned to Guard Dog and not yet placed — the open window
    // is the one the rules put between assigning and placing.
    let dog_in_play = state.investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == dog_id)
        .expect("Guard Dog still in play");
    assert_eq!(
        dog_in_play.accumulated_damage, 0,
        "AoO damage assigned to Guard Dog, not yet placed"
    );
    assert_eq!(state.investigators[&INV].damage(), 0);
    // Move not yet resolved (parked on ActionResolution).
    assert_eq!(
        state.investigators[&INV].current_location,
        Some(HERE),
        "move not yet resolved while the damage window is open"
    );
    // No retaliate damage yet.
    assert_eq!(state.enemies[&ATTACKER].damage, 0);

    // Step 2: fire Guard Dog's reaction (the single pending trigger).
    let session = session.pick(dog);
    session.assert_at_turn_menu(INV);
    let result = session.finish();
    let state = result.state;

    // Guard Dog dealt 1 retaliate damage to the AoO attacker.
    assert_eq!(
        state.enemies[&ATTACKER].damage, 1,
        "Guard Dog's reaction dealt 1 damage to the AoO attacker"
    );
    // …and only then did the assigned damage land on it.
    assert_eq!(
        state.investigators[&INV]
            .cards_in_play
            .iter()
            .find(|c| c.instance_id == dog_id)
            .map(|c| c.accumulated_damage),
        Some(2),
        "AoO damage placed on Guard Dog once its when cell has run"
    );
    assert_event!(
        result.events,
        Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == ATTACKER
    );

    // AoO attacker never exhausts (RR p.7).
    assert!(
        !state.enemies[&ATTACKER].exhausted,
        "AoO attacker does not exhaust even after Guard Dog retaliates (RR p.7)"
    );

    // Move completed: ActionResolution frame resumed after the window closed.
    assert_eq!(
        state.investigators[&INV].current_location,
        Some(HALLWAY),
        "move resolved: investigator reached the destination after Guard Dog retaliated"
    );
    assert_eq!(
        state.enemies[&ATTACKER].current_location,
        Some(HALLWAY),
        "engaged enemy moved with the investigator"
    );
    // InvestigatorMoved emitted after the damage window closed.
    assert_event!(
        result.events,
        Event::InvestigatorMoved { investigator, to, .. }
            if *investigator == INV && *to == HALLWAY
    );

    // No windows remain after the full cycle.
    assert!(
        state.open_windows().is_empty(),
        "no windows stranded after Guard Dog reaction + move resume: {:?}",
        state.open_windows()
    );
}
