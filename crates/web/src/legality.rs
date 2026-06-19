//! Which core-loop action controls are enabled, given the current state
//! and the latest engine outcome (P6.6). A pure UX affordance: the
//! server stays authoritative and rejects anything illegal (see the P6.6
//! design spec, decision S2). Consumed by P6.7's action buttons.

use std::collections::BTreeSet;

use game_core::state::{GameState, Phase};
use game_core::EngineOutcome;

/// A clickable core-loop action in the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActionControl {
    StartScenario,
    Move,
    Investigate,
    PlayCard,
    EndTurn,
    AdvanceAct,
    Fight,
    Evade,
    Draw,
}

/// The controls the player may click right now.
///
/// Gating, in order:
/// 0. A latched `game.resolution` is terminal: the scenario is over, so
///    no action is legal. Checked first — it dominates every cursor,
///    pause, and phase below.
/// 1. An `AwaitingInput` pause blocks every core-loop action — only the
///    pending `ResolveInput` (the prompt UI) is legal. This keys off the
///    outcome, so it covers every suspension mode the engine surfaces as
///    `AwaitingInput` (reaction windows, hunter moves, hand-size discard,
///    the skill-test commit window), not just the commit prompt.
/// 2. `round == 0` is the pre-start state straight from a scenario
///    `setup()` (the engine bumps to round 1 at `StartScenario` and only
///    ever increments), so `StartScenario` is the sole legal action — it
///    seeds hands and opens the setup mulligan loop. The mulligan and the
///    Mythos step-1.4 encounter draw are both `AwaitingInput`s (case 1
///    above), so they need no dedicated control — they flow through the
///    `ResolveInput` prompt UI like every other suspension.
/// 3. Otherwise, the controls the current `Phase` permits.
///
/// Finer checks (resources, action budget, clue presence) are
/// deliberately not mirrored — the server's `Rejected` is the truth.
#[must_use]
pub fn enabled_controls(game: &GameState, outcome: &EngineOutcome) -> BTreeSet<ActionControl> {
    use ActionControl::{
        AdvanceAct, Draw, EndTurn, Evade, Fight, Investigate, Move, PlayCard, StartScenario,
    };

    if game.resolution.is_some() {
        return BTreeSet::new();
    }
    if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
        return BTreeSet::new();
    }
    if game.round == 0 {
        return BTreeSet::from([StartScenario]);
    }
    match game.phase {
        Phase::Investigation => BTreeSet::from([
            Move,
            Investigate,
            PlayCard,
            EndTurn,
            AdvanceAct,
            Fight,
            Evade,
            Draw,
        ]),
        Phase::Mythos | Phase::Enemy | Phase::Upkeep => BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::ActionControl::{
        AdvanceAct, Draw, EndTurn, Evade, Fight, Investigate, Move, PlayCard,
    };
    use super::{enabled_controls, ActionControl};
    use game_core::state::GameStateBuilder;
    use game_core::state::{InvestigatorId, Phase};
    use game_core::test_support::fixtures::{awaiting_commit_input, test_investigator};
    use game_core::{EngineOutcome, Resolution};
    use std::collections::BTreeSet;

    fn investigation_game() -> game_core::state::GameState {
        // round 1: an in-progress game is never round 0 (the engine bumps
        // to 1 at StartScenario), and round 0 now gates to StartScenario.
        GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(InvestigatorId(1))
            .with_phase(Phase::Investigation)
            .with_round(1)
            .build()
    }

    #[test]
    fn round_zero_enables_only_start_scenario() {
        // The state straight from a scenario `setup()`: phase Mythos,
        // round 0, no cursors. The only legal action is StartScenario.
        let game = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(InvestigatorId(1))
            .build();
        assert_eq!(game.round, 0, "precondition: pre-start state");
        assert_eq!(
            enabled_controls(&game, &EngineOutcome::Done),
            BTreeSet::from([ActionControl::StartScenario])
        );
    }

    #[test]
    fn awaiting_input_disables_everything() {
        let game = investigation_game();
        let out = awaiting_commit_input("commit");
        assert_eq!(enabled_controls(&game, &out), BTreeSet::new());
    }

    // The former `mulligan_pending_enables_only_mulligan` (#348 2c-iii-a) and
    // `mythos_draw_pending_enables_only_draw_encounter` (#348 2c-iii-b) tests
    // are gone: the setup mulligan and the Mythos step-1.4 encounter draw are
    // now `AwaitingInput`s, so both are covered by
    // `awaiting_input_disables_everything` — they flow through the
    // `ResolveInput` prompt UI, not a dedicated control.

    #[test]
    fn investigation_phase_enables_the_core_loop() {
        let game = investigation_game();
        assert_eq!(
            enabled_controls(&game, &EngineOutcome::Done),
            BTreeSet::from([
                Move,
                Investigate,
                PlayCard,
                EndTurn,
                AdvanceAct,
                Fight,
                Evade,
                Draw
            ])
        );
    }

    #[test]
    fn non_investigation_phases_enable_nothing() {
        for phase in [Phase::Mythos, Phase::Enemy, Phase::Upkeep] {
            let mut game = investigation_game();
            game.phase = phase;
            assert_eq!(
                enabled_controls(&game, &EngineOutcome::Done),
                BTreeSet::new(),
                "phase {phase:?} should enable nothing"
            );
        }
    }

    #[test]
    fn resolution_disables_all_controls() {
        // investigation_game() would normally enable the full core loop;
        // a latched resolution must dominate and enable nothing.
        let mut game = investigation_game();
        game.resolution = Some(Resolution::Won { id: "demo".into() });
        assert_eq!(
            enabled_controls(&game, &EngineOutcome::Done),
            BTreeSet::new(),
            "a resolved scenario enables nothing"
        );
        game.resolution = Some(Resolution::Lost {
            reason: "eliminated".into(),
        });
        assert_eq!(
            enabled_controls(&game, &EngineOutcome::Done),
            BTreeSet::new(),
            "a lost scenario enables nothing"
        );
    }
}
