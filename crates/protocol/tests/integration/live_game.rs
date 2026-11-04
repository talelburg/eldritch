//! [`LiveGame`] driven from outside: built from a staged board and its settled
//! prompt, then submitted to and committed.

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::{
    ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget, Responder,
};
use game_core::event::Event;
use game_core::rng::RngState;
use game_core::scenario::{ResolutionId, ScenarioEnding};
use game_core::state::{GameState, GameStateBuilder, InvestigatorId, LocationId, Phase};
use game_core::test_support::{self, location, seat, TEST_REGISTRIES};
use protocol::live::{LiveGame, LiveSummary, Refused};
use protocol::logged_answer::LoggedResponse;
use protocol::{AccountId, Assignment, LogPosition, ServerMessage};

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// The account holding [`INV`].
const HOLDER: AccountId = AccountId(7);
/// An account holding no investigator: a viewer.
const VIEWER: AccountId = AccountId(8);

/// The position the [`at_turn_menu`] game's open turn waits at: three answers
/// are already in its log.
const TURN_MENU: LogPosition = LogPosition(3);

/// [`INV`] assigned to [`HOLDER`], and [`VIEWER`] holding nobody.
fn holder_holds_inv() -> Assignment {
    Assignment::from_iter([(INV, HOLDER)])
}

/// One investigator with 3 resources and their turn open, as a live game three
/// answers into its log, with [`INV`] held by [`HOLDER`] and the RNG partway
/// through its stream.
fn at_turn_menu() -> LiveGame {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_rng(RngState::at(41, 5))
        .with_location(location(HERE))
        .with_seat(seat(1).at(HERE).resources(3))
        .open_turn(INV)
        .session();
    let outcome = test_support::awaiting_request(session.prompt().clone());
    LiveGame::new(
        session.state().clone(),
        outcome,
        vec![Event::ScenarioStarted],
        TURN_MENU,
        holder_holds_inv(),
        TEST_REGISTRIES,
    )
}

/// The turn menu's answer that takes a resource.
fn take_a_resource(state: &GameState) -> PlayerAction {
    let idx = enumerate::legal_actions(state, TEST_REGISTRIES.cards)
        .iter()
        .position(|a| *a == TurnAction::Resource { investigator: INV })
        .expect("taking a resource is legal at the open turn");
    PlayerAction::ResolveInput {
        investigator: INV,
        response: InputResponse::PickSingle(OptionId(
            u32::try_from(idx).expect("an action index fits u32"),
        )),
    }
}

fn resources(state: &GameState) -> u8 {
    state.investigators[&INV].resources
}

#[test]
fn a_legal_answer_is_accepted_with_its_logged_answer_and_applied() {
    let mut live = at_turn_menu();
    let action = take_a_resource(live.state());
    let PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(picked),
        ..
    } = action
    else {
        unreachable!("take_a_resource picks one option");
    };

    let accepted = live
        .submit(HOLDER, TURN_MENU, action)
        .expect("the engine accepts it");

    let LoggedResponse::PickSingle { option } = &accepted.answer().response else {
        panic!("logged as {:?}", accepted.answer().response);
    };
    assert_eq!(
        option.id, picked,
        "the logged answer names the option picked"
    );
    let draws = accepted.draws();
    let applied = live.commit(accepted).applied_for(HOLDER);
    assert_eq!(
        resources(&applied.state),
        4,
        "the Applied holds the next state"
    );
    assert_eq!(
        draws,
        live.state().rng.draws,
        "the draws checkpoint is the next state's"
    );
}

#[test]
fn submitting_changes_nothing_until_commit() {
    let live = at_turn_menu();
    let before = live.state().clone();
    let outcome_before = live.outcome().clone();

    let _accepted = live
        .submit(HOLDER, TURN_MENU, take_a_resource(live.state()))
        .expect("the engine accepts it");

    assert_eq!(*live.state(), before);
    assert_eq!(*live.outcome(), outcome_before);
    assert_eq!(live.prompt(), TURN_MENU);
}

#[test]
fn commit_advances_to_the_applied_state() {
    let mut live = at_turn_menu();
    let accepted = live
        .submit(HOLDER, TURN_MENU, take_a_resource(live.state()))
        .expect("the engine accepts it");

    let applied = live.commit(accepted).applied_for(HOLDER);

    assert_eq!(live.prompt(), LogPosition(4));
    assert_eq!(*live.outcome(), applied.outcome);
    assert_eq!(resources(live.state()), 4);
    assert_eq!(resources(&applied.state), 4);
}

#[test]
fn an_applied_names_the_position_it_answered_and_the_next_prompt() {
    let mut live = at_turn_menu();
    let accepted = live
        .submit(HOLDER, TURN_MENU, take_a_resource(live.state()))
        .expect("the engine accepts it");

    let applied = live.commit(accepted).applied_for(HOLDER);

    assert_eq!(applied.answered, LogPosition(3));
    assert_eq!(applied.prompt, LogPosition(4));
}

#[test]
fn an_applied_says_who_answered_and_what_they_picked() {
    let mut live = at_turn_menu();
    let accepted = live
        .submit(HOLDER, TURN_MENU, take_a_resource(live.state()))
        .expect("the engine accepts it");

    let applied = live.commit(accepted).applied_for(VIEWER);

    assert_eq!(applied.by, INV);
    let LoggedResponse::PickSingle { option } = &applied.response else {
        panic!("answered with {:?}", applied.response);
    };
    assert_eq!(option.label, "Gain resource");
    assert_eq!(option.target, Some(OptionTarget::ResourcePool(INV)));
}

#[test]
fn an_applied_carries_no_rng_state() {
    let mut live = at_turn_menu();
    let accepted = live
        .submit(HOLDER, TURN_MENU, take_a_resource(live.state()))
        .expect("the engine accepts it");
    let committed = live.commit(accepted);
    assert_ne!(
        live.state().rng,
        RngState::new(0),
        "the live game holds a real RNG state"
    );

    for account in [HOLDER, VIEWER] {
        assert_eq!(
            committed.applied_for(account).state.rng,
            RngState::new(0),
            "account {account} is sent no seed and no draw count"
        );
    }
}

#[test]
fn an_answer_the_engine_rejects_is_refused() {
    let live = at_turn_menu();

    let refused = live
        .submit(
            HOLDER,
            TURN_MENU,
            PlayerAction::ResolveInput {
                investigator: INV,
                response: InputResponse::PickSingle(OptionId(999)),
            },
        )
        .expect_err("no option 999 is offered");

    assert!(matches!(refused, Refused::Engine(_)), "got {refused:?}");
}

#[test]
fn an_answer_to_an_earlier_position_is_refused_as_a_stale_prompt() {
    let live = at_turn_menu();

    let refused = live
        .submit(HOLDER, LogPosition(2), take_a_resource(live.state()))
        .expect_err("position 2 was answered already");

    assert_eq!(refused, Refused::StalePrompt);
}

#[test]
fn an_in_range_option_from_a_second_tab_on_the_old_prompt_is_refused() {
    // Tab A takes a resource; tab B, still showing that same turn menu, then
    // picks the same option. It is still in range on the new turn menu, so
    // only the position tells the two prompts apart.
    let mut live = at_turn_menu();
    let tab_b = take_a_resource(live.state());
    let tab_a = live
        .submit(HOLDER, TURN_MENU, take_a_resource(live.state()))
        .expect("the engine accepts tab A's answer");
    let _ = live.commit(tab_a);

    let refused = live
        .submit(HOLDER, TURN_MENU, tab_b)
        .expect_err("tab B answered the prompt tab A already answered");

    assert_eq!(refused, Refused::StalePrompt);
}

#[test]
fn an_account_answering_for_an_investigator_it_does_not_hold_is_refused_as_not_yours() {
    let live = at_turn_menu();

    let refused = live
        .submit(VIEWER, TURN_MENU, take_a_resource(live.state()))
        .expect_err("the viewer holds no investigator");

    assert_eq!(refused, Refused::NotYours);
}

#[test]
fn a_stale_answer_for_another_accounts_investigator_is_refused_as_stale() {
    // The position is checked first: a viewer's stale tab is stale, whoever
    // it answers for.
    let live = at_turn_menu();

    let refused = live
        .submit(VIEWER, LogPosition(2), take_a_resource(live.state()))
        .expect_err("position 2 was answered already");

    assert_eq!(refused, Refused::StalePrompt);
}

#[test]
fn an_answer_for_an_investigator_not_yours_is_refused_before_the_engine_runs() {
    // Option 999 is one the engine would reject; the account check refuses
    // it first.
    let live = at_turn_menu();

    let refused = live
        .submit(
            VIEWER,
            TURN_MENU,
            PlayerAction::ResolveInput {
                investigator: INV,
                response: InputResponse::PickSingle(OptionId(999)),
            },
        )
        .expect_err("the viewer holds no investigator");

    assert_eq!(refused, Refused::NotYours);
}

#[test]
fn hello_carries_the_build_account_assignment_position_outcome_and_setup_events() {
    let live = at_turn_menu();
    assert_ne!(
        live.state().rng,
        RngState::new(0),
        "the live game holds a real RNG state"
    );

    let ServerMessage::Hello {
        build,
        you,
        assigned,
        prompt,
        state,
        outcome,
        events,
    } = live.hello_for(VIEWER)
    else {
        panic!("expected Hello");
    };

    assert_eq!(build, protocol::build_id());
    assert_eq!(you, VIEWER);
    assert_eq!(assigned, holder_holds_inv());
    assert_eq!(assigned.holder(INV), Some(HOLDER));
    assert_eq!(prompt, TURN_MENU);
    assert_eq!(resources(&state), 3);
    assert_eq!(state.rng, RngState::new(0), "Hello carries no RNG state");
    assert_eq!(outcome, *live.outcome());
    assert_eq!(events, vec![Event::ScenarioStarted]);
}

// The summary for an account, which "Your games" reads.

/// Tal's account, holding [`INV`].
const TAL: AccountId = AccountId(1);
/// Sam's account, holding [`INV2`].
const SAM: AccountId = AccountId(2);
/// The second seated investigator.
const INV2: InvestigatorId = InvestigatorId(2);

/// Two investigators, [`INV`] held by [`TAL`] and [`INV2`] by [`SAM`], in
/// round 3's Investigation phase, waiting on `request`.
fn two_seats_awaiting(request: InputRequest) -> LiveGame {
    two_seats(test_support::awaiting_request(request), None)
}

/// [`two_seats_awaiting`]'s two investigators, after an apply left `outcome`
/// with the scenario's ending latched at `ending`.
fn two_seats(outcome: EngineOutcome, ending: Option<ScenarioEnding>) -> LiveGame {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(seat(1).at(HERE))
        .with_seat(seat(2).at(HERE))
        .build();
    state.round = 3;
    state.phase = Phase::Investigation;
    state.ending = ending;
    LiveGame::new(
        state,
        outcome,
        Vec::new(),
        LogPosition(0),
        Assignment::from_iter([(INV, TAL), (INV2, SAM)]),
        TEST_REGISTRIES,
    )
}

/// The investigators a summary waits on, as `(investigator, holder, yours)`.
fn waited(summary: &LiveSummary) -> Vec<(InvestigatorId, Option<AccountId>, bool)> {
    summary
        .waiting_on
        .iter()
        .map(|waited| (waited.investigator, waited.holder, waited.yours))
        .collect()
}

#[test]
fn a_prompt_for_your_investigator_needs_you() {
    let live = two_seats_awaiting(
        InputRequest::confirm("Mulligan?").answered_by(Responder::Investigator(INV2)),
    );

    let summary = live.summary_for(SAM);

    assert_eq!(waited(&summary), vec![(INV2, Some(SAM), true)]);
    assert!(summary.needs_you());
    assert_eq!(summary.waiting_for(), vec![SAM]);
    assert_eq!((summary.round, summary.phase), (3, Phase::Investigation));
}

#[test]
fn a_prompt_for_another_accounts_investigator_waits_for_that_account() {
    let live = two_seats_awaiting(
        InputRequest::confirm("Mulligan?").answered_by(Responder::Investigator(INV)),
    );

    let summary = live.summary_for(SAM);

    assert_eq!(waited(&summary), vec![(INV, Some(TAL), false)]);
    assert!(!summary.needs_you());
    assert_eq!(summary.waiting_for(), vec![TAL]);
}

#[test]
fn a_lead_prompt_waits_on_the_lead_investigator() {
    let live = two_seats_awaiting(
        InputRequest::confirm("Advance the act?").answered_by(Responder::Lead(INV2)),
    );

    assert_eq!(
        waited(&live.summary_for(SAM)),
        vec![(INV2, Some(SAM), true)]
    );
    assert_eq!(live.summary_for(TAL).waiting_for(), vec![SAM]);
}

#[test]
fn a_window_holding_options_for_two_accounts_needs_both() {
    let live = two_seats_awaiting(
        InputRequest::pick_single(
            "Reaction window",
            vec![
                ChoiceOption::new(OptionId(0), "Tal's reaction").offered_to(INV),
                ChoiceOption::new(OptionId(1), "Sam's reaction").offered_to(INV2),
                ChoiceOption::new(OptionId(2), "Tal passes").offered_to(INV),
            ],
        )
        .answered_by(Responder::Several),
    );

    let for_sam = live.summary_for(SAM);
    let for_tal = live.summary_for(TAL);

    assert_eq!(
        waited(&for_sam),
        vec![(INV, Some(TAL), false), (INV2, Some(SAM), true)]
    );
    assert!(for_sam.needs_you(), "Sam holds an option");
    assert!(for_tal.needs_you(), "Tal holds an option");
    assert_eq!(for_sam.waiting_for(), vec![TAL, SAM]);
}

#[test]
fn a_viewer_is_never_needed() {
    let live = two_seats_awaiting(
        InputRequest::pick_single(
            "Reaction window",
            vec![
                ChoiceOption::new(OptionId(0), "Tal's reaction").offered_to(INV),
                ChoiceOption::new(OptionId(1), "Sam's reaction").offered_to(INV2),
            ],
        )
        .answered_by(Responder::Several),
    );

    let summary = live.summary_for(VIEWER);

    assert!(!summary.needs_you());
    assert!(summary.waiting_on.iter().all(|waited| !waited.yours));
    assert_eq!(summary.waiting_for(), vec![TAL, SAM]);
}

#[test]
fn a_game_that_finished_resolving_its_ending_is_finished_with_that_ending() {
    let ending = ScenarioEnding::Resolution(ResolutionId::new(1));
    let live = two_seats(EngineOutcome::Done, Some(ending));

    let summary = live.summary_for(SAM);

    assert_eq!(summary.finished, Some(ending));
    assert!(summary.waiting_on.is_empty());
}

#[test]
fn a_game_that_reached_no_resolution_is_finished_too() {
    let live = two_seats(EngineOutcome::Done, Some(ScenarioEnding::NoResolution));

    assert_eq!(
        live.summary_for(SAM).finished,
        Some(ScenarioEnding::NoResolution)
    );
}

#[test]
fn a_game_still_resolving_its_ending_is_not_finished() {
    // A game-end prompt, asked after the ending latched and before it
    // finished resolving.
    let live = two_seats(
        test_support::awaiting_request(
            InputRequest::confirm("Acknowledge").answered_by(Responder::Investigator(INV2)),
        ),
        Some(ScenarioEnding::Resolution(ResolutionId::new(1))),
    );

    let summary = live.summary_for(SAM);

    assert_eq!(summary.finished, None);
    assert!(summary.needs_you());
}

#[test]
fn a_game_being_played_is_not_finished() {
    let live = two_seats_awaiting(
        InputRequest::confirm("Mulligan?").answered_by(Responder::Investigator(INV2)),
    );

    assert_eq!(live.summary_for(SAM).finished, None);
}
