//! Scavenging 01073 end-to-end against the real corpus.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01073):
//!
//! > [reaction] After you successfully investigate by 2 or more, exhaust
//! > Scavenging: Choose an [[Item]] card in your discard pile and add it to your
//! > hand.
//!
//! Rulings (<https://arkhamdb.com/card/01073>), the second:
//!
//! > Success is determined in Step 6 of the Skill Test Timing, and committed
//! > cards get discarded in Step 8, so you cannot use Scavenging to get back an
//! > ***Item*** card that you've just committed to the same investigation
//! > attempt.
//!
//! The Item is picked from a name-labelled choice, like Scrying 01061's deck
//! picks: a discard-pile card has no board surface to anchor on, so the pick is
//! sent raw by position.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionId;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, SkillKind,
};
use game_core::test_support::{location, seat, BoardLookup as _, Seat, TestSession};

const SCAVENGING: &str = "01073";
/// Items: Flashlight (an `[intellect]` icon, so it can be committed to an
/// investigation) and Knife.
const FLASHLIGHT: &str = "01087";
const KNIFE: &str = "01086";
/// Not an Item.
const GUTS: &str = "01089";

const INV: InvestigatorId = InvestigatorId(1);
/// A shroud-2 Study with 1 clue.
const HERE: LocationId = LocationId(10);

/// Investigator 1 at `HERE` with Scavenging in play, intellect `intellect`,
/// and `discard` as their discard pile, bottom first.
fn scavenger(intellect: i8, discard: &[&str]) -> Seat {
    seat(1)
        .at(HERE)
        .skill(SkillKind::Intellect, intellect)
        .in_play([SCAVENGING])
        .discard(discard.iter().copied())
}

fn board(investigator: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(investigator)
        .with_location(location(HERE).clues(1))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .session()
}

fn investigate(session: TestSession, commit: &[&str]) -> TestSession {
    session
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(commit)
}

/// Take the `index`th Item offered from the discard pile.
///
/// Exempt from the anchor-pick rule, and sent raw: the choice's options are
/// cards **in a discard pile**, with no board anchor and neither of ADR 0015's
/// natures, which that ADR knowingly excludes and defers. No session step can
/// name one, so the pick is positional.
fn pick(index: u32) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(index)),
    })
}

fn labels(session: &TestSession) -> Vec<&str> {
    session
        .prompt()
        .options
        .iter()
        .map(|o| o.label.as_str())
        .collect()
}

fn codes(cards: &[&str]) -> Vec<CardCode> {
    cards.iter().map(|c| CardCode::new(*c)).collect()
}

fn exhausted(session: &TestSession) -> bool {
    let scavenging = session.state().in_play(INV, SCAVENGING);
    session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == scavenging)
        .expect("Scavenging in play")
        .exhausted
}

#[test]
fn succeeding_by_two_offers_scavenging_and_takes_the_chosen_item_to_hand() {
    // Intellect 4 against shroud 2: succeeded by 2.
    let session = board(scavenger(4, &[FLASHLIGHT, GUTS, KNIFE]));
    let scavenging = session.state().card(INV, SCAVENGING);

    let window = investigate(session, &[]);
    assert_eq!(window.offered_at(&scavenging), 1, "{:?}", window.prompt());

    let choice = window.pick(scavenging);
    assert!(exhausted(&choice), "exhausting Scavenging is its cost");
    assert_eq!(
        labels(&choice),
        ["Flashlight", "Knife"],
        "only the Items in the discard pile are offered",
    );

    let done = choice.apply(pick(1));
    done.assert_at_turn_menu(INV);
    let inv = &done.state().investigators[&INV];
    assert!(inv.hand.contains(&CardCode::new(KNIFE)), "Knife to hand");
    assert_eq!(inv.discard, codes(&[FLASHLIGHT, GUTS]));
    assert_event!(
        done.events(),
        Event::CardAddedToHandFromDiscard { investigator, code }
            if *investigator == INV && code.as_str() == KNIFE
    );
}

#[test]
fn succeeding_by_one_does_not_offer_scavenging() {
    // Intellect 3 against shroud 2: succeeded by 1.
    let done = investigate(board(scavenger(3, &[FLASHLIGHT])), &[]);
    done.assert_at_turn_menu(INV);
    assert_eq!(
        done.state().investigators[&INV].discard,
        codes(&[FLASHLIGHT])
    );
}

#[test]
fn scavenging_is_not_offered_with_no_item_in_the_discard_pile() {
    let done = investigate(board(scavenger(4, &[GUTS])), &[]);
    done.assert_at_turn_menu(INV);
    let scavenging = done.state().in_play(INV, SCAVENGING);
    assert!(!exhausted(&done), "{scavenging:?} was never used");
}

#[test]
fn an_item_committed_to_the_same_investigation_cannot_be_fetched() {
    // Intellect 3 + Flashlight's [intellect] icon = 4 against shroud 2:
    // succeeded by 2. The committed Flashlight is still in limbo at ST.6, so
    // the only Item in the discard pile is the Knife, which is taken without a
    // choice.
    let session = board(scavenger(3, &[KNIFE]).hand([FLASHLIGHT]));
    let scavenging = session.state().card(INV, SCAVENGING);

    let window = investigate(session, &[FLASHLIGHT]);
    let done = window.pick(scavenging);
    done.assert_at_turn_menu(INV);
    let inv = &done.state().investigators[&INV];
    assert_eq!(inv.hand, codes(&[KNIFE]), "the Knife, not the Flashlight");
    assert_eq!(
        inv.discard,
        codes(&[FLASHLIGHT]),
        "the committed Flashlight is discarded at ST.8, after Scavenging resolved",
    );
}

#[test]
fn only_a_committed_item_leaves_scavenging_unoffered() {
    // The committed Flashlight is the only Item, and it isn't in the discard
    // pile yet when the after cell opens.
    let session = board(scavenger(3, &[GUTS]).hand([FLASHLIGHT]));
    let done = investigate(session, &[FLASHLIGHT]);
    done.assert_at_turn_menu(INV);
    assert_eq!(
        done.state().investigators[&INV].discard,
        codes(&[GUTS, FLASHLIGHT])
    );
}
