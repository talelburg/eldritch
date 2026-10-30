//! Hospital Debts 01011: resources moved onto a card, twice per round, by
//! anyone standing with "Skids".
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01011):
//! > **Revelation** - Put Hospital Debts into play in your threat area.
//! > [fast]: Move 1 resource from your resource pool to Hospital Debts. (Limit
//! > twice per round.)
//! > **Forced** - When the game ends, if Hospital Debts has fewer than 6
//! > resources on it: You earn 2 fewer experience for this scenario.
//!
//! Ruling (<https://arkhamdb.com/card/01011>):
//! > Any investigator at the same location as "Skids" O'Toole with Hospital
//! > Debts in his threat area may trigger the [lighting] to add resources to
//! > Hospital Debts, as per the FAQ [V1.0, section 2.1].
//!
//! The limit is per investigator: `glossary/Limits_and_Maximums.md`, *"Unless
//! stated otherwise, limits are player specific."*, and The Stranger's ruling
//! (<https://arkhamdb.com/card/03047a>): *"Limit once per round is
//! player-specific, so another investigator will trigger the ability even if
//! you have already triggered it this round."*
//!
//! The Forced experience clause is not modelled, so it is untested here.
//! TODO(#1062): earn 2 fewer experience below 6 resources.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, GameState, GameStateBuilder, InvestigatorId, LocationId,
    MythosPhaseFrame, MythosResume, Phase,
};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};
use game_core::{assert_event_sequence, assert_no_event};

const HOSPITAL_DEBTS: &str = "01011";

/// The bearer, standing in for "Skids".
const SKIDS: InvestigatorId = InvestigatorId(1);
/// A second investigator.
const FRIEND: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
const THERE: LocationId = LocationId(11);

/// Skids at `HERE` with Hospital Debts in his threat area and 5 resources, and
/// `FRIEND` at `friend_at` with 5 resources; seating Skids then the friend,
/// with `active`'s turn open.
fn board(friend_at: LocationId, active: InvestigatorId) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HERE))
        .with_location(test_support::location(THERE))
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .resources(5)
                .threat_area([HOSPITAL_DEBTS]),
        )
        .with_seat(seat(2).at(friend_at).actions(3).resources(5))
        .with_seating([SKIDS, FRIEND])
        .open_turn(active)
        .session()
}

/// `investigator` triggering Hospital Debts' `[fast]` ability.
fn trigger(state: &GameState, investigator: InvestigatorId) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator,
        source: AbilitySource::InPlay(state.in_play(SKIDS, HOSPITAL_DEBTS)),
        // Printed index 1: index 0 is the Revelation.
        address: AbilityAddress::Printed(1),
    }
}

/// Take the trigger off `investigator`'s turn menu.
fn pay_in(session: TestSession, investigator: InvestigatorId) -> TestSession {
    let action = trigger(session.state(), investigator);
    session.take(&action)
}

/// Resources sitting on Hospital Debts.
fn paid(state: &GameState) -> u8 {
    state.investigators[&SKIDS]
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == HOSPITAL_DEBTS)
        .expect("Hospital Debts is in Skids' threat area")
        .resources
}

/// Whether `investigator`'s turn menu offers the trigger.
fn offered(state: &GameState, investigator: InvestigatorId) -> bool {
    enumerate::legal_actions(state, TEST_REGISTRIES.cards).contains(&trigger(state, investigator))
}

/// Assert `investigator`'s third trigger is refused for the usage limit: not
/// offered, and rejected when submitted anyway.
fn assert_third_refused(session: TestSession, investigator: InvestigatorId) {
    assert!(
        !offered(session.state(), investigator),
        "a third trigger by {investigator:?} must not be offered"
    );
    let action = trigger(session.state(), investigator);
    let reason = session.assert_rejects(|s| s.submit_off_menu(&action));
    assert!(
        reason.contains("usage limit"),
        "refused for the limit, got: {reason}"
    );
}

/// *"Revelation - Put Hospital Debts into play in your threat area."* Drawn at
/// the Mythos encounter draw, it lands in the drawer's threat area with nothing
/// on it, and is not discarded.
#[test]
fn revelation_puts_it_into_the_drawers_threat_area() {
    let drawing = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE))
        .with_location(test_support::location(HERE))
        .with_seating([SKIDS])
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([SKIDS])
        .with_encounter_deck([HOSPITAL_DEBTS])
        .session();
    drawing.assert_at_encounter_draw();

    // Drawing it reveals it. Once in play, its `[fast]` ability is offered at
    // the next player window, which is where the session rests.
    let window = drawing.confirm();
    assert_eq!(
        window.offered_at(&window.state().card(SKIDS, HOSPITAL_DEBTS)),
        1,
        "the zero-action ability is offered in the window as soon as the card is in play"
    );

    let state = window.state();
    assert_eq!(paid(state), 0, "enters with no resources on it");
    assert!(
        !state
            .encounter_discard
            .iter()
            .any(|c| c.as_str() == HOSPITAL_DEBTS),
        "it stays in play rather than being discarded after its revelation"
    );
}

/// *"\[fast\]: Move 1 resource from your resource pool to Hospital Debts."* One
/// resource leaves Skids' pool and sits on the card, for no action — announced
/// as a move, neither a gain nor a payment. The official FAQ sets moving apart
/// from gaining: abilities that *"“move” resources from one card to another or
/// from one card to your resource pool … do not trigger Stylish Coat’s
/// ability"*, which answers only an effect that says "gain".
#[test]
fn triggering_it_moves_one_resource_from_your_pool_onto_it() {
    let session = pay_in(board(HERE, SKIDS), SKIDS);
    session.assert_at_turn_menu(SKIDS);

    let state = session.state();
    assert_eq!(paid(state), 1, "the resource is on Hospital Debts");
    assert_eq!(state.investigators[&SKIDS].resources, 4, "it left the pool");
    assert_eq!(
        state.investigators[&SKIDS].actions_remaining, 3,
        "a [fast] ability costs no action"
    );

    let debts = state.in_play(SKIDS, HOSPITAL_DEBTS);
    assert_event_sequence!(
        session.events(),
        Event::ResourcesMovedToCard {
            investigator: SKIDS,
            instance_id,
            amount: 1,
        } if *instance_id == debts,
    );
    assert_no_event!(session.events(), Event::ResourcesGained { .. });
    assert_no_event!(session.events(), Event::ResourcesPaid { .. });
}

/// *"(Limit twice per round.)"* Two triggers land; the third is refused.
#[test]
fn it_triggers_twice_per_round_and_the_third_is_refused() {
    let session = pay_in(pay_in(board(HERE, SKIDS), SKIDS), SKIDS);
    assert_eq!(paid(session.state()), 2);
    assert_eq!(session.state().investigators[&SKIDS].resources, 3);

    assert_third_refused(session, SKIDS);
}

/// The ruling: *"Any investigator at the same location as "Skids" O'Toole with
/// Hospital Debts in his threat area may trigger the [lighting]"*. "Your
/// resource pool" is the triggering investigator's, so the friend pays it.
#[test]
fn a_colocated_investigator_triggers_it_from_their_own_pool() {
    let session = pay_in(board(HERE, FRIEND), FRIEND);
    session.assert_at_turn_menu(FRIEND);

    let state = session.state();
    assert_eq!(paid(state), 1, "the resource is on Skids' Hospital Debts");
    assert_eq!(
        state.investigators[&FRIEND].resources, 4,
        "the friend paid it"
    );
    assert_eq!(
        state.investigators[&SKIDS].resources, 5,
        "Skids paid nothing"
    );
}

/// An investigator somewhere else cannot reach Skids' threat area.
#[test]
fn an_investigator_at_another_location_cannot_trigger_it() {
    let session = board(THERE, FRIEND);
    assert!(
        !offered(session.state(), FRIEND),
        "Hospital Debts must not be offered to an investigator at another location"
    );
    let action = trigger(session.state(), FRIEND);
    session.assert_rejects(|s| s.submit_off_menu(&action));
}

/// With an empty pool there is no resource to move, so the ability cannot
/// change the game state and is not offered.
#[test]
fn it_is_not_offered_with_an_empty_resource_pool() {
    let session = board(HERE, SKIDS).edit_state(|state| {
        state.investigators.get_mut(&SKIDS).unwrap().resources = 0;
    });
    assert!(
        !offered(session.state(), SKIDS),
        "nothing to move, so nothing to offer"
    );
}

/// The limit is player-specific (The Stranger's ruling, above): after Skids has
/// used both of his this round, the friend still gets two of their own — and
/// then theirs are spent too.
#[test]
fn each_investigator_gets_their_own_two_triggers_per_round() {
    let skids_done = pay_in(pay_in(board(HERE, SKIDS), SKIDS), SKIDS);
    let round = skids_done.state().round;
    // Skids' third is refused for the limit before his turn ends.
    assert!(
        !offered(skids_done.state(), SKIDS),
        "Skids' third trigger must not be offered"
    );

    // The friend's turn opens with a fast window offering it; they pass it, to
    // take it from their turn menu instead.
    let friends_turn = skids_done.take(&TurnAction::EndTurn).skip();
    friends_turn.assert_at_turn_menu(FRIEND);
    assert_eq!(friends_turn.state().round, round, "still the same round");
    assert!(
        offered(friends_turn.state(), FRIEND),
        "Skids' two triggers do not count against the friend's limit"
    );

    let friend_done = pay_in(pay_in(friends_turn, FRIEND), FRIEND);
    assert_eq!(paid(friend_done.state()), 4, "two from each");
    assert_eq!(friend_done.state().investigators[&FRIEND].resources, 3);

    assert_third_refused(friend_done, FRIEND);
}
