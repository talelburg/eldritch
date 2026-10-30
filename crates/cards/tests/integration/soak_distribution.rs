//! The defending player distributes an enemy attack's damage
//! across themselves and eligible soakers, one point at a time (RR p.7),
//! driven through the real `apply` enemy-phase path against the corpus registry.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::state::{CardInstanceId, GameState, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{enemy, location, seat, SeatLookup as _, TestSession};

const GUARD_DOG: &str = "01021"; // Ally, 3 health / 1 sanity, retaliate reaction

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);

/// Investigation-phase state: `INV`'s open turn controlling a Guard Dog, with
/// one ready enemy engaged, dealing 2 damage and 0 horror. `EndTurn` advances
/// into the Enemy phase and runs the attack.
fn attack_state() -> GameState {
    // Non-empty deck so the post-attack Upkeep draw doesn't trigger the
    // draw-from-empty horror penalty (which soaks onto a sanity asset
    // and would muddy these damage-only tests).
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(seat(1).at(HERE).in_play([GUARD_DOG]).deck([GUARD_DOG; 5]))
        .with_enemy_engaged(enemy(7).health(5).attack_damage(2).attack_horror(0), INV)
        .open_turn(INV)
        .build()
}

/// The Guard Dog's instance id on `state`.
fn dog(state: &GameState) -> CardInstanceId {
    state.in_play(INV, GUARD_DOG)
}

/// True iff the session rests at the interactive soak-distribution per-point
/// prompt, as opposed to Guard Dog's reaction window, which also offers Guard
/// Dog: the distribution must be answered, the window may be passed.
fn at_distribution_prompt(session: &TestSession) -> bool {
    !session.prompt().skippable
}

fn guard_dog_damage(state: &GameState, inst: CardInstanceId) -> Option<u8> {
    state.investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == inst)
        .map(|c| c.accumulated_damage)
}

#[test]
fn two_damage_attack_splits_one_to_guard_dog_one_to_self() {
    let state = attack_state();
    let dog = dog(&state);
    let me = state.investigators[&INV].card_anchor();

    // EndTurn → enemy phase → distribution prompt (Guard Dog has capacity).
    // First point → Guard Dog; still contested → second prompt → self.
    let session = TestSession::new(state, TEST_REGISTRIES)
        .take(&TurnAction::EndTurn)
        .pick(OptionTarget::CardInstance(dog))
        .pick(me.clone());

    // The distribution is complete but nothing is placed: Guard Dog is in the
    // assignment, so its `when` cell opens between the two rules steps,
    // and that is the window here rather than a further distribution prompt.
    assert!(
        !at_distribution_prompt(&session),
        "Guard Dog's when-cell window opens once distribution drains: {:?}",
        session.prompt()
    );
    assert_eq!(
        guard_dog_damage(session.state(), dog),
        Some(0),
        "assigned to Guard Dog, not yet placed"
    );
    assert_eq!(
        session.state().investigators[&INV].damage(),
        0,
        "and none of it placed on the investigator yet either"
    );

    // Firing the retaliate lets the deal reach its placement — and the whole
    // assignment lands at once (RR p.7 "simultaneously").
    let session = session.pick(OptionTarget::CardInstance(dog));
    assert_eq!(
        guard_dog_damage(session.state(), dog),
        Some(1),
        "1 damage placed on Guard Dog"
    );
    assert_eq!(
        session.state().investigators[&INV].damage(),
        1,
        "1 damage placed on the investigator, in the same moment"
    );
}

#[test]
fn player_may_decline_to_soak_taking_all_damage() {
    let state = attack_state();
    let dog = dog(&state);
    let me = state.investigators[&INV].card_anchor();

    // Both points to the investigator — decline to soak.
    let session = TestSession::new(state, TEST_REGISTRIES)
        .take(&TurnAction::EndTurn)
        .pick(me.clone())
        .pick(me);

    assert_eq!(
        session.state().investigators[&INV].damage(),
        2,
        "investigator took all 2 damage"
    );
    assert_eq!(
        guard_dog_damage(session.state(), dog),
        Some(0),
        "Guard Dog untouched (declined to soak)"
    );
}

#[test]
fn a_full_soaker_drops_out_of_the_next_prompt() {
    let mut state = attack_state();
    let dog = dog(&state);
    // Pre-damage Guard Dog to 2 (health 3) → 1 remaining capacity.
    state.investigators.get_mut(&INV).unwrap().cards_in_play[0].accumulated_damage = 2;

    // First point → Guard Dog (its last point of capacity).
    let session = TestSession::new(state, TEST_REGISTRIES)
        .take(&TurnAction::EndTurn)
        .pick(OptionTarget::CardInstance(dog));

    // Guard Dog is now full, so the second point is auto-assigned to the
    // investigator with NO further distribution prompt. What is open instead is
    // Guard Dog's `when` cell on the completed assignment — a lethal one,
    // which it is still entitled to react to (`data/arkhamdb-faq/core/01021.md`).
    assert!(
        !at_distribution_prompt(&session),
        "the full soaker drops out — no second distribution prompt: {:?}",
        session.prompt()
    );
    let session = session.pick(OptionTarget::CardInstance(dog));
    assert_eq!(
        session.state().investigators[&INV].damage(),
        1,
        "the overflow point went to the investigator"
    );
    assert!(
        guard_dog_damage(session.state(), dog).is_none(),
        "Guard Dog filled to capacity is defeated and discarded",
    );
}
