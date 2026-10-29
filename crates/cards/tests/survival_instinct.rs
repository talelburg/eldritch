//! End-to-end tests for Survival Instinct (01081).
//!
//! Printed text (`data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > If this skill test is successful during an evasion attempt, the evading
//! > investigator may immediately disengage from each other enemy engaged with
//! > him or her, and may move to a connecting location.
//!
//! Ruling (<https://arkhamdb.com/card/01081>): *"The clause "may immediately
//! disengage from each other enemy" gives you the choice whether or not to
//! disengage from "each other enemy", but doesn't allow you to pick and choose
//! which to disengage from."*
//!
//! Each "may" is a two-option Decision, answered in printed order with
//! `pick_nth`: the disengage choice first (0 = disengage, 1 = don't), then the
//! move choice (0 = move, 1 = don't).

mod common;

use cards::TEST_REGISTRIES;
use game_core::assert_event_sequence;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionTarget, PromptNature};
use game_core::event::Event;
use game_core::state::{
    ChaosBag, ChaosToken, Enemy, EnemyId, GameStateBuilder, InvestigatorId, LocationId, SkillKind,
};
use game_core::test_support::{self, seat, TestSession};

use crate::common::with_attic;

const SURVIVAL_INSTINCT: &str = "01081";
const INV: InvestigatorId = InvestigatorId(1);
/// A second investigator, left behind in the Study.
const INV2: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
const HALLWAY: LocationId = LocationId(11);
const CELLAR: LocationId = LocationId(12);
const GHOUL: EnemyId = EnemyId(100);
const RAT: EnemyId = EnemyId(101);

const DISENGAGE: usize = 0;
const STAY_ENGAGED: usize = 1;
const MOVE: usize = 0;
const STAY: usize = 1;

/// An evade-2 enemy.
fn enemy(id: EnemyId) -> Enemy {
    let mut enemy = test_support::test_enemy(id.0, "Ghoul");
    enemy.evade = 2;
    enemy
}

/// Investigator 1 at the Study with Survival Instinct in hand, stock agility 3,
/// at their open turn. The Study connects to each of `connections`; each of
/// `engaged` is engaged with them; the bag holds only `token`.
fn board(connections: &[LocationId], engaged: &[EnemyId], token: ChaosToken) -> TestSession {
    let mut study = test_support::test_location(HERE.0, "Study");
    study.connections = connections.to_vec();
    let mut builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(study)
        .with_location(test_support::test_location(HALLWAY.0, "Hallway"))
        .with_location(test_support::test_location(CELLAR.0, "Cellar"))
        .with_seat(seat(1).at(HERE).actions(3).hand([SURVIVAL_INSTINCT]))
        .with_chaos_bag(ChaosBag::new([token]));
    for &id in engaged {
        builder = builder.with_enemy_engaged(enemy(id), INV);
    }
    builder.open_turn(INV).session()
}

/// Evade the Ghoul with the basic action, committing Survival Instinct:
/// agility 3 + 1 icon + `Numeric(0)` = 4 against evade 2 succeeds.
fn evade_committing(session: TestSession) -> TestSession {
    session
        .take(&TurnAction::Evade {
            investigator: INV,
            enemy: GHOUL,
        })
        .commit(&[SURVIVAL_INSTINCT])
}

fn location(session: &TestSession) -> Option<LocationId> {
    session.state().investigators[&INV].current_location
}

fn engaged_with(session: &TestSession, enemy: EnemyId) -> Option<InvestigatorId> {
    session.state().enemies[&enemy].engaged_with
}

fn assert_decision(session: &TestSession) {
    assert_eq!(
        session.prompt().nature,
        PromptNature::Decision,
        "expected a may-choice, got {:?}",
        session.prompt()
    );
}

#[test]
fn disengaging_and_moving_leaves_the_other_enemy_behind() {
    let session = evade_committing(board(&[HALLWAY], &[GHOUL, RAT], ChaosToken::Numeric(0)));
    assert_decision(&session);

    let session = session.pick_nth(DISENGAGE);
    assert_decision(&session);
    // One connecting location: choosing to move goes there.
    let session = session.pick_nth(MOVE);

    session.assert_at_turn_menu(INV);
    assert_eq!(location(&session), Some(HALLWAY));
    assert_eq!(
        engaged_with(&session, RAT),
        None,
        "the rat stayed in the Study"
    );
    assert_eq!(session.state().enemies[&RAT].current_location, Some(HERE));
    let ghoul = &session.state().enemies[&GHOUL];
    assert!(ghoul.exhausted, "the evaded enemy is exhausted");
    assert_eq!(ghoul.engaged_with, None);
}

/// The disengaged rat is still ready, so with nobody moving away the
/// engagement rule engages it again once the card has resolved.
#[test]
fn disengaging_without_moving_lets_the_other_enemy_engage_again() {
    let session = evade_committing(board(&[HALLWAY], &[GHOUL, RAT], ChaosToken::Numeric(0)))
        .pick_nth(DISENGAGE)
        .pick_nth(STAY);

    session.assert_at_turn_menu(INV);
    assert_eq!(location(&session), Some(HERE));
    assert_eq!(engaged_with(&session, RAT), Some(INV));
    assert_eq!(
        engaged_with(&session, GHOUL),
        None,
        "the evaded ghoul is exhausted"
    );
}

/// Not disengaging is the other half of the all-or-none choice; moving then
/// takes the still-engaged rat along.
#[test]
fn moving_without_disengaging_brings_the_engaged_enemy_along() {
    let session = evade_committing(board(&[HALLWAY], &[GHOUL, RAT], ChaosToken::Numeric(0)))
        .pick_nth(STAY_ENGAGED)
        .pick_nth(MOVE);

    session.assert_at_turn_menu(INV);
    assert_eq!(location(&session), Some(HALLWAY));
    assert_eq!(engaged_with(&session, RAT), Some(INV));
    assert_eq!(
        session.state().enemies[&RAT].current_location,
        Some(HALLWAY)
    );
}

#[test]
fn declining_both_changes_nothing_but_the_evasion() {
    let session = evade_committing(board(&[HALLWAY], &[GHOUL, RAT], ChaosToken::Numeric(0)))
        .pick_nth(STAY_ENGAGED)
        .pick_nth(STAY);

    session.assert_at_turn_menu(INV);
    assert_eq!(location(&session), Some(HERE));
    assert_eq!(engaged_with(&session, RAT), Some(INV));
}

/// With no other enemy engaged, there is nothing to disengage from, so only
/// the move is asked.
#[test]
fn with_no_other_enemy_only_the_move_is_offered() {
    let session = evade_committing(board(&[HALLWAY], &[GHOUL], ChaosToken::Numeric(0)));
    assert_decision(&session);

    let session = session.pick_nth(MOVE);

    session.assert_at_turn_menu(INV);
    assert_eq!(location(&session), Some(HALLWAY));
}

/// With two connecting locations, the move asks which.
#[test]
fn the_move_goes_to_a_chosen_connecting_location() {
    let session = evade_committing(board(&[HALLWAY, CELLAR], &[GHOUL], ChaosToken::Numeric(0)))
        .pick_nth(MOVE);
    assert_eq!(session.offered_at(&OptionTarget::Location(HALLWAY)), 1);
    assert_eq!(session.offered_at(&OptionTarget::Location(CELLAR)), 1);

    let session = session.pick(OptionTarget::Location(CELLAR));

    session.assert_at_turn_menu(INV);
    assert_eq!(location(&session), Some(CELLAR));
}

/// With no connecting location and no other enemy, the card has nothing to
/// offer and asks nothing.
#[test]
fn with_nowhere_to_go_and_no_other_enemy_nothing_is_asked() {
    let session = evade_committing(board(&[], &[GHOUL], ChaosToken::Numeric(0)));

    session.assert_at_turn_menu(INV);
    assert_eq!(location(&session), Some(HERE));
}

/// A failed evasion leaves the card inert: 3 + 1 - 3 = 1 against evade 2.
#[test]
fn a_failed_evasion_is_inert() {
    let session = evade_committing(board(&[HALLWAY], &[GHOUL, RAT], ChaosToken::Numeric(-3)));

    session.assert_at_turn_menu(INV);
    assert_eq!(location(&session), Some(HERE));
    assert_eq!(engaged_with(&session, GHOUL), Some(INV));
    assert_eq!(engaged_with(&session, RAT), Some(INV));
}

/// A successful test that is not an evasion attempt leaves the card inert.
#[test]
fn a_successful_test_other_than_an_evasion_is_inert() {
    let session = board(&[HALLWAY], &[RAT], ChaosToken::Numeric(0))
        .test_skill(INV, SkillKind::Agility, 2)
        .commit(&[SURVIVAL_INSTINCT]);

    session.assert_at_turn_menu(INV);
    assert_eq!(location(&session), Some(HERE));
    assert_eq!(engaged_with(&session, RAT), Some(INV));
}

/// Inside the evasion test, the rat left in the Study engages the
/// investigator who stays there before the Attic's horror resolves. The
/// engagement is the move's ENGAGE step, part of the card's effect, and
/// `glossary/Effects.md` reads: *"All aspects of an effect have timing
/// priority over all "after..." triggering conditions that might arise as a
/// consequence of that effect."* Both land before the skill test ends.
#[test]
fn the_rat_left_behind_engages_whoever_stays_before_the_attics_horror() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat(1).at(HERE).actions(3).hand([SURVIVAL_INSTINCT]))
        .with_seat(seat(2).at(HERE))
        .with_seating([INV, INV2])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_enemy_engaged(enemy(GHOUL), INV)
        .with_enemy_engaged(enemy(RAT), INV)
        .open_turn(INV)
        .session();
    let (session, attic) = with_attic(session, &[HERE]);

    let session = evade_committing(session).pick_nth(DISENGAGE).pick_nth(MOVE);

    session.assert_at_turn_menu(INV);
    assert_eq!(location(&session), Some(attic));
    assert_eq!(engaged_with(&session, RAT), Some(INV2));
    assert_eq!(session.state().investigators[&INV].horror(), 1);
    assert_event_sequence!(
        session.events(),
        Event::EnemyDisengaged { enemy, investigator } if *enemy == RAT && *investigator == INV,
        Event::InvestigatorMoved { to, .. } if *to == attic,
        Event::EnemyEngaged { enemy, investigator } if *enemy == RAT && *investigator == INV2,
        Event::HorrorPlacedOnInvestigator { investigator, amount: 1 } if *investigator == INV,
        Event::SkillTestEnded { .. },
    );
}
