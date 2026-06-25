//! End-to-end test that Magnifying Glass's +1 intellect applies
//! during Investigate but not during a bare intellect test.
//!
//! The acceptance criterion from #45 (per-skill-test-kind scope) was
//! "Magnifying Glass's full impl lands and the simulator applies +1
//! intellect to investigate tests but not to other intellect tests."
//! This file closes that loop with the real card and real registry.

use std::sync::Once;

use game_core::engine::enumerate::legal_actions;
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, InvestigatorId, LocationId, Phase, SkillKind, TokenModifiers,
};
use game_core::test_support::{
    apply_no_commits, perform_skill_test_no_commits, take_turn_action, test_investigator,
    test_location, GameStateBuilder, TestSession,
};
use game_core::{assert_event, Action, InputResponse, OptionId, PlayerAction, TurnAction};

const MAGNIFYING_GLASS: &str = "01030";

fn install_real_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Build a state with Magnifying Glass in hand, the controller in the
/// Investigation phase placed at a 4-shroud location with a clue, and
/// a single-Numeric(0) chaos bag so the token modifier is always 0.
/// Skill defaults (3 intellect) plus the card's bonus (when in play)
/// cleanly cross / miss difficulty 4 depending on whether the bonus
/// applies.
fn state_with_mg_in_hand() -> (game_core::GameState, InvestigatorId, LocationId) {
    install_real_registry();

    let id = InvestigatorId(1);
    let loc_id = LocationId(101);
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.hand = vec![CardCode::new(MAGNIFYING_GLASS)];

    let mut location = test_location(101, "Study");
    location.shroud = 4;
    location.clues = 1;

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_investigator_turn(id)
        .with_location(location)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (state, id, loc_id)
}

#[test]
fn investigate_succeeds_at_shroud_4_after_playing_magnifying_glass() {
    // 3 intellect + 1 (Magnifying Glass) + 0 (token) = 4 vs shroud 4 → succeed by 0.
    let (state, id, _loc) = state_with_mg_in_hand();

    let after_play = take_turn_action(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );
    assert!(matches!(
        after_play.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(
        after_play.state.investigators[&id].cards_in_play[0].code,
        CardCode::new(MAGNIFYING_GLASS),
    );

    let result = TestSession::new(after_play.state)
        .take(&TurnAction::Investigate { investigator: id })
        .resolve_choices(|c| {
            c.commit_cards(&[]);
        })
        .run();
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == id
    );
}

#[test]
fn investigate_fails_at_shroud_4_without_magnifying_glass_in_play() {
    // Same setup minus the card in play — 3 + 0 < 4 → fail by 1.
    //
    // MG is in hand (an Asset, not yet played) so the commit window parks
    // (`FastWindow` idle, returns Done) rather than suspending as AwaitingInput.
    // `apply_no_commits` drives through parked windows by skipping them;
    // we submit via OptionId (not the typed PlayerAction::Investigate) to stay
    // on the new dispatch path.
    let (state, id, _loc) = state_with_mg_in_hand();
    assert!(state.investigators[&id].cards_in_play.is_empty());

    let actions = legal_actions(&state);
    let idx = actions
        .iter()
        .position(|a| a == &TurnAction::Investigate { investigator: id })
        .expect("Investigate must be legal");
    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        }),
    );
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 1, .. }
            if *investigator == id
    );
}

#[test]
fn bare_intellect_test_unaffected_by_magnifying_glass_in_play() {
    // The bonus is gated to `SkillTestKind::Investigate`. A bare
    // intellect test (e.g. a treachery testing intellect) goes
    // through a plain skill test (`SkillTestKind::Plain`) —
    // the Magnifying Glass contribution must NOT apply. 3 + 0 < 4
    // → fail by 1, even with the card in play.
    let (state, id, _loc) = state_with_mg_in_hand();

    let after_play = take_turn_action(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );
    assert!(matches!(
        after_play.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    let result = perform_skill_test_no_commits(after_play.state, id, SkillKind::Intellect, 4);
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 1, .. }
            if *investigator == id
    );
}
