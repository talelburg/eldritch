//! .41 Derringer 01047: a firearm whose Fight spends 1 ammo, gives +2
//! [combat], and deals +1 damage when the attack succeeds by 2 or more.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01047):
//! > Uses (3 ammo).
//! > [action] Spend 1 ammo: **Fight.** You get +2 [combat] for this attack. If
//! > you succeed by 2 or more, this attack deals +1 damage.
//!
//! No rulings (`data/arkhamdb-faq/no-rulings.txt`).
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, EnemyId, GameState, GameStateBuilder,
    InvestigatorId, LocationId, SkillKind, UseKind,
};
use game_core::test_support::{self, seat, uses, Seat, SeatLookup as _, StagedEnemy, TestSession};

const DERRINGER: &str = "01047";

const INV: InvestigatorId = InvestigatorId(1);
const ENEMY: EnemyId = EnemyId(100);
const HERE: LocationId = LocationId(10);

/// A fight-3 enemy with enough health that no attack here defeats it.
fn ghoul() -> StagedEnemy {
    test_support::enemy(ENEMY).fight(3).health(5)
}

/// `seat`'s turn at `HERE`, engaged with [`ghoul`], drawing a `Numeric(0)`
/// token: the attack's margin is the tested skill, +2, minus 3.
fn board(seat: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat.at(HERE))
        .with_location(test_support::location(HERE))
        .with_enemy_engaged(ghoul(), INV)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .session()
}

/// An investigator with `combat`, the Derringer in play with 3 ammo.
fn armed(combat: i8) -> Seat {
    seat(1)
        .skill(SkillKind::Combat, combat)
        .in_play([(DERRINGER, uses(UseKind::Ammo, 3))])
}

/// Activate the Derringer's Fight against the lone engaged enemy, committing
/// nothing, back to the turn menu.
fn fire(session: TestSession) -> TestSession {
    let source = AbilitySource::InPlay(session.state().in_play(INV, DERRINGER));
    let session = session
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source,
            address: AbilityAddress::Printed(0),
        })
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

fn ammo(state: &GameState) -> u8 {
    let weapon = state.in_play(INV, DERRINGER);
    state.investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == weapon)
        .and_then(|c| c.uses.get(&UseKind::Ammo).copied())
        .expect("the Derringer carries an ammo pool")
}

fn damage(session: &TestSession) -> u8 {
    session.state().enemies[&ENEMY].damage
}

/// Played from hand, it enters with the 3 ammo its corpus row prints.
#[test]
fn enters_play_with_three_ammo() {
    let session = board(seat(1).resources(3).hand([DERRINGER])).take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    });
    session.assert_at_turn_menu(INV);
    assert_eq!(ammo(session.state()), 3);
}

/// Combat 1 + 2 = 3 vs fight 3: a hit by 0, which the +2 alone makes. One
/// ammo spent, base damage.
#[test]
fn the_attack_spends_one_ammo_and_gets_plus_two_combat() {
    let session = fire(board(armed(1)));
    assert_event!(session.events(), Event::EnemyDamaged { amount: 1, .. });
    assert_eq!(damage(&session), 1);
    assert_eq!(ammo(session.state()), 2, "1 ammo spent");
}

/// Combat 2 + 2 = 4 vs fight 3: succeeds by 1, base damage.
#[test]
fn succeeding_by_exactly_1_deals_base_damage() {
    let session = fire(board(armed(2)));
    assert_eq!(damage(&session), 1);
}

/// Combat 3 + 2 = 5 vs fight 3: succeeds by 2, +1 damage.
#[test]
fn succeeding_by_exactly_2_deals_one_extra_damage() {
    let session = fire(board(armed(3)));
    assert_event!(session.events(), Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(damage(&session), 2);
}

/// Combat 4 + 2 = 6 vs fight 3: succeeds by 3, still only +1.
#[test]
fn succeeding_by_3_deals_one_extra_damage() {
    let session = fire(board(armed(4)));
    assert_eq!(damage(&session), 2);
}
