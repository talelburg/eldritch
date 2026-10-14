//! End-to-end test for Arcane Studies (01062): the two `[fast]` activated
//! abilities record a `ThisSkillTest`-scoped modifier for willpower (index 0)
//! or intellect (index 1).
//!
//! Printed text (`data/arkhamdb-snapshot/pack/core/core.json`):
//! *"\[fast\] Spend 1 resource: You get +1 \[willpower\] for this skill
//! test."* / *"\[fast\] Spend 1 resource: You get +1 \[intellect\] for this
//! skill test."*
//!
//! Ruling (<https://arkhamdb.com/card/01062>): *"You can use \[fast\]fast
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

const ARCANE_STUDIES: &str = "01062";
const INV: InvestigatorId = InvestigatorId(1);

/// The printed order of Arcane Studies' two `[fast]` abilities.
const WILLPOWER_ABILITY: usize = 0;
const INTELLECT_ABILITY: usize = 1;

/// One Arcane Studies in play, the controller at their open turn with
/// `resources` resources, and a single `Numeric(0)` chaos bag.
fn with_arcane_studies(resources: u8) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).resources(resources).in_play([ARCANE_STUDIES]))
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .session()
}

fn arcane_studies(state: &GameState) -> OptionTarget {
    state.card(INV, ARCANE_STUDIES)
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
    let card = arcane_studies(session.state());
    let mut session = session.test_skill(INV, skill, difficulty);
    for _ in 0..times {
        session = session.pick_nth_at(card.clone(), ability);
    }
    let session = session.skip().commit(&[]).skip();
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn willpower_ability_buffs_a_willpower_test() {
    // 3 willpower + 1 + 0 (token) = 4 vs 4 → succeed by 0; unbuffed it fails.
    let session = with_arcane_studies(5);
    let result = test_buying(session, SkillKind::Willpower, 4, WILLPOWER_ABILITY, 1);

    assert_event!(
        result.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Willpower, margin: 0 }
            if *investigator == INV
    );
    assert_eq!(result.state().investigators[&INV].resources, 4, "1 resource paid");
}

#[test]
fn intellect_ability_buffs_an_intellect_test() {
    let result = test_buying(
        with_arcane_studies(5),
        SkillKind::Intellect,
        4,
        INTELLECT_ABILITY,
        1,
    );

    assert_event!(
        result.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == INV
    );
}

#[test]
fn willpower_ability_does_not_buff_an_intellect_test() {
    // The row targets willpower; 3 intellect + 0 vs 4 → fail by 1.
    let result = test_buying(
        with_arcane_studies(5),
        SkillKind::Intellect,
        4,
        WILLPOWER_ABILITY,
        1,
    );

    assert_event!(
        result.events(),
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 1, .. }
            if *investigator == INV
    );
}

/// The ruling: no limit beyond the cost. Three buys in one test stack to +3,
/// for 3 resources. (A fourth resource is left over, so the window stays open
/// for the pass the flow expects.)
#[test]
fn the_ability_can_be_bought_as_often_as_it_can_be_paid() {
    // 3 willpower + 3 + 0 = 6 vs 6 → succeed by 0.
    let result = test_buying(
        with_arcane_studies(4),
        SkillKind::Willpower,
        6,
        WILLPOWER_ABILITY,
        3,
    );

    assert_event!(
        result.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Willpower, margin: 0 }
            if *investigator == INV
    );
    assert_eq!(result.state().investigators[&INV].resources, 1, "3 resources paid");
}
