//! Integration: the "Max N committed per skill test" cap is enforced
//! at the commit window, against the real `cards::REGISTRY`. Guts 01089
//! carries `commit_limit: Some(1)`, so committing two copies to one test
//! is rejected; committing one is fine.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, SkillKind, TokenModifiers,
};
use game_core::test_support::{seat, TestSession};

const GUTS: &str = "01089";
const INV: InvestigatorId = InvestigatorId(1);

/// Investigator's turn, holding `copies` copies of Guts, with a deterministic
/// chaos bag, paused at the commit window of a difficulty-1 Willpower test.
fn at_commit_window(copies: usize) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        // A card for Guts' success draw to land on. Without it the draw hits
        // an empty deck, which applies the deck-out horror —
        // irrelevant noise for a test about the commit cap.
        .with_seat(seat(1).hand(vec![GUTS; copies]).deck(["spare-1"]))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
        .test_skill(INV, SkillKind::Willpower, 1)
}

/// Committing two copies of a `Max 1 committed` card to one test is rejected.
#[test]
fn committing_over_the_cap_is_rejected() {
    let session = at_commit_window(2).commit(&[GUTS, GUTS]);
    // Two copies of Guts (Max 1 committed) must be rejected.
    session.expect_rejected();
    // Validate-first: the in-flight test is untouched, still awaiting commit.
    assert!(session.state().has_skill_test_in_flight());
}

/// Committing a single copy is within the cap and resolves normally.
#[test]
fn committing_at_the_cap_is_allowed() {
    let session = at_commit_window(1).commit(&[GUTS]);
    session.assert_at_turn_menu(INV);
    assert!(!session.state().has_skill_test_in_flight());
    assert!(
        session.state().investigators[&INV]
            .discard
            .contains(&CardCode::new(GUTS)),
        "the committed Guts resolved and was discarded",
    );
}
