//! #311 integration: the "Max N committed per skill test" cap is enforced
//! at the commit window, against the real `cards::REGISTRY`. Guts 01089
//! carries `commit_limit: Some(1)`, so committing two copies to one test
//! is rejected; committing one is fine.
//!
//! Own process → installs `cards::REGISTRY`.

use cards::REGISTRY;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::card_registry;
use game_core::engine::{self, EngineOutcome, OptionId};
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameState, InvestigatorId, Phase, SkillKind, TokenModifiers,
};
use game_core::test_support::{self, GameStateBuilder};

const GUTS: &str = "01089";
const INV: InvestigatorId = InvestigatorId(1);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = card_registry::install(REGISTRY);
}

/// Investigator holding `copies` copies of Guts, mid-Investigation, with a
/// deterministic chaos bag.
fn board(copies: usize) -> GameState {
    let mut inv = test_support::test_investigator(1);
    inv.skills.willpower = 3;
    inv.hand = vec![CardCode::new(GUTS); copies];
    // A card for Guts' success draw to land on. Without it the draw hits an
    // empty deck, which now applies the deck-out horror (#636) — irrelevant
    // noise for a test about the commit cap.
    inv.deck = vec![CardCode::new("spare-1")];
    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator(inv)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
}

fn commit(indices: Vec<u32>) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickMultiple {
            selected: indices.into_iter().map(OptionId).collect(),
        },
    })
}

/// Committing two copies of a `Max 1 committed` card to one test is rejected.
#[test]
fn committing_over_the_cap_is_rejected() {
    let paused = test_support::perform_skill_test(board(2), INV, SkillKind::Willpower, 1);
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    let r = engine::apply(paused.state, commit(vec![0, 1]));
    assert!(
        matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "two copies of Guts (Max 1 committed) must be rejected, got {:?}",
        r.outcome,
    );
    // Validate-first: the in-flight test is untouched, still awaiting commit.
    assert!(r.state.has_skill_test_in_flight());
}

/// Committing a single copy is within the cap and resolves normally.
#[test]
fn committing_at_the_cap_is_allowed() {
    let paused = test_support::perform_skill_test(board(1), INV, SkillKind::Willpower, 1);
    let r = engine::apply(paused.state, commit(vec![0]));
    assert_eq!(r.outcome, EngineOutcome::Done);
}
