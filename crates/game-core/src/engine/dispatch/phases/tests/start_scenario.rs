use card_dsl::card_data::SkillKind;

use super::*;
use crate::action::RosterEntry;
use crate::engine::Responder;
use crate::state::{
    CardCode, GameStateBuilder, LastingEffect, LastingEffectKind, Lifetime, SkillSubstitution,
};
use crate::test_support::{TestSession, TEST_INV, TEST_REGISTRIES};
use crate::{engine, test_support};

#[test]
fn start_scenario_rejects_when_roster_would_seat_zero_investigators() {
    let state = GameStateBuilder::new().build();
    let result = engine::seat_and_open(state, &[], 0, TEST_REGISTRIES);
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.round, 0, "state unchanged on reject");
    assert!(result.events.is_empty(), "no events on reject");
}

#[test]
fn round_end_clears_round_lifetime_lasting_effects() {
    // Appendix II step 4.6: "until the end of the round" lasting effects
    // expire as the round ends — in upkeep_round_end, before `RoundEnded` is
    // emitted, not at the next Mythos step. That they expire ahead of the
    // round-end abilities is `tests/round_lifetime.rs`'s.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_seating([id])
        .with_round(1)
        .ending_upkeep_phase()
        .build();
    state.lasting_effects.push(LastingEffect {
        lifetime: Lifetime::Round,
        kind: LastingEffectKind::SkillSubstitution(SkillSubstitution {
            investigator: id,
            use_skill: SkillKind::Intellect,
            for_skills: vec![SkillKind::Combat, SkillKind::Agility],
        }),
    });

    // Settling runs the round end; play goes on to round 2's Mythos phase.
    let session = TestSession::new(state, TEST_REGISTRIES);
    crate::assert_event!(
        session.events(),
        Event::PhaseEnded {
            phase: Phase::Upkeep
        }
    );
    assert_eq!(session.state().round, 2, "the round ended");
    assert!(
        session.state().lasting_effects.is_empty(),
        "round end (step 4.6) clears round-scoped substitutions",
    );
}

#[test]
fn seat_and_open_rejects_an_empty_roster() {
    let state = GameStateBuilder::new().build();
    let result = engine::seat_and_open(state, &[], 0, TEST_REGISTRIES);
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
// pinned by the `crates/cards` integration test, which names
// `cards::TEST_REGISTRIES`.
/// `seat_and_open` shuffles the shared encounter deck (like the player
/// decks) with the scenario-start RNG: the deck's multiset is preserved
/// and `EncounterDeckShuffled` fires.
#[test]
fn start_scenario_shuffles_the_encounter_deck() {
    let codes = ["e1", "e2", "e3", "e4", "e5"];
    let state = GameStateBuilder::new().with_encounter_deck(codes).build();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck: vec![],
        random_basic_weaknesses: 0,
    }];

    let result = engine::seat_and_open(state, &roster, 0, TEST_REGISTRIES);

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
        random_basic_weaknesses: 0,
    }];
    let result = engine::seat_and_open(state, &roster, 0, TEST_REGISTRIES);
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.round, 0, "state unchanged on reject");
    assert!(result.events.is_empty());
}

/// Two different investigators, `TEST_INV` and `test_inv(2)`, with empty decks.
fn two_seat_roster() -> Vec<RosterEntry> {
    [CardCode::new(TEST_INV), test_support::test_inv(2)]
        .into_iter()
        .map(|investigator| RosterEntry {
            investigator,
            deck: vec![],
            random_basic_weaknesses: 0,
        })
        .collect()
}

/// Appendix III step 3: *"Choose one of those investigators to be the lead
/// investigator for this game."* The lead is a roster index at creation; with
/// the second seat chosen, it is stored, and the setup mulligan — *"Each
/// player, in player order, may mulligan once at this time."* (step 8) — asks
/// the lead first and the first seat second.
#[test]
fn seating_with_the_second_seat_as_lead_mulligans_it_first() {
    let result = engine::seat_and_open(
        GameStateBuilder::new().build(),
        &two_seat_roster(),
        1,
        TEST_REGISTRIES,
    );

    assert_eq!(result.state.lead, Some(InvestigatorId(2)));
    let session = TestSession::new(result.state, TEST_REGISTRIES);
    assert_eq!(
        session.prompt().responder,
        Responder::Investigator(InvestigatorId(2))
    );
    let session = session.choose_from_hand(&[]);
    assert_eq!(
        session.prompt().responder,
        Responder::Investigator(InvestigatorId(1))
    );
}

/// A lead index past the roster's end names no investigator: seating rejects
/// with the state unchanged.
#[test]
fn seating_rejects_a_lead_index_past_the_roster() {
    let result = engine::seat_and_open(
        GameStateBuilder::new().build(),
        &two_seat_roster(),
        2,
        TEST_REGISTRIES,
    );

    let EngineOutcome::Rejected { reason } = &result.outcome else {
        panic!("expected a rejection, got {:?}", result.outcome);
    };
    assert!(reason.contains("lead"), "rejected for its lead: {reason}");
    assert_eq!(result.state.round, 0, "state unchanged on reject");
    assert_eq!(result.state.lead, None);
    assert!(result.events.is_empty());
}
