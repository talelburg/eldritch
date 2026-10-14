use super::*;
use crate::test_support::TEST_REGISTRY;

const DRAW: TurnAction = TurnAction::Draw { investigator: INV };

/// `drawer`'s open turn at A.
fn draw_board(drawer: Seat) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_seat(drawer.at(HERE))
        .with_location(test_support::test_location(HERE.0, "A"))
        .with_rng_seed(13)
        .open_turn(INV)
}

/// Whether `state`'s menu offers `investigator` a Draw.
fn offers_draw(state: &GameState, investigator: InvestigatorId) -> bool {
    legal_actions(state, &TEST_REGISTRY)
        .iter()
        .any(|a| matches!(a, TurnAction::Draw { investigator: i } if *i == investigator))
}

#[test]
fn draw_with_non_empty_deck_draws_one_and_spends_action() {
    let session = draw_board(seat(1).deck(["test-001", "test-002"]))
        .session()
        .take(&DRAW);
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::ActionsRemainingChanged { investigator, new_count: 2 }
            if *investigator == INV
    );
    assert_event!(
        session.events(),
        Event::CardsDrawn { investigator, count: 1 } if *investigator == INV
    );
    assert_no_event!(session.events(), Event::DeckShuffled { .. });
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.hand.len(), 1);
    assert_eq!(inv.deck.len(), 1);
    assert_eq!(inv.hand[0], CardCode::new("test-001"));
    assert_eq!(inv.actions_remaining, 2);
}

#[test]
fn draw_with_empty_deck_reshuffles_discard_draws_and_takes_one_horror() {
    // Per the Rules Reference: empty deck on draw triggers
    // reshuffle, then draw, then 1 horror penalty.
    let session = draw_board(seat(1).discard(["test-A", "test-B", "test-C"]))
        .session()
        .take(&DRAW);
    session.assert_at_turn_menu(INV);
    let events = session.events();
    assert_event!(
        events,
        Event::DeckShuffled { investigator } if *investigator == INV
    );
    assert_event!(
        events,
        Event::CardsDrawn { investigator, count: 1 } if *investigator == INV
    );
    assert_event!(
        events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == INV
    );
    let shuffle_idx = events
        .iter()
        .position(|e| matches!(e, Event::DeckShuffled { .. }))
        .expect("DeckShuffled missing");
    let draw_idx = events
        .iter()
        .position(|e| matches!(e, Event::CardsDrawn { .. }))
        .expect("CardsDrawn missing");
    let horror_idx = events
        .iter()
        .position(|e| matches!(e, Event::HorrorTaken { .. }))
        .expect("HorrorTaken missing");
    assert!(
        shuffle_idx < draw_idx && draw_idx < horror_idx,
        "Expected order DeckShuffled ({shuffle_idx}) < CardsDrawn ({draw_idx}) < \
         HorrorTaken ({horror_idx})"
    );
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.hand.len(), 1);
    assert_eq!(inv.deck.len(), 2);
    assert!(inv.discard.is_empty());
    assert_eq!(inv.horror(), 1);
}

#[test]
fn draw_with_both_empty_deals_one_horror_no_shuffle_no_card() {
    // Per the Rules Reference: empty discard means no shuffle
    // happens. We still apply the 1-horror penalty as the safer
    // reading of "would-draw-from-empty-deck" (see handler doc).
    let session = draw_board(seat(1)).session().take(&DRAW);
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == INV
    );
    assert_event!(
        session.events(),
        Event::CardsDrawn { investigator, count: 0 } if *investigator == INV
    );
    assert_no_event!(session.events(), Event::DeckShuffled { .. });
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.horror(), 1);
    assert_eq!(inv.actions_remaining, 2);
    assert!(inv.hand.is_empty());
    assert!(inv.deck.is_empty());
    assert!(inv.discard.is_empty());
}

#[test]
fn draw_outside_investigation_phase_is_rejected() {
    let state = draw_board(seat(1)).with_phase(Phase::Mythos).build();
    // Mythos phase → Draw is not legal.
    assert!(!offers_draw(&state, INV));
}

#[test]
fn draw_by_non_active_investigator_is_rejected() {
    let other = InvestigatorId(2);
    let state = draw_board(seat(1)).with_seat(seat(2)).build();
    // Non-active investigator → their Draw is not legal.
    assert!(!offers_draw(&state, other));
}

#[test]
fn draw_with_zero_actions_is_rejected() {
    let state = draw_board(seat(1).actions(0)).build();
    // No actions remaining → Draw is not legal.
    assert!(!offers_draw(&state, INV));
}

#[test]
fn draw_by_defeated_investigator_is_rejected() {
    let mut state = draw_board(seat(1)).build();
    // No seat setter carries status.
    state.investigators.get_mut(&INV).unwrap().status = Status::Defeated;
    // Defeated status → Draw is not legal.
    assert!(!offers_draw(&state, INV));
}

#[test]
fn draw_with_low_sanity_investigator_defeated_by_reshuffle_horror() {
    // Interaction: 1-sanity investigator + empty deck + empty
    // discard → the 1-horror penalty defeats the investigator
    // through the dealt horror's placement (#80, #1027). Verifies the Draw
    // flow correctly composes with the defeat helpers.
    // Pre-load accumulated_horror so remaining sanity = 1 (lethal with the 1-horror penalty).
    // max_sanity()=8 from TEST_INV; 7+1=8=defeated.
    let result = draw_board(seat(1).harm(0, 7))
        .session()
        .take(&DRAW)
        .finish();
    // The only investigator is defeated: the game is over.
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == INV
    );
    assert_event!(
        result.events,
        Event::InvestigatorEliminated {
            investigator,
            cause: EliminationCause::Horror,
        } if *investigator == INV
    );
    assert_eq!(result.state.investigators[&INV].status, Status::Defeated);
}
