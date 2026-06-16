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
/// Standard bag (which contains `AutoFail`) is replaced with a single-token
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

/// Drive one Investigate action through its commit window (committing
/// nothing), asserting it resolves to `Done` (Roland has no after-investigate
/// reaction in play, so no window opens). Returns the post-commit state.
fn investigate_once(state: GameState) -> GameState {
    let paused = apply(
        state,
        Action::Player(PlayerAction::Investigate { investigator: INV }),
    );
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "Investigate should pause at the commit window, got {:?}",
        paused.outcome,
    );
    let resolved = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::CommitCards { indices: vec![] },
        }),
    );
    assert_eq!(resolved.outcome, EngineOutcome::Done);
    resolved.state
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

/// Won via the real defeat→advance→win latch. Drive act 1 for real
/// (investigate the Study twice → `AdvanceAct`), then take the documented
/// act-2 fallback (the Hallway has 0 clues, so its round-end clue-spend has
/// no local source): seed the act deck to the terminal act and place the
/// Ghoul Priest one hit from death, then drive the defeating Fight. The win
/// itself — `act_01110`'s forced advance on the Priest's defeat — is real.
#[test]
fn defeating_the_ghoul_priest_latches_won() {
    use game_core::state::EnemyId;
    use game_core::test_support::test_enemy;

    // --- Act 1, driven for real: 2 clues from the Study, then AdvanceAct.
    let mut state = seated_roland();
    state = investigate_once(state); // Study clues 2 → 1
    state = investigate_once(state); // Study clues 1 → 0; Roland holds 2
    assert_eq!(
        state.investigators[&INV].clues, 2,
        "two successful investigates of the Study"
    );
    let advanced = apply(
        state,
        Action::Player(PlayerAction::AdvanceAct { investigator: INV }),
    );
    assert_eq!(advanced.outcome, EngineOutcome::Done);
    let mut state = advanced.state;
    let loc = state.investigators[&INV]
        .current_location
        .expect("relocated by the act-1 reverse"); // the Hallway

    // --- Act-2 fallback (seeded): make the terminal act (01110) current and
    // place the Ghoul Priest one hit from death, engaged with Roland. The
    // act-2 round-end clue-spend + spawn is unit-tested in C3d / act_01109.
    state.act_index = state.act_deck.len() - 1; // terminal act 01110
    let priest_id = EnemyId(901);
    let mut priest = test_enemy(901, "Ghoul Priest");
    priest.code = CardCode::new("01116");
    priest.fight = 4;
    priest.max_health = 5; // solo health
    priest.damage = 4; // one hit from death
    priest.current_location = Some(loc);
    priest.engaged_with = Some(INV);
    priest.retaliate = true; // moot on a successful Fight
    state.enemies.insert(priest_id, priest);
    // Seed the action economy: act 1 consumed Roland's turn; restore an
    // action so the defeating Fight can be taken this turn (test economy,
    // not the resolution).
    state
        .investigators
        .get_mut(&INV)
        .expect("Roland seated")
        .actions_remaining = 3;

    // --- Drive the defeating Fight: combat 4 + Numeric(0) ≥ fight 4 →
    // success → deal 1 → damage 5 ≥ health 5 → defeated → act 3 advances →
    // Resolution::Won { R1 }.
    let paused = apply(
        state,
        Action::Player(PlayerAction::Fight {
            investigator: INV,
            enemy: priest_id,
        }),
    );
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "Fight should pause at the commit window, got {:?}",
        paused.outcome,
    );
    let result = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::CommitCards { indices: vec![] },
        }),
    );

    assert_event!(result.events, Event::EnemyDefeated { .. });
    assert_event!(result.events, Event::ScenarioResolved { .. });
    assert!(
        matches!(result.state.resolution, Some(Resolution::Won { .. })),
        "expected a Won resolution, got {:?}",
        result.state.resolution,
    );
}
