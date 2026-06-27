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

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{CardCode, InvestigatorId, Phase, Status, Zone};
use game_core::test_support::{
    dispatch_turn_action_unchecked, test_investigator, test_location, GameStateBuilder,
};
use game_core::{assert_event_count, assert_event_sequence, assert_no_event};
use game_core::{LocationId, TurnAction};

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

/// Install the real card registry exactly once for this integration-
/// test binary. Idempotent at the `OnceLock` level; this `Once`
/// wrapper additionally avoids the futile second `install` call.
#[ctor::ctor(unsafe)]
fn install_real_registry() {
    // It's fine if this is `Err` — another test in this binary
    // already installed. The function-pointer struct is `Copy`
    // and stateless, so re-install attempts are harmless.
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Build a one-investigator scenario state at the controller's
/// location, mid-investigation, with `hand` already in hand.
fn play_state(hand: Vec<&str>) -> (game_core::GameState, InvestigatorId, LocationId) {
    let id = InvestigatorId(1);
    let loc_id = LocationId(101);
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.hand = hand.into_iter().map(CardCode::new).collect();

    let location = test_location(101, "Study");

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_location(location)
        .build();

    (state, id, loc_id)
}

#[test]
fn play_holy_rosary_emits_card_played_and_lands_in_play() {
    let (state, id, _loc) = play_state(vec![HOLY_ROSARY]);

    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = &result.state.investigators[&id];
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
            if *investigator == id && code.as_str() == HOLY_ROSARY
    );
    assert_no_event!(result.events, Event::CardDiscarded { .. });
}

#[test]
fn asset_enters_play_with_instance_id_from_state_counter() {
    // The per-state counter assigns a fresh CardInstanceId to each
    // asset entering play and advances after each assignment.
    use game_core::state::CardInstanceId;

    let (state, id, _loc) = play_state(vec![HOLY_ROSARY]);
    assert_eq!(state.card_instance_ids.peek(), 0, "counter starts at 0");

    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );
    assert_eq!(result.outcome, EngineOutcome::Done);

    let in_play = &result.state.investigators[&id].cards_in_play;
    assert_eq!(in_play.len(), 1);
    assert_eq!(
        in_play[0].instance_id,
        CardInstanceId(0),
        "first asset gets id 0",
    );
    assert_eq!(
        result.state.card_instance_ids.peek(),
        1,
        "counter advances after assigning",
    );
}

#[test]
fn two_copies_of_magnifying_glass_get_distinct_instance_ids() {
    // Magnifying Glass: Hand slot, deck_limit 2 — playing both copies
    // is rules-valid (two Hand slots per investigator). The counter
    // must assign distinct ids so per-instance state stays separable.
    use game_core::state::CardInstanceId;

    let (state, id, _loc) = play_state(vec![MAGNIFYING_GLASS, MAGNIFYING_GLASS]);

    let after_first = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );
    assert_eq!(after_first.outcome, EngineOutcome::Done);

    let after_second = dispatch_turn_action_unchecked(
        after_first.state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );
    assert_eq!(after_second.outcome, EngineOutcome::Done);

    let in_play = &after_second.state.investigators[&id].cards_in_play;
    assert_eq!(in_play.len(), 2);
    assert_eq!(in_play[0].instance_id, CardInstanceId(0));
    assert_eq!(in_play[1].instance_id, CardInstanceId(1));
    assert_eq!(
        after_second.state.card_instance_ids.peek(),
        2,
        "counter advances once per play",
    );
}

#[test]
fn play_working_a_hunch_resolves_on_play_and_discards() {
    let (mut state, id, loc_id) = play_state(vec![WORKING_A_HUNCH]);

    // Working a Hunch's OnPlay is "discover 1 clue at your location."
    // Seed the location with a clue so the discover is visible in the
    // event stream (rulebook lets you play with 0 clues too — that's
    // a separate no-op test below).
    state.locations.get_mut(&loc_id).unwrap().clues = 1;

    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = &result.state.investigators[&id];
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
        result.state.locations[&loc_id].clues, 0,
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
        Event::CluePlaced { investigator, count: 1 } if *investigator == id
    );
    assert_event_count!(
        result.events,
        1,
        Event::CardDiscarded { code, from: Zone::Hand, investigator }
            if *investigator == id && code.as_str() == WORKING_A_HUNCH
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
    let (state, id, loc_id) = play_state(vec![WORKING_A_HUNCH]);
    assert_eq!(state.locations[&loc_id].clues, 0);

    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );

    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "playing a clue-discovery event at a 0-clue location must be rejected, got {:?}",
        result.outcome,
    );
    let inv = &result.state.investigators[&id];
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
    let (state, id, _loc) = play_state(vec![UNKNOWN_CODE]);
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
    // Hand untouched.
    assert_eq!(
        result.state.investigators[&id].hand,
        vec![CardCode::new(UNKNOWN_CODE)],
    );
}

#[test]
fn play_non_event_or_asset_card_is_rejected() {
    // Only Asset and Event are playable from hand. Investigator
    // (and skill, treachery, enemy, location, agenda, act, scenario,
    // story) all reject. Roland Banks (01001) is the in-corpus
    // sample for the non-playable case.
    let (state, id, _loc) = play_state(vec![ROLAND_BANKS]);
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
    // Hand untouched.
    assert_eq!(
        result.state.investigators[&id].hand,
        vec![CardCode::new(ROLAND_BANKS)],
    );
}

#[test]
fn play_unimplemented_card_is_rejected() {
    // The canary card is in the corpus (metadata resolves) but has
    // no ability implementation yet. The deck-import gate (Phase 9)
    // will refuse decks containing unimplemented cards; PlayCard
    // double-checks rather than silently no-op.
    let (state, id, _loc) = play_state(vec![UNIMPLEMENTED_ASSET]);
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
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
    // migration: the card must be discarded exactly once (single flush site)
    // and pending_played_event must be cleared on Done.
    let (state, id, _loc) = play_state(vec![EMERGENCY_CACHE]);

    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = &result.state.investigators[&id];
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
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| matches!(
                e,
                Event::CardDiscarded {
                    from: Zone::Hand,
                    ..
                }
            ))
            .count(),
        1,
        "exactly one CardDiscarded from Hand"
    );
    assert!(
        result.state.pending_played_event.is_none(),
        "pending_played_event must be cleared on Done"
    );
}

#[test]
fn asset_play_enters_play_through_the_frame() {
    // Play Machete 01020 (asset, no OnPlay), no engaged enemy. Invariant
    // guard for the PlayFromHand frame migration: the asset must land in
    // cards_in_play and be removed from hand, and no CardDiscarded fires.
    let (state, id, _loc) = play_state(vec![MACHETE]);

    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = &result.state.investigators[&id];
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
    // check should reject before the registry lookup runs.
    let id = InvestigatorId(1);
    let mut inv = test_investigator(1);
    inv.hand = vec![CardCode::new(HOLY_ROSARY)];
    inv.status = Status::Killed;

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .build();

    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
}
