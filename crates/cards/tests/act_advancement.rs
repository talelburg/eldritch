//! Act-3 objective: defeating the Ghoul Priest (01116) advances Act 3
//! (01110), whose reverse asks the lead investigator which of the two printed
//! resolution points to reach (#775). The Ghoul Priest enemy + its spawn land
//! in C3 (#231); here we drive the forced dispatch directly with the real
//! registry. End-to-end defeat->ending via a real Fight is C7b (#245).

use card_dsl::dsl::EventTiming;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::{EngineOutcome, OptionId, OptionTarget, TurnAction};
use game_core::scenario::{ResolutionId, ScenarioEnding};
use game_core::state::{Act, CardCode, InvestigatorId, Phase};
use game_core::test_support::{
    dispatch_turn_action_unchecked, test_investigator, GameStateBuilder,
};

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

fn act3_state() -> game_core::state::GameState {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new().with_turn_order([inv]).build();
    // Act 3 is current and terminal-Won (mirrors the_gathering setup()).
    state.act_deck = vec![Act {
        code: CardCode("01110".into()),
        clue_threshold: 0,
    }];
    state
}

/// Fire 01110's objective and return the prompt its reverse suspends on,
/// together with the state carrying the suspended choice.
fn defeat_the_ghoul_priest() -> (game_core::state::GameState, game_core::engine::InputRequest) {
    let mut state = act3_state();
    let mut events = Vec::new();
    let out = game_core::test_support::fire_forced_on_enemy_defeat(
        &mut state,
        &mut events,
        CardCode("01116".into()), // the Ghoul Priest
        // The `at` cell — 01110 prints *"If the Ghoul Priest is Defeated"*.
        EventTiming::At,
    );
    let EngineOutcome::AwaitingInput { request, .. } = out else {
        panic!("expected 01110's reverse to ask for the resolution point, got {out:?}");
    };
    (state, request)
}

/// The printed reverse offers both resolution points, verbatim, and anchors
/// them to the act — the card the choice is printed on, whose reverse face the
/// advance is already showing (#555 / ADR 0011). Before #775 the act latched R1
/// without asking.
#[test]
fn defeating_ghoul_priest_offers_the_lead_both_printed_resolution_points() {
    let (state, request) = defeat_the_ghoul_priest();
    assert!(state.ending.is_none(), "nothing latches before the pick");
    let labels: Vec<&str> = request.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "It was never much of a home. Burn it down! (→R1)",
            "This hell-pit is my home! No way are we burning it! (→R2)",
        ],
    );
    for option in &request.options {
        assert_eq!(
            option.target,
            Some(OptionTarget::Act),
            "the choice is printed on the act, so it renders there",
        );
    }
}

/// Picking the first bullet reaches R1; the second, R2. Both are the *same*
/// prompt — the only difference is the pick — and each lands on its own
/// `ScenarioEnding`, which is what the board's ending banner renders off
/// (ADR 0012).
#[test]
fn each_printed_bullet_reaches_its_own_resolution() {
    for (pick, expected) in [(0u32, 1u8), (1, 2)] {
        let (state, _) = defeat_the_ghoul_priest();
        let result = game_core::engine::apply(
            state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickSingle(OptionId(pick)),
            }),
        );
        assert_eq!(
            result.state.ending,
            Some(ScenarioEnding::Resolution(ResolutionId::new(expected))),
            "pick {pick} should reach Resolution {expected}, got {:?}",
            result.state.ending,
        );
    }
}

#[test]
fn defeating_other_enemy_does_not_advance_act_3() {
    let mut state = act3_state();
    let mut events = Vec::new();
    let out = game_core::test_support::fire_forced_on_enemy_defeat(
        &mut state,
        &mut events,
        CardCode("01103".into()), // some other enemy, not the Ghoul Priest
        EventTiming::At,
    );
    assert_eq!(out, EngineOutcome::Done);
    assert!(
        state.ending.is_none(),
        "only the Ghoul Priest's defeat advances Act 3"
    );
}

/// Act 01110 ("What Have You Done?") advances only via its Forced
/// `EnemyDefeated` objective (the Ghoul Priest), so its corpus clue threshold is
/// `null` -> 0. The deliberate clue-spend `AdvanceAct` action must be rejected
/// for it — otherwise the player could "spend 0 clues to advance" and instantly
/// latch the terminal Won resolution, bypassing the Ghoul Priest fight (#486).
/// The legitimate defeat path stays covered by
/// `defeating_ghoul_priest_advances_act_3_to_won` above.
#[test]
fn advance_act_action_rejected_for_act_3_objective() {
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.clues = 5; // plenty — reject must be the objective, not affordability
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .build();
    // Act 3 is current and terminal-Won (mirrors the_gathering setup()).
    state.act_deck = vec![Act {
        code: CardCode("01110".into()),
        clue_threshold: 0,
    }];

    let result =
        dispatch_turn_action_unchecked(state, &TurnAction::AdvanceAct { investigator: inv });
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "AdvanceAct must be rejected for Act 3's non-clue objective"
    );
    assert!(
        result.state.ending.is_none(),
        "rejected AdvanceAct must not latch the Won resolution (no instant win)"
    );
    assert_eq!(result.state.act_index, 0, "act did not advance");
    assert_eq!(result.state.investigators[&inv].clues, 5, "no clues spent");
}

/// Act 01109 ("The Barrier") advances only at the end of the round (its
/// `When`-`RoundEnded` group objective), so the `AdvanceAct` *action* is rejected.
/// Registry-based detection (`act_advances_at_round_end`, #434) — needs the real
/// registry, so it lives here rather than as a game-core lib unit test.
#[test]
fn advance_act_rejected_for_round_end_advance_act() {
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.clues = 9; // plenty — reject must be the objective, not affordability
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .build();
    state.act_deck = vec![Act {
        code: CardCode("01109".into()),
        clue_threshold: 3,
    }];

    let result =
        dispatch_turn_action_unchecked(state, &TurnAction::AdvanceAct { investigator: inv });
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.act_index, 0, "act did not advance");
    assert_eq!(result.state.investigators[&inv].clues, 9, "no clues spent");
}
