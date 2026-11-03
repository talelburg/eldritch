//! The order of a skill test's **ST.7 results** (`GLOSSARY.md`) when
//! committed cards add them. RR ST.7
//! (`data/rules-reference/rules/Appendix_II_Timing_and_Gameplay.md`):
//!
//! > If there are multiple results to be applied during this step, the
//! > investigator performing the test applies those results in the order of
//! > his or her choice.
//!
//! The Skids O'Toole 01003 elder sign + Overpower 01091 case is in
//! `skids_otoole.rs`; the card result + scenario token case is in the
//! `scenarios` crate's `st7_result_order.rs`.

use card_dsl::dsl::HarmKind;
use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    ChaosBag, ChaosToken, CommittedIndex, GameStateBuilder, InvestigatorId, LocationId, SkillKind,
};
use game_core::test_support::{location, seat, TestSession};
use game_core::{assert_event_count, assert_event_sequence};

/// Perception 01090: *"Max 1 committed per skill test. If this test is
/// successful, draw 1 card."*
const PERCEPTION: &str = "01090";
/// Fearless 01067: *"If this skill test is successful, heal 1 horror."* It
/// prints no commit limit, so two copies can be committed to one test.
const FEARLESS: &str = "01067";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// A successful Investigate with Perception committed, on a blank token, has
/// one ST.7 result after the clue discovery: Perception's draw. It applies
/// with no prompt, after the clue.
#[test]
fn a_single_addition_applies_after_the_clue_without_asking() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .hand([PERCEPTION])
                .deck(["spare-1", "spare-2"]),
        )
        .with_location(location(HERE).shroud(2).clues(1))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .session()
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[PERCEPTION]);

    session.assert_at_turn_menu(INV);
    assert_event_sequence!(
        session.events(),
        Event::SkillTestSucceeded { .. },
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV,
        Event::CardsDrawn { investigator, count: 1 } if *investigator == INV,
        Event::SkillTestEnded { .. },
    );
}

/// A willpower (1) test with two copies of Fearless committed, on a blank
/// token, by an investigator with 2 horror: two ST.7 results are pending.
/// Returns the session at the result-order prompt.
fn two_fearless_committed() -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).hand([FEARLESS, FEARLESS]).harm(0, 2))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .session()
        .test_skill(INV, SkillKind::Willpower, 1)
        .commit(&[FEARLESS, FEARLESS])
}

/// Each copy is its own option, anchored to its position in the committed
/// list, so the two differ in the (label, target, investigator) key replay
/// finds an option by (ADR 0022).
#[test]
fn two_identical_copies_are_two_distinct_options() {
    let session = two_fearless_committed();
    assert_eq!(
        session.offered_targets(),
        [
            Some(OptionTarget::CommittedCard {
                index: CommittedIndex(0)
            }),
            Some(OptionTarget::CommittedCard {
                index: CommittedIndex(1)
            }),
        ],
        "{:?}",
        session.prompt()
    );
}

/// Either copy first, both heal: 2 horror becomes 0, and the second applies
/// with no further prompt.
#[test]
fn either_copy_first_both_heal() {
    for index in [0, 1] {
        let session = two_fearless_committed().pick(OptionTarget::CommittedCard {
            index: CommittedIndex(index),
        });
        session.assert_at_turn_menu(INV);
        assert_event_count!(
            session.events(),
            2,
            Event::Healed { investigator, kind: HarmKind::Horror, amount: 1 } if *investigator == INV
        );
        assert_eq!(
            session.state().investigators[&INV].horror(),
            0,
            "copy {index} first"
        );
    }
}
