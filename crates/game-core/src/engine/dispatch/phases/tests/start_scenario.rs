use card_dsl::card_data::SkillKind;

use super::*;
use crate::action::RosterEntry;
use crate::state::{CardCode, GameStateBuilder, SkillSubstitution};
use crate::test_support::{TestSession, TEST_INV};
use crate::{engine, test_support};

#[test]
fn start_scenario_rejects_when_roster_would_seat_zero_investigators() {
    let state = GameStateBuilder::new().build();
    let result = engine::seat_and_open_with_registries(state, &[], engine::TEST_REGISTRIES);
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.round, 0, "state unchanged on reject");
    assert!(result.events.is_empty(), "no events on reject");
}

#[test]
fn round_end_clears_round_scoped_skill_substitutions() {
    // RR p.24 step 4.6: "until the end of the round" effects expire as the
    // round ends — in upkeep_round_end_teardown (after the round-end forced
    // abilities), not the next Mythos step.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_registries(engine::TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_turn_order([id])
        .with_round(1)
        .ending_upkeep_phase()
        .build();
    state.skill_substitutions.push(SkillSubstitution {
        investigator: id,
        use_skill: SkillKind::Intellect,
        for_skills: vec![SkillKind::Combat, SkillKind::Agility],
    });

    // Settling runs the round end; play goes on to round 2's Mythos phase.
    let session = TestSession::new_with_registries(state, engine::TEST_REGISTRIES);
    crate::assert_event!(
        session.events(),
        Event::PhaseEnded {
            phase: Phase::Upkeep
        }
    );
    assert_eq!(session.state().round, 2, "the round ended");
    assert!(
        session.state().skill_substitutions.is_empty(),
        "round end (step 4.6) clears round-scoped substitutions",
    );
}

#[test]
fn seat_and_open_rejects_an_empty_roster() {
    let state = GameStateBuilder::new().build();
    let result = engine::seat_and_open_with_registries(state, &[], engine::TEST_REGISTRIES);
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "an empty roster must reject, got {:?}",
        result.outcome
    );
}

// A non-empty roster whose entry cannot be resolved to investigator
// stats rejects with state unchanged. game-core unit tests run against
// the synthetic test registry, which knows no "01001", so resolution
// fails on the unknown-code path. The registry-backed happy path is
// pinned by the `crates/cards` integration test, which installs
// `cards::REGISTRY`.
/// `seat_and_open` shuffles the shared encounter deck (like the player
/// decks) with the scenario-start RNG: the deck's multiset is preserved
/// and `EncounterDeckShuffled` fires.
#[test]
fn start_scenario_shuffles_the_encounter_deck() {
    let mut state = GameStateBuilder::new().build();
    let codes = ["e1", "e2", "e3", "e4", "e5"];
    state.encounter_deck = codes.iter().map(|c| CardCode::new(*c)).collect();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck: vec![],
    }];

    let result = engine::seat_and_open_with_registries(state, &roster, engine::TEST_REGISTRIES);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    crate::assert_event!(result.events, Event::EncounterDeckShuffled);
    let mut after: Vec<&str> = result
        .state
        .encounter_deck
        .iter()
        .map(CardCode::as_str)
        .collect();
    after.sort_unstable();
    assert_eq!(after, codes, "shuffle preserves the deck's contents");
}

#[test]
fn start_scenario_rejects_unresolvable_roster_entry() {
    let state = GameStateBuilder::new().build();
    let roster = vec![RosterEntry {
        investigator: CardCode::new("01001"),
        deck: vec![],
    }];
    let result = engine::seat_and_open_with_registries(state, &roster, engine::TEST_REGISTRIES);
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.round, 0, "state unchanged on reject");
    assert!(result.events.is_empty());
}
