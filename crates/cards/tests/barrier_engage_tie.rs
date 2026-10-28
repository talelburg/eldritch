//! Regression for #1139: The Barrier 01109 advances when the investigators in
//! the Hallway tie on the Ghoul Priest's prey, and the spawn asks whom he
//! engages.
//!
//! ## Verified card text (`data/arkhamdb-snapshot/pack/core/`)
//!
//! **The Barrier (01109)**, `back_text` verbatim:
//!
//! > The barrier blocking passage into the parlor has vanished. Reveal the
//! > Parlor.
//! > Put the set-aside Lita Chantler into play in the Parlor.
//! > Spawn the set-aside Ghoul Priest in the Hallway.
//!
//! **Ghoul Priest (01116)**: *"**Prey** - Highest [combat]."* Daisy Walker
//! 01002 and Agnes Baker 01004 both print `skill_combat` 2, so two of them in
//! the Hallway tie.
//!
//! `data/rules-reference/rules/glossary/Prey.md`: *"if multiple investigators
//! are tied in meeting these instructions, the lead investigator may decide
//! among them"*. Neither 01109 nor 01116 has rulings
//! (`data/arkhamdb-faq/no-rulings.txt`).
//!
//! The spawn is the reverse's last line, reached through
//! `NativeCx::put_set_aside_card_into_play` after the reverse has revealed the
//! Parlor and placed Lita. The tie pushes its engagement prompt and the native
//! returns, so the prompt surfaces after the reverse with the board already
//! showing what the act did. The lead is asked, wherever they sit.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{InputKind, OptionTarget, Responder};
use game_core::state::{Act, CardCode, GameStateBuilder, InvestigatorId, Location, LocationId};
use game_core::test_support::{seat, TestSession};

const DAISY: InvestigatorId = InvestigatorId(1);
const AGNES: InvestigatorId = InvestigatorId(2);
const HALLWAY: LocationId = LocationId(1);
const PARLOR: LocationId = LocationId(2);
const GHOUL_PRIEST: &str = "01116";
const LITA: &str = "01117";

/// Daisy Walker and Agnes Baker in the Hallway, the Parlor unrevealed beside
/// it, the Ghoul Priest and Lita Chantler set aside, and The Barrier the
/// current act, with Daisy holding its 3 clues. It is the round's last turn,
/// the one after the lead's, so ending it runs on to the round end, where the
/// act's objective is offered: *"When the round ends, investigators in the
/// hallway may, as a group, spend the requisite number of clues to advance."*
/// Daisy, in the first seat, is the lead.
fn barrier_with_two_tied_in_the_hallway() -> TestSession {
    barrier_with_two_tied_in_the_hallway_led_by(DAISY)
}

/// [`barrier_with_two_tied_in_the_hallway`] with `lead` the lead investigator.
fn barrier_with_two_tied_in_the_hallway_led_by(lead: InvestigatorId) -> TestSession {
    let mut parlor = Location::new(PARLOR, CardCode::new("01115"), "Parlor", 2, 0);
    parlor.revealed = false;
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        // A spare deck card each, so Upkeep step 4.4's draw has something to
        // take.
        .with_seat(
            seat(1)
                .as_card("01002")
                .at(HALLWAY)
                .clues(3)
                .deck(["01088"]),
        )
        .with_seat(seat(2).as_card("01004").at(HALLWAY).deck(["01088"]))
        .with_location(Location::new(
            HALLWAY,
            CardCode::new("01112"),
            "Hallway",
            1,
            0,
        ))
        .with_location(parlor)
        .with_turn_order([DAISY, AGNES])
        .with_lead(lead)
        .open_turn(if lead == DAISY { AGNES } else { DAISY })
        .build();
    state.set_aside_cards = vec![CardCode::new(GHOUL_PRIEST), CardCode::new(LITA)];
    // The act deck needs a next card: a terminal act's reverse must end the
    // scenario.
    state.act_deck = vec![
        Act {
            code: CardCode::new("01109"),
            clue_threshold: 3,
        },
        Act {
            code: CardCode::new("01110"),
            clue_threshold: 0,
        },
    ];
    TestSession::new(state, TEST_REGISTRIES)
}

/// Advance The Barrier from the round end's objective, and stop at whatever
/// the advance asks next.
fn advance_the_barrier() -> TestSession {
    advance(barrier_with_two_tied_in_the_hallway())
}

/// Advance The Barrier on `session` from the round end's objective.
fn advance(session: TestSession) -> TestSession {
    session.take(&TurnAction::EndTurn).pick(OptionTarget::Act)
}

/// The reverse has done everything it prints by the time the tie is asked: the
/// Parlor is revealed with Lita Chantler in it, and the Ghoul Priest is in the
/// Hallway, engaged with nobody yet.
#[test]
fn the_reverse_resolves_before_the_engagement_is_asked() {
    let session = advance_the_barrier();
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
    let session = advance_the_barrier();
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
    let session = advance_the_barrier();
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
    let session = advance_the_barrier();
    assert_eq!(session.prompt().responder, Responder::Lead(DAISY));
}

/// With the lead in the second seat, that seat is asked, not the first.
#[test]
fn a_lead_in_the_second_seat_is_asked_whom_the_ghoul_priest_engages() {
    let session = advance(barrier_with_two_tied_in_the_hallway_led_by(AGNES));
    let agnes = session.state().investigators[&AGNES].card_anchor();
    assert_eq!(session.offered_at(&agnes), 1, "{:?}", session.prompt());
    assert_eq!(session.prompt().responder, Responder::Lead(AGNES));
}
