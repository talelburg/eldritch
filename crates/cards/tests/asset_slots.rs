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
use game_core::{apply, Action, InputResponse, LocationId, PlayerAction, TurnAction};

const BEAT_COP: &str = "01018"; // Guardian Ally
const GUARD_DOG: &str = "01021"; // Guardian Ally
const MACHETE: &str = "01020"; // single Hand
const KNIFE: &str = "01086"; // single Hand
const FLASHLIGHT: &str = "01087"; // single Hand

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

fn resolve(state: game_core::GameState, id: game_core::OptionId) -> game_core::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(id),
        }),
    )
}

/// Find the option whose label contains `needle` in an `AwaitingInput` outcome.
fn pick(outcome: &EngineOutcome, needle: &str) -> game_core::OptionId {
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("expected AwaitingInput, got {outcome:?}");
    };
    request
        .options
        .iter()
        .find(|o| o.label.contains(needle))
        .unwrap_or_else(|| panic!("no option matching {needle:?} in {:?}", request.options))
        .id
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

    // Ordering: CardPlayed (announcement) fires before CardDiscarded (make-room).
    // There is no separate Event::EnteredPlay game event; the asset's entry is
    // witnessed by its presence in cards_in_play (asserted above). This ordering
    // assertion verifies that the play was announced before the slot was cleared,
    // i.e. the engine follows the correct sequence per RR p.19.
    let played_pos = r2
        .events
        .iter()
        .position(|e| matches!(e, Event::CardPlayed { code, .. } if code.as_str() == GUARD_DOG))
        .expect("CardPlayed Guard Dog not found");
    let discard_pos = r2
        .events
        .iter()
        .position(|e| {
            matches!(
                e,
                Event::CardDiscarded { code, from: Zone::InPlay, .. }
                    if code.as_str() == BEAT_COP
            )
        })
        .expect("CardDiscarded Beat Cop not found");
    assert!(
        played_pos < discard_pos,
        "CardPlayed (announcement) must precede CardDiscarded (make-room, RR p.19)"
    );
}

#[test]
fn third_hand_asset_prompts_to_choose_which_to_discard() {
    // Two distinct single-Hand assets fill both Hand slots; playing a third
    // single-Hand asset must free 1 — a genuine 2-candidate choice.
    let (state, id) = play_state(vec![MACHETE, KNIFE, FLASHLIGHT]);
    let r1 = play(state, id); // Machete enters (Hand 1/2)
    let r2 = play(r1.state, id); // Knife enters (Hand 2/2)
    assert_eq!(r2.state.investigators[&id].cards_in_play.len(), 2);

    // Flashlight (index 0) — Hand full, 2 candidates → suspend for a choice.
    let r3 = play(r2.state, id);
    assert!(
        matches!(r3.outcome, EngineOutcome::AwaitingInput { .. }),
        "expected a make-room prompt, got {:?}",
        r3.outcome
    );

    // Discard Machete to make room.
    let r4 = resolve(r3.state, pick(&r3.outcome, MACHETE));
    assert_eq!(r4.outcome, EngineOutcome::Done);
    let inv = &r4.state.investigators[&id];
    let codes: Vec<&str> = inv.cards_in_play.iter().map(|c| c.code.as_str()).collect();
    assert_eq!(
        codes,
        vec![KNIFE, FLASHLIGHT],
        "Machete discarded, Knife + Flashlight in play"
    );
    assert_eq!(inv.discard, vec![CardCode::new(MACHETE)]);
}

#[test]
fn out_of_range_make_room_pick_is_rejected_and_keeps_the_prompt() {
    let (state, id) = play_state(vec![MACHETE, KNIFE, FLASHLIGHT]);
    let r1 = play(state, id);
    let r2 = play(r1.state, id);
    let r3 = play(r2.state, id);
    assert!(matches!(r3.outcome, EngineOutcome::AwaitingInput { .. }));

    // Option 99 is out of range → Rejected, the prompt persists.
    let r4 = resolve(r3.state, game_core::OptionId(99));
    assert!(
        matches!(r4.outcome, EngineOutcome::Rejected { .. }),
        "out-of-range pick rejects: {:?}",
        r4.outcome
    );
    // Still mid-investigation with both Hand assets and the pending Flashlight in
    // hand — nothing was discarded.
    let inv = &r4.state.investigators[&id];
    assert_eq!(inv.cards_in_play.len(), 2);
    assert!(inv.discard.is_empty());
    assert!(inv.hand.contains(&CardCode::new(FLASHLIGHT)));
}
