//! Asset slot limits + discard-to-make-room (#498), against the real corpus.
//!
//! Mirrors the `play_card.rs` harness: a process-global registry install and a
//! one-investigator mid-investigation state.

use cards::REGISTRY;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{EngineOutcome, OptionId, OptionTarget};
use game_core::event::Event;
use game_core::state::{CardCode, DiscardPile, GameStateBuilder, InvestigatorId, LocationId, Zone};
use game_core::test_support::{self, TestSession};

const BEAT_COP: &str = "01018"; // Guardian Ally
const GUARD_DOG: &str = "01021"; // Guardian Ally
const MACHETE: &str = "01020"; // single Hand
const KNIFE: &str = "01086"; // single Hand
const FLASHLIGHT: &str = "01087"; // single Hand

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// A one-investigator open turn with `hand` in hand, plenty of resources and
/// actions.
fn play_state(hand: Vec<&str>) -> (TestSession, InvestigatorId) {
    let id = InvestigatorId(1);
    let loc_id = LocationId(101);
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.resources = 20;
    inv.actions_remaining = 6;
    inv.hand = hand.into_iter().map(CardCode::new).collect();

    let session = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_support::test_location(101, "Study"))
        .open_turn(id)
        .session();
    (session, id)
}

/// Play the first card in hand from the turn menu.
fn play(session: TestSession, id: InvestigatorId) -> TestSession {
    session.take(&TurnAction::PlayCard {
        investigator: id,
        hand_index: 0,
    })
}

fn at_turn_menu(session: &TestSession, id: InvestigatorId) -> bool {
    session.prompt().target == Some(OptionTarget::TurnControl(id))
}

#[test]
fn playing_a_second_ally_auto_discards_the_first() {
    let (session, id) = play_state(vec![BEAT_COP, GUARD_DOG]);

    // Beat Cop enters (Ally slot now full).
    let session = play(session, id);
    assert!(at_turn_menu(&session, id));
    assert_eq!(session.state().investigators[&id].cards_in_play.len(), 1);
    assert_eq!(
        session.state().investigators[&id].cards_in_play[0].code,
        CardCode::new(BEAT_COP)
    );

    // Guard Dog (the only card left in hand, index 0) — Ally slot full, single
    // candidate (Beat Cop) → auto-discard Beat Cop, Guard Dog enters.
    let r2 = play(session, id).finish();
    assert!(matches!(r2.outcome, EngineOutcome::AwaitingInput { .. }));
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
            Event::CardDiscarded { code, from: Zone::InPlay, to: DiscardPile::Investigator(investigator) }
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
    let (session, id) = play_state(vec![MACHETE, KNIFE, FLASHLIGHT]);
    let session = play(session, id); // Machete enters (Hand 1/2)
    let session = play(session, id); // Knife enters (Hand 2/2)
    assert_eq!(session.state().investigators[&id].cards_in_play.len(), 2);

    // Flashlight (index 0) — Hand full, 2 candidates → suspend for a choice.
    let session = play(session, id);
    assert!(
        !at_turn_menu(&session, id),
        "expected a make-room prompt, got {:?}",
        session.prompt()
    );

    // Discard Machete (played first) to make room, by the instance its
    // option anchors to.
    let machete = session.state().investigators[&id].cards_in_play[0].instance_id;
    let session = session.pick(OptionTarget::CardInstance(machete));
    assert!(at_turn_menu(&session, id));
    let inv = &session.state().investigators[&id];
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
    let (session, id) = play_state(vec![MACHETE, KNIFE, FLASHLIGHT]);
    let session = play(play(play(session, id), id), id);
    let prompt = session.prompt().clone();
    assert!(!at_turn_menu(&session, id));

    // Option 99 is out of range → Rejected, the prompt persists. A malformed
    // id is the point here, so the response is raw rather than a `pick`.
    let session = session.apply(Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(99)),
    }));
    assert!(!session.expect_rejected().is_empty());
    assert_eq!(
        session.prompt(),
        &prompt,
        "the make-room prompt still stands after a rejected pick"
    );
    // Both Hand assets in play and Flashlight still mid-play — nothing was
    // discarded.
    let inv = &session.state().investigators[&id];
    assert_eq!(inv.cards_in_play.len(), 2);
    assert!(inv.discard.is_empty());
    assert_eq!(
        session.state().play_in_progress().map(|(_, c)| c.clone()),
        Some(CardCode::new(FLASHLIGHT)),
        "the mid-play asset rides the make-room prompt",
    );
}
