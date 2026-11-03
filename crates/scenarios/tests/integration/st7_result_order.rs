//! RR ST.7's results in the performer's order, through a real treachery and a
//! real scenario token.
//!
//! Rotting Remains 01163: *"Revelation - Test \[willpower\] (3). For each point
//! you fail by, take 1 horror."* The Gathering 01104's Cultist token: *"-1. If
//! you fail, take 1 horror."* Failing the test on the Cultist leaves two ST.7
//! results pending, and RR ST.7 (`Appendix_II_Timing_and_Gameplay.md`) gives
//! their order to the performer: *"If there are multiple results to be applied
//! during this step, the investigator performing the test applies those
//! results in the order of his or her choice."*
//!
//! The tests live here rather than in the `cards` crate because the Cultist's
//! horror comes from the scenario module, which `cards::TEST_REGISTRIES` does
//! not carry.

use game_core::action::InputResponse;
use game_core::assert_event_sequence;
use game_core::engine::{InputKind, OptionId, PromptNature, Responder};
use game_core::event::Event;
use game_core::scenario::ScenarioId;
use game_core::state::{
    ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, MythosPhaseFrame,
    MythosResume, Phase, SkillKind,
};
use game_core::test_support::{location, seat, TestSession};
use scenarios::the_gathering;

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(20);
const ROTTING_REMAINS: &str = "01163";

/// The card result's option: Rotting Remains is in limbo while its Revelation
/// resolves, so the option has no board entity and is picked by label.
const CARD_RESULT: &str = "Result of Rotting Remains";
/// The Cultist token's option: a scenario token has no board entity either.
const TOKEN_RESULT: &str = "Result of the Cultist token";

/// Mythos step 1.4 of a Gathering game, with a willpower-2 investigator about
/// to draw Rotting Remains and a chaos bag holding only `token`. Willpower 2
/// on the Cultist's -1 against difficulty 3 fails by 2, so the card result is
/// 2 horror and the token result 1, and the two orders are told apart by the
/// order of the horror events.
fn drawing_rotting_remains(token: ChaosToken) -> TestSession {
    let state = GameStateBuilder::new()
        .with_registries(scenarios::TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE).skill(SkillKind::Willpower, 2))
        .with_location(location(HERE))
        .with_seating([INV])
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([INV])
        .with_encounter_deck([ROTTING_REMAINS])
        .with_scenario_id(ScenarioId::new(the_gathering::ID))
        .with_chaos_bag(ChaosBag::new([token]))
        .build();
    TestSession::new(state, scenarios::TEST_REGISTRIES)
}

/// Draw Rotting Remains on the Cultist and commit nothing, so the test fails
/// and the session rests at the order prompt.
fn failed_on_the_cultist() -> TestSession {
    drawing_rotting_remains(ChaosToken::Cultist)
        .confirm()
        .commit(&[])
}

/// The amounts of every horror placed on `INV`, in order.
fn horror_placed(session: &TestSession) -> Vec<u8> {
    session
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::HorrorPlacedOnInvestigator {
                investigator,
                amount,
            } if *investigator == INV => Some(*amount),
            _ => None,
        })
        .collect()
}

#[test]
fn the_performer_is_asked_which_horror_lands_first() {
    let session = failed_on_the_cultist();
    let request = session.prompt();
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert_eq!(request.nature, PromptNature::Selection, "{request:?}");
    assert!(!request.skippable, "{request:?}");
    assert_eq!(
        request.responder,
        Responder::Investigator(INV),
        "{request:?}"
    );
    let offered: Vec<(&str, bool)> = request
        .options
        .iter()
        .map(|option| (option.label.as_str(), option.target.is_none()))
        .collect();
    assert_eq!(offered, [(CARD_RESULT, true), (TOKEN_RESULT, true)]);
    assert!(
        horror_placed(&session).is_empty(),
        "no result applies before the performer picks"
    );
}

#[test]
fn picking_the_card_result_first_takes_its_horror_first() {
    let session = failed_on_the_cultist().pick_unanchored(CARD_RESULT);
    assert_eq!(horror_placed(&session), [2, 1]);
    assert_eq!(session.state().investigators[&INV].horror(), 3);
    session.assert_at_turn_menu(INV);
}

#[test]
fn picking_the_token_result_first_takes_its_horror_first() {
    let session = failed_on_the_cultist().pick_unanchored(TOKEN_RESULT);
    assert_eq!(horror_placed(&session), [1, 2]);
    assert_eq!(session.state().investigators[&INV].horror(), 3);
    session.assert_at_turn_menu(INV);
}

#[test]
fn an_answer_naming_no_offered_option_is_rejected_and_the_prompt_stays_up() {
    let session = failed_on_the_cultist();
    session.assert_rejects(|s| s.answer_as(INV, InputResponse::PickSingle(OptionId(99))));
    let session = failed_on_the_cultist()
        .answer_as(INV, InputResponse::PickSingle(OptionId(99)))
        .pick_unanchored(TOKEN_RESULT);
    assert_eq!(horror_placed(&session), [1, 2]);
}

#[test]
fn a_token_with_no_result_leaves_the_card_result_alone_with_no_prompt() {
    // Willpower 2 on a -1 against 3 fails by 2: Rotting Remains' 2 horror is
    // the only ST.7 result, so it applies without asking.
    let session = drawing_rotting_remains(ChaosToken::Numeric(-1))
        .confirm()
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    assert_eq!(horror_placed(&session), [2]);
    // The horror lands at ST.7, after the failure and before the test ends,
    // and Rotting Remains is discarded once its Revelation is done.
    assert_event_sequence!(
        session.events(),
        Event::SkillTestFailed { investigator, by: 2, .. } if *investigator == INV,
        Event::HorrorPlacedOnInvestigator { investigator, amount: 2 } if *investigator == INV,
        Event::SkillTestEnded { investigator } if *investigator == INV,
        Event::CardDiscarded { code, .. } if code.as_str() == ROTTING_REMAINS,
    );
}
