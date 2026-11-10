//! Knife 01086's option labels against the real corpus.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01086):
//!
//! > [action]: <b>Fight.</b> You get +1 [combat] for this attack.
//! > [action] Discard Knife: <b>Fight.</b> You get +2 [combat] for this
//! > attack. This attack deals +1 damage.
//!
//! Both abilities print the Fight designator, so the word alone would label
//! them alike; each carries a distinguisher quoted from its own line.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::state::{EnemyId, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, seat};

const KNIFE: &str = "01086";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);

#[test]
fn the_turn_menu_tells_knifes_two_fights_apart() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE).in_play([KNIFE]))
        .with_location(test_support::location(HERE))
        .with_enemy(test_support::enemy(ENEMY).engaged_with(INV))
        .open_turn(INV)
        .session();

    let knife: Vec<&str> = session
        .offered_labels()
        .into_iter()
        .filter(|label| label.starts_with("Knife: Fight"))
        .collect();
    assert_eq!(
        knife,
        ["Knife: Fight — +1 [combat]", "Knife: Fight — Discard Knife"],
        "offered: {:?}",
        session.offered_labels()
    );
}
