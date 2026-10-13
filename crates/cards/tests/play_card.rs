//! End-to-end `PlayCard` integration tests with the real card corpus.
//!
//! These tests install the `cards::REGISTRY` global once and exercise
//! the engine's `PlayCard` handler against actual Phase-2 cards (Holy
//! Rosary, Working a Hunch). Lives in `cards/tests/` rather than
//! `game-core/src/engine/`'s unit tests because:
//!
//! - The engine crate can't depend on `cards` (cycle) so it has no
//!   way to access real card metadata or abilities.
//! - Installing `REGISTRY` is process-global. As an integration test
//!   binary this file gets its own process, so its install doesn't
//!   collide with `game-core`'s own tests (which deliberately don't
//!   install a registry, exercising only validation paths that
//!   short-circuit before the registry lookup).

use cards::REGISTRY;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInstanceId, DiscardPile, GameStateBuilder, InvestigatorId, LocationId, Status,
    Zone,
};
use game_core::test_support::{self, seat, Seat, TestSession};
use game_core::{assert_event_count, assert_event_sequence, assert_no_event};

/// Holy Rosary (01059) — Mystic asset, +1 willpower constant.
const HOLY_ROSARY: &str = "01059";

/// Working a Hunch (01037) — Seeker event, on-play "discover 1 clue
/// at your location."
const WORKING_A_HUNCH: &str = "01037";

/// Fire Axe (02032) — a Dunwich Legacy Survivor weapon asset, in the
/// corpus but unimplemented. Chosen as the "unimplemented but known"
/// rejection canary precisely because it's far-future content (the
/// Dunwich cycle is Phase 10), so it stays unimplemented long after
/// the Core Set assets land and won't churn this test.
const UNIMPLEMENTED_ASSET: &str = "02032";

/// Magnifying Glass (01030) — Seeker Hand-slot, deck-limit 2. Two
/// copies in play simultaneously is rules-valid (one Hand slot each,
/// both slots filled); used here for the multi-copy counter test.
const MAGNIFYING_GLASS: &str = "01030";

/// Roland Banks (01001) — investigator card. Represents the player
/// character itself; never legally in hand. Used as the
/// "investigator-type-from-hand" rejection case.
const ROLAND_BANKS: &str = "01001";

const UNKNOWN_CODE: &str = "99999";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);

/// Play the first card in hand.
const PLAY_FIRST: TurnAction = TurnAction::PlayCard {
    investigator: INV,
    hand_index: 0,
};

/// Install the real card registry exactly once for this integration-
/// test binary. Idempotent at the `OnceLock` level; this `Once`
/// wrapper additionally avoids the futile second `install` call.
#[ctor::ctor(unsafe)]
fn install_real_registry() {
    // It's fine if this is `Err` — another test in this binary
    // already installed. The function-pointer struct is `Copy`
    // and stateless, so re-install attempts are harmless.
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Investigator 1 at the Study, holding `hand`.
fn holding(hand: &[&str]) -> Seat {
    seat(1).at(HERE).hand(hand.iter().copied())
}

/// A one-investigator scenario at `investigator`'s turn menu, at the Study,
/// which holds `clues` clues.
fn play_board(investigator: Seat, clues: u8) -> TestSession {
    let mut location = test_support::test_location(HERE.0, "Study");
    location.clues = clues;
    GameStateBuilder::new()
        .with_seat(investigator)
        .with_location(location)
        .open_turn(INV)
        .session()
}

/// A one-investigator scenario at the controller's turn menu, at a clueless
/// Study, with `hand` already in hand.
fn play_state(hand: &[&str]) -> TestSession {
    play_board(holding(hand), 0)
}

/// Send the play of the first card in hand straight to its handler, after
/// checking the menu does not offer it: proving the handler itself rejects
/// what the menu filters out.
fn play_off_menu(session: TestSession) -> TestSession {
    assert!(
        !enumerate::legal_actions(session.state()).contains(&PLAY_FIRST),
        "the turn menu does not offer a play its handler rejects",
    );
    let refused = session.submit_off_menu(&PLAY_FIRST);
    refused.expect_rejected();
    refused
}

#[test]
fn play_holy_rosary_emits_card_played_and_lands_in_play() {
    let session = play_state(&[HOLY_ROSARY]).take(&PLAY_FIRST);

    session.assert_at_turn_menu(INV);
    let result = session.finish();
    let inv = &result.state.investigators[&INV];
    assert!(inv.hand.is_empty(), "hand should be empty after play");
    assert_eq!(
        inv.cards_in_play.len(),
        1,
        "asset should land in cards_in_play"
    );
    assert_eq!(inv.cards_in_play[0].code, CardCode::new(HOLY_ROSARY));
    assert!(
        !inv.cards_in_play[0].exhausted,
        "asset enters play ready, not exhausted",
    );
    assert!(inv.discard.is_empty(), "asset should not land in discard");

    assert_event_count!(
        result.events,
        1,
        Event::CardPlayed { investigator, code }
            if *investigator == INV && code.as_str() == HOLY_ROSARY
    );
    assert_no_event!(result.events, Event::CardDiscarded { .. });
}

#[test]
fn asset_enters_play_with_instance_id_from_state_counter() {
    // The per-state counter assigns a fresh CardInstanceId to each
    // asset entering play and advances after each assignment.

    let session = play_state(&[HOLY_ROSARY]);
    assert_eq!(
        session.state().card_instance_ids.peek(),
        0,
        "counter starts at 0"
    );

    let session = session.take(&PLAY_FIRST);
    session.assert_at_turn_menu(INV);

    let in_play = &session.state().investigators[&INV].cards_in_play;
    assert_eq!(in_play.len(), 1);
    assert_eq!(
        in_play[0].instance_id,
        CardInstanceId(0),
        "first asset gets id 0",
    );
    assert_eq!(
        session.state().card_instance_ids.peek(),
        1,
        "counter advances after assigning",
    );
}

#[test]
fn two_copies_of_magnifying_glass_get_distinct_instance_ids() {
    // Magnifying Glass: Hand slot, deck_limit 2 — playing both copies
    // is rules-valid (two Hand slots per investigator). The counter
    // must assign distinct ids so per-instance state stays separable.

    let after_first = play_state(&[MAGNIFYING_GLASS, MAGNIFYING_GLASS]).take(&PLAY_FIRST);
    after_first.assert_at_turn_menu(INV);

    let after_second = after_first.take(&PLAY_FIRST);
    after_second.assert_at_turn_menu(INV);

    let in_play = &after_second.state().investigators[&INV].cards_in_play;
    assert_eq!(in_play.len(), 2);
    assert_eq!(in_play[0].instance_id, CardInstanceId(0));
    assert_eq!(in_play[1].instance_id, CardInstanceId(1));
    assert_eq!(
        after_second.state().card_instance_ids.peek(),
        2,
        "counter advances once per play",
    );
}

#[test]
fn play_working_a_hunch_resolves_on_play_and_discards() {
    // Working a Hunch's OnPlay is "discover 1 clue at your location."
    // Seed the location with a clue so the discover is visible in the
    // event stream (rulebook lets you play with 0 clues too — that's
    // a separate no-op test below).
    let session = play_board(holding(&[WORKING_A_HUNCH]), 1).take(&PLAY_FIRST);

    session.assert_at_turn_menu(INV);
    let result = session.finish();
    let inv = &result.state.investigators[&INV];
    assert!(inv.hand.is_empty(), "hand should be empty after play");
    assert!(
        inv.cards_in_play.is_empty(),
        "event should not land in cards_in_play",
    );
    assert_eq!(
        inv.discard,
        vec![CardCode::new(WORKING_A_HUNCH)],
        "event should land in discard after on-play",
    );
    assert_eq!(inv.clues, 1, "discovered 1 clue from location");
    assert_eq!(
        result.state.locations[&HERE].clues, 0,
        "location's clue moved to investigator",
    );

    // Each expected event fires exactly once. Ordering is asserted
    // separately below because the macros are order-insensitive.
    assert_event_count!(
        result.events,
        1,
        Event::CardPlayed { code, .. } if code.as_str() == WORKING_A_HUNCH
    );
    assert_event_count!(
        result.events,
        1,
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV
    );
    assert_event_count!(
        result.events,
        1,
        Event::CardDiscarded { code, from: Zone::Hand, to: DiscardPile::Investigator(investigator) }
            if *investigator == INV && code.as_str() == WORKING_A_HUNCH
    );

    // Ordering: CardPlayed < CluePlaced < CardDiscarded. Matters
    // because event listeners building causal chains (#52 reaction
    // windows) will key off the order.
    assert_event_sequence!(
        result.events,
        Event::CardPlayed { .. },
        Event::CluePlaced { .. },
        Event::CardDiscarded { .. },
    );
}

#[test]
fn play_working_a_hunch_on_empty_location_is_rejected() {
    // RR p.11 (#495): "An event card cannot be played unless the resolution of
    // its effect has the potential to change the game state." Working a Hunch's
    // only effect is "discover 1 clue at your location"; at a 0-clue location it
    // would discover nothing, so the play is **rejected** — not played-and-
    // fizzled. The card stays in hand, nothing is discarded, no clue moves.
    let session = play_state(&[WORKING_A_HUNCH]);
    assert_eq!(session.state().locations[&HERE].clues, 0);

    let result = play_off_menu(session).finish();

    let inv = &result.state.investigators[&INV];
    assert_eq!(
        inv.hand,
        vec![CardCode::new(WORKING_A_HUNCH)],
        "rejected play leaves the card in hand"
    );
    assert!(inv.discard.is_empty(), "rejected play discards nothing");
    assert_no_event!(result.events, Event::CluePlaced { .. });
    assert_no_event!(result.events, Event::CardPlayed { .. });
}

#[test]
fn play_unknown_card_code_is_rejected() {
    let result = play_off_menu(play_state(&[UNKNOWN_CODE])).finish();
    assert!(result.events.is_empty());
    // Hand untouched.
    assert_eq!(
        result.state.investigators[&INV].hand,
        vec![CardCode::new(UNKNOWN_CODE)],
    );
}

#[test]
fn play_non_event_or_asset_card_is_rejected() {
    // Only Asset and Event are playable from hand. Investigator
    // (and skill, treachery, enemy, location, agenda, act, scenario,
    // story) all reject. Roland Banks (01001) is the in-corpus
    // sample for the non-playable case.
    let result = play_off_menu(play_state(&[ROLAND_BANKS])).finish();
    assert!(result.events.is_empty());
    // Hand untouched.
    assert_eq!(
        result.state.investigators[&INV].hand,
        vec![CardCode::new(ROLAND_BANKS)],
    );
}

#[test]
fn play_unimplemented_card_is_rejected() {
    // The canary card is in the corpus (metadata resolves) but has
    // no ability implementation yet. The deck-import gate (Phase 9)
    // will refuse decks containing unimplemented cards; PlayCard
    // double-checks rather than silently no-op.
    let result = play_off_menu(play_state(&[UNIMPLEMENTED_ASSET])).finish();
    assert!(result.events.is_empty());
}

/// Emergency Cache (01088) — Neutral event, non-fast, `OnPlay` `GainResources(3)`.
const EMERGENCY_CACHE: &str = "01088";

/// Machete (01020) — Guardian weapon asset, no `OnPlay`.
const MACHETE: &str = "01020";

#[test]
fn normal_event_play_discards_exactly_once() {
    // Play Emergency Cache 01088 (event, OnPlay GainResources 3) with no
    // engaged enemy (no AoO). Invariant guard for the PlayFromHand frame
    // migration: the card must be discarded exactly once (single disposal site)
    // and no play may be left in progress once it completes.
    let session = play_state(&[EMERGENCY_CACHE]).take(&PLAY_FIRST);

    session.assert_at_turn_menu(INV);
    let result = session.finish();
    let inv = &result.state.investigators[&INV];
    // Card left hand and landed in discard.
    assert!(inv.hand.is_empty(), "hand should be empty after play");
    assert!(inv.cards_in_play.is_empty(), "event should not enter play");
    assert_eq!(
        inv.discard,
        vec![CardCode::new(EMERGENCY_CACHE)],
        "event should be in discard"
    );
    // Resources gained.
    assert!(inv.resources > 0, "resources should be gained");
    // Exactly one CardDiscarded(Hand) — the migration invariant.
    assert_event_count!(
        result.events,
        1,
        Event::CardDiscarded {
            from: Zone::Hand,
            ..
        }
    );
    assert!(
        result.state.play_in_progress().is_none(),
        "no play may be left in progress once it completes"
    );
}

#[test]
fn asset_play_enters_play_through_the_frame() {
    // Play Machete 01020 (asset, no OnPlay), no engaged enemy. Invariant
    // guard for the PlayFromHand frame migration: the asset must land in
    // cards_in_play and be removed from hand, and no CardDiscarded fires.
    let session = play_state(&[MACHETE]).take(&PLAY_FIRST);

    session.assert_at_turn_menu(INV);
    let result = session.finish();
    let inv = &result.state.investigators[&INV];
    // Asset removed from hand.
    assert!(inv.hand.is_empty(), "hand should be empty after play");
    // Asset landed in cards_in_play.
    assert_eq!(
        inv.cards_in_play.len(),
        1,
        "asset should land in cards_in_play"
    );
    assert_eq!(inv.cards_in_play[0].code, CardCode::new(MACHETE));
    // No discard for assets.
    assert!(inv.discard.is_empty(), "asset should not land in discard");
    assert_no_event!(result.events, Event::CardDiscarded { .. });
}

#[test]
fn play_card_after_defeat_is_rejected() {
    // Belt-and-suspenders: even with REGISTRY installed, the status
    // check should reject before the registry lookup runs. A seat has no
    // status setter, so the defeat is a board edit before settling.
    let mut state = GameStateBuilder::new()
        .with_seat(seat(1).hand([HOLY_ROSARY]))
        .open_turn(INV)
        .build();
    state.investigators.get_mut(&INV).expect("seated").status = Status::Defeated;

    let result = play_off_menu(TestSession::new(state)).finish();
    assert!(result.events.is_empty());
}

// --- Resource-cost payment (#501) -----------------------------------------
//
// Playing a card is paying its cost (RR p.22, Initiation Sequence): a card
// cannot be played unless its resource cost can be paid in full, and on a
// successful play the cost is deducted before attacks of opportunity resolve.
// Holy Rosary (01059) costs 2 resources (verified against the corpus).

#[test]
fn play_card_rejected_when_resources_below_cost() {
    let session = play_board(holding(&[HOLY_ROSARY]).resources(1), 0); // cost 2

    let result = play_off_menu(session).finish();

    let inv = &result.state.investigators[&INV];
    assert_eq!(inv.resources, 1, "resources unchanged on a rejected play");
    assert_eq!(inv.hand.len(), 1, "card stays in hand on a rejected play");
    assert!(inv.cards_in_play.is_empty(), "nothing enters play");
    assert!(result.events.is_empty(), "no events on rejection");
}

#[test]
fn play_card_deducts_resource_cost_and_emits_resources_paid() {
    let session = play_board(holding(&[HOLY_ROSARY]).resources(5), 0).take(&PLAY_FIRST); // cost 2

    session.assert_at_turn_menu(INV);
    let result = session.finish();
    assert_eq!(
        result.state.investigators[&INV].resources, 3,
        "5 - cost 2 = 3 after a successful play",
    );
    assert_event_count!(
        result.events,
        1,
        Event::ResourcesPaid { investigator, amount }
            if *investigator == INV && *amount == 2
    );
}

#[test]
fn play_card_at_exactly_its_cost_succeeds() {
    let session = play_board(holding(&[HOLY_ROSARY]).resources(2), 0).take(&PLAY_FIRST); // cost 2

    session.assert_at_turn_menu(INV);
    assert_eq!(
        session.state().investigators[&INV].resources,
        0,
        "paying the full cost empties the pool",
    );
}

#[test]
fn play_single_hand_asset_is_not_slot_rejected_on_empty_hands() {
    // Sanity: the new slot gate must not over-reject a normal single-slot asset
    // with both Hand slots free. Machete takes a single Hand slot and costs 3;
    // test_investigator starts with 5 resources, so no override is needed.
    let session = play_state(&[MACHETE]).take(&PLAY_FIRST);
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].cards_in_play.len(), 1);
}

#[test]
fn play_zero_cost_card_is_free_and_emits_no_resources_paid() {
    // Emergency Cache (01088) costs 0: affordable even at 0 resources, and a
    // 0-resource deduction emits no ResourcesPaid event.
    let session = play_board(holding(&[EMERGENCY_CACHE]).resources(0), 0).take(&PLAY_FIRST);

    session.assert_at_turn_menu(INV);
    assert_no_event!(session.finish().events, Event::ResourcesPaid { .. });
}
