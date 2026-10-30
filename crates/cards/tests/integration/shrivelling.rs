//! Shrivelling 01060 end-to-end against the real corpus.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01060):
//!
//! > Uses (4 charges).
//! > [action] Spend 1 charge: <b>Fight.</b> This attack uses [willpower]
//! > instead of [combat] and deals +1 damage. If a [skull], [cultist],
//! > [tablet], [elder_thing], or [auto_fail] symbol is revealed during this
//! > attack, take 1 horror.
//!
//! Ruling (<https://arkhamdb.com/card/01060>):
//!
//! > The *"If a symbol is revealed..."* effect triggers during Step 3 of the
//! > Skill Test Timing.
//!
//! Runs against `cards::TEST_REGISTRIES`, whose scenario registry is empty, so
//! the four scenario symbols initiate no chaos symbol effect of their own: any
//! horror taken is Shrivelling's.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, EnemyId, GameState, GameStateBuilder,
    InvestigatorId, LocationId, SkillKind, UseKind,
};
use game_core::test_support::{enemy, location, seat, uses, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_count, assert_event_sequence, assert_no_event};

const SHRIVELLING: &str = "01060";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);

/// Board: Shrivelling in play with 4 charges, the active investigator at
/// willpower **5** and combat **1**, engaged with a fight-3, health-3 enemy,
/// and a chaos bag holding only `token`. Against fight 3 the two skills land on
/// opposite sides (5 hits, 1 misses), so the test's outcome says which skill
/// the attack used.
fn board(token: ChaosToken) -> GameState {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Willpower, 5)
                .skill(SkillKind::Combat, 1)
                .in_play([(SHRIVELLING, uses(UseKind::Charges, 4))]),
        )
        .with_location(location(HERE))
        .with_enemy_engaged(enemy(ENEMY).fight(3).health(3), INV)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([token]))
        .build()
}

fn charges(state: &GameState) -> u8 {
    let card = state.in_play(INV, SHRIVELLING);
    state.investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == card)
        .and_then(|c| c.uses.get(&UseKind::Charges).copied())
        .expect("Shrivelling carries a charge pool")
}

/// Activate Shrivelling's Fight, committing nothing, and come back to the turn
/// menu.
fn attack(token: ChaosToken) -> TestSession {
    let state = board(token);
    let card = state.in_play(INV, SHRIVELLING);
    let session = TestSession::new(state, TEST_REGISTRIES)
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(card),
            address: AbilityAddress::Printed(0),
        })
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn the_attack_tests_willpower_and_deals_one_extra_damage() {
    // Willpower 5 vs fight 3 succeeds (combat 1 would not), and the hit deals
    // 1 + 1 = 2.
    let session = attack(ChaosToken::Numeric(0));
    assert_event!(
        session.events(),
        Event::SkillTestStarted {
            skill: SkillKind::Willpower,
            ..
        }
    );
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_event!(session.events(), Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(session.state().enemies[&ENEMY].damage, 2);
}

#[test]
fn each_use_spends_one_charge() {
    let session = attack(ChaosToken::Numeric(0));
    assert_eq!(charges(session.state()), 3);
}

#[test]
fn each_listed_symbol_deals_one_horror_at_reveal() {
    for token in [
        ChaosToken::Skull,
        ChaosToken::Cultist,
        ChaosToken::Tablet,
        ChaosToken::ElderThing,
        ChaosToken::AutoFail,
    ] {
        let session = attack(token);
        assert_event_count!(
            session.events(),
            1,
            Event::HorrorPlacedOnInvestigator { amount: 1, .. }
        );
        assert_eq!(
            session.state().investigators[&INV].horror(),
            1,
            "{token:?} deals 1 horror"
        );
        // Step 3 of Skill Test Timing: the horror lands after the token is
        // revealed and before the test's result is determined.
        assert_event_sequence!(
            session.events(),
            Event::ChaosTokenRevealed { token: t, .. } if *t == token,
            Event::HorrorPlacedOnInvestigator { .. },
            Event::SkillTestEnded { .. },
        );
    }
}

#[test]
fn the_horror_is_taken_before_the_result_is_determined() {
    // `[auto_fail]` fails the attack; the horror comes first all the same.
    let session = attack(ChaosToken::AutoFail);
    assert_event_sequence!(
        session.events(),
        Event::HorrorPlacedOnInvestigator { .. },
        Event::SkillTestFailed { .. },
    );
    assert_no_event!(session.events(), Event::EnemyDamaged { .. });
}

#[test]
fn any_other_token_deals_no_horror() {
    for token in [ChaosToken::Numeric(0), ChaosToken::ElderSign] {
        let session = attack(token);
        assert_no_event!(session.events(), Event::HorrorPlacedOnInvestigator { .. });
        assert_eq!(
            session.state().investigators[&INV].horror(),
            0,
            "{token:?} deals no horror"
        );
    }
}

#[test]
fn it_enters_play_with_four_charges() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE).hand([SHRIVELLING]).resources(3))
        .with_location(location(HERE))
        .open_turn(INV)
        .build();
    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    });
    session.assert_at_turn_menu(INV);
    assert_eq!(charges(session.state()), 4);
}
