//! Wendy's Amulet 01014's discard-pile play reaches the board: the client
//! renders no discard pile (TODO(#1101): show it), so the engine anchors the
//! option on the Amulet, and the Amulet's in-play menu — [`InPlayCardView`],
//! which reads its options through [`interaction::options_for`] — offers it.
//!
//! Picking that entry sends the option's id, as the menu does, and plays the
//! top event.
//!
//! Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > You may play the topmost event in your discard pile as if it were in your hand.
//!
//! Host-only: the browser tests in this directory are wasm32-only, and this
//! one needs no DOM.
//!
//! [`InPlayCardView`]: web::card::InPlayCardView
#![cfg(not(target_arch = "wasm32"))]

use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::OptionTarget;
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, seat, SeatLookup as _};
use web::interaction;

const AMULET: &str = "01014";
/// Event. Gain 3 resources.
const EMERGENCY_CACHE: &str = "01088";
/// A non-event discard card.
const GUTS: &str = "01089";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

#[test]
fn the_amulets_menu_entry_plays_the_top_event() {
    let s = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(
            seat(1)
                .at(HERE)
                .in_play([AMULET])
                .discard([GUTS, EMERGENCY_CACHE]),
        )
        .open_turn(INV)
        .session();
    let amulet = s.state().in_play(INV, AMULET);

    let menu = interaction::options_for(&s.prompt().options, &OptionTarget::CardInstance(amulet));
    let [entry] = menu.as_slice() else {
        panic!("one entry on the Amulet's menu: {menu:?}");
    };
    assert_eq!(entry.label, "Play Emergency Cache (Wendy's Amulet)");
    let resources = s.state().investigators[&INV].resources;

    let s = s.apply(Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(entry.id),
    }));

    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.resources, resources + 3, "Emergency Cache resolved");
    assert_eq!(inv.discard, [CardCode::new(GUTS)]);
    assert_eq!(inv.deck, [CardCode::new(EMERGENCY_CACHE)]);
}
