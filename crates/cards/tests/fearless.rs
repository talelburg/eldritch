//! End-to-end test for Fearless (01067): committed to a successful test, it
//! heals 1 horror; to a failed one, nothing.
//!
//! Printed text (`data/arkhamdb-snapshot/pack/core/core.json`): *"If this
//! skill test is successful, heal 1 horror."*
//!
//! Ruling (<https://arkhamdb.com/card/01067>): *"If you commit this card to
//! another player's skill test, and the test is successful, you heal 1 horror
//! (not the other player)."* These tests are solo, where the committer is the
//! performer.
//!
//! TODO(#65): commits to another investigator's skill test, which the
//! committer binding needs.

use card_dsl::dsl::HarmKind;
use cards::TEST_REGISTRIES;
use game_core::event::Event;
use game_core::state::{
    ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, SkillKind, TokenModifiers,
};
use game_core::test_support::{seat, Seat, TestSession};
use game_core::{assert_event, assert_no_event};

const FEARLESS: &str = "01067";
const INV: InvestigatorId = InvestigatorId(1);

/// `investigator`'s turn with a one-token bag of `token`.
fn board(investigator: Seat, token: ChaosToken) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(investigator)
        .with_chaos_bag(ChaosBag::new([token]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
}

/// A willpower (1) test with Fearless committed.
fn commit_fearless(session: TestSession) -> TestSession {
    let session = session
        .test_skill(INV, SkillKind::Willpower, 1)
        .commit(&[FEARLESS]);
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn fearless_heals_one_horror_on_a_successful_test() {
    let s = commit_fearless(board(
        seat(1).hand([FEARLESS]).harm(0, 2),
        ChaosToken::Numeric(0),
    ));

    assert_event!(
        s.events(),
        Event::Healed { investigator, kind: HarmKind::Horror, amount: 1 } if *investigator == INV
    );
    assert_eq!(s.state().investigators[&INV].horror(), 1);
}

#[test]
fn fearless_heals_nothing_on_a_failed_test() {
    let s = commit_fearless(board(
        seat(1).hand([FEARLESS]).harm(0, 2),
        ChaosToken::AutoFail,
    ));

    assert_no_event!(s.events(), Event::Healed { .. });
    assert_eq!(s.state().investigators[&INV].horror(), 2);
}

/// With no horror to heal, the success clause is a no-op, not a rejection.
#[test]
fn fearless_on_a_successful_test_with_no_horror_heals_nothing() {
    let s = commit_fearless(board(seat(1).hand([FEARLESS]), ChaosToken::Numeric(0)));

    assert_event!(s.events(), Event::SkillTestSucceeded { .. });
    assert_no_event!(s.events(), Event::Healed { .. });
}
