//! The Barrier 01109 at the round end: whom its clue spend is offered to, and
//! whom the Ghoul Priest engages when its reverse spawns him on a tie.
//!
//! ## Verified card text (`data/arkhamdb-snapshot/pack/core/`)
//!
//! **The Barrier (01109)**, `text` verbatim: *"**Objective** - When the round
//! ends, investigators in the hallway may, as a group, spend the requisite
//! number of clues to advance."* `back_text` verbatim:
//!
//! > The barrier blocking passage into the parlor has vanished. Reveal the
//! > Parlor.
//! > Put the set-aside Lita Chantler into play in the Parlor.
//! > Spawn the set-aside Ghoul Priest in the Hallway.
//!
//! **Ghoul Priest (01116)**: *"**Prey** - Highest [combat]."* Neither 01109
//! nor 01116 has rulings (`data/arkhamdb-faq/no-rulings.txt`).
//!
//! ## The offer
//!
//! An act's reaction binds the lead unless the ability names who may act (ADR
//! 0019); this one names the investigators in the Hallway, so the offer goes to
//! the first of them in player order, and the group spends in that order. Each
//! Hallway in these tests holds Roland Banks (01001, combat 4) and at most
//! Daisy Walker (01002, combat 2), so the spawn on the reverse has no tie to
//! break.
//!
//! ## The engagement tie
//!
//! Daisy Walker 01002 and Agnes Baker 01004 both print `skill_combat` 2, so two
//! of them in the Hallway tie on the Ghoul Priest's prey.
//! `data/rules-reference/rules/glossary/Prey.md`: *"if multiple investigators
//! are tied in meeting these instructions, the lead investigator may decide
//! among them"*.
//!
//! The spawn is the reverse's last line, reached through
//! `NativeCx::put_set_aside_card_into_play` after the reverse has revealed the
//! Parlor and placed Lita. The tie pushes its engagement prompt and the native
//! returns, so the prompt surfaces after the reverse with the board already
//! showing what the act did. The lead is asked, wherever they sit.
//!
//! Integration test so it can run against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{InputKind, OptionTarget, Responder};
use game_core::state::{CardCode, GameState, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{location, seat, Either, Seat, TestSession};

const SEAT_1: InvestigatorId = InvestigatorId(1);
const SEAT_2: InvestigatorId = InvestigatorId(2);
const SEAT_3: InvestigatorId = InvestigatorId(3);
const DAISY: InvestigatorId = SEAT_1;
const AGNES: InvestigatorId = SEAT_2;
const HALLWAY: LocationId = LocationId(1);
const PARLOR: LocationId = LocationId(2);
const ATTIC: LocationId = LocationId(3);
const GHOUL_PRIEST: &str = "01116";
const LITA: &str = "01117";

/// The Barrier current with What Have You Done? (01110) behind it, the
/// Hallway and the Attic in play with the Parlor unrevealed beside them, and
/// the Ghoul Priest and Lita Chantler set aside for the reverse. Each seat gets
/// a spare deck card for Upkeep 4.4's draw. `lead` holds the lead; `last` has
/// the round's last turn open, so ending it runs on to the round end, where
/// the act's objective is offered.
fn barrier_table(
    seats: Vec<Seat<Either>>,
    seating: &[InvestigatorId],
    lead: InvestigatorId,
    last: InvestigatorId,
) -> TestSession {
    let mut builder = GameStateBuilder::new().with_registries(TEST_REGISTRIES);
    for s in seats {
        builder = builder.with_seat(s.deck(["01088"]));
    }
    let mut state: GameState = builder
        .with_location(location(HALLWAY).as_card("01112"))
        .with_location(location(PARLOR).as_card("01115").unrevealed())
        .with_location(location(ATTIC).as_card("01113"))
        .with_seating(seating.iter().copied())
        .with_lead(lead)
        .open_turn(last)
        .with_act_deck(["01109", "01110"])
        .build();
    state.set_aside_cards = vec![CardCode::new(GHOUL_PRIEST), CardCode::new(LITA)];
    TestSession::new(state, TEST_REGISTRIES)
}

// ---- the offer -----------------------------------------------------------

/// The investigator the round-end window's one act option is offered to, as
/// the prompt names them.
fn offered_to(window: &TestSession) -> InvestigatorId {
    let act: Vec<_> = window
        .prompt()
        .options
        .iter()
        .filter(|o| o.target == Some(OptionTarget::Act))
        .collect();
    let [option] = act[..] else {
        panic!(
            "the round end offers The Barrier's clue spend once: {:?}",
            window.prompt()
        );
    };
    option
        .investigator
        .expect("a reaction window's option names whom it is offered to")
}

fn clues(session: &TestSession, id: InvestigatorId) -> u8 {
    session.state().investigators[&id].clues
}

#[test]
fn with_the_lead_outside_the_hallway_the_hallway_investigator_is_offered_it() {
    // Seat 1 (Daisy) leads from the Attic; seat 2 (Roland) holds the
    // threshold in the Hallway. Seat 2's turn is the round's last.
    let window = barrier_table(
        vec![
            seat(1).as_card("01002").at(ATTIC).clues(1).into(),
            seat(2).as_card("01001").at(HALLWAY).clues(3).into(),
        ],
        &[SEAT_1, SEAT_2],
        SEAT_1,
        SEAT_2,
    )
    .take(&TurnAction::EndTurn);
    assert_eq!(offered_to(&window), SEAT_2);

    let advanced = window.pick(OptionTarget::Act);
    assert_eq!(advanced.state().act_index, 1, "The Barrier advanced");
    assert_eq!(
        (clues(&advanced, SEAT_1), clues(&advanced, SEAT_2)),
        (1, 0),
        "the Hallway investigator spent the threshold; the lead's clue stays",
    );
}

#[test]
fn the_first_hallway_investigator_in_player_order_is_offered_it_and_spends_first() {
    // Seat 2 leads from the Attic, so player order is 2, 3, 1. Seats 3
    // (Daisy) and 1 (Roland) are in the Hallway: seat 3 comes first, though
    // seat 1 is first in seating. Seat 1's turn is the round's last.
    let window = barrier_table(
        vec![
            seat(1).as_card("01001").at(HALLWAY).clues(2).into(),
            seat(2).at(ATTIC).into(),
            seat(3).as_card("01002").at(HALLWAY).clues(2).into(),
        ],
        &[SEAT_1, SEAT_2, SEAT_3],
        SEAT_2,
        SEAT_1,
    )
    .take(&TurnAction::EndTurn);
    assert_eq!(offered_to(&window), SEAT_3);

    let advanced = window.pick(OptionTarget::Act);
    assert_eq!(advanced.state().act_index, 1, "The Barrier advanced");
    assert_eq!(
        (clues(&advanced, SEAT_3), clues(&advanced, SEAT_1)),
        (0, 1),
        "the group spends in player order: seat 3's two, then one of seat 1's",
    );
}

#[test]
fn with_the_lead_in_the_hallway_the_lead_is_offered_it() {
    // Seat 2 (Roland) leads from the Hallway beside seat 1 (Daisy), who comes
    // first in seating but second in player order.
    let window = barrier_table(
        vec![
            seat(1).as_card("01002").at(HALLWAY).clues(2).into(),
            seat(2).as_card("01001").at(HALLWAY).clues(2).into(),
        ],
        &[SEAT_1, SEAT_2],
        SEAT_2,
        SEAT_1,
    )
    .take(&TurnAction::EndTurn);
    assert_eq!(offered_to(&window), SEAT_2);
}

// ---- the engagement tie --------------------------------------------------

/// Daisy Walker and Agnes Baker tied in the Hallway, Daisy holding the act's 3
/// clues, with `lead` the lead investigator. It is the round's last turn, the
/// one after the lead's.
fn two_tied_in_the_hallway_led_by(lead: InvestigatorId) -> TestSession {
    barrier_table(
        vec![
            seat(1).as_card("01002").at(HALLWAY).clues(3).into(),
            seat(2).as_card("01004").at(HALLWAY).into(),
        ],
        &[DAISY, AGNES],
        lead,
        if lead == DAISY { AGNES } else { DAISY },
    )
}

/// Advance The Barrier on `session` from the round end's objective, and stop
/// at whatever the advance asks next.
fn advance(session: TestSession) -> TestSession {
    session.take(&TurnAction::EndTurn).pick(OptionTarget::Act)
}

/// Regression for #1139: The Barrier advances when the investigators in the
/// Hallway tie on the Ghoul Priest's prey. The reverse has done everything it
/// prints by the time the tie is asked: the Parlor is revealed with Lita
/// Chantler in it, and the Ghoul Priest is in the Hallway, engaged with nobody
/// yet.
#[test]
fn the_reverse_resolves_before_the_engagement_is_asked() {
    let session = advance(two_tied_in_the_hallway_led_by(DAISY));
    let state = session.state();

    assert!(state.locations[&PARLOR].revealed, "the Parlor is revealed");
    assert!(
        state.locations[&PARLOR]
            .cards_at_location
            .iter()
            .any(|card| card.code.as_str() == LITA),
        "Lita Chantler is in the Parlor: {:?}",
        state.locations[&PARLOR].cards_at_location,
    );
    let priest = state
        .enemies
        .values()
        .find(|e| e.code.as_str() == GHOUL_PRIEST)
        .expect("the Ghoul Priest is in play");
    assert_eq!(priest.current_location, Some(HALLWAY));
    assert_eq!(
        priest.engaged_with, None,
        "nobody is engaged before the answer"
    );
}

/// The engagement prompt offers the two tied investigators, each on their
/// investigator card.
#[test]
fn the_engagement_tie_offers_the_tied_investigators() {
    let session = advance(two_tied_in_the_hallway_led_by(DAISY));
    let daisy = session.state().investigators[&DAISY].card_anchor();
    let agnes = session.state().investigators[&AGNES].card_anchor();

    let prompt = session.prompt();
    assert_eq!(prompt.kind, InputKind::PickSingle, "{prompt:?}");
    assert_eq!(
        (
            session.offered_at(&daisy),
            session.offered_at(&agnes),
            prompt.options.len()
        ),
        (1, 1, 2),
        "the tie offers Daisy and Agnes: {prompt:?}",
    );
}

/// Answering engages the chosen investigator, and the act advances.
#[test]
fn the_answer_engages_the_chosen_investigator_and_the_act_advances() {
    let session = advance(two_tied_in_the_hallway_led_by(DAISY));
    let agnes = session.state().investigators[&AGNES].card_anchor();

    let session = session.pick(agnes);
    let state = session.state();

    let priest = state
        .enemies
        .values()
        .find(|e| e.code.as_str() == GHOUL_PRIEST)
        .expect("the Ghoul Priest is in play");
    assert_eq!(priest.engaged_with, Some(AGNES));
    assert_eq!(state.act_index, 1, "The Barrier advanced to act 3");
}

/// The lead is asked whom the Ghoul Priest engages: `glossary/Prey.md`, *"the
/// lead investigator may decide among them"*. Daisy, in the first seat, is the
/// lead.
#[test]
fn the_lead_is_asked_whom_the_ghoul_priest_engages() {
    let session = advance(two_tied_in_the_hallway_led_by(DAISY));
    assert_eq!(session.prompt().responder, Responder::Lead(DAISY));
}

/// With the lead in the second seat, that seat is asked, not the first.
#[test]
fn a_lead_in_the_second_seat_is_asked_whom_the_ghoul_priest_engages() {
    let session = advance(two_tied_in_the_hallway_led_by(AGNES));
    let agnes = session.state().investigators[&AGNES].card_anchor();
    assert_eq!(session.offered_at(&agnes), 1, "{:?}", session.prompt());
    assert_eq!(session.prompt().responder, Responder::Lead(AGNES));
}
