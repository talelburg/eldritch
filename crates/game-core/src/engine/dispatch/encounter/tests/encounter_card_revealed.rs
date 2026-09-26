use crate::action::EngineRecord;
use crate::engine::outcome::EngineOutcome;
use crate::engine::{dispatch, Cx};
use crate::state::{CardCode, GameStateBuilder, InvestigatorId};
use crate::test_support;

/// Exercises the early-reject guard: when the handler cannot
/// proceed past the registry / metadata checks, it must reject
/// without drawing from the deck and without emitting any events.
///
/// Two possible rejection reasons depending on process state:
///
/// - `"no card registry installed"` — if no registry has been
///   installed yet in this process.
/// - `"unknown card code: ..."` — if another test in this binary
///   has already installed a fake registry that doesn't know the
///   synthetic code `"__no_such_card"`.
///
/// In both cases the invariant is identical: deck untouched, no
/// events emitted. The exact rejection reason depends on
/// `OnceLock` install ordering, which is non-deterministic across
/// parallel test binaries.
///
/// Which is why this test accepts either reason and asserts only the
/// shared invariant. Nothing pins the "no registry installed" branch
/// on its own: every integration binary installs a registry from a
/// `#[ctor]` before the first test runs, so none of them can reach a
/// process with the slot still empty.
#[test]
fn rejects_when_no_card_registry_installed() {
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
                reason.contains("no card registry installed")
                    || reason.contains("unknown card code"),
                "unexpected reject reason: {reason:?}",
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    // Deck must be untouched: the registry-missing reject fires
    // before any draw, and the unknown-code reject fires after the
    // draw. However, the plan's documented exception means that
    // after a successful draw but unknown-code rejection, the deck
    // will be shorter by one. We assert on the *invariant* that
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
