//! Roland's .38 Special 01006 end-to-end against the real `cards::REGISTRY`.
//!
//! Verifies the clue-conditional combat modifier (+3 with a clue on the
//! location, +1 without) and ammo spend.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, EnemyId, GameState, GameStateBuilder,
    InvestigatorId, LocationId, TokenModifiers, UseKind,
};
use game_core::test_support::{self, seat, uses, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_no_event};

const SPECIAL: &str = "01006";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);

/// Board: .38 Special in play with 4 ammo, the active investigator (combat
/// 3) at a location holding `loc_clues`, engaged with a fight-**5** enemy
/// (health 3). Fight 5 distinguishes the modifier branches: +3 → total 6
/// hits, +1 → total 4 misses. A `Numeric(0)` chaos bag is deterministic.
fn board(loc_clues: u8) -> GameState {
    let mut enemy = test_support::test_enemy(ENEMY.0, "Ghoul");
    enemy.fight = 5;
    enemy.max_health = 3;

    let mut location = test_support::test_location(HERE.0, "Study");
    location.clues = loc_clues;

    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .in_play([(SPECIAL, uses(UseKind::Ammo, 4))]),
        )
        .with_location(location)
        // Co-located: a weapon Fight targets enemies at your location.
        .with_enemy_engaged(enemy, INV)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
}

fn ammo(state: &GameState) -> u8 {
    let weapon = state.in_play(INV, SPECIAL);
    state.investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == weapon)
        .and_then(|c| c.uses.get(&UseKind::Ammo).copied())
        .expect("weapon carries an ammo pool")
}

/// Fire the .38 Special's Fight ability, committing nothing.
fn fire(state: GameState) -> TestSession {
    let weapon = state.in_play(INV, SPECIAL);
    let session = TestSession::new(state, TEST_REGISTRIES)
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(weapon),
            address: AbilityAddress::Printed(0),
        })
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn plus_three_with_a_clue_on_location_hits_for_two() {
    // Clue present → +3: combat 3 + 3 = 6 vs fight 5 → success, 1 + 1 = 2.
    let session = fire(board(1));
    assert_event!(session.events(), Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(session.state().enemies[&ENEMY].damage, 2);
    assert_eq!(ammo(session.state()), 3, "1 ammo spent");
}

#[test]
fn plus_one_without_a_clue_misses() {
    // No clue → +1: combat 3 + 1 = 4 vs fight 5 → fail, no damage. Ammo is
    // still spent on activation regardless of the result.
    let session = fire(board(0));
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_no_event!(session.events(), Event::EnemyDamaged { .. });
    assert_eq!(session.state().enemies[&ENEMY].damage, 0);
    assert_eq!(ammo(session.state()), 3, "ammo spent even on a miss");
}
