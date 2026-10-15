//! Opportunist 01053: a skill returned to hand when the test succeeds by 3 or
//! more.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01053):
//! > Commit only to a skill test you are performing.
//! > If you succeed by 3 or more, return Opportunist to your hand after this
//! > test instead of discarding it.
//!
//! 1 wild icon. No rulings (`data/arkhamdb-faq/no-rulings.txt`).
//!
//! The commit restriction is checked against the committing investigator, but
//! only the investigator performing a test can commit to it today, so the
//! restriction cannot be broken through `apply`.
// TODO(#65): once other investigators can commit, test that Opportunist can't
// be committed to another investigator's skill test.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, DiscardPile, GameStateBuilder, InvestigatorId, SkillKind,
    TokenModifiers, Zone,
};
use game_core::test_support::{seat, TestSession};
use game_core::{assert_event, assert_no_event};

const OPPORTUNIST: &str = "01053";
/// Unexpected Courage 01093: 2 wild icons, no ability.
const UNEXPECTED_COURAGE: &str = "01093";

const INV: InvestigatorId = InvestigatorId(1);

/// A difficulty-2 Willpower test, drawing a `Numeric(0)` token, with `hand`
/// committed by an investigator of `willpower`: Opportunist's wild icon makes
/// the margin `willpower + 1 - 2` before any other committed card.
fn commit_to_willpower_test(willpower: i8, hand: &[&str]) -> TestSession {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .skill(SkillKind::Willpower, willpower)
                .hand(hand.iter().copied()),
        )
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
        .test_skill(INV, SkillKind::Willpower, 2)
        .commit(hand);
    session.assert_at_turn_menu(INV);
    session
}

fn in_hand(session: &TestSession, code: &str) -> bool {
    session.state().investigators[&INV]
        .hand
        .contains(&CardCode::new(code))
}

fn in_discard(session: &TestSession, code: &str) -> bool {
    session.state().investigators[&INV]
        .discard
        .contains(&CardCode::new(code))
}

#[test]
fn succeeding_by_exactly_3_returns_opportunist_to_hand() {
    // Willpower 4 + 1 wild vs 2: succeeds by 3.
    let session = commit_to_willpower_test(4, &[OPPORTUNIST]);
    assert!(
        in_hand(&session, OPPORTUNIST),
        "Opportunist is back in hand"
    );
    assert!(!in_discard(&session, OPPORTUNIST), "and not discarded");
    assert_event!(
        session.events(),
        Event::CommittedCardReturnedToHand { investigator: INV, code } if code.as_str() == OPPORTUNIST
    );
    assert_no_event!(
        session.events(),
        Event::CardDiscarded { code, .. } if code.as_str() == OPPORTUNIST
    );
}

#[test]
fn succeeding_by_exactly_2_discards_opportunist() {
    // Willpower 3 + 1 wild vs 2: succeeds by 2.
    let session = commit_to_willpower_test(3, &[OPPORTUNIST]);
    assert!(!in_hand(&session, OPPORTUNIST));
    assert!(
        in_discard(&session, OPPORTUNIST),
        "Opportunist is discarded"
    );
    assert_event!(
        session.events(),
        Event::CardDiscarded {
            code,
            from: Zone::Hand,
            to: DiscardPile::Investigator(INV),
        } if code.as_str() == OPPORTUNIST
    );
    assert_no_event!(session.events(), Event::CommittedCardReturnedToHand { .. });
}

#[test]
fn failing_discards_opportunist() {
    // Willpower 0 + 1 wild vs 2: fails by 1.
    let session = commit_to_willpower_test(0, &[OPPORTUNIST]);
    assert!(!in_hand(&session, OPPORTUNIST));
    assert!(
        in_discard(&session, OPPORTUNIST),
        "Opportunist is discarded"
    );
    assert_no_event!(session.events(), Event::CommittedCardReturnedToHand { .. });
}

/// The margin counts every committed card, and only Opportunist goes back:
/// Unexpected Courage, committed beside it, is discarded as usual.
#[test]
fn only_opportunist_returns_when_committed_with_another_card() {
    // Willpower 2 + 1 wild + 2 wild vs 2: succeeds by 3.
    let session = commit_to_willpower_test(2, &[OPPORTUNIST, UNEXPECTED_COURAGE]);
    assert!(
        in_hand(&session, OPPORTUNIST),
        "Opportunist is back in hand"
    );
    assert!(
        in_discard(&session, UNEXPECTED_COURAGE),
        "Unexpected Courage is discarded",
    );
    assert!(!in_hand(&session, UNEXPECTED_COURAGE));
}
