//! End-to-end tests for Dig Deep (01077): two `[fast]` abilities, each
//! spending 1 resource for +1 to a skill for this skill test.
//!
//! Printed text (`data/arkhamdb-snapshot/pack/core/core.json`):
//! *"\[fast\] Spend 1 resource: You get +1 \[willpower\] for this skill
//! test."* / *"\[fast\] Spend 1 resource: You get +1 \[agility\] for this
//! skill test."*
//!
//! Ruling (<https://arkhamdb.com/card/01077>): *"You can use \[fast\]fast
//! actions as many times as you want, as long as you can pay the cost; there
//! is no limit."*

use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    ChaosBag, ChaosToken, GameState, GameStateBuilder, InvestigatorId, SkillKind, TokenModifiers,
};
use game_core::test_support::{seat, SeatLookup as _, TestSession};

const DIG_DEEP: &str = "01077";
const INV: InvestigatorId = InvestigatorId(1);

/// The printed order of Dig Deep's two abilities, both offered on the card.
const WILLPOWER_ABILITY: usize = 0;
const AGILITY_ABILITY: usize = 1;

/// Dig Deep in play, the controller at their open turn with `resources`
/// resources and the stock 3 in every skill, and a lone `Numeric(0)` token.
fn with_dig_deep(resources: u8) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).resources(resources).in_play([DIG_DEEP]))
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .session()
}

fn dig_deep(state: &GameState) -> OptionTarget {
    state.card(INV, DIG_DEEP)
}

/// Run a `skill` test at `difficulty`, buying each of `abilities` in turn at
/// the test's first player window, then passing it, committing nothing, and
/// passing the window after the commit.
fn test_buying(
    session: TestSession,
    skill: SkillKind,
    difficulty: i8,
    abilities: &[usize],
) -> TestSession {
    let card = dig_deep(session.state());
    let session = abilities.iter().fold(
        session.test_skill(INV, skill, difficulty),
        |session, &ability| session.pick_nth_at(card.clone(), ability),
    );
    let session = session.skip().commit(&[]).skip();
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn willpower_ability_buffs_the_willpower_test_it_is_bought_during() {
    // 3 + 1 vs 4: succeeds by 0, where 3 alone would fail.
    let session = test_buying(with_dig_deep(5), SkillKind::Willpower, 4, &[WILLPOWER_ABILITY]);

    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Willpower, margin: 0 }
            if *investigator == INV
    );
    assert_eq!(session.state().investigators[&INV].resources, 4);
}

#[test]
fn agility_ability_buffs_the_agility_test_it_is_bought_during() {
    let session = test_buying(with_dig_deep(5), SkillKind::Agility, 4, &[AGILITY_ABILITY]);

    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Agility, margin: 0 }
            if *investigator == INV
    );
}

#[test]
fn willpower_ability_does_not_buff_an_agility_test() {
    // The row names willpower; the agility test ignores it: 3 vs 4 fails by 1.
    let session = test_buying(with_dig_deep(5), SkillKind::Agility, 4, &[WILLPOWER_ABILITY]);

    assert_event!(
        session.events(),
        Event::SkillTestFailed { investigator, skill: SkillKind::Agility, by: 1, .. }
            if *investigator == INV
    );
}

/// The ruling: no limit but the cost. Three buys in one test stack to +3.
#[test]
fn the_same_ability_can_be_bought_again_in_one_test() {
    let session = test_buying(
        with_dig_deep(5),
        SkillKind::Agility,
        6,
        &[AGILITY_ABILITY, AGILITY_ABILITY, AGILITY_ABILITY],
    );

    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Agility, margin: 0 }
            if *investigator == INV
    );
    assert_eq!(session.state().investigators[&INV].resources, 2, "3 paid");
}

/// The cost is the limit: with 2 resources, two buys, and then the card is
/// offered no more.
#[test]
fn the_abilities_stop_being_offered_once_the_resources_run_out() {
    let session = with_dig_deep(2);
    let card = dig_deep(session.state());

    let session = session
        .test_skill(INV, SkillKind::Willpower, 5)
        .pick_nth_at(card.clone(), WILLPOWER_ABILITY)
        .pick_nth_at(card.clone(), WILLPOWER_ABILITY);

    assert_eq!(session.state().investigators[&INV].resources, 0);
    assert_eq!(session.offered_at(&card), 0, "nothing left to pay with");

    // With nothing to offer, the window passed on its own and the test rests
    // at its commit window; the window after the commit has nothing either.
    let session = session.commit(&[]);
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Willpower, margin: 0 }
            if *investigator == INV
    );
}
