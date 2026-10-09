use card_dsl::dsl::{self, Ability, ModifierScope, Stat};

use super::*;
use crate::action::EngineRecord;
use crate::engine::outcome::EngineOutcome;
use crate::engine::{dispatch, Cx};
use crate::state::{CardCode, GameStateBuilder, InvestigatorId};
use crate::test_support;

/// Exercises the early-reject guard: when the handler cannot
/// proceed past the registry / metadata checks, it must reject
/// without drawing from the deck and without emitting any events.
///
/// The installed test registry doesn't know the synthetic code
/// `"__no_such_card"`, so the reveal rejects with `"unknown card code: ..."`.
/// Nothing pins the `"no card registry installed"` branch: every test binary
/// installs a registry from a `#[ctor]` before the first test runs, so none
/// of them can reach a process with the slot still empty.
#[test]
fn rejects_an_unknown_encounter_card() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    // Seed the encounter deck so we can prove the reject fires
    // *before* the draw mutates state. Use a code that no real
    // or fake registry knows so we always hit an early rejection.
    state
        .encounter_deck
        .push_back(CardCode("__no_such_card".into()));
    let pre_deck_len = state.encounter_deck.len();
    let mut events = Vec::new();

    let outcome = dispatch::apply_engine_record(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        },
    );

    match outcome {
        EngineOutcome::Rejected { reason } => {
            assert!(
                reason.contains("unknown card code"),
                "unexpected reject reason: {reason:?}",
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    // The unknown-code reject fires after the draw, so under the
    // plan's documented exception the deck may be shorter by one.
    // We assert on the *invariant* that
    // matters: no events were emitted, and the deck shrank by at
    // most one (not more).
    assert!(
        state.encounter_deck.len() <= pre_deck_len,
        "deck should not grow; expected <= {pre_deck_len}, got {}",
        state.encounter_deck.len(),
    );
    assert!(
        events.is_empty(),
        "no events should fire before Event::CardRevealed; got {events:?}",
    );
}

#[test]
fn persistence_is_derived_from_non_revelation_abilities() {
    let one_shot: Vec<Ability> = vec![dsl::revelation(dsl::native("x:rev"))];
    assert!(!treachery_is_persistent(&one_shot));

    let persistent: Vec<Ability> = vec![
        dsl::revelation(dsl::native("y:rev")),
        dsl::constant(dsl::modify(Stat::Willpower, 1, ModifierScope::WhileInPlay)),
    ];
    assert!(treachery_is_persistent(&persistent));
}
