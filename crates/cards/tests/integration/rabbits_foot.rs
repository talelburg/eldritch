//! End-to-end tests for Rabbit's Foot (01075).
//!
//! Printed text (`data/arkhamdb-snapshot/pack/core/core.json`):
//! *"\[reaction\] After you fail a skill test, exhaust Rabbit's Foot: Draw 1
//! card."*
//!
//! TODO(#1082): the ruling (<https://arkhamdb.com/card/01075>) that a card drawn
//! this way with the same triggering condition is playable in the same window
//! is not modelled (the impl's `# Module gap`), so nothing here tests it.

use cards::TEST_REGISTRIES;
use game_core::engine::OptionTarget;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, SkillKind,
};
use game_core::test_support::{seat, BoardLookup as _, TestSession};

const RABBITS_FOOT: &str = "01075";
/// The card on top of the deck, for the draw to find.
const LEATHER_COAT: &str = "01072";
const INV: InvestigatorId = InvestigatorId(1);
const OTHER: InvestigatorId = InvestigatorId(2);

/// Investigator 1 with Rabbit's Foot in play and two cards in their deck,
/// investigator 2 beside them, at investigator 1's open turn. Every skill is
/// the stock 3 and the bag is a lone `Numeric(0)`.
fn board() -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .deck([LEATHER_COAT, LEATHER_COAT])
                .in_play([RABBITS_FOOT]),
        )
        .with_seat(seat(2))
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .session()
}

fn foot(session: &TestSession) -> OptionTarget {
    session.state().card(INV, RABBITS_FOOT)
}

fn foot_exhausted(session: &TestSession) -> bool {
    let id = session.state().in_play(INV, RABBITS_FOOT);
    session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .any(|c| c.instance_id == id && c.exhausted)
}

/// `investigator` runs a willpower test at `difficulty`, committing nothing.
fn test(session: TestSession, investigator: InvestigatorId, difficulty: i8) -> TestSession {
    session
        .test_skill(investigator, SkillKind::Willpower, difficulty)
        .commit(&[])
}

#[test]
fn after_you_fail_a_test_exhausting_the_foot_draws_a_card() {
    let session = board();
    let foot = foot(&session);

    // 3 vs 4 fails, and the after-window offers the foot.
    let session = test(session, INV, 4);
    assert_eq!(session.offered_at(&foot), 1, "got {:?}", session.prompt());

    let session = session.pick(foot);
    session.assert_at_turn_menu(INV);
    assert_eq!(
        session.state().investigators[&INV].hand,
        vec![CardCode::new(LEATHER_COAT)],
        "drew the top card",
    );
    assert!(foot_exhausted(&session), "exhausting it is the cost");
}

#[test]
fn the_reaction_may_be_declined() {
    let session = board();

    let session = test(session, INV, 4).skip();

    session.assert_at_turn_menu(INV);
    assert!(session.state().investigators[&INV].hand.is_empty());
    assert!(!foot_exhausted(&session));
}

/// Used once, the foot is exhausted, so a second failure offers nothing.
#[test]
fn an_exhausted_foot_is_not_offered() {
    let session = board();
    let foot = foot(&session);
    let session = test(session, INV, 4).pick(foot);

    let session = test(session, INV, 4);

    session.assert_at_turn_menu(INV);
    assert_eq!(
        session.state().investigators[&INV].hand,
        vec![CardCode::new(LEATHER_COAT)],
        "only the first draw",
    );
}

#[test]
fn a_successful_test_does_not_offer_the_foot() {
    let session = test(board(), INV, 3);

    session.assert_at_turn_menu(INV);
    assert!(session.state().investigators[&INV].hand.is_empty());
}

/// *"After you fail"*: another investigator's failure is not the
/// controller's.
#[test]
fn another_investigators_failure_does_not_offer_the_foot() {
    let session = board();
    let foot = foot(&session);

    let session = test(session, OTHER, 4);

    assert_eq!(session.offered_at(&foot), 0, "got {:?}", session.prompt());
    assert!(session.state().investigators[&INV].hand.is_empty());
}
