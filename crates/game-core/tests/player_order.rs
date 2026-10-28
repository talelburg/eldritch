//! Player order starts at the lead investigator, not at the first seat.
//!
//! `glossary/In_Player_Order.md`: *"If the players are instructed to perform a
//! sequence "in player order," the lead investigator performs his or her part
//! of the sequence first, followed by the other players in clockwise order."*
//!
//! Each test seats two investigators `[1, 2]` and makes seat 2 the lead, so
//! player order is seat 2, then seat 1. Each asserts both who goes first and
//! who goes second, since a walk that starts at the lead but stops at the end
//! of the seating would leave seat 1 out.

use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, Phase};
use game_core::test_support::{self, seat, TestSession, TEST_REGISTRIES, TEST_TREACHERY};

const FIRST_SEAT: InvestigatorId = InvestigatorId(1);
const LEAD: InvestigatorId = InvestigatorId(2);

/// Two seats `[1, 2]` with seat 2 the lead, each with a card to draw at
/// Upkeep 4.4, and two blank treacheries for Mythos 1.4 to draw.
fn lead_on_the_second_seat() -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).deck([test_support::TEST_ASSET]))
        .with_seat(seat(2).deck([test_support::TEST_ASSET]))
        .with_turn_order([FIRST_SEAT, LEAD])
        .with_lead(LEAD)
}

/// Settle `board`, with the two treacheries on the encounter deck (no builder
/// setter stages it).
fn session(board: GameStateBuilder) -> TestSession {
    let mut state = board.build();
    state.encounter_deck = [TEST_TREACHERY, TEST_TREACHERY]
        .into_iter()
        .map(CardCode::new)
        .collect();
    TestSession::new(state, TEST_REGISTRIES)
}

/// The investigators an event names, in the order the events fired, for the
/// events `select` picks out.
fn in_order(
    events: &[Event],
    select: impl Fn(&Event) -> Option<InvestigatorId>,
) -> Vec<InvestigatorId> {
    events.iter().filter_map(select).collect()
}

/// Appendix II 1.4: *"In player order, each investigator draws the top card of
/// the encounter deck…"*
#[test]
fn mythos_draws_start_at_the_lead() {
    let session = session(lead_on_the_second_seat().ending_upkeep_phase());
    session.assert_at_encounter_draw();
    assert_eq!(session.state().current_encounter_drawer(), Some(LEAD));
    let session = session.confirm();
    session.assert_at_encounter_draw();
    assert_eq!(session.state().current_encounter_drawer(), Some(FIRST_SEAT));
}

/// Investigation 2.2 walks player order as a stand-in for the investigators'
/// choice of who takes the next turn (#1185): the lead's turn, then seat 1's,
/// and seat 1's ending ends the phase.
#[test]
fn investigation_turns_start_at_the_lead() {
    let session = session(lead_on_the_second_seat().ending_upkeep_phase())
        .confirm()
        .confirm();
    session.assert_at_turn_menu(LEAD);
    let session = session.take(&TurnAction::EndTurn);
    session.assert_at_turn_menu(FIRST_SEAT);
    let session = session.take(&TurnAction::EndTurn);
    assert!(
        session.events().iter().any(|e| matches!(
            e,
            Event::PhaseEnded {
                phase: Phase::Investigation
            }
        )),
        "the first seat is last in player order, so their turn ends the phase"
    );
}

/// Appendix II 3.3: *"Resolve engaged enemy attacks in player order, with each
/// player resolving all of his or her engaged enemies before advancing to the
/// next player."* The phase ends on seat 1's turn, the last in player order.
#[test]
fn enemy_attacks_start_at_the_lead() {
    let mut on_first = test_support::test_enemy(1, "Engaged with seat 1");
    on_first.attack_damage = 1;
    let mut on_lead = test_support::test_enemy(2, "Engaged with the lead");
    on_lead.attack_damage = 1;
    let session = session(
        lead_on_the_second_seat()
            .with_enemy_engaged(on_first, FIRST_SEAT)
            .with_enemy_engaged(on_lead, LEAD)
            .open_turn(FIRST_SEAT),
    )
    .take(&TurnAction::EndTurn);

    let attacked = in_order(session.events(), |e| match e {
        Event::DamagePlacedOnInvestigator { investigator, .. } => Some(*investigator),
        _ => None,
    });
    assert_eq!(attacked, vec![LEAD, FIRST_SEAT]);
}

/// Appendix II 4.4: *"Each investigator draws 1 card and gains 1 resource."*,
/// in player order.
#[test]
fn upkeep_draws_and_resources_start_at_the_lead() {
    let session =
        session(lead_on_the_second_seat().open_turn(FIRST_SEAT)).take(&TurnAction::EndTurn);

    let drew = in_order(session.events(), |e| match e {
        Event::CardsDrawn { investigator, .. } => Some(*investigator),
        _ => None,
    });
    assert_eq!(drew, vec![LEAD, FIRST_SEAT]);
    let gained = in_order(session.events(), |e| match e {
        Event::ResourcesGained { investigator, .. } => Some(*investigator),
        _ => None,
    });
    assert_eq!(gained, vec![LEAD, FIRST_SEAT]);
}
