//! End-to-end test for Hard Knocks (01049): the two `[fast]` activated
//! abilities record a `ThisSkillTest`-scoped modifier for combat (index 0) or
//! agility (index 1).
//!
//! Printed text (`data/arkhamdb-snapshot/pack/core/core.json`):
//! *"\[fast\] Spend 1 resource: You get +1 \[combat\] for this skill test."* /
//! *"\[fast\] Spend 1 resource: You get +1 \[agility\] for this skill test."*
//!
//! Ruling (<https://arkhamdb.com/card/01049>): *"You can use \[fast\]fast
//! actions as many times as you want, as long as you can pay the cost; there
//! is no limit."*
//!
//! The shape is Hyperawareness 01034's, whose test file covers the shared
//! machinery (no buff without a test in flight, the empty wallet, the expiry
//! into the next test). This file pins what differs: the two stats, and the
//! unlimited repeat the ruling grants.

use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    ChaosBag, ChaosToken, GameState, GameStateBuilder, InvestigatorId, SkillKind, TokenModifiers,
};
use game_core::test_support::{seat, SeatLookup as _, TestSession};

const HARD_KNOCKS: &str = "01049";
const INV: InvestigatorId = InvestigatorId(1);

/// The printed order of Hard Knocks' two `[fast]` abilities.
const COMBAT_ABILITY: usize = 0;
const AGILITY_ABILITY: usize = 1;

/// One Hard Knocks in play, the controller at their open turn with
/// `resources` resources, and a single `Numeric(0)` chaos bag.
fn with_hard_knocks(resources: u8) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).resources(resources).in_play([HARD_KNOCKS]))
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .session()
}

fn hard_knocks(state: &GameState) -> OptionTarget {
    state.card(INV, HARD_KNOCKS)
}

/// Run a `skill` test at `difficulty`, buying `ability` `times` times at the
/// test's ST.1 player window, then committing nothing.
fn test_buying(
    session: TestSession,
    skill: SkillKind,
    difficulty: i8,
    ability: usize,
    times: usize,
) -> TestSession {
    let card = hard_knocks(session.state());
    let mut session = session.test_skill(INV, skill, difficulty);
    for _ in 0..times {
        session = session.pick_nth_at(card.clone(), ability);
    }
    let session = session.skip().commit(&[]).skip();
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn combat_ability_buffs_a_combat_test() {
    // 3 combat + 1 + 0 (token) = 4 vs 4 → succeed by 0; unbuffed it fails.
    let result = test_buying(with_hard_knocks(5), SkillKind::Combat, 4, COMBAT_ABILITY, 1);

    assert_event!(
        result.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Combat, margin: 0 }
            if *investigator == INV
    );
    assert_eq!(
        result.state().investigators[&INV].resources,
        4,
        "1 resource paid"
    );
}

#[test]
fn agility_ability_buffs_an_agility_test() {
    let result = test_buying(
        with_hard_knocks(5),
        SkillKind::Agility,
        4,
        AGILITY_ABILITY,
        1,
    );

    assert_event!(
        result.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Agility, margin: 0 }
            if *investigator == INV
    );
}

#[test]
fn combat_ability_does_not_buff_an_agility_test() {
    // The row targets combat; 3 agility + 0 vs 4 → fail by 1.
    let result = test_buying(
        with_hard_knocks(5),
        SkillKind::Agility,
        4,
        COMBAT_ABILITY,
        1,
    );

    assert_event!(
        result.events(),
        Event::SkillTestFailed { investigator, skill: SkillKind::Agility, by: 1, .. }
            if *investigator == INV
    );
}

/// The ruling: no limit beyond the cost. Three buys in one test stack to +3,
/// for 3 resources. (A fourth resource is left over, so the window stays open
/// for the pass the flow expects.)
#[test]
fn the_ability_can_be_bought_as_often_as_it_can_be_paid() {
    // 3 agility + 3 + 0 = 6 vs 6 → succeed by 0.
    let result = test_buying(
        with_hard_knocks(4),
        SkillKind::Agility,
        6,
        AGILITY_ABILITY,
        3,
    );

    assert_event!(
        result.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Agility, margin: 0 }
            if *investigator == INV
    );
    assert_eq!(
        result.state().investigators[&INV].resources,
        1,
        "3 resources paid"
    );
}
