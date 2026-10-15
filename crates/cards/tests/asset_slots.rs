//! Asset slot limits + discard-to-make-room (#498), against the real corpus.
//!
//! Mirrors the `play_card.rs` harness: the real registries, named, and a
//! one-investigator mid-investigation state.

use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::assert_event;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::{OptionId, OptionTarget};
use game_core::event::Event;
use game_core::state::{CardCode, DiscardPile, GameStateBuilder, InvestigatorId, LocationId, Zone};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};

const BEAT_COP: &str = "01018"; // Guardian Ally
const GUARD_DOG: &str = "01021"; // Guardian Ally
const MACHETE: &str = "01020"; // single Hand
const KNIFE: &str = "01086"; // single Hand
const FLASHLIGHT: &str = "01087"; // single Hand

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);

/// A one-investigator open turn with `hand` in hand, plenty of resources and
/// actions.
fn play_state(hand: &[&str]) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .resources(20)
                .actions(6)
                .hand(hand.iter().copied()),
        )
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .session()
}

/// Play the first card in hand from the turn menu.
fn play(session: TestSession) -> TestSession {
    session.take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    })
}

/// Machete, Knife and Flashlight in hand; the first two played to fill both
/// Hand slots, then Flashlight played into the full slots, which suspends on
/// the make-room choice between the two.
fn third_hand_asset_make_room() -> TestSession {
    let session = play(play(play_state(&[MACHETE, KNIFE, FLASHLIGHT]))); // Hand 2/2
    assert_eq!(session.state().investigators[&INV].cards_in_play.len(), 2);
    let machete = session.state().in_play(INV, MACHETE);
    let knife = session.state().in_play(INV, KNIFE);

    // Flashlight (index 0) — Hand full, 2 candidates → suspend for a choice.
    let session = play(session);
    assert_eq!(
        (
            session.offered_at(&OptionTarget::CardInstance(machete)),
            session.offered_at(&OptionTarget::CardInstance(knife)),
        ),
        (1, 1),
        "expected a make-room prompt offering both Hand assets, got {:?}",
        session.prompt()
    );
    session
}

#[test]
fn playing_a_second_ally_auto_discards_the_first() {
    let session = play_state(&[BEAT_COP, GUARD_DOG]);

    // Beat Cop enters (Ally slot now full).
    let session = play(session);
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].cards_in_play.len(), 1);
    assert_eq!(
        session.state().investigators[&INV].cards_in_play[0].code,
        CardCode::new(BEAT_COP)
    );

    // Guard Dog (the only card left in hand, index 0) — Ally slot full, single
    // candidate (Beat Cop) → auto-discard Beat Cop, Guard Dog enters.
    let session = play(session);
    session.assert_at_turn_menu(INV);
    let r2 = session.finish();
    let id = INV;
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
    assert_event!(
        r2.events,
        Event::CardDiscarded { code, from: Zone::InPlay, to: DiscardPile::Investigator(investigator) }
            if *investigator == id && code.as_str() == BEAT_COP
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
    let session = third_hand_asset_make_room();

    // Discard Machete (played first) to make room, by the instance its
    // option anchors to.
    let machete = session.state().in_play(INV, MACHETE);
    let session = session.pick(OptionTarget::CardInstance(machete));
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
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
    let session = third_hand_asset_make_room();
    let prompt = session.prompt().clone();

    // Option 99 is out of range → Rejected, the prompt persists. A malformed
    // id is the point here — the subject is the wire `ResolveInput` itself —
    // so the response is raw rather than a `pick`.
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
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.cards_in_play.len(), 2);
    assert!(inv.discard.is_empty());
    assert_eq!(
        session.state().play_in_progress().map(|(_, c)| c.clone()),
        Some(CardCode::new(FLASHLIGHT)),
        "the mid-play asset rides the make-room prompt",
    );
}
