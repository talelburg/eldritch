//! Switchblade 01044: a Fast asset whose Fight deals +1 damage when the attack
//! succeeds by 2 or more.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01044):
//! > Fast.
//! > [action]: **Fight.** If you succeed by 2 or more, this attack deals +1
//! > damage.
//!
//! No rulings (`data/arkhamdb-faq/no-rulings.txt`).
//!
//! The margin is read when the attack's damage is determined (RR ST.7), so a
//! card committed after the ability was activated counts towards it.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, ChaosBag, ChaosToken, EnemyId, GameStateBuilder,
    InvestigatorId, LocationId, SkillKind,
};
use game_core::test_support::{enemy, location, seat, Seat, SeatLookup as _, TestSession};

const SWITCHBLADE: &str = "01044";
/// Unexpected Courage 01093: *"Max 1 committed per skill test."*, 2 wild
/// icons, no ability.
const UNEXPECTED_COURAGE: &str = "01093";

const INV: InvestigatorId = InvestigatorId(1);
const ENEMY: EnemyId = EnemyId(100);
const HERE: LocationId = LocationId(10);

/// `seat`'s turn at `HERE`, engaged with a fight-3 enemy with enough health
/// that no attack here defeats it, drawing a `Numeric(0)` token: the attack's
/// margin is the tested skill minus 3.
fn board(seat: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat.at(HERE))
        .with_location(location(HERE))
        .with_enemy_engaged(enemy(ENEMY).fight(3).health(5), INV)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .session()
}

/// An investigator with `combat`, Switchblade in play.
fn armed(combat: i8) -> Seat {
    seat(1)
        .skill(SkillKind::Combat, combat)
        .in_play([SWITCHBLADE])
}

/// Activate Switchblade's Fight against the lone engaged enemy, committing
/// `commit`, back to the turn menu.
fn attack(session: TestSession, commit: &[&str]) -> TestSession {
    let source = AbilitySource::InPlay(session.state().in_play(INV, SWITCHBLADE));
    let session = session
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source,
            address: AbilityAddress::Printed(0),
        })
        .commit(commit);
    session.assert_at_turn_menu(INV);
    session
}

fn damage(session: &TestSession) -> u8 {
    session.state().enemies[&ENEMY].damage
}

#[test]
fn switchblade_plays_without_an_action() {
    let session = board(seat(1).resources(5).hand([SWITCHBLADE]));
    let actions_before = session.state().investigators[&INV].actions_remaining;
    let session = session.take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    });
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert!(
        inv.cards_in_play
            .iter()
            .any(|c| c.code == CardCode::new(SWITCHBLADE)),
        "Switchblade is in play",
    );
    assert_eq!(
        inv.actions_remaining, actions_before,
        "Fast: no action spent"
    );
}

#[test]
fn succeeding_by_exactly_1_deals_base_damage() {
    // Combat 4 vs fight 3: succeeds by 1.
    let session = attack(board(armed(4)), &[]);
    assert_event!(session.events(), Event::EnemyDamaged { amount: 1, .. });
    assert_eq!(damage(&session), 1);
}

#[test]
fn succeeding_by_exactly_2_deals_one_extra_damage() {
    // Combat 5 vs fight 3: succeeds by 2.
    let session = attack(board(armed(5)), &[]);
    assert_event!(session.events(), Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(damage(&session), 2);
}

#[test]
fn succeeding_by_3_deals_one_extra_damage() {
    // Combat 6 vs fight 3: succeeds by 3 — still only +1.
    let session = attack(board(armed(6)), &[]);
    assert_event!(session.events(), Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(damage(&session), 2);
}

/// Combat 3 vs fight 3 succeeds by 0 when the ability is activated; Unexpected
/// Courage's 2 wild icons, committed after that, raise the margin to 2.
#[test]
fn a_card_committed_after_activation_counts_towards_the_margin() {
    let session = attack(
        board(armed(3).hand([UNEXPECTED_COURAGE])),
        &[UNEXPECTED_COURAGE],
    );
    assert_event!(session.events(), Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(damage(&session), 2);
}
