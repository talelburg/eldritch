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
    // StartScenario opens the mulligan prompt (AwaitingInput); each
    // investigator then submits a single mulligan (ResolveInput) before the
    // turn's actions begin.
    let started = apply(
        state,
        Action::Player(PlayerAction::StartScenario { roster }),
    );
    assert!(
        matches!(started.outcome, EngineOutcome::AwaitingInput { .. }),
        "StartScenario opens the mulligan prompt, got {:?}",
        started.outcome
    );
    let after_mulligan = apply(
        started.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
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

    // Seed: Roland one hit from death. After cp2a, accumulated_damage is the
    // source of truth; max_health() reads from cards::REGISTRY (9 for Roland).
    {
        let roland = state.investigators.get_mut(&INV).expect("Roland seated");
        roland.investigator_card.accumulated_damage = roland.max_health() - 1;
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

/// Won via the real progression + defeat→advance→win latch. Drives act 1
/// (`AdvanceAct`) and act 2 (the C3d round-end clue-spend window) for real —
/// act 2's reverse spawns the **real** Ghoul Priest — then fights that
/// spawned Priest to trigger `what_have_you_done`'s forced advance on the terminal
/// act → `Resolution::Won { R1 }`.
///
/// Two seeds, both off the resolution path: clues (acquiring them via
/// Attic/Cellar investigation is unit-tested elsewhere — the focus here is
/// the act-advancement chain), and the spawned Priest's health (solo Roland
/// has no weapon and 5 sanity, so he cannot out-damage a 5-health Retaliate
/// Hunter dealing 2 horror/attack without going insane first — the kill is
/// the one necessary shortcut). The encounter deck is emptied so round-2's
/// Mythos draw doesn't inject random interference.
#[test]
fn act_progression_and_ghoul_priest_defeat_latches_won() {
    let mut state = seated_roland();
    {
        // Seed: clues for both thresholds (act 1 = 2, act 2 = 3).
        let roland = state.investigators.get_mut(&INV).expect("Roland seated");
        roland.clues = 5;
    }
    // Seed: round-2's Mythos draws exactly one benign card — Ancient Evils
    // (01166), whose Revelation only places 1 doom (the agenda threshold is
    // 3, so it can't advance to a loss). This keeps the Mythos deterministic
    // and harmless rather than drawing a random damaging/spawning card.
    state.encounter_deck.clear();
    state.encounter_deck.push_back(CardCode::new("01166"));

    // --- Act 1 (real): spend clues to advance → the reverse builds the board
    // and relocates Roland to the Hallway (the act-2 contributor location).
    let advanced = apply(
        state,
        Action::Player(PlayerAction::AdvanceAct { investigator: INV }),
    );
    assert_eq!(advanced.outcome, EngineOutcome::Done);
    assert_eq!(advanced.state.act_index, 1, "act 1 advanced to act 2");

    // --- Act 2 (real): end the round → the C3d round-end clue-spend window
    // opens (Roland holds 3 clues in the Hallway) → Confirm spends them →
    // act 2 advances and its reverse spawns the real Ghoul Priest (01116).
    let round_end = apply(advanced.state, Action::Player(PlayerAction::EndTurn));
    assert!(
        matches!(round_end.outcome, EngineOutcome::AwaitingInput { .. }),
        "EndTurn should open the act-2 round-end window, got {:?}",
        round_end.outcome,
    );
    assert!(matches!(
        round_end.state.continuations.last(),
        Some(game_core::state::Continuation::TimingPointWindow {
            event: game_core::engine::TimingEvent::RoundEnded,
            mode: game_core::state::TimingMode::Reaction,
            ..
        })
    ));
    let after_confirm = apply(
        round_end.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    assert!(
        !matches!(after_confirm.outcome, EngineOutcome::Rejected { .. }),
        "round-end Confirm rejected: {:?}",
        after_confirm.outcome,
    );
    assert_eq!(
        after_confirm.state.act_index, 2,
        "act 2 advanced to the terminal act 3 (01110)"
    );

    // Round 2 begins in the Mythos phase; draw the seeded Ancient Evils
    // (1 doom) to advance into Investigation, where Roland can take the Fight.
    let mythos = apply(
        after_confirm.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert_eq!(mythos.outcome, EngineOutcome::Done);
    let mut state = mythos.state;

    // --- Seed only the spawned Priest's health + engagement (see doc above).
    let priest_id = {
        let priest = state
            .enemies
            .values_mut()
            .find(|e| e.code.as_str() == "01116")
            .expect("act 2's reverse spawned the real Ghoul Priest");
        priest.damage = priest.max_health - 1; // one hit from death
        priest.engaged_with = Some(INV);
        priest.id
    };
    // Ensure Roland is mid-Investigation with an action for the Fight.
    state
        .investigators
        .get_mut(&INV)
        .expect("Roland seated")
        .actions_remaining = 3;

    // --- Drive the defeating Fight against the real spawned Priest: combat 4
    // + Numeric(0) ≥ fight 4 → success → deal 1 → defeated → act 3 advances →
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
            response: InputResponse::PickMultiple { selected: vec![] },
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
