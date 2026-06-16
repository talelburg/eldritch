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
use game_core::event::Event;
use game_core::scenario::Resolution;
use game_core::state::{CardCode, ChaosBag, ChaosToken, GameState, InvestigatorId};
use game_core::{assert_event, Action, InputResponse, PlayerAction};

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

/// Lost via the real all-investigators-defeated latch: Roland is seeded one
/// hit from death with an engaged Ghoul Minion, then a real Enemy-phase
/// attack defeats him and `check_all_defeated` latches `Resolution::Lost`.
#[test]
fn enemy_attack_defeats_roland_and_latches_lost() {
    use game_core::state::EnemyId;
    use game_core::test_support::test_enemy;

    let mut state = seated_roland();

    // Seed: Roland one hit from death (health 9 → damage 8).
    {
        let roland = state.investigators.get_mut(&INV).expect("Roland seated");
        roland.damage = roland.max_health - 1;
    }
    let loc = state.investigators[&INV]
        .current_location
        .expect("Roland is at a location");

    // Seed: a Ghoul Minion engaged with Roland (the `test_enemy` fixture
    // defaults to attack_damage 1 ≥ his 1 remaining health → lethal).
    let enemy_id = EnemyId(900);
    let mut minion = test_enemy(900, "Ghoul Minion");
    minion.code = CardCode::new("01160");
    minion.current_location = Some(loc);
    minion.engaged_with = Some(INV);
    state.enemies.insert(enemy_id, minion);

    // Drive: end Roland's turn → tick into the Enemy phase → the engaged
    // enemy attacks → Roland defeated → all-defeated → Resolution::Lost.
    let result = apply(state, Action::Player(PlayerAction::EndTurn));

    assert_event!(result.events, Event::AllInvestigatorsDefeated);
    assert_event!(result.events, Event::ScenarioResolved { .. });
    assert!(
        matches!(result.state.resolution, Some(Resolution::Lost { .. })),
        "expected a Lost resolution, got {:?}",
        result.state.resolution,
    );
}
