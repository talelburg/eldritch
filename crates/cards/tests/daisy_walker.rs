//! Daisy Walker 01002: the additional action only a Tome's `[action]` ability
//! can use.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01002):
//! > You may take an additional action during your turn, which can only be
//! > used on \[\[Tome\]\] \[action\] abilities.
//! > \[elder_sign\] effect: +0. If you succeed, draw 1 card for each
//! > \[\[Tome\]\] you control.
//!
//! Her ruling (<https://arkhamdb.com/card/01002>): *"If you are instructed to
//! lose 1 or more actions, you have that many fewer actions to take during
//! your turn. This is referring to your normal three “full” actions. So if you
//! are instructed to lose 1 or more actions, those must be the ones that are
//! “lost” first. If you have no more of those actions to lose, then you start
//! losing “additional” actions, of your choice. So, for example, if you are
//! playing Daisy and an effect instructs you to “lose 2 actions”, you would
//! have 1 normal action and Daisy’s special additional action left."*

use cards::TEST_REGISTRIES;
use game_core::assert_no_event;
use game_core::engine;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, GameState, GameStateBuilder, InvestigatorId, LocationId,
};
use game_core::test_support::{self, seat, Seat, SeatLookup as _, TestSession};

const DAISY: &str = "01002";
/// *"Item. Tome."* `[action] Exhaust Old Book of Lore: …`
const OLD_BOOK_OF_LORE: &str = "01031";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// Daisy at `HERE`, with a deck to search and draw from.
fn daisy() -> Seat {
    seat(1)
        .as_card(DAISY)
        .at(HERE)
        .deck(["90001", "90002", "90003", "90004"])
}

/// `investigator`'s open turn, alone at `HERE`.
fn board(investigator: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Library"))
        .with_seat(investigator)
        .open_turn(INV)
        .session()
}

/// Activate the first printed ability of `code`, in play under Daisy.
fn activate(session: &TestSession, code: &str) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(session.state().in_play(INV, code)),
        address: AbilityAddress::Printed(0),
    }
}

fn standard_actions(state: &GameState) -> u8 {
    state.investigators[&INV].actions_remaining
}

/// Old Book of Lore's `[action]` is a Tome `[action]` ability: her additional
/// action pays for it, and her three standard actions are untouched.
#[test]
fn old_book_of_lore_is_paid_by_her_additional_action() {
    let session = board(daisy().in_play([OLD_BOOK_OF_LORE]));
    let book = activate(&session, OLD_BOOK_OF_LORE);
    let session = session.take(&book);
    assert_eq!(standard_actions(session.state()), 3);
    assert_no_event!(session.events(), Event::ActionsRemainingChanged { .. });
    assert_eq!(
        engine::actions_available(session.state(), TEST_REGISTRIES.cards, INV),
        3
    );
}
