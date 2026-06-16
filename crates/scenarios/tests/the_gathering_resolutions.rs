//! C7b — the Slice-1 "done" gate: drive solo Roland through The Gathering
//! to a genuine engine-latched Won and Lost resolution, against the real
//! `scenarios` + `cards` registries.
//!
//! Hybrid fidelity (see the C7b design spec): drive the cheap, deterministic
//! real progression and seed only the expensive preconditions, so the
//! resolution itself is always engine-latched. Test-determinism stand-ins
//! (a controlled chaos bag, a minimal roster deck, seeded health/act state)
//! are called out at their use sites.

use std::sync::Once;

use game_core::action::RosterEntry;
use game_core::engine::{apply, EngineOutcome};
use game_core::state::{CardCode, ChaosBag, ChaosToken, GameState, InvestigatorId};
use game_core::{Action, PlayerAction};

const ROLAND: &str = "01001";
const INV: InvestigatorId = InvestigatorId(1);

fn install() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::scenario_registry::install(scenarios::REGISTRY);
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// The Gathering set up + solo Roland seated and past the mulligan, ready
/// to act in the Investigation phase. Determinism stand-in: the random
/// Standard bag (which contains AutoFail) is replaced with a single-token
/// `Numeric(0)` bag so skill tests resolve predictably.
fn seated_roland() -> GameState {
    install();
    let mut state = scenarios::the_gathering::setup();
    // Stand-in: deterministic chaos bag (production serves Standard).
    state.chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)]);

    // Stand-in: a minimal deck (the resolution paths don't read deck
    // contents). Eight copies of a real neutral event so the opening hand
    // of 5 draws cleanly.
    let roster = vec![RosterEntry {
        investigator: CardCode::new(ROLAND),
        deck: vec![CardCode::new("01088"); 8],
    }];
    // StartScenario completes (Done) with the mulligan cursor seeded; each
    // investigator then submits a single Mulligan action before the turn's
    // actions begin.
    let started = apply(
        state,
        Action::Player(PlayerAction::StartScenario { roster }),
    );
    assert_eq!(started.outcome, EngineOutcome::Done);
    let after_mulligan = apply(
        started.state,
        Action::Player(PlayerAction::Mulligan {
            investigator: INV,
            indices_to_redraw: vec![],
        }),
    );
    assert_eq!(after_mulligan.outcome, EngineOutcome::Done);
    after_mulligan.state
}

#[test]
fn solo_roland_is_seated_in_the_study_ready_to_act() {
    let state = seated_roland();
    assert_eq!(state.round, 1);
    assert!(
        state.investigators.contains_key(&INV),
        "Roland seated as investigator 1"
    );
    assert!(state.resolution.is_none(), "no resolution latched at setup");
}
