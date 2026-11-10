//! Regression for #1362: a realistic board mounts and rebuilds without one
//! Leptos warning that a signal was read "outside a reactive tracking context".
//!
//! Every board component is rebuilt whenever the store changes, so its body
//! reads the live prompt's options, the multi-select flag and the decision-live
//! flag untracked, as they stand at mount. A tracked read there logs the
//! warning on every mount. Leptos raises it only in debug wasm builds, which is
//! what `wasm-pack test` runs.
//!
//! Known limit: this sees only the components the fixture mounts.
#![cfg(target_arch = "wasm32")]

use game_core::state::{EnemyId, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, enemy, location, seat};
use leptos::prelude::*;
use leptos::task;
use wasm_bindgen_test::*;

use crate::common::{self, WarnCapture};

/// Wendy Adams 01005.
const WENDY: InvestigatorId = InvestigatorId(1);
/// Wendy's Amulet 01014: *"You may play the topmost event in your discard pile
/// as if it were in your hand."* It puts a context menu on the pile's top card.
const AMULET: &str = "01014";
/// Knife 01086, an asset, at the bottom of her discard pile.
const KNIFE: &str = "01086";
/// Emergency Cache 01088, an event: *"Gain 3 resources."* On top of her
/// discard pile, and in her hand, where its "Play …" option gives the hand
/// card a menu.
const EMERGENCY_CACHE: &str = "01088";
/// The Gathering act 1, "Trapped".
const ACT_TRAPPED: &str = "01108";
/// The Gathering agenda 1, "What's Going On?!".
const AGENDA_WHATS_GOING_ON: &str = "01105";
/// The Study 01111.
const STUDY: LocationId = LocationId(10);

/// Wendy's open turn at the engine's own prompt. On the board: a hand card, the
/// Amulet and her investigator card in play, an engaged enemy, the act and
/// agenda, a two-card discard pile, and End turn, with menus on the hand card
/// and the pile's top card.
fn wendys_turn() -> web::store::ClientState {
    let state = GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_location(location(STUDY).as_card("01111"))
        .with_seat(
            seat(WENDY.0)
                .as_card("01005")
                .at(STUDY)
                .hand([EMERGENCY_CACHE])
                .in_play([AMULET])
                .discard([KNIFE, EMERGENCY_CACHE]),
        )
        .with_enemy(enemy(EnemyId(1)).engaged_with(WENDY))
        .with_act_deck([ACT_TRAPPED])
        .with_agenda_deck([AGENDA_WHATS_GOING_ON])
        .open_turn(WENDY)
        .build();
    let seeded = test_support::TestSession::new(state, cards::TEST_REGISTRIES);
    let outcome = test_support::awaiting_request(seeded.prompt().clone());
    common::hello(seeded.state().clone(), outcome)
}

#[wasm_bindgen_test]
async fn the_board_mounts_and_rebuilds_without_an_untracked_read_warning() {
    let client = wendys_turn();
    let warnings = WarnCapture::start();
    let handle = common::mount_board(client.clone(), &cards::TEST_REGISTRY).await;

    // The fixture reaches every site that reads a board signal.
    let root = &handle.root;
    for sel in [
        ".hand-slot.actionable",
        ".card-slot",
        ".card--enemy",
        ".card--act",
        ".card--agenda",
        ".pile-top.actionable",
        ".turn-control",
    ] {
        assert!(!common::absent(root, sel), "the fixture mounts `{sel}`");
    }

    // A second store change rebuilds every board component.
    handle.store.update(|s| *s = client);
    task::tick().await;

    assert_eq!(warnings.untracked_reads(), Vec::<String>::new());
}
