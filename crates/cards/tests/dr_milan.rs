//! C6b integration: Dr. Milan Christopher 01033 end-to-end against the
//! real `cards::REGISTRY` — the +1 intellect constant ability and the
//! after-successful-investigate reaction (engine window from #241).
//!
//! Own process → installs `cards::REGISTRY`.

use std::sync::Once;

use game_core::engine::EngineOutcome;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, InvestigatorId, LocationId, Phase,
    TokenModifiers,
};
use game_core::test_support::{test_investigator, test_location, GameStateBuilder};
use game_core::{Action, GameState, InputResponse, PlayerAction};

const DR_MILAN: &str = "01033";
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);

fn install() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Board: the investigator at a 1-clue location of shroud 2 with **base
/// intellect 1**, Dr. Milan in play, a `Numeric(0)` bag. Without Dr.
/// Milan's +1 intellect the Investigate (1 vs 2) would fail; with it
/// (2 vs 2) it succeeds — so the constant ability is load-bearing here.
fn board() -> GameState {
    install();
    let mut inv = test_investigator(1);
    inv.current_location = Some(LOC);
    inv.skills.intellect = 1;
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(DR_MILAN),
        CardInstanceId(1),
    ));
    let mut loc = test_location(10, "Study"); // shroud 2 by default
    loc.clues = 1;
    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator(inv)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
}

fn commit_nothing() -> Action {
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::CommitCards { indices: vec![] },
    })
}

#[test]
fn dr_milan_plus_one_intellect_succeeds_then_reaction_gains_resource() {
    let state = board();
    let resources_before = state.investigators[&INV].resources;

    // Investigate → commit window.
    let paused_commit = game_core::engine::apply(
        state,
        Action::Player(PlayerAction::Investigate { investigator: INV }),
    );
    assert!(matches!(
        paused_commit.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    // Commit nothing → intellect 1 + Dr. Milan's +1 = 2 ≥ shroud 2 →
    // success → clue discovered → after-investigate window suspends.
    let paused_reaction = game_core::engine::apply(paused_commit.state, commit_nothing());
    assert!(
        matches!(paused_reaction.outcome, EngineOutcome::AwaitingInput { .. }),
        "the +1 intellect should make the test succeed and open Dr. Milan's window, got {:?}",
        paused_reaction.outcome,
    );

    // Fire the reaction → gain 1 resource → resume → Done.
    let resumed = game_core::engine::apply(
        paused_reaction.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickIndex(0),
        }),
    );
    assert_eq!(resumed.outcome, EngineOutcome::Done);
    assert_eq!(resumed.state.locations[&LOC].clues, 0, "clue discovered");
    assert_eq!(
        resumed.state.investigators[&INV].resources,
        resources_before + 1,
        "Dr. Milan's reaction gained a resource",
    );
}
