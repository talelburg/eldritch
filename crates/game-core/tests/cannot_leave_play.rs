//! `Restriction::CannotLeavePlay` against a mock registry (#1093): the
//! leave-play router refuses a card that cannot leave play, and elimination
//! removes it anyway.
//!
//! The corpus carrier is The Necronomicon 01009, *"It cannot leave play while
//! it has 1 or more horror on it."*, covered end to end in
//! `crates/cards/tests/the_necronomicon.rs`. `glossary/Cannot.md`: *"The word
//! "cannot" is absolute, and cannot be countermanded by other abilities."*
//! The one override is `glossary/Elimination.md` step 0: *"For the purpose of
//! resolving weakness cards, the game has ended for the eliminated
//! investigator. Trigger any “when the game ends” abilities on each weakness
//! the eliminated investigator owns that is in play. Then, remove those
//! weaknesses from the game."*
//!
//! A synthetic hand-slot weakness carrying the restriction models the primitive
//! (ADR 0016), so these tests also reach exits the corpus cannot: removal from
//! the game, and the slot gate for a two-handed asset, which no implemented
//! card has. Its own test binary: the mock registry is built once per process.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Slot};
use card_dsl::dsl::{self, CmpOp, Condition, Quantity, Restriction};
use game_core::engine::board::{NotLeft, Placement};
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    CardCode, CardInstanceId, GameState, GameStateBuilder, InvestigatorId, LocationId, Zone,
};
use game_core::test_support::{self, horror, seat, MockRegistry, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_no_event};

/// A hand-slot weakness that cannot leave play while it has horror on it.
const PINNED: &str = "TEST-PINNED";
/// A two-handed asset, in hand.
const TWO_HANDED: &str = "TEST-TWO-HANDED";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

fn hand_asset(code: &str, slots: Vec<Slot>, weakness: bool) -> CardMetadata {
    CardMetadata {
        code: code.to_string(),
        name: code.to_string(),
        text: None,
        traits: vec![],
        back_name: None,
        back_text: None,
        pack_code: "test".to_string(),
        weakness,
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            slots,
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 1,
            uses: None,
            play_only_during_turn: false,
        },
    }
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(hand_asset(PINNED, vec![Slot::Hand], true))
            .with_card(hand_asset(TWO_HANDED, vec![Slot::Hand, Slot::Hand], false))
            .with_abilities(PINNED, || {
                vec![dsl::constant(dsl::restrict(Restriction::CannotLeavePlay {
                    while_: Condition::Compare {
                        quantity: Quantity::HorrorOnSource,
                        op: CmpOp::Ge,
                        value: 1,
                    },
                }))]
            })
            .with_abilities(TWO_HANDED, Vec::new)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// `INV`'s open turn at `HERE`, with `PINNED` in their threat area carrying
/// `horror_on_it` horror and `TWO_HANDED` in hand.
fn board(horror_on_it: u8) -> TestSession {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .threat_area([(PINNED, horror(horror_on_it))])
                .hand([TWO_HANDED]),
        )
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .session()
}

/// Run `exit` on `PINNED` through a native's exits, as a card-local native
/// discarding or removing it would. Returns the exit's answer and its events.
fn exit_pinned(
    session: &TestSession,
    exit: fn(&mut NativeCx, CardInstanceId) -> Result<Placement, NotLeft>,
) -> (GameState, Result<(), NotLeft>, Vec<Event>) {
    let mut state = session.state().clone();
    let pinned = state.in_play(INV, PINNED);
    let mut left = Err(NotLeft::NotInPlay);
    let (outcome, events) = test_support::run_native(
        &mut state,
        registries().cards,
        |cx, _| {
            left = exit(cx, pinned).map(|_| ());
            EngineOutcome::Done
        },
        &EvalContext::for_controller(INV),
    );
    assert!(matches!(outcome, EngineOutcome::Done));
    (state, left, events)
}

fn discard(cx: &mut NativeCx, id: CardInstanceId) -> Result<Placement, NotLeft> {
    cx.discard_from_play(id)
}

fn remove(cx: &mut NativeCx, id: CardInstanceId) -> Result<Placement, NotLeft> {
    cx.remove_from_game(id)
}

fn pinned_horror(state: &GameState) -> Option<u8> {
    state.investigators[&INV]
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == PINNED)
        .map(|c| c.accumulated_horror)
}

#[test]
fn the_router_refuses_a_card_that_cannot_leave_play() {
    let session = board(1);
    for exit in [discard, remove] {
        let (state, left, events) = exit_pinned(&session, exit);
        assert_eq!(left, Err(NotLeft::Refused));
        assert_eq!(pinned_horror(&state), Some(1), "it stays, horror and all");
        assert!(
            events.is_empty(),
            "a refused exit emits nothing: {events:?}"
        );
        let inv = &state.investigators[&INV];
        assert!(inv.discard.is_empty() && inv.removed_from_game.is_empty());
    }
}

#[test]
fn with_no_horror_on_it_the_card_leaves_play() {
    let session = board(0);
    let (state, left, _) = exit_pinned(&session, discard);
    assert_eq!(left, Ok(()));
    assert_eq!(pinned_horror(&state), None);
    assert_eq!(
        state.investigators[&INV].discard,
        vec![CardCode::new(PINNED)]
    );
}

/// `TEST_INV` has 8 health, so 8 damage eliminates them. Step 1 removes the
/// weakness they own in their threat area with its horror still on it.
#[test]
fn elimination_removes_a_card_that_cannot_leave_play() {
    let session = board(2).take_damage(INV, 8);
    let state = session.state();
    assert_eq!(pinned_horror(state), None, "it left the threat area");
    assert!(
        state.investigators[&INV]
            .removed_from_game
            .contains(&CardCode::new(PINNED)),
        "removed from the game, to its owner's pile"
    );
    assert_event!(
        session.events(),
        Event::CardRemovedFromGame { code, from: Zone::ThreatArea } if code.as_str() == PINNED
    );
}

/// The slot gate counts a card make-room may not discard as fixed: with the
/// pinned card in one of the two hand slots, a two-handed asset cannot be
/// played, rather than reaching a make-room with nothing to offer.
#[test]
fn a_two_handed_asset_is_unplayable_beside_a_pinned_hand_slot_card() {
    let play = TurnAction::PlayCard {
        investigator: INV,
        hand_index: 0,
    };
    let pinned = board(1);
    assert!(
        !enumerate::legal_actions(pinned.state(), registries().cards).contains(&play),
        "the play is off the menu"
    );
    let reason = pinned.assert_rejects(|s| s.submit_off_menu(&play));
    assert!(reason.contains("Hand"), "{reason}");

    // Without horror the pinned card may go, so make-room discards it.
    let free = board(0).take(&play);
    free.assert_at_turn_menu(INV);
    let inv = &free.state().investigators[&INV];
    assert_eq!(inv.discard, vec![CardCode::new(PINNED)]);
    assert_no_event!(free.events(), Event::CardRemovedFromGame { .. });
}
