//! A forced run re-checks its abilities (#607): once the lead resolves one
//! ability of a 2+ forced run, a sibling that can no longer change the game
//! state is withdrawn and logged rather than resolved, and a run left with no
//! abilities closes on its own.
//!
//! `glossary/Ability.md`: *"If a forced ability does not have the potential to
//! change the game state, the ability does not initiate."*
//!
//! Synthetic probes only (ADR 0016). Each `_frl_*` code models one primitive —
//! a forced ability that empties a location of clues, one that discovers a clue
//! there, one that changes the state whatever the board — and none stands in
//! for a printed card.

use std::sync::LazyLock;

use card_dsl::dsl::{self, EventPattern, EventTiming, InvestigatorTarget, LocationTarget};
use game_core::engine::Registries;
use game_core::event::{Event, LapseReason};
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    CardCode, Continuation, GameStateBuilder, InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{self, seat, MockRegistry, SeatLookup as _, TestSession};

/// `Forced - At the end of the round: Discover 2 clues at your location.`
/// Empties [`LOCATION`], which holds [`LOCATION_CLUES`].
const EMPTIES: &str = "_frl_forced_empties_location";
/// `Forced - At the end of the round: Discover 1 clue at your location.` Has
/// the potential to change the game state only while the location has a clue.
const DISCOVERS: &str = "_frl_forced_discovers";
/// `Forced - At the end of the round: Gain 2 resources.` Changes the game
/// state whatever the board.
const GAINS: &str = "_frl_forced_gains";

const INV: InvestigatorId = InvestigatorId(1);
const LOCATION: LocationId = LocationId(1);

/// The clues on [`LOCATION`] at the end of the round.
const LOCATION_CLUES: u8 = 2;

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| {
        let at_round_end = |effect| {
            vec![dsl::forced_on_event(
                EventPattern::RoundEnded,
                EventTiming::At,
                effect,
            )]
        };
        Registries {
            cards: MockRegistry::new()
                .with_abilities(EMPTIES, move || {
                    at_round_end(dsl::discover_clue(
                        LocationTarget::YourLocation,
                        LOCATION_CLUES,
                    ))
                })
                .with_abilities(DISCOVERS, move || {
                    at_round_end(dsl::discover_clue(LocationTarget::YourLocation, 1))
                })
                .with_abilities(GAINS, move || {
                    at_round_end(dsl::gain_resources(InvestigatorTarget::You, 2))
                })
                .build(),
            scenarios: &ScenarioRegistry::EMPTY,
        }
    });
    *REGISTRIES
}

/// The investigator at a location with 2 clues, holding `codes` in their
/// threat area, at the end of round 1: settling runs the Upkeep phase end
/// into the round-end coordinator, which offers the lead the forced run.
fn round_ends_holding(codes: &[&str]) -> TestSession {
    let mut location = test_support::test_location(LOCATION.0, "Mock location");
    location.clues = LOCATION_CLUES;
    GameStateBuilder::new()
        .with_registries(registries())
        .with_location(location)
        .with_seat(seat(1).at(LOCATION).threat_area(codes.iter().copied()))
        .with_turn_order([INV])
        .with_round(1)
        .ending_upkeep_phase()
        .session()
}

/// Every forced option withdrawn so far, with its reason.
fn lapses(session: &TestSession) -> Vec<(InvestigatorId, CardCode, LapseReason)> {
    session
        .events()
        .iter()
        .filter_map(|e| match e {
            Event::ReactionOptionLapsed {
                investigator,
                code,
                reason,
            } => Some((*investigator, code.clone(), *reason)),
            _ => None,
        })
        .collect()
}

#[test]
fn an_ability_a_sibling_left_unable_to_change_the_game_state_is_withdrawn_from_the_run() {
    let session = round_ends_holding(&[EMPTIES, DISCOVERS, GAINS]);
    let (empties, discovers, gains) = (
        session.state().card(INV, EMPTIES),
        session.state().card(INV, DISCOVERS),
        session.state().card(INV, GAINS),
    );
    assert_eq!(
        session
            .prompt()
            .options
            .iter()
            .filter_map(|option| option.target.clone())
            .collect::<Vec<_>>(),
        vec![empties.clone(), discovers, gains.clone()],
        "with 2 clues on the location, all three abilities can change the game state"
    );

    let session = session.pick(empties);
    assert_eq!(
        session
            .prompt()
            .options
            .iter()
            .filter_map(|option| option.target.clone())
            .collect::<Vec<_>>(),
        vec![gains.clone()],
        "the lead's next prompt no longer offers the discovery"
    );
    assert_eq!(
        lapses(&session),
        vec![(INV, CardCode::new(DISCOVERS), LapseReason::NoStateChange)],
        "the withdrawal is logged with the reason the gate gives"
    );

    let before = session.state().investigators[&INV].resources;
    let session = session.pick(gains);
    let investigator = &session.state().investigators[&INV];
    assert_eq!(
        investigator.resources,
        before + 2,
        "the ability left standing resolved"
    );
    assert_eq!(
        investigator.clues, LOCATION_CLUES,
        "only the emptying discovery resolved; the withdrawn one never did"
    );
}

#[test]
fn a_run_whose_remaining_abilities_all_lapse_closes_on_its_own() {
    let session = round_ends_holding(&[EMPTIES, DISCOVERS]);
    let empties = session.state().card(INV, EMPTIES);
    let session = session.pick(empties);

    assert_eq!(
        lapses(&session),
        vec![(INV, CardCode::new(DISCOVERS), LapseReason::NoStateChange)],
    );
    // The emptied run closed rather than prompting the lead with no options:
    // the round ended and play went on to the next round's Mythos phase.
    assert_eq!(
        session.state().phase,
        Phase::Mythos,
        "{:?}",
        session.prompt()
    );
    assert_eq!(session.state().round, 2);
    // The subject is the run's frame closing, so the stack is read directly.
    assert!(
        !session
            .state()
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::TimingPointWindow(_))),
        "no window frame is left on the stack: {:?}",
        session.state().continuations,
    );
}
