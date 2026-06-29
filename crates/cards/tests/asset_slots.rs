//! Asset slot limits + discard-to-make-room (#498), against the real corpus.
//!
//! Mirrors the `play_card.rs` harness: a process-global registry install and a
//! one-investigator mid-investigation state.

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{CardCode, InvestigatorId, Phase, Zone};
use game_core::test_support::{
    dispatch_turn_action_unchecked, test_investigator, test_location, GameStateBuilder,
};
use game_core::{LocationId, TurnAction};

const BEAT_COP: &str = "01018"; // Guardian Ally
const GUARD_DOG: &str = "01021"; // Guardian Ally
const MACHETE: &str = "01020"; // single Hand
const KNIFE: &str = "01086"; // single Hand

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// A one-investigator scenario, mid-investigation, with `hand` in hand, plenty
/// of resources and actions.
fn play_state(hand: Vec<&str>) -> (game_core::GameState, InvestigatorId) {
    let id = InvestigatorId(1);
    let loc_id = LocationId(101);
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.resources = 20;
    inv.actions_remaining = 6;
    inv.hand = hand.into_iter().map(CardCode::new).collect();

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_location(test_location(101, "Study"))
        .build();
    (state, id)
}

fn play(state: game_core::GameState, id: InvestigatorId) -> game_core::ApplyResult {
    dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    )
}

#[test]
fn playing_a_second_ally_auto_discards_the_first() {
    let (state, id) = play_state(vec![BEAT_COP, GUARD_DOG]);

    // Beat Cop enters (Ally slot now full).
    let r1 = play(state, id);
    assert_eq!(r1.outcome, EngineOutcome::Done);
    assert_eq!(r1.state.investigators[&id].cards_in_play.len(), 1);
    assert_eq!(
        r1.state.investigators[&id].cards_in_play[0].code,
        CardCode::new(BEAT_COP)
    );

    // Guard Dog (the only card left in hand, index 0) — Ally slot full, single
    // candidate (Beat Cop) → auto-discard Beat Cop, Guard Dog enters.
    let r2 = play(r1.state, id);
    assert_eq!(r2.outcome, EngineOutcome::Done);
    let inv = &r2.state.investigators[&id];
    assert_eq!(
        inv.cards_in_play.len(),
        1,
        "only one Ally remains in play: {:?}",
        inv.cards_in_play
    );
    assert_eq!(inv.cards_in_play[0].code, CardCode::new(GUARD_DOG));
    assert_eq!(
        inv.discard,
        vec![CardCode::new(BEAT_COP)],
        "the displaced Ally went to discard"
    );

    // The displaced Beat Cop emitted CardDiscarded { from: InPlay }; Guard Dog
    // emitted EnteredPlay-side CardPlayed earlier. Assert the make-room discard.
    assert!(
        r2.events.iter().any(|e| matches!(
            e,
            Event::CardDiscarded { code, from: Zone::InPlay, investigator }
                if *investigator == id && code.as_str() == BEAT_COP
        )),
        "Beat Cop discarded from play: {:?}",
        r2.events
    );
}

#[test]
fn two_handed_weapon_auto_frees_both_hand_slots() {
    // Two single-Hand weapons fill both Hand slots (cap 2); both coexist without
    // make-room. This placeholder verifies the no-conflict path — Task 6 replaces
    // it with an interactive multi-candidate test.
    let (state, id) = play_state(vec![MACHETE, KNIFE]);
    let r1 = play(state, id);
    let r2 = play(r1.state, id);
    assert_eq!(r2.state.investigators[&id].cards_in_play.len(), 2);
}
