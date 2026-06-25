//! Which core-loop action controls are enabled, given the current state
//! and the latest engine outcome (P6.6). A pure UX affordance: the
//! server stays authoritative and rejects anything illegal (see the P6.6
//! design spec, decision S2).
//!
//! After 2b (#447) the only bespoke control left is `StartScenario`: the open
//! turn surfaces its action menu as an `AwaitingInput`, so every in-game action
//! (move/investigate/fight/play/…) flows through the `ResolveInput` prompt UI
//! (`AwaitingInputView`), not a dedicated
//! button. `StartScenario` is the one pre-game action that precedes any
//! `AwaitingInput` (it seeds hands and opens the setup mulligan).

use std::collections::BTreeSet;

use game_core::state::GameState;
use game_core::EngineOutcome;

/// A clickable core-loop action in the client. After 2b (#447) the only bespoke
/// control is session start; in-game actions are rendered from the engine's
/// `AwaitingInput` action menu instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActionControl {
    StartScenario,
}

/// The controls the player may click right now.
///
/// Gating, in order:
/// 0. A latched `game.resolution` is terminal: the scenario is over, so no
///    action is legal.
/// 1. An `AwaitingInput` pause means the only legal input is the pending
///    `ResolveInput` (the prompt UI) — this covers the open-turn action menu and
///    every framework suspension (mulligan, encounter draw, commit/reaction
///    windows, …).
/// 2. `round == 0` is the pre-start state straight from a scenario `setup()`
///    (the engine bumps to round 1 at `StartScenario`), so `StartScenario` is
///    the sole legal action.
/// 3. Otherwise (an in-game round between prompts), nothing: gameplay flows
///    through the `AwaitingInput` menu, which case 1 already gates.
#[must_use]
pub fn enabled_controls(game: &GameState, outcome: &EngineOutcome) -> BTreeSet<ActionControl> {
    if game.resolution.is_some() {
        return BTreeSet::new();
    }
    if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
        return BTreeSet::new();
    }
    if game.round == 0 {
        return BTreeSet::from([ActionControl::StartScenario]);
    }
    BTreeSet::new()
}

#[cfg(test)]
mod tests {
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
        // Covers the open-turn action menu and every framework suspension —
        // all surface as `AwaitingInput` and flow through the `ResolveInput`
        // prompt UI, not a bespoke control.
        let game = investigation_game();
        let out = awaiting_commit_input("commit");
        assert_eq!(enabled_controls(&game, &out), BTreeSet::new());
    }

    #[test]
    fn in_game_round_between_prompts_enables_nothing() {
        // After 2b (#447) there are no bespoke in-game controls: an in-game
        // round (round >= 1) enables nothing regardless of phase — gameplay
        // flows through the `AwaitingInput` menu.
        for phase in [
            Phase::Investigation,
            Phase::Mythos,
            Phase::Enemy,
            Phase::Upkeep,
        ] {
            let mut game = investigation_game();
            game.phase = phase;
            assert_eq!(
                enabled_controls(&game, &EngineOutcome::Done),
                BTreeSet::new(),
                "phase {phase:?} should enable no bespoke control"
            );
        }
    }

    #[test]
    fn resolution_disables_all_controls() {
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
