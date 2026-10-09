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

use card_dsl::dsl::{self, EventPattern, EventTiming, LocationTarget};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{Cx, EngineOutcome, OptionTarget, TimingEvent};
use game_core::event::{Event, LapseReason};
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, GameStateBuilder, InvestigatorId, LocationId, Owner,
};
use game_core::test_support::{self, MockRegistry, TestSession};

/// `Forced - At the end of the round: Remove every clue from your location.`
const EMPTIES: &str = "_frl_forced_empties_location";
/// `Forced - At the end of the round: Discover 1 clue at your location.` Has
/// the potential to change the game state only while the location has a clue.
const DISCOVERS: &str = "_frl_forced_discovers";
/// `Forced - At the end of the round: Gain 2 resources.` Changes the game
/// state whatever the board.
const GAINS: &str = "_frl_forced_gains";

const INV: InvestigatorId = InvestigatorId(1);
const LOCATION: LocationId = LocationId(1);

fn empty_your_location(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let location = cx.state.investigators[&ctx.controller]
        .current_location
        .expect("the controller is at a location");
    cx.state
        .locations
        .get_mut(&location)
        .expect("the location is in play")
        .clues = 0;
    EngineOutcome::Done
}

fn gain_2(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    cx.state
        .investigators
        .get_mut(&ctx.controller)
        .expect("the controller is seated")
        .resources += 2;
    cx.events.push(Event::ResourcesGained {
        investigator: ctx.controller,
        amount: 2,
    });
    EngineOutcome::Done
}

#[ctor::ctor(unsafe)]
fn install() {
    let at_round_end = |effect| {
        vec![dsl::forced_on_event(
            EventPattern::RoundEnded,
            EventTiming::At,
            effect,
        )]
    };
    MockRegistry::new()
        .with_abilities(EMPTIES, move || {
            at_round_end(dsl::native("_frl:empty_your_location"))
        })
        .with_abilities(DISCOVERS, move || {
            at_round_end(dsl::discover_clue(LocationTarget::YourLocation, 1))
        })
        .with_abilities(GAINS, move || at_round_end(dsl::native("_frl:gain_2")))
        .with_native_effect("_frl:empty_your_location", empty_your_location)
        .with_native_effect("_frl:gain_2", gain_2)
        .install();
}

fn instance(i: usize) -> CardInstanceId {
    CardInstanceId(u32::try_from(i + 1).expect("small index"))
}

/// The investigator at a location with 2 clues, holding `codes` in their
/// threat area (instance ids from 1, in order), at the end of the round.
fn round_ends_holding(codes: &[&str]) -> TestSession {
    let mut investigator = test_support::test_investigator(1);
    for (i, code) in codes.iter().enumerate() {
        investigator.threat_area.push(CardInPlay::enter_play(
            CardCode::new(*code),
            instance(i),
            Owner::EncounterDeck,
        ));
    }
    let mut location = test_support::test_location(1, "Mock location");
    location.clues = 2;
    GameStateBuilder::new()
        .with_location(location)
        .with_investigator_at(investigator, LOCATION)
        .with_turn_order([INV])
        .session()
        .fire_at(TimingEvent::RoundEnded)
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

/// The anchors the current prompt offers, in order.
fn offered(session: &TestSession) -> Vec<OptionTarget> {
    session
        .prompt()
        .options
        .iter()
        .filter_map(|option| option.target.clone())
        .collect()
}

#[test]
fn an_ability_a_sibling_left_unable_to_change_the_game_state_is_withdrawn_from_the_run() {
    let session = round_ends_holding(&[EMPTIES, DISCOVERS, GAINS]);
    assert_eq!(
        offered(&session),
        vec![
            OptionTarget::CardInstance(instance(0)),
            OptionTarget::CardInstance(instance(1)),
            OptionTarget::CardInstance(instance(2)),
        ],
        "with 2 clues on the location, all three abilities can change the game state"
    );

    let session = session.pick(OptionTarget::CardInstance(instance(0)));
    assert_eq!(
        offered(&session),
        vec![OptionTarget::CardInstance(instance(2))],
        "the lead's next prompt no longer offers the discovery"
    );
    assert_eq!(
        lapses(&session),
        vec![(INV, CardCode::new(DISCOVERS), LapseReason::NoStateChange)],
        "the withdrawal is logged with the reason the gate gives"
    );

    let before = session.state().investigators[&INV].resources;
    let session = session.pick(OptionTarget::CardInstance(instance(2)));
    let investigator = &session.state().investigators[&INV];
    assert_eq!(
        investigator.resources,
        before + 2,
        "the ability left standing resolved"
    );
    assert_eq!(
        investigator.clues, 0,
        "the withdrawn discovery never resolved"
    );
}

#[test]
fn a_run_whose_remaining_abilities_all_lapse_closes_on_its_own() {
    let session =
        round_ends_holding(&[EMPTIES, DISCOVERS]).pick(OptionTarget::CardInstance(instance(0)));

    assert_eq!(
        lapses(&session),
        vec![(INV, CardCode::new(DISCOVERS), LapseReason::NoStateChange)],
    );
    let result = session.finish();
    assert!(
        matches!(result.outcome, EngineOutcome::Done),
        "the emptied run closed rather than prompting the lead with no options, got {:?}",
        result.outcome,
    );
    assert!(
        result.state.continuations.is_empty(),
        "nothing is left on the stack: {:?}",
        result.state.continuations,
    );
}
