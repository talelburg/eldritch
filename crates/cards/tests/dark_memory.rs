//! End-to-end tests for Dark Memory 01013 (Agnes Baker's signature weakness)
//! against the real `cards::TEST_REGISTRIES`.
//!
//! Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > Place 1 doom on the current agenda. This effect can cause the current
//! > agenda to advance.
//! > <b>Forced</b> - If Dark Memory is in your hand at the end of your turn,
//! > reveal it and take 2 horror.
//!
//! Rulings (<https://arkhamdb.com/card/01013>):
//!
//! > - You must either play this card (spend an action and pay 2 resources to
//! >   place 1 doom on the agenda) and place it into the discard pile, or take 2
//! >   horror at the end of each turn while it's still in your hand.
//! > - You cannot choose to discard a Weakness card from your hand, neither for
//! >   exceeding the max number of cards in hand, nor for card effects that make
//! >   you 'choose and discard'. You can, however, discard it at random, or if an
//! >   effect makes you discard your whole hand.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::{InputKind, OptionTarget};
use game_core::event::Event;
use game_core::state::{Agenda, CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId};
use game_core::test_support::{seat, TestSession};
use game_core::{assert_event, assert_no_event};

/// `ArkhamDB` code for Dark Memory.
const DARK_MEMORY: &str = "01013";

/// `ArkhamDB` code for Frozen in Fear.
const FROZEN_IN_FEAR: &str = "01164";

const INV: InvestigatorId = InvestigatorId(1);
const INV2: InvestigatorId = InvestigatorId(2);
const INV3: InvestigatorId = InvestigatorId(3);

/// Investigator 1's open turn, three seats, investigator 1 holding `hand`.
fn turn_holding(hand: &[&str]) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).hand(hand.iter().copied()))
        .with_seat(seat(2))
        .with_seat(seat(3))
        .with_turn_order([INV, INV2, INV3])
        .open_turn(INV)
        .session()
}

/// The Gathering's agendas, What's Going On?! 01105 (threshold 3) first.
const AGENDAS: [(&str, u8); 2] = [("01105", 3), ("01106", 7)];

/// Investigator 1's open turn, solo, holding only Dark Memory with the 2
/// resources it costs, and 01105 current with `doom` on it.
fn turn_to_play_it(doom: u8) -> TestSession {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).hand([DARK_MEMORY]).resources(2))
        .with_turn_order([INV])
        .open_turn(INV)
        .build();
    // The builder stages no agenda.
    state.agenda_deck = AGENDAS
        .iter()
        .map(|&(code, doom_threshold)| Agenda {
            code: CardCode::new(code),
            doom_threshold,
        })
        .collect();
    state.agenda_index = 0;
    state.agenda_doom = doom;
    TestSession::new(state, TEST_REGISTRIES)
}

const PLAY_IT: TurnAction = TurnAction::PlayCard {
    investigator: INV,
    source: PlaySource::Hand(0),
};

#[test]
fn playing_dark_memory_places_one_doom_on_the_current_agenda() {
    let session = turn_to_play_it(0).take(&PLAY_IT);

    session.assert_at_turn_menu(INV);
    let state = session.state();
    assert_eq!(state.agenda_doom, 1);
    assert_eq!(state.agenda_index, 0, "1 of 3 doom does not advance");
    let inv = &state.investigators[&INV];
    assert_eq!(inv.resources, 0, "it costs 2");
    assert!(inv.hand.is_empty());
    assert_eq!(inv.discard, vec![CardCode::new(DARK_MEMORY)]);
}

#[test]
fn playing_dark_memory_can_advance_the_agenda() {
    // 2 of 3 doom: Dark Memory's doom meets the threshold, and "This effect can
    // cause the current agenda to advance" advances it now, not at Mythos 1.3.
    // 01105's reverse is the lead-investigator proxy's choice; branch 1 is "the lead
    // investigator takes 2 horror".
    let session = turn_to_play_it(2).take(&PLAY_IT).pick_nth(1);

    assert_event!(session.events(), Event::AgendaAdvanced { from: 0 });
    let state = session.state();
    assert_eq!(state.agenda_index, 1);
    assert_eq!(state.agenda_doom, 0);
}

#[test]
fn holding_dark_memory_at_the_end_of_your_turn_deals_two_horror() {
    let session = turn_holding(&[DARK_MEMORY]).take(&TurnAction::EndTurn);

    session.assert_at_turn_menu(INV2);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.horror(), 2, "the forced ability takes 2 horror");
    assert_eq!(
        inv.hand,
        vec![CardCode::new(DARK_MEMORY)],
        "revealing it does not discard it: it stays in hand",
    );
    assert_event!(
        session.events(),
        Event::HorrorPlacedOnInvestigator { investigator, amount: 2 } if *investigator == INV
    );
}

#[test]
fn dark_memory_does_not_fire_at_the_end_of_another_investigators_turn() {
    // "at the end of **your** turn": investigator 2's turn ending reaches
    // nobody's hand but investigator 2's.
    let session = turn_holding(&[DARK_MEMORY])
        .take(&TurnAction::EndTurn)
        .take(&TurnAction::EndTurn);

    session.assert_at_turn_menu(INV3);
    assert_eq!(
        session.state().investigators[&INV].horror(),
        2,
        "only investigator 1's own turn end dealt horror",
    );
}

#[test]
fn dark_memory_beside_another_end_of_turn_forced_ability_fires_in_place() {
    // Frozen in Fear 01164 (*"Forced - At the end of your turn: Test
    // \[willpower\] (3)."*) shares the cell, so the player orders the two.
    // Dark Memory resolves first: its horror lands, and it is not played — no cost
    // paid, no doom placed, still in hand.
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .hand([DARK_MEMORY])
                .threat_area([FROZEN_IN_FEAR])
                .resources(2),
        )
        .with_seat(seat(2))
        .with_turn_order([INV, INV2])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .session()
        .take(&TurnAction::EndTurn)
        .pick(OptionTarget::HandCardByCode {
            investigator: INV,
            code: CardCode::new(DARK_MEMORY),
        });

    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.horror(), 2);
    assert_eq!(inv.resources, 2, "Dark Memory was not played");
    assert_eq!(inv.hand, vec![CardCode::new(DARK_MEMORY)]);
    assert_eq!(session.state().agenda_doom, 0);
    assert_no_event!(session.events(), Event::CardPlayed { .. });

    // Frozen in Fear, the run's last option, resolves next; the turn then
    // passes.
    let frozen = session.state().investigators[&INV].threat_area[0].instance_id;
    let session = session.pick(OptionTarget::CardInstance(frozen)).commit(&[]);
    session.assert_at_turn_menu(INV2);
}

// ---------------------------------------------------------------------------
// Upkeep step 4.5. `glossary/Weakness.md`: *"A player may not optionally choose
// to discard a weakness card from hand, unless a card explicitly specifies
// otherwise."*
// ---------------------------------------------------------------------------

/// The neutral Core events and skills 01086–01093, none a weakness.
const FILLER: [&str; 8] = [
    "01086", "01087", "01088", "01089", "01090", "01091", "01092", "01093",
];

/// Step 4.5's hand-size check, solo, investigator 1 holding `hand`.
fn ending_upkeep_holding(hand: &[&str]) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).hand(hand.iter().copied()))
        .with_turn_order([INV])
        .ending_upkeep_phase()
        .session()
}

/// The eight filler cards plus Dark Memory: nine cards, one over the cap.
fn filler_and_dark_memory() -> Vec<&'static str> {
    FILLER.iter().copied().chain([DARK_MEMORY]).collect()
}

#[test]
fn the_hand_size_discard_rejects_dark_memory() {
    let session = ending_upkeep_holding(&filler_and_dark_memory());
    assert_eq!(session.prompt().kind, InputKind::PickMultiple);

    let session = session.choose_from_hand(&[DARK_MEMORY]);
    session.expect_rejected();
    assert_eq!(session.state().investigators[&INV].hand.len(), 9);
}

#[test]
fn the_hand_size_discard_takes_a_non_weakness_and_keeps_dark_memory() {
    let session = ending_upkeep_holding(&filler_and_dark_memory()).choose_from_hand(&["01086"]);

    session.assert_at_encounter_draw();
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.hand.len(), 8);
    assert!(inv.hand.contains(&CardCode::new(DARK_MEMORY)));
    assert_eq!(inv.discard, vec![CardCode::new("01086")]);
}

#[test]
fn an_overflow_of_weaknesses_is_not_discarded_and_the_hand_stays_over_the_limit() {
    // The Official FAQ (`data/official-faq/Frequently_Asked_Questions.md`,
    // Forced Learning): "Weaknesses cannot be chosen to be discarded from your
    // hand. … If you draw 2 weaknesses, you cannot discard either one, and must
    // resolve both of them." Nine weaknesses and nothing else: no discard is
    // asked for, and step 4.5 ends over the cap.
    let session = ending_upkeep_holding(&[DARK_MEMORY; 9]);

    session.assert_at_encounter_draw();
    assert_no_event!(session.events(), Event::CardDiscarded { .. });
    assert_eq!(session.state().investigators[&INV].hand.len(), 9);
}

#[test]
fn an_overflow_partly_of_weaknesses_discards_only_the_non_weaknesses() {
    // Ten cards, two over the cap, but only one is not a weakness: the discard
    // takes exactly that one and the hand stays at nine.
    let mut hand = vec![DARK_MEMORY; 9];
    hand.push("01086");
    let session = ending_upkeep_holding(&hand);
    assert_eq!(
        session.prompt().prompt,
        "You have more than 8 cards in hand — choose 1 card to discard. Weaknesses can't be \
         chosen.",
        "the prompt asks for the one discard the pick must hold",
    );

    // Rejected, the same prompt stays pending for a legal answer.
    let session = session.choose_from_hand(&["01086", DARK_MEMORY]);
    session.expect_rejected();

    let session = session.choose_from_hand(&["01086"]);
    session.assert_at_encounter_draw();
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.hand, vec![CardCode::new(DARK_MEMORY); 9]);
    assert_eq!(inv.discard, vec![CardCode::new("01086")]);
}
