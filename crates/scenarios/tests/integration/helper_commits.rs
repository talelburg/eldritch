//! A helper's committed card when the performer is eliminated mid-test.
//!
//! Daisy commits Unexpected Courage 01093 to Roland's test, which fails on
//! The Gathering 01104's Cultist token: *"-1. If you fail, take 1 horror."*
//! The horror defeats Roland at ST.7. `glossary/Elimination.md` removes from
//! the game only *his* cards: *"The cards he or she controls in play and all
//! of the cards in his or her out-of-play areas … are removed from the
//! game."* Daisy's card is still hers, and goes to her discard pile.
//!
//! The test lives here rather than in the `cards` crate because the Cultist's
//! horror comes from the scenario module, which `cards::TEST_REGISTRIES` does
//! not carry.

use game_core::scenario::ScenarioId;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, SkillKind, Status,
};
use game_core::test_support::{location, seat};
use scenarios::the_gathering;

const UNEXPECTED_COURAGE: &str = "01093";
/// Guts 01089: two willpower icons. Its draw is on success, and the test fails.
const GUTS: &str = "01089";
/// The investigator performing the test, one horror from defeat.
const ROLAND: InvestigatorId = InvestigatorId(1);
/// A helper at Roland's location.
const DAISY: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(20);

#[test]
fn a_helpers_card_goes_to_the_helpers_discard_when_the_performer_is_eliminated() {
    // Willpower 1 + 2 wild (Daisy's Unexpected Courage) + 2 willpower
    // (Roland's Guts) - 1 (Cultist) = 4 fails against 5.
    let session = GameStateBuilder::new()
        .with_registries(scenarios::TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Willpower, 1)
                .harm(0, 7)
                .hand([GUTS]),
        )
        .with_seat(seat(2).at(HERE).hand([UNEXPECTED_COURAGE]))
        .with_location(location(HERE))
        .with_seating([ROLAND, DAISY])
        .with_scenario_id(ScenarioId::new(the_gathering::ID))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Cultist]))
        .open_turn(ROLAND)
        .session()
        .test_skill(ROLAND, SkillKind::Willpower, 5)
        .commit(&[UNEXPECTED_COURAGE])
        .commit(&[GUTS]);

    let state = session.state();
    assert_eq!(state.investigators[&ROLAND].status, Status::Defeated);
    assert_eq!(
        state.investigators[&DAISY].discard,
        vec![CardCode::new(UNEXPECTED_COURAGE)],
        "Daisy's committed card is discarded to her pile",
    );
    assert_eq!(
        state.investigators[&ROLAND].removed_from_game,
        vec![CardCode::new(GUTS)],
        "Roland's own committed card is removed from the game, and Daisy's is not",
    );
    assert!(!state.has_skill_test_in_flight());
}
