//! #476 regression: a framework Fast window no longer strands. Real registries,
//! Roland (01001), Rotting Remains (01163) on the encounter deck, a forced
//! Cultist bag, and Magnifying Glass (01030, a Fast asset) in hand. After the
//! Mythos draw + failed willpower test resolve, the `InvestigatorTurnBegins` Fast
//! window finds a fast play eligible and surfaces a SKIPPABLE `PickSingle` (not a
//! `Done`-idle strand). Skipping reaches the open turn; picking the option plays
//! the asset.

use game_core::action::RosterEntry;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{self, InputKind, OptionTarget};
use game_core::scenario_registry;
use game_core::state::{CardCode, ChaosToken, InvestigatorId, Phase};
use game_core::test_support::{self, TestSession};
use scenarios::the_gathering;

const INV: InvestigatorId = InvestigatorId(1);
const MAGNIFYING_GLASS: &str = "01030";

#[ctor::ctor(unsafe)]
fn install_registries() {
    let _ = scenario_registry::install(scenarios::REGISTRY);
    test_support::install_registry_with_test_cards(cards::REGISTRY);
}

/// Drive to the post-Mythos-draw fast window: Roland holds Magnifying Glass,
/// fails the Rotting Remains willpower test (Cultist -1), and the
/// `InvestigatorTurnBegins` fast window prompts. Returns the session there.
fn to_fast_window() -> TestSession {
    let roster = vec![RosterEntry {
        investigator: CardCode::new("01001"),
        deck: vec![],
    }];
    // seat_and_open rests at the mulligan; Roland keeps his (empty) hand.
    let mut state = TestSession::new(engine::seat_and_open(the_gathering::setup(), &roster).state)
        .choose_from_hand(&[])
        .finish()
        .state;
    state.encounter_deck = vec![CardCode::new("01163")].into();
    state.encounter_discard.clear();
    state.chaos_bag.tokens = vec![ChaosToken::Cultist];
    state
        .investigators
        .get_mut(&INV)
        .unwrap()
        .hand
        .push(CardCode::new(MAGNIFYING_GLASS));

    // End the turn into round 2's Mythos draw, draw Rotting Remains, and
    // commit nothing to its willpower test.
    TestSession::new(state)
        .take(&TurnAction::EndTurn)
        .confirm()
        .commit(&[])
}

#[test]
fn parked_fast_window_prompts_instead_of_stranding() {
    let session = to_fast_window();
    // `prompt()` panics on a `Done` rest, so reaching it is the no-strand claim.
    let request = session.prompt();
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert!(
        request.skippable,
        "the fast window must be skippable (pass): {request:?}"
    );
    assert!(
        !request.options.is_empty(),
        "the fast window lists the eligible fast plays: {request:?}"
    );
    assert_eq!(session.state().phase, Phase::Investigation);
    assert_eq!(session.state().round, 2);
}

#[test]
fn skipping_the_fast_window_reaches_the_open_turn() {
    to_fast_window().skip().assert_at_turn_menu(INV);
}

#[test]
fn playing_the_fast_card_puts_magnifying_glass_in_play() {
    let session = to_fast_window();
    // The Fast window anchors each play to the hand card it plays.
    let glass = OptionTarget::HandCard {
        investigator: INV,
        hand_index: session.state().investigators[&INV]
            .hand
            .iter()
            .position(|c| *c == CardCode::new(MAGNIFYING_GLASS))
            .and_then(|i| u8::try_from(i).ok())
            .expect("Magnifying Glass is in hand"),
    };
    let session = session.pick(glass);
    assert!(
        session.state().investigators[&INV]
            .cards_in_play
            .iter()
            .any(|c| c.code == CardCode::new(MAGNIFYING_GLASS)),
        "Magnifying Glass entered play after the fast-window pick"
    );
}
