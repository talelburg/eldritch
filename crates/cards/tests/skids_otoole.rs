//! "Skids" O'Toole 01003, seated from his printed card: his `[fast]` additional
//! action, its once-per-turn limit, and his `[elder_sign]`.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01003):
//! > [fast] During your turn, spend 2 resources: You may take an additional
//! > action this turn. (Limit once per turn.)
//! > [elder_sign] effect: +2. If you succeed, gain 2 resources.
//!
//! His one ruling (<https://arkhamdb.com/card/01003>) is about Parallel Skids'
//! deckbuilding, and bears on neither ability.
//!
//! The action pool's rules (`glossary/Action.md`, FAQ 1.10): *"the first action
//! he or she takes that is able to qualify as that additional action
//! automatically uses that additional action"*, and *"If an effect causes an
//! investigator to lose one or more actions, that investigator has that many
//! fewer standard actions to take that turn"*.

use card_dsl::card_data::CardKind;
use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::{self, EngineOutcome};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, Agenda, CardCode, ChaosBag, ChaosToken, GameState,
    GameStateBuilder, InvestigatorId, LocationId, Phase, TokenModifiers,
};
use game_core::test_support::{self, seat, Seat, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

const SKIDS: &str = "01003";
/// *"The first time you perform one of the following actions (move, fight, or
/// evade) each round, it costs 1 additional action."*
const FROZEN_IN_FEAR: &str = "01164";
/// *"Place 1 doom on the current agenda."* The next round's encounter draw.
const ANCIENT_EVILS: &str = "01166";
const RISE_OF_THE_GHOULS: &str = "01106";
/// Filler for Upkeep's draw.
const EMERGENCY_CACHE: &str = "01088";
/// Beat Cop (01018): its `[fast]` ability keeps the Enemy phase's player
/// window standing.
const BEAT_COP: &str = "01018";
/// Ghoul Minion (01160), for Beat Cop to have a target.
const GHOUL_MINION: &str = "01160";

const INV: InvestigatorId = InvestigatorId(1);
const OTHER: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
const THERE: LocationId = LocationId(11);

/// Skids seated from his printed card at `HERE`, with the stock 5 resources.
fn skids() -> Seat {
    seat(1).as_card(SKIDS).at(HERE)
}

/// `skids`'s open turn at `HERE`, which connects to `THERE`.
fn staged(skids: Seat) -> GameStateBuilder {
    let mut here = test_support::test_location(HERE.0, "Here");
    here.connections = vec![THERE];
    let mut there = test_support::test_location(THERE.0, "There");
    there.connections = vec![HERE];
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(skids)
        .with_location(here)
        .with_location(there)
        .open_turn(INV)
}

/// His `[fast]` ability, on his investigator card.
fn buy(state: &GameState) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(state.investigators[&INV].investigator_card.instance_id),
        address: AbilityAddress::Printed(0),
    }
}

const RESOURCE: TurnAction = TurnAction::Resource { investigator: INV };

/// The actions he can still take this turn: standard plus additional.
fn actions(state: &GameState) -> u8 {
    engine::actions_available(state, TEST_REGISTRIES.cards, INV)
}

fn standard_actions(state: &GameState) -> u8 {
    state.investigators[&INV].actions_remaining
}

fn resources(state: &GameState) -> u8 {
    state.investigators[&INV].resources
}

fn menu(session: &TestSession) -> Vec<TurnAction> {
    enumerate::legal_actions(session.state(), TEST_REGISTRIES.cards)
}

/// Run `NativeCx::lose_actions` for him, as Blinding Light 01066's *"lose 1
/// action this turn"* does.
fn lose(session: TestSession, count: u8) -> TestSession {
    session.edit_state(|state| {
        let (outcome, _) = test_support::run_native(
            state,
            TEST_REGISTRIES.cards,
            |cx, _| {
                cx.lose_actions(INV, count);
                EngineOutcome::Done
            },
            &engine::EvalContext::for_controller(INV),
        );
        assert!(matches!(outcome, EngineOutcome::Done));
    })
}

/// *"spend 2 resources: You may take an additional action this turn."* A
/// `[fast]` ability costs no action, so he has four.
#[test]
fn spending_two_resources_gives_him_a_fourth_action_for_no_action() {
    let session = staged(skids()).session();
    assert_eq!(actions(session.state()), 3);

    let buy = buy(session.state());
    let session = session.take(&buy);
    session.assert_at_turn_menu(INV);
    assert_eq!(resources(session.state()), 3, "2 resources spent");
    assert_eq!(
        actions(session.state()),
        4,
        "three standard and one additional"
    );
    assert_no_event!(session.events(), Event::ActionsRemainingChanged { .. });
}

/// With his standard actions spent the menu still offers actions, and the
/// fourth is his last.
#[test]
fn the_menu_offers_the_additional_action_once_the_standard_ones_are_spent() {
    let session = staged(skids()).session();
    let buy = buy(session.state());
    let session = session
        .take(&buy)
        .take(&RESOURCE)
        .take(&RESOURCE)
        .take(&RESOURCE);
    assert_eq!(actions(session.state()), 1);
    assert!(
        menu(&session).contains(&RESOURCE),
        "a fourth action is offered"
    );

    let session = session.take(&RESOURCE);
    assert_eq!(actions(session.state()), 0);
    assert_eq!(resources(session.state()), 3 + 4, "four Resource actions");
    assert!(!menu(&session).contains(&RESOURCE), "no fifth action");
}

/// FAQ 1.10: the first action that qualifies uses the additional action, so
/// the standard ones are untouched by it.
#[test]
fn his_next_action_uses_the_additional_action_first() {
    let session = staged(skids()).session();
    let buy = buy(session.state());
    let session = session.take(&buy).take(&RESOURCE);
    assert_eq!(
        standard_actions(session.state()),
        3,
        "the additional one was used"
    );
    assert_eq!(actions(session.state()), 3);
    assert_no_event!(session.events(), Event::ActionsRemainingChanged { .. });
}

/// *"(Limit once per turn.)"*
#[test]
fn the_ability_is_not_offered_a_second_time_that_turn() {
    let session = staged(skids()).session();
    let buy = buy(session.state());
    let session = session.take(&buy);
    assert!(resources(session.state()) >= 2, "he could still pay");
    assert!(!menu(&session).contains(&buy), "not offered again");

    let _ = session.assert_rejects(|s| s.submit_off_menu(&buy));
}

/// *"During your turn"*: during another investigator's turn he may not buy it.
#[test]
fn the_ability_is_refused_outside_his_turn() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(skids())
        .with_seat(seat(2).at(HERE))
        .with_location(test_support::test_location(HERE.0, "Here"))
        .open_turn(OTHER)
        .session();
    let buy = buy(session.state());
    assert!(!menu(&session).contains(&buy));

    let _ = session.assert_rejects(|s| s.submit_off_menu(&buy));
}

/// *"During your turn"*: the Enemy phase is no one's turn. Beat Cop 01018's
/// `[fast]` ability keeps the phase's player window standing, so the window
/// would offer his if it could.
#[test]
fn the_ability_is_not_offered_in_the_enemy_phase() {
    let mut ghoul = test_support::test_enemy(7, GHOUL_MINION);
    ghoul.code = CardCode::new(GHOUL_MINION);
    let paused = staged(skids().in_play([BEAT_COP]).deck([EMERGENCY_CACHE]))
        .with_enemy_engaged(ghoul, INV)
        .session()
        .take(&TurnAction::EndTurn);
    assert_eq!(paused.state().phase, Phase::Enemy);
    let card = paused.state().investigators[&INV].card_anchor();
    assert_eq!(
        paused.offered_at(&card),
        0,
        "the Enemy phase window does not offer it: {:?}",
        paused.prompt(),
    );
}

/// Once his turn is ending the ability is not offered: the action it buys would
/// be forfeited at once (FAQ 1.31, `glossary/Action.md`: *"Any unused actions
/// are forfeited at the end of an investigator's turn"*). Frozen in Fear
/// 01164's *"**Forced** - At the end of your turn: Test \[willpower\] (3)."*
/// opens a skill test there, whose player windows his `[fast]` would otherwise
/// keep standing.
#[test]
fn the_ability_is_not_offered_once_his_turn_is_ending() {
    let session = staged(skids().threat_area([FROZEN_IN_FEAR]))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .session()
        .take(&TurnAction::EndTurn);
    assert!(
        session.state().current_skill_test().is_some(),
        "the end-of-turn test is in flight: {:?}",
        session.prompt(),
    );
    let card = session.state().investigators[&INV].card_anchor();
    assert_eq!(
        session.offered_at(&card),
        0,
        "not offered in the end-of-turn window: {:?}",
        session.prompt(),
    );
}

/// FAQ 1.10: *"the investigator’s three standard actions are the ones that are
/// "lost" first"*. One action lost after the grant leaves him three, one of
/// them the additional one; losing more takes that one last.
#[test]
fn losing_actions_takes_the_standard_ones_first() {
    let session = staged(skids()).session();
    let buy = buy(session.state());
    let session = lose(session.take(&buy), 1);
    assert_eq!(actions(session.state()), 3);
    assert_eq!(
        standard_actions(session.state()),
        2,
        "a standard action lost"
    );

    let session = lose(session, 2);
    assert_eq!(actions(session.state()), 1, "the additional action is left");
    assert_eq!(standard_actions(session.state()), 0);

    let session = lose(session, 1);
    assert_eq!(actions(session.state()), 0);
}

/// An action costing two (a Move under Frozen in Fear) can be paid with his
/// last standard action and the additional one.
#[test]
fn an_action_costing_two_spends_the_additional_action_and_a_standard_one() {
    let session = staged(skids().threat_area([FROZEN_IN_FEAR]))
        .session()
        .take(&RESOURCE)
        .take(&RESOURCE);
    assert_eq!(actions(session.state()), 1);
    let move_there = TurnAction::Move {
        investigator: INV,
        destination: THERE,
    };
    assert!(
        !menu(&session).contains(&move_there),
        "one action can't pay two"
    );

    let buy = buy(session.state());
    let session = session.take(&buy);
    assert!(menu(&session).contains(&move_there), "two actions can");
    let session = session.take(&move_there);
    assert_eq!(
        session.state().investigators[&INV].current_location,
        Some(THERE)
    );
    assert_eq!(actions(session.state()), 0);
}

/// FAQ 1.31 (`glossary/Action.md`): *"Any unused actions are forfeited at the
/// end of an investigator's turn"*. An unused grant does not carry into his
/// next turn, and his next turn is a new period for the once-per-turn limit.
#[test]
fn an_unused_additional_action_is_gone_at_the_end_of_his_turn() {
    let session = staged(skids().deck([EMERGENCY_CACHE]))
        .session()
        .edit_state(|state| {
            let threshold = match cards::by_code(RISE_OF_THE_GHOULS)
                .expect("agenda in corpus")
                .kind
            {
                CardKind::Agenda { doom_threshold } => doom_threshold,
                ref kind => panic!("{RISE_OF_THE_GHOULS} is not an Agenda ({kind:?})"),
            };
            state.agenda_deck = vec![Agenda {
                code: CardCode::new(RISE_OF_THE_GHOULS),
                doom_threshold: threshold,
            }];
            state.encounter_deck = [CardCode::new(ANCIENT_EVILS)].into_iter().collect();
        });
    let round = session.state().round;
    let buy = buy(session.state());
    let session = session.take(&buy);
    assert_eq!(actions(session.state()), 4);

    let session = session.take(&TurnAction::EndTurn);
    session.assert_at_encounter_draw();
    // The next round: his turn begins (Appendix II step 2.2) with a player
    // window.
    let session = session.confirm();
    assert_eq!(session.state().phase, Phase::Investigation);
    assert_eq!(session.state().round, round + 1);
    assert_eq!(session.state().active_investigator, Some(INV));
    assert_eq!(
        actions(session.state()),
        3,
        "the unused grant was forfeited"
    );

    // The window is already his turn, so the ability is offered there, a new
    // period for the limit, and what it buys counts.
    let card = session.state().investigators[&INV].card_anchor();
    assert_eq!(
        session.offered_at(&card),
        1,
        "offered again on his next turn"
    );
    let session = session.pick(card);
    session.assert_at_turn_menu(INV);
    assert_eq!(actions(session.state()), 4);
}

/// Skids investigating with only the elder sign in the bag: intellect 3, +2.
fn investigate_on_the_elder_sign(shroud: u8) -> TestSession {
    let mut here = test_support::test_location(HERE.0, "Here");
    here.shroud = shroud;
    here.clues = 1;
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(skids())
        .with_location(here)
        .with_chaos_bag(ChaosBag::new([ChaosToken::ElderSign]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
        .take(&TurnAction::Investigate { investigator: INV })
        // His `[fast]` ability keeps the test's player windows open: pass them.
        .skip()
        .commit(&[])
        .skip()
}

/// *"+2. If you succeed, gain 2 resources."* Intellect 3 + 2 against shroud 5
/// succeeds by 0 (without the +2 it would fail), and the resources come after
/// the investigation's own clue.
#[test]
fn the_elder_sign_adds_two_and_on_success_gains_two_resources_after_the_clue() {
    let session = investigate_on_the_elder_sign(5);
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { margin: 0, .. }
    );
    assert_event_sequence!(
        session.events(),
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV,
        Event::ResourcesGained { investigator, amount: 2 } if *investigator == INV,
    );
    assert_eq!(resources(session.state()), 5 + 2);
}

/// *"If you succeed"*: a failed test gains nothing.
#[test]
fn the_elder_sign_gains_nothing_when_the_test_fails() {
    let session = investigate_on_the_elder_sign(6);
    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestFailed { by: 1, .. });
    assert_no_event!(session.events(), Event::ResourcesGained { .. });
    assert_eq!(resources(session.state()), 5);
}
