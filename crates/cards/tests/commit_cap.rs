//! #311 integration: the "Max N committed per skill test" cap is enforced
//! at the commit window, against the real `cards::REGISTRY`. Guts 01089
//! carries `commit_limit: Some(1)`, so committing two copies to one test
//! is rejected; committing one is fine.
//!
//! Own process → installs `cards::REGISTRY`.

use cards::REGISTRY;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, SkillKind, TokenModifiers,
};
use game_core::test_support::{self, seat, TestSession};

const GUTS: &str = "01089";
const INV: InvestigatorId = InvestigatorId(1);

#[ctor::ctor(unsafe)]
fn install() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Investigator's turn, holding `copies` copies of Guts, with a deterministic
/// chaos bag, paused at the commit window of a difficulty-1 Willpower test.
fn at_commit_window(copies: usize) -> TestSession {
    GameStateBuilder::new()
        // A card for Guts' success draw to land on. Without it the draw hits
        // an empty deck, which now applies the deck-out horror (#636) —
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
    let session = at_commit_window(2).commit(&[CardCode::new(GUTS), CardCode::new(GUTS)]);
    // Two copies of Guts (Max 1 committed) must be rejected.
    session.expect_rejected();
    // Validate-first: the in-flight test is untouched, still awaiting commit.
    assert!(session.state().has_skill_test_in_flight());
}

/// Committing a single copy is within the cap and resolves normally.
#[test]
fn committing_at_the_cap_is_allowed() {
    let session = at_commit_window(1).commit(&[CardCode::new(GUTS)]);
    session.assert_at_turn_menu(INV);
    assert!(!session.state().has_skill_test_in_flight());
    assert!(
        session.state().investigators[&INV]
            .discard
            .contains(&CardCode::new(GUTS)),
        "the committed Guts resolved and was discarded",
    );
}
