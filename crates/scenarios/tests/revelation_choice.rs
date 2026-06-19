//! #380: a treachery whose Revelation suspends **directly** into a choice
//! (not nested in a skill test) must still be discarded once the choice
//! resolves. Before the `EncounterCard`-frame fix the disposal was stranded —
//! `resolve_encounter_card` set the `pending_revelation_discard` slot on
//! suspend, but only the skill-test driver's teardown ever flushed it, so a
//! choice-only Revelation never reached `encounter_discard`.

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome, OptionId};
use game_core::state::{CardCode, GameState, InvestigatorId};
use game_core::{Action, InputResponse, PlayerAction};
use scenarios::test_fixtures::synth_cards::{SYNTH_CHOICE_TREACHERY_CODE, TEST_REGISTRY};
use scenarios::test_fixtures::synthetic;

static INSTALL: Once = Once::new();
fn install() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

/// Synthetic setup → mulligan → the sole investigator's round-ending EndTurn
/// cascades to the Mythos step-1.4 encounter-draw prompt. Seed the encounter
/// deck (after StartScenario's shuffle) with only the choice-treachery on top.
fn at_mythos_draw_with_choice_treachery() -> GameState {
    install();
    let mut state = synthetic::setup();
    for action in [
        Action::Player(PlayerAction::StartScenario { roster: vec![] }),
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
        Action::Player(PlayerAction::EndTurn),
    ] {
        state = apply(state, action).state;
    }
    synthetic::with_encounter_deck(&mut state, vec![CardCode::new(SYNTH_CHOICE_TREACHERY_CODE)]);
    state
}

#[test]
fn revelation_suspending_into_a_choice_discards_after_the_pick() {
    let inv = InvestigatorId(1);
    let state = at_mythos_draw_with_choice_treachery();

    // Confirm the draw → the Revelation's ChooseOne suspends for the pick.
    let drawn = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert!(
        matches!(drawn.outcome, EngineOutcome::AwaitingInput { .. }),
        "the Revelation choice suspends, got {:?}",
        drawn.outcome
    );
    let res_before = drawn.state.investigators[&inv].resources;

    // Pick branch 0 (gain 2 resources). The choice resolves, and the framework
    // disposes of the treachery to encounter_discard.
    let resolved = apply(
        drawn.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    assert_eq!(
        resolved.state.investigators[&inv].resources,
        res_before + 2,
        "branch 0 granted 2 resources",
    );
    assert!(
        resolved
            .state
            .encounter_discard
            .contains(&CardCode::new(SYNTH_CHOICE_TREACHERY_CODE)),
        "the treachery discards once its directly-suspended choice resolves",
    );
}
