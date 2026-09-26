use card_dsl::card_data::ClueValue;
use card_dsl::dsl::{IntExpr, Stat};

use super::*;
use crate::action::{EngineRecord, InputResponse, PlayerAction};
use crate::event::FailureReason;
use crate::scenario::{ResolutionId, ScenarioEnding, ScenarioId, ScenarioModule};
use crate::state::{
    AbilityAddress, AbilitySource, Act, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    EliminationCause, EnemyId, GameStateBuilder, InvestigationResume, InvestigatorId, Lifetime,
    LocationId, Phase, RecordedModifier, SkillKind, SkillTestId, Status, TokenModifiers,
    TokenResolution, Zone,
};
use crate::test_support::{self, ScriptedResolver};
use crate::{assert_event, assert_event_count, assert_event_sequence, assert_no_event};

mod activate_ability;
mod aoo;
mod commit_window;
mod deck;
mod draw;
mod end_turn;
mod engage_action;
mod fight_evade;
mod investigate;
mod investigator_defeat;
mod move_action;
mod mulligan;
mod perform_skill_test;
mod play_card;
mod reject_rollback;
mod resolution;
mod resource_action;
mod seat_and_open;
mod start_scenario;

/// Drive one open-turn action through the `ResolveInput(PickSingle)` routing
/// path, draining the skill-test commit window automatically (like
/// `apply_no_commits` does). Works for Investigate, Fight, Evade, and any
/// other action that initiates a skill test.
///
/// Internally this finds the `OptionId` for `action` in `legal_actions`,
/// then calls `apply_no_commits` with a `ResolveInput(PickSingle(idx))`
/// so the commit-window drain loop fires automatically.
fn take_action_no_commits(state: GameState, action: &TurnAction) -> ApplyResult {
    let actions = legal_actions(&state);
    let idx = actions.iter().position(|a| a == action).unwrap_or_else(|| {
        panic!("take_action_no_commits: {action:?} is not legal; offered: {actions:?}")
    });
    test_support::apply_no_commits(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(
                u32::try_from(idx).expect("action index fits u32"),
            )),
        }),
    )
}

/// Bag of a single `Numeric(0)` token — the next draw is always a
/// no-op modifier, so test totals = skill exactly.
fn bag_only_zero() -> ChaosBag {
    ChaosBag::new([ChaosToken::Numeric(0)])
}

/// Build a scenario suitable for Investigate tests: one investigator
/// at a location with `clues` clues and `shroud` shroud, in
/// Investigation phase, with the investigator active and 3 actions.
/// Bag is `Numeric(0)` so the test outcome depends purely on
/// (intellect vs shroud).
fn investigate_scenario(clues: u8, shroud: u8) -> (InvestigatorId, LocationId, GameState) {
    // Registry needed for max_health()/max_sanity() after cp2a.
    test_support::install_test_registry();
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(10);
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.actions_remaining = 3;
    let mut loc = test_support::test_location(10, "Study");
    loc.clues = clues;
    loc.shroud = shroud;
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(loc)
        .with_chaos_bag(bag_only_zero())
        .with_phase(Phase::Investigation)
        .with_active_investigator(inv_id)
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(inv_id)
        .build();
    (inv_id, loc_id, state)
}

/// Build a deck of `n` cards with codes "test-001", "test-002",
/// etc. so tests can identify exact ordering.
fn make_test_deck(n: usize) -> Vec<CardCode> {
    (1..=n).map(|i| CardCode(format!("test-{i:03}"))).collect()
}

/// Build a scenario suitable for Move tests: one investigator at
/// location A, with A connected to B (and only A→B; B has no
/// connections back). Investigation phase, active investigator,
/// 3 actions. Returns (investigator id, A id, B id, state).
fn move_scenario() -> (InvestigatorId, LocationId, LocationId, GameState) {
    // Registry needed for max_health()/max_sanity() after cp2a.
    test_support::install_test_registry();
    let inv_id = InvestigatorId(1);
    let a = LocationId(10);
    let b = LocationId(11);
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(a);
    inv.actions_remaining = 3;
    let mut loc_a = test_support::test_location(10, "A");
    loc_a.connections = vec![b];
    let loc_b = test_support::test_location(11, "B");
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_phase(Phase::Investigation)
        .with_active_investigator(inv_id)
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(inv_id)
        .build();
    (inv_id, a, b, state)
}

/// Build a scenario suitable for Fight/Evade tests: one
/// investigator engaged with one enemy. Bag is `Numeric(0)` so
/// the test outcome is determined purely by (skill vs
/// fight/evade). Investigation phase, investigator is active,
/// 3 actions. Returns (investigator id, enemy id, state).
fn fight_evade_scenario() -> (InvestigatorId, EnemyId, GameState) {
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(40);
    let mut inv = test_support::test_investigator(1);
    inv.actions_remaining = 3;
    // Investigator and enemy share a location: Fight is co-location-gated
    // (RR p.12, #401), Evade engagement-gated (RR p.11) — this satisfies both.
    inv.current_location = Some(loc_id);
    let mut enemy = test_support::test_enemy(100, "Test Ghoul");
    enemy.fight = 3;
    enemy.evade = 3;
    enemy.max_health = 2;
    enemy.engaged_with = Some(inv_id);
    enemy.current_location = Some(loc_id);
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(test_support::test_location(40, "Test Hall"))
        .with_chaos_bag(bag_only_zero())
        .with_phase(Phase::Investigation)
        .with_active_investigator(inv_id)
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(inv_id)
        .build();
    (inv_id, enemy_id, state)
}

/// Move scenario with a ready enemy engaged with the active
/// investigator at the origin. A connects to B (one-way).
/// Returns (inv id, A, B, enemy id, state).
fn move_scenario_with_engaged_enemy() -> (InvestigatorId, LocationId, LocationId, EnemyId, GameState)
{
    let (inv_id, a, b, mut state) = move_scenario();
    let enemy_id = EnemyId(200);
    let mut enemy = test_support::test_enemy(200, "Engaged Ghoul");
    enemy.current_location = Some(a);
    enemy.engaged_with = Some(inv_id);
    enemy.attack_damage = 1;
    enemy.attack_horror = 0;
    state.enemies.insert(enemy_id, enemy);
    (inv_id, a, b, enemy_id, state)
}

/// From a suspended `AwaitingInput` outcome, the attack-order (#143)
/// `PickSingle` `OptionId` whose label matches `enemy`'s debug repr.
fn attack_order_pick(outcome: &EngineOutcome, enemy: EnemyId) -> OptionId {
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("expected an attack-order prompt, got {outcome:?}");
    };
    request
        .options
        .iter()
        .find(|o| o.label == format!("{enemy:?}"))
        .unwrap_or_else(|| panic!("{enemy:?} not offered in {:?}", request.options))
        .id
}
