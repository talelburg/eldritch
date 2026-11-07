//! Scrying 01061, as each account at the table sees it: the account answering
//! gets the card modal, and every other account, viewers included, sees the
//! looked-at cards face down on the chooser's tab, with no controls.
//!
//! *"Look at the top 3 cards of any investigator's deck or the encounter deck.
//! Return them to the top of that deck in any order."* Looking at is not a
//! search (FAQ 2.13, `glossary/Search.md`: *"if an investigator "looks at" the
//! top 3 cards of their deck, this is not a search effect"*), and only a search
//! carries the no-reveal clause. Hiding the cards anyway is a product choice
//! (#1243), to play as the table does in person.
//!
//! Each game is seated as [`tabs::mount_table`]'s: Tal, [`YOU`], holds Roland
//! Banks and Agnes Baker, Sam holds Daisy Walker, and Alex is a viewer. Daisy
//! holds Scrying, so she chooses and the row is on her tab, whoever's deck it
//! is. Each prompt is the engine's own, reached through a test session.
#![cfg(target_arch = "wasm32")]

use game_core::engine::enumerate::TurnAction;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, GameStateBuilder, LocationId, UseKind,
};
use game_core::test_support::{self, location, seat, uses, BoardLookup as _, TestSession};
use wasm_bindgen_test::*;

use crate::common::{self, BoardHandle, YOU};
use crate::lobby::SAM;
use crate::tabs::{self, AGNES, ALEX, DAISY, ROLAND};

/// The Study 01111, where everyone stands.
const STUDY: LocationId = LocationId(10);

const SCRYING: &str = "01061";
const GUTS: &str = "01089";
const PERCEPTION: &str = "01090";
const EMERGENCY_CACHE: &str = "01088";

/// The deck decision's branches on Scrying, in printed order.
const INVESTIGATOR_DECK: usize = 0;
const ENCOUNTER_DECK: usize = 1;

/// Daisy's turn in the Study, with a charged Scrying in play. Roland's deck
/// and the encounter deck each hold 3 cards.
fn daisys_turn() -> TestSession {
    GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_location(location(STUDY).as_card("01111"))
        .with_seat(seat(ROLAND.0).as_card("01001").at(STUDY).deck([
            GUTS,
            PERCEPTION,
            EMERGENCY_CACHE,
        ]))
        .with_seat(seat(AGNES.0).as_card("01004").at(STUDY).deck([GUTS]))
        .with_seat(
            seat(DAISY.0)
                .as_card("01002")
                .at(STUDY)
                .deck([GUTS])
                .in_play([(SCRYING, uses(UseKind::Charges, 3))]),
        )
        .with_seating([ROLAND, DAISY, AGNES])
        .open_turn(DAISY)
        .session()
        .edit_state(|state| {
            state.encounter_deck = ["01166", "01167", "01168"].map(CardCode::new).into();
        })
}

/// Daisy activates Scrying and picks a deck branch.
fn daisy_scries(branch: usize) -> TestSession {
    let session = daisys_turn();
    let scrying = session.state().in_play(DAISY, SCRYING);
    session
        .take(&TurnAction::ActivateAbility {
            investigator: DAISY,
            source: AbilitySource::InPlay(scrying),
            address: AbilityAddress::Printed(0),
        })
        .pick_nth(branch)
}

/// Daisy scries Roland's deck: the session rests at her first pick among his
/// top 3.
fn daisy_scries_rolands_deck() -> TestSession {
    let session = daisy_scries(INVESTIGATOR_DECK);
    let roland = session.state().investigator_card(ROLAND);
    session.pick(roland)
}

/// The board as `you` sees the prompt `session` rests at.
async fn mount_as(you: protocol::AccountId, session: &TestSession) -> BoardHandle {
    let outcome = test_support::awaiting_request(session.prompt().clone());
    tabs::mount_table_at(you, session.state().clone(), outcome).await
}

/// How many `sel` there are under the board.
fn count(handle: &BoardHandle, sel: &str) -> u32 {
    handle.root.query_selector_all(sel).expect("query").length()
}

/// Whether `sel` is absent under the board.
fn absent(handle: &BoardHandle, sel: &str) -> bool {
    count(handle, sel) == 0
}

/// Tal and Alex see Roland's 3 looked-at cards face down on Daisy's tab, the
/// chooser's, with player backs, no controls, no names, and no modal. Sam,
/// who chooses, has the modal.
#[wasm_bindgen_test]
async fn every_other_account_sees_a_scried_deck_face_down_on_the_choosers_tab() {
    let session = daisy_scries_rolands_deck();
    for you in [YOU, ALEX] {
        let handle = mount_as(you, &session).await;
        common::click_tab(&handle.root, "Daisy Walker").await;

        assert_eq!(
            common::text_of(&handle.root, ".dock .choice-row .choice-row-caption"),
            "being looked at",
            "{you:?}",
        );
        assert_eq!(count(&handle, ".dock .choice-row .deck-back"), 3, "{you:?}");
        assert!(
            absent(&handle, ".choice-row .encounter-back"),
            "{you:?}: a player deck's backs"
        );
        assert!(absent(&handle, ".card-modal"), "{you:?}: no modal");
        assert!(
            absent(&handle, ".choice-row button") && absent(&handle, ".choice-row .card-name"),
            "{you:?}: no controls and no card named",
        );

        common::click_tab(&handle.root, "Roland Banks").await;
        assert!(
            absent(&handle, ".choice-row"),
            "{you:?}: only on the chooser's tab"
        );
    }
    let sam = mount_as(SAM, &session).await;
    assert_eq!(
        count(&sam, ".card-modal .card-modal-verb"),
        3,
        "Sam chooses"
    );
    assert!(absent(&sam, ".choice-row"));
}

/// Scrying the encounter deck: everyone else sees encounter backs.
#[wasm_bindgen_test]
async fn every_other_account_sees_a_scried_encounter_deck_as_encounter_backs() {
    let session = daisy_scries(ENCOUNTER_DECK);
    for you in [YOU, ALEX] {
        let handle = mount_as(you, &session).await;
        common::click_tab(&handle.root, "Daisy Walker").await;

        assert_eq!(
            common::text_of(&handle.root, ".dock .choice-row .choice-row-caption"),
            "being looked at",
            "{you:?}",
        );
        assert_eq!(
            count(&handle, ".dock .choice-row .encounter-back"),
            3,
            "{you:?}"
        );
        assert!(absent(&handle, ".card-modal"), "{you:?}");
        assert!(absent(&handle, ".choice-row .card-name"), "{you:?}");
    }
}
