//! Scavenging 01073, as each account at the table sees it: the chooser gets
//! the card modal, with Take on each Item in their discard pile, and every
//! other account, viewers included, sees the same Items face up on the
//! chooser's tab, with no controls. A discard pile is open information.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01073):
//! *"[reaction] After you successfully investigate by 2 or more, exhaust
//! Scavenging: Choose an \[\[Item\]\] card in your discard pile and add it to
//! your hand."*
//!
//! Each game is seated as [`tabs::mount_table`]'s: Tal, [`YOU`], holds Roland
//! Banks and Agnes Baker, Sam holds Daisy Walker, and Alex is a viewer. Each
//! prompt is the engine's own, reached through a test session, and a click
//! goes back through the in-process server.
#![cfg(target_arch = "wasm32")]

use game_core::engine::enumerate::TurnAction;
use game_core::state::{CardCode, ChaosBag, ChaosToken, GameStateBuilder, LocationId};
use game_core::test_support::{self, location, seat, BoardLookup as _, TestSession};
use wasm_bindgen_test::*;

use crate::common::{self, Harness, YOU};
use crate::lobby::SAM;
use crate::tabs::{self, AGNES, ALEX, DAISY, ROLAND};

/// A shroud-1 location with a clue, where everyone stands.
const HERE: LocationId = LocationId(10);

const SCAVENGING: &str = "01073";
/// Items.
const FLASHLIGHT: &str = "01087";
const KNIFE: &str = "01086";
/// Not an Item.
const GUTS: &str = "01089";

/// Roland's discard pile, bottom first: two Items around a skill.
const ROLANDS_DISCARD: [&str; 3] = [FLASHLIGHT, GUTS, KNIFE];

/// The table at `HERE`, Roland with Scavenging in play and
/// [`ROLANDS_DISCARD`], on his turn. Roland's intellect 3 against shroud 1,
/// with a 0 in the bag, succeeds by 2.
fn table() -> TestSession {
    GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_location(location(HERE).shroud(1).clues(1))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_seat(
            seat(ROLAND.0)
                .as_card("01001")
                .at(HERE)
                .in_play([SCAVENGING])
                .discard(ROLANDS_DISCARD),
        )
        .with_seat(seat(AGNES.0).as_card("01004").at(HERE))
        .with_seat(seat(DAISY.0).as_card("01002").at(HERE))
        .with_seating([ROLAND, DAISY, AGNES])
        .open_turn(ROLAND)
        .session()
}

/// Roland investigates, succeeds by 2, and exhausts Scavenging: the session
/// rests at his choice among the Items in his discard pile.
fn roland_scavenges(session: TestSession) -> TestSession {
    let scavenging = session.state().card(ROLAND, SCAVENGING);
    session
        .take(&TurnAction::Investigate {
            investigator: ROLAND,
        })
        .commit(&[])
        .pick(scavenging)
}

/// Tal holds Roland, who chooses: the Items are in his modal with Take, top
/// of the pile first, the rest of the pile folds under a count, and the prompt
/// strip lists none of them. Take adds that Item to his hand.
#[wasm_bindgen_test]
async fn the_chooser_takes_an_item_from_the_modal() {
    let session = roland_scavenges(table());
    let outcome = test_support::awaiting_request(session.prompt().clone());
    let mut h = Harness::mount(session.state().clone(), outcome).await;
    let root = h.board.root.clone();

    assert_eq!(
        common::offered(&root),
        [
            ("Knife".to_owned(), "Take".to_owned()),
            ("Flashlight".to_owned(), "Take".to_owned()),
        ],
    );
    assert_eq!(
        common::text_of(&root, ".card-modal-others > summary"),
        "1 other card",
    );
    assert_eq!(
        common::texts_in(&root, ".card-modal-others .card-name"),
        common::names(&["Guts"]),
    );
    assert!(
        h.absent(".prompt-strip .strip-option"),
        "the strip lists none of the Items: the modal carries them",
    );

    common::click(&common::one(
        &common::named(&root, ".card-modal-card", ".card-name", "Flashlight"),
        ".card-modal-verb",
    ));
    h.pump().await;

    assert!(h.absent(".card-modal"), "answering closes the modal");
    let roland = &h.state().investigators[&ROLAND];
    assert_eq!(roland.hand, vec![CardCode::new(FLASHLIGHT)]);
    assert_eq!(
        roland.discard,
        vec![CardCode::new(GUTS), CardCode::new(KNIFE)]
    );
}

/// Sam, and a viewer, see the same Items face up on Roland's tab, with no
/// controls and no modal, and none of the pile's other cards.
#[wasm_bindgen_test]
async fn every_other_account_sees_the_items_face_up_on_rolands_tab() {
    let session = roland_scavenges(table());
    for you in [SAM, ALEX] {
        let handle = tabs::mount_as(you, &session).await;
        common::click_tab(&handle.root, "Roland Banks").await;

        assert_eq!(
            common::texts_in(&handle.root, ".dock .choice-row .card-name"),
            common::names(&["Knife", "Flashlight"]),
            "{you:?}",
        );
        assert_eq!(
            common::text_of(&handle.root, ".dock .choice-row .choice-row-caption"),
            "being chosen from",
            "{you:?}",
        );
        assert_eq!(
            common::count(&handle.root, ".choice-row button"),
            0,
            "{you:?}"
        );
        assert_eq!(
            common::count(&handle.root, ".card-modal"),
            0,
            "{you:?}: no modal"
        );

        common::click_tab(&handle.root, "Daisy Walker").await;
        assert_eq!(
            common::count(&handle.root, ".choice-row"),
            0,
            "{you:?}: only on Roland's tab"
        );
    }
    let tal = tabs::mount_as(YOU, &session).await;
    assert_eq!(
        common::count(&tal.root, ".choice-row"),
        0,
        "Tal chooses in the modal"
    );
    assert_eq!(
        common::offered(&tal.root),
        [
            ("Knife".to_owned(), "Take".to_owned()),
            ("Flashlight".to_owned(), "Take".to_owned()),
        ],
    );
}
