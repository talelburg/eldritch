//! The `RoundEnded` condition: an agenda's `OnEvent(RoundEnded)`
//! Forced ability fires at the end of the round (step 4.6).

use std::sync::LazyLock;

use card_dsl::dsl::{self, EventPattern, EventTiming, InvestigatorTarget};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx, OptionTarget, Registries};
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    Agenda, CardCode, GameState, GameStateBuilder, InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{self, seat, MockRegistry, SeatLookup as _, TestSession};

const AGENDA: &str = "TEST-AGENDA";

/// A second round-end forced source: a threat-area card with an
/// `OnEvent(RoundEnded)` forced ability dealing 1 horror to its controller.
/// Mirrors the real two-source collision (agenda 01107's doom + Dissonant
/// Voices 01165's discard) so the lead orders both at round end (#213).
const DISSONANT: &str = "TEST-DISSONANT";

fn set_doom(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    cx.place_doom_on_agenda(5);
    EngineOutcome::Done
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        // `TEST_INV` rides `build`'s composed `metadata_for_test_inv`, so capacity
        // reads work under this registry.
        cards: MockRegistry::new()
            .with_abilities(AGENDA, || {
                vec![dsl::forced_on_event(
                    EventPattern::RoundEnded,
                    EventTiming::At,
                    dsl::native("test:set-doom"),
                )]
            })
            .with_abilities(DISSONANT, || {
                vec![dsl::forced_on_event(
                    EventPattern::RoundEnded,
                    EventTiming::At,
                    dsl::deal_horror(InvestigatorTarget::You, 1u8),
                )]
            })
            .with_native_effect("test:set-doom", set_doom)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// Make the test agenda current on a built `state`; the agenda deck has no
/// builder setter.
fn with_test_agenda(mut state: GameState) -> GameState {
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(AGENDA),
        doom_threshold: 10,
    }];
    state.agenda_index = 0;
    state
}

#[test]
fn round_ended_fires_agenda_forced_ability() {
    // The end of the Upkeep phase, and with it the round: settling runs step
    // 4.6, where the agenda's round-end forced fires, and play goes on into the
    // next round's Mythos phase.
    let state = with_test_agenda(
        GameStateBuilder::new()
            .with_registries(registries())
            .with_seat(seat(1).at(HERE))
            .with_location(test_support::test_location(HERE.0, "Study"))
            .with_turn_order([INV])
            .ending_upkeep_phase()
            .build(),
    );
    let session = TestSession::new_with_registries(state, registries());

    // The round really ended: play is in the next round's Mythos phase.
    assert_eq!(session.state().phase, Phase::Mythos);
    assert_eq!(session.state().round, 1, "round bumped on Mythos entry");
    assert_eq!(
        session.state().agenda_doom,
        6,
        "RoundEnded fired the agenda ability (native placed 5), then Mythos entry \
         placed its +1 doom",
    );
}

#[test]
fn two_round_end_forced_suspend_then_resume_the_upkeep_tail() {
    // #213 end-to-end through `apply`: two simultaneous round-end forced
    // abilities (agenda doom + a threat-area discard-shape) make `upkeep_phase_end`
    // open the forced run and suspend for the lead's ordering. Resolving both
    // closes the run, whose `UpkeepAfterRoundEnded` continuation resumes the
    // upkeep tail — the Upkeep→Mythos transition — that a terminal close would
    // have dropped.
    let state = with_test_agenda(
        GameStateBuilder::new()
            .with_registries(registries())
            .with_seat(
                seat(1)
                    .at(HERE)
                    .actions(0)
                    // Non-empty deck so the upkeep 4.4 draw doesn't fire a
                    // deckout horror penalty and muddy the horror assertion.
                    .deck(["filler-card"])
                    .threat_area([DISSONANT]),
            )
            .with_location(test_support::test_location(HERE.0, "Study"))
            .open_turn(INV)
            .build(),
    );
    let dissonant = state.card(INV, DISSONANT);

    // EndTurn drives Investigation → Enemy → Upkeep and reaches step 4.6,
    // where the two round-end forced abilities fire simultaneously. The
    // forced run suspends for the lead's ordering choice rather than
    // transitioning to Mythos.
    let paused = TestSession::new_with_registries(state, registries()).take(&TurnAction::EndTurn);
    assert_eq!(
        (
            paused.offered_at(&OptionTarget::Agenda),
            paused.offered_at(&dissonant),
        ),
        (1, 1),
        "two round-end forced must present the lead a choice: {:?}",
        paused.prompt(),
    );
    assert_eq!(
        paused.state().phase,
        Phase::Upkeep,
        "no Upkeep→Mythos transition until both forced resolve",
    );
    assert_eq!(
        paused.state().agenda_doom,
        0,
        "no forced resolves until ordered"
    );
    assert_eq!(paused.state().investigators[&INV].horror(), 0);

    // Resolve the first ordered forced; the second is still pending.
    let after_first = paused.pick(OptionTarget::Agenda);
    assert_eq!(
        after_first.offered_at(&dissonant),
        1,
        "second round-end forced still pending: {:?}",
        after_first.prompt(),
    );
    assert_eq!(
        after_first.state().phase,
        Phase::Upkeep,
        "still mid-forced-run; no transition yet",
    );

    // Resolve the second: the forced run closes, its UpkeepAfterRoundEnded
    // continuation runs the upkeep tail (no act round-end window here) and
    // transitions Upkeep → Mythos.
    let done = after_first.pick(dissonant);
    // The upkeep tail runs through to Mythos, which pauses at the step-1.4
    // encounter-draw prompt after placing its +1 doom.
    done.assert_at_encounter_draw();
    assert_eq!(
        done.state().agenda_doom,
        6,
        "agenda round-end forced resolved (native placed 5), then Mythos entry \
         placed its +1 doom — the upkeep tail ran through to the next phase",
    );
    assert_eq!(
        done.state().investigators[&INV].horror(),
        1,
        "threat-area round-end forced resolved",
    );
    assert_eq!(
        done.state().phase,
        Phase::Mythos,
        "upkeep tail resumed: Upkeep → Mythos after both forced resolved",
    );
    assert_eq!(
        done.state().round,
        1,
        "round bumped (0 → 1) on Mythos entry"
    );
}
