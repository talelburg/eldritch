//! Regression for #631: a card committed to a skill test is the same
//! card at ST.2, at ST.5, and at ST.8 — even when the hand it came from is
//! mutated in between.
//!
//! The engine deliberately opens the Rules Reference p.26 player window between
//! ST.2 and ST.3, and that window offers Fast plays out of the very hand the
//! commits came from. Committing therefore moves the cards out of the hand and
//! into limbo on the in-flight record — `Limbo.md`, the Rules Reference
//! glossary entry added by the FAQ:
//!
//! > While the effects of an event or treachery card are being resolved, or
//! > while a skill is committed to a skill test, it is neither in play, in the
//! > discard pile, nor is it in an investigator's hand.
//!
//! Lives at `crates/cards/tests/` because it needs the real corpus: the
//! divergence only reproduces with cards whose printed icons and Fast play
//! gates come from `cards::REGISTRY`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, DiscardPile, GameStateBuilder, InvestigatorId, LocationId,
    TokenModifiers, Zone,
};
use game_core::test_support::{self, seat, TestSession};
use game_core::{assert_event, assert_event_count};

/// Working a Hunch — `01037`. "Fast. Play only during your turn. / Discover 1
/// clue at your location." Two intellect icons; costs 2 resources.
const WORKING_A_HUNCH: &str = "01037";
/// Deduction — `01039`. "If this skill test is successful while investigating a
/// location, discover 1 additional clue at that location." One intellect icon.
const DEDUCTION: &str = "01039";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(20);

/// The investigator's turn at a 3-clue, shroud-2 location, `hand` in hand,
/// `resources` resources (Working a Hunch costs 2), and a single-`Numeric(0)`
/// chaos bag so only the committed icons move the total.
fn board(hand: &[&str], resources: u8) -> TestSession {
    let mut loc = test_support::test_location(HERE.0, "Study");
    loc.clues = 3;
    loc.shroud = 2;

    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(loc)
        .with_seat(
            seat(1)
                .at(HERE)
                .hand(hand.iter().copied())
                .resources(resources),
        )
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .session()
}

/// The anchor of the one copy of `code` in the investigator's hand, as a Fast
/// window offers it.
fn hand_card(session: &TestSession, code: &str) -> OptionTarget {
    let hand = &session.state().investigators[&INV].hand;
    let mut at = hand.iter().enumerate().filter(|(_, c)| c.as_str() == code);
    let (i, _) = at
        .next()
        .unwrap_or_else(|| panic!("{code} not in hand {hand:?}"));
    assert!(at.next().is_none(), "several {code} in hand {hand:?}");
    OptionTarget::HandCard {
        investigator: INV,
        hand_index: u8::try_from(i).expect("hand index fits u8"),
    }
}

/// Take the Investigate action off the open-turn menu, decline the ST.1 Fast
/// window untouched, and commit `commit` at ST.2. Declining ST.1 is what makes
/// a play land in the window under test: the session rests at the ST.2→ST.3
/// Fast window, the first to open after the commit.
fn investigate_to_st2_window(session: TestSession, commit: &[&str]) -> TestSession {
    let st1 = session.take(&TurnAction::Investigate { investigator: INV });
    assert!(
        st1.prompt().skippable,
        "the ST.1 Fast window opens before the commit: {:?}",
        st1.prompt(),
    );
    let st2 = st1.skip().commit(commit);
    assert!(
        st2.prompt().skippable,
        "a Fast window opens after the commit window closed: {:?}",
        st2.prompt(),
    );
    st2
}

/// [`investigate_to_st2_window`], then play `play` in the ST.2→ST.3 window.
/// The test resolves and play is back at the turn menu with the skill test
/// fully torn down.
fn investigate_playing(session: TestSession, commit: &[&str], play: &str) -> TestSession {
    let window = investigate_to_st2_window(session, commit);
    let card = hand_card(&window, play);
    let resolved = window.pick(card);
    assert_turn_resolved(&resolved);
    resolved
}

/// Back at the open-turn action menu, with the skill test fully torn down.
fn assert_turn_resolved(session: &TestSession) {
    session.assert_at_turn_menu(INV);
    assert!(
        !session.state().has_skill_test_in_flight(),
        "SkillTest frame torn down"
    );
}

#[test]
fn a_fast_play_in_the_st2_window_does_not_move_the_committed_card() {
    // The divergence scenario. Hand is [Working a Hunch, Deduction]; commit
    // Deduction (hand index 1), then play Working a Hunch in the ST.2→ST.3
    // window. A commit bound by hand index would point past the end of the
    // one-card hand at ST.5.
    let r = investigate_playing(
        board(&[WORKING_A_HUNCH, DEDUCTION], 2),
        &[DEDUCTION],
        WORKING_A_HUNCH,
    );

    // ST.5 counted Deduction's single intellect icon: base intellect 3 + 1 = 4
    // vs shroud 2 → success with margin 2. Without the icon the margin is 1, so
    // this pins the icon sum to the committed card rather than to whatever the
    // stale index happened to reach.
    assert_event!(
        r.events(),
        Event::SkillTestSucceeded { investigator, margin: 2, .. } if *investigator == INV
    );

    // ST.8 discarded Deduction. Working a Hunch is in the discard too — but as
    // a played event (RR Appendix I), not as a committed card.
    let inv = &r.state().investigators[&INV];
    assert!(inv.hand.is_empty(), "hand = {:?}", inv.hand);
    assert!(
        inv.discard.contains(&CardCode::new(DEDUCTION)),
        "committed Deduction discarded at ST.8; discard = {:?}",
        inv.discard,
    );
    assert_event!(
        r.events(),
        Event::CardDiscarded { code, from: Zone::Hand, to: DiscardPile::Investigator(investigator) }
            if *investigator == INV && code.as_str() == DEDUCTION
    );

    // Three clues off the location: 1 for Working a Hunch's OnPlay, 1 for the
    // successful Investigate, 1 more for Deduction's OnSkillTestResolution.
    assert_eq!(r.state().locations[&HERE].clues, 0);
    assert_eq!(inv.clues, 3);
}

#[test]
fn the_st2_window_does_not_offer_a_committed_card_as_a_play() {
    // Working a Hunch has two intellect icons, so it is both committable and a
    // legal Fast play. Hold two copies and 4 resources — enough to play either —
    // then commit one. The ST.2→ST.3 window must offer the *other* copy and only
    // the other copy: at ST.2 the committed one left the hand for the test (RR
    // glossary, "Limbo"), so it is not available to be played.
    let window = investigate_to_st2_window(
        board(&[WORKING_A_HUNCH, WORKING_A_HUNCH], 4),
        &[WORKING_A_HUNCH],
    );
    let uncommitted = hand_card(&window, WORKING_A_HUNCH);
    assert_eq!(
        (
            window.offered_at(&uncommitted),
            window.prompt().options.len()
        ),
        (1, 1),
        "exactly the uncommitted copy is offered: {:?}",
        window.prompt(),
    );

    assert_turn_resolved(&window.skip());
}

#[test]
fn only_the_committed_copy_of_a_duplicated_card_is_discarded() {
    // A hand holds `CardCode`s, so two copies of one card are the same value and
    // "the wrong copy was discarded" is not observable from the end state — this
    // is the guard that exactly *one* copy is consumed. The falsifiable
    // duplicate case is the sibling test below, which commits both.
    //
    // Two copies of Deduction in hand, one committed — and a Working a Hunch
    // played in the ST.2→ST.3 window so the surviving copy's hand position is
    // not the one the commit named. Exactly one copy reaches the discard pile at
    // ST.8; the other stays in hand.
    let r = investigate_playing(
        board(&[WORKING_A_HUNCH, DEDUCTION, DEDUCTION], 2),
        &[DEDUCTION],
        WORKING_A_HUNCH,
    );
    let inv = &r.state().investigators[&INV];
    assert_eq!(
        inv.hand,
        vec![CardCode::new(DEDUCTION)],
        "one copy retained"
    );
    assert_eq!(
        inv.discard,
        vec![CardCode::new(WORKING_A_HUNCH), CardCode::new(DEDUCTION)],
        "the played event, then the one committed copy",
    );
    // One for the played event, one for the committed copy — not two Deductions.
    assert_event_count!(r.events(), 2, Event::CardDiscarded { .. });
}

#[test]
fn committing_both_copies_of_a_card_discards_both_after_a_hand_shift() {
    // Both Deductions committed, then Working a Hunch played in the ST.2→ST.3
    // window. Under the old hand-index binding the descending-order ST.8 removal
    // ran off the end of the two-card hand and left one copy behind.
    let r = investigate_playing(
        board(&[WORKING_A_HUNCH, DEDUCTION, DEDUCTION], 2),
        &[DEDUCTION, DEDUCTION],
        WORKING_A_HUNCH,
    );
    let inv = &r.state().investigators[&INV];
    assert!(inv.hand.is_empty(), "hand = {:?}", inv.hand);
    assert_eq!(
        inv.discard,
        vec![
            CardCode::new(WORKING_A_HUNCH),
            CardCode::new(DEDUCTION),
            CardCode::new(DEDUCTION),
        ],
    );
}
