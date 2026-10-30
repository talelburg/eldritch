use card_dsl::card_data::ClueValue;
use card_dsl::dsl::{IntExpr, Stat};

use super::*;
use crate::action::{EngineRecord, InputResponse, PlayerAction};
use crate::event::FailureReason;
use crate::scenario::ScenarioId;
use crate::state::{
    AbilityAddress, AbilitySource, CardInstanceId, ChaosBag, ChaosToken, DiscardPile,
    EliminationCause, EnemyId, GameStateBuilder, InvestigatorId, Lifetime, LocationId, Phase,
    RecordedModifier, SkillKind, SkillTestId, Status, TokenModifiers, TokenResolution, Zone,
};
use crate::test_support::{
    self, act, enemy, location, seat, Seat, StagedEnemy, TestSession, TEST_REGISTRIES,
};
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
mod start_scenario;

/// The investigator every board seats first, and whose turn it opens.
const INV: InvestigatorId = InvestigatorId(1);
/// Where `INV` stands.
const HERE: LocationId = LocationId(10);
/// The Move board's destination, connected one-way from `HERE`.
const CONNECTED: LocationId = LocationId(11);
/// The Fight / Evade board's target.
const GHOUL: EnemyId = EnemyId(100);
/// `INV` moves from `HERE` to `CONNECTED`.
const MOVE_TO_CONNECTED: TurnAction = TurnAction::Move {
    investigator: INV,
    destination: CONNECTED,
};

/// Bag of a single `Numeric(0)` token — the next draw is always a
/// no-op modifier, so test totals = skill exactly.
fn bag_only_zero() -> ChaosBag {
    ChaosBag::new([ChaosToken::Numeric(0)])
}

/// Take `action` from the turn menu and commit nothing to the skill test it
/// starts: the session rests wherever play goes after the test.
fn take_committing_nothing(board: GameStateBuilder, action: &TurnAction) -> TestSession {
    board.session().take(action).commit(&[])
}

/// `investigator`'s open turn at the Study, which holds `clues` clues behind
/// `shroud`. Bag is `Numeric(0)` so the test outcome depends purely on
/// (intellect vs shroud).
fn investigate_board(investigator: Seat, clues: u8, shroud: u8) -> GameStateBuilder {
    let study = location(HERE).clues(clues).shroud(shroud);
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(investigator)
        .with_location(study)
        .with_chaos_bag(bag_only_zero())
        .open_turn(INV)
}

/// Build a deck of `n` cards with codes "test-001", "test-002",
/// etc. so tests can identify exact ordering.
fn make_test_deck(n: usize) -> Vec<CardCode> {
    (1..=n).map(|i| CardCode(format!("test-{i:03}"))).collect()
}

/// `mover`'s open turn, with `HERE` connected to `CONNECTED` (one way:
/// `CONNECTED` has no connection back).
fn move_board(mover: Seat) -> GameStateBuilder {
    let here = location(HERE).connections(vec![CONNECTED]);
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(mover)
        .with_location(here)
        .with_location(location(CONNECTED))
        .open_turn(INV)
}

/// An enemy that deals `damage` damage and `horror` horror when it attacks.
fn attacker(id: u32, damage: u8, horror: u8) -> StagedEnemy {
    enemy(id).attack_damage(damage).attack_horror(horror)
}

/// The Fight / Evade target: fight 3, evade 3, 2 health.
fn ghoul() -> StagedEnemy {
    enemy(GHOUL).fight(3).evade(3).health(2)
}

/// `fighter`'s open turn at `HERE`, engaged with `enemy` there: Fight is
/// co-location-gated (RR p.12), Evade engagement-gated (RR p.11) — this
/// satisfies both. Bag is `Numeric(0)` so the test outcome is determined
/// purely by (skill vs fight/evade).
fn fight_evade_board(fighter: Seat, enemy: StagedEnemy) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(fighter)
        .with_location(location(HERE))
        .with_enemy(enemy.engaged_with(INV))
        .with_chaos_bag(bag_only_zero())
        .open_turn(INV)
}
