//! `seat(..).as_card(..)` reads a real investigator's printed card from
//! `cards::REGISTRY`, the metadata real seating reads (#1014).
//!
//! Roland Banks (01001), per `data/arkhamdb-snapshot/pack/core/core.json`:
//! `"skill_willpower": 3`, `"skill_intellect": 3`, `"skill_combat": 4`,
//! `"skill_agility": 2`, `"health": 9`, `"sanity": 5`. A Roland seated by
//! writing his code onto `test_investigator` would keep its 3/3/3/3 skills.

use cards::TEST_REGISTRIES;
use game_core::state::{GameStateBuilder, InvestigatorId, Skills};
use game_core::test_support::seat;

const ROLAND: &str = "01001";

#[test]
fn a_seated_roland_has_his_printed_skills_name_and_capacity() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).as_card(ROLAND))
        .build();
    let roland = &state.investigators[&InvestigatorId(1)];
    assert_eq!(
        roland.skills,
        Skills {
            willpower: 3,
            intellect: 3,
            combat: 4,
            agility: 2,
        },
    );
    assert_eq!(roland.name, "Roland Banks");
    assert_eq!(
        (
            roland.max_health(TEST_REGISTRIES.cards),
            roland.max_sanity(TEST_REGISTRIES.cards)
        ),
        (9, 5)
    );
}
