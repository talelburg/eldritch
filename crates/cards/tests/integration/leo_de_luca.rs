//! Leo De Luca 01048: the additional action a card in play grants.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01048):
//! > You may take an additional action during your turn.
//!
//! Ally slot, health 2, sanity 2, cost 6.
//!
//! His ruling (<https://arkhamdb.com/card/01048>): *"When spending actions
//! during your turn, the first action you spend each turn takes up the
//! 'additional' designation. If Leo De Luca is discarded mid-turn, you do not
//! lose 1 action from your total pool of actions for the turn. If you then play
//! a second copy of the card, you gain a further additional action."*

use card_dsl::card_data::CardKind;
use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{self, PlaySource, TurnAction};
use game_core::engine::{self, EngineOutcome, OptionTarget};
use game_core::state::{
    AbilityAddress, AbilitySource, Agenda, CardCode, CardInstanceId, Enemy, GameState,
    GameStateBuilder, InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{self, seat, Either, Seat, SeatLookup as _, TestSession};

const LEO: &str = "01048";
const SKIDS: &str = "01003";
/// Guardian Ally, to fill the Ally slot ahead of Leo.
const BEAT_COP: &str = "01018";
/// *"Place 1 doom on the current agenda."* The next round's encounter draw.
const ANCIENT_EVILS: &str = "01166";
const RISE_OF_THE_GHOULS: &str = "01106";
/// Filler for Upkeep's draw.
const EMERGENCY_CACHE: &str = "01088";

const INV: InvestigatorId = InvestigatorId(1);
const OTHER: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);

const RESOURCE: TurnAction = TurnAction::Resource { investigator: INV };
const PLAY: TurnAction = TurnAction::PlayCard {
    investigator: INV,
    source: PlaySource::Hand(0),
};

/// An investigator at `HERE` with Leo in play.
fn with_leo() -> Seat {
    seat(1).at(HERE).in_play([LEO])
}

/// `investigator`'s open turn at `HERE`.
fn staged(investigator: impl Into<Seat<Either>>) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(investigator)
        .with_location(test_support::test_location(HERE.0, "Here"))
        .open_turn(INV)
}

/// A ready enemy whose attack deals 2 damage and no horror: enough to defeat
/// Leo's 2 health.
fn ghoul() -> Enemy {
    let mut enemy = test_support::test_enemy(100, "Ghoul");
    enemy.attack_damage = 2;
    enemy.attack_horror = 0;
    enemy
}

/// The actions they can still take this turn: standard plus additional.
fn actions(state: &GameState) -> u8 {
    engine::actions_available(state, TEST_REGISTRIES.cards, INV)
}

fn standard_actions(state: &GameState) -> u8 {
    state.investigators[&INV].actions_remaining
}

fn menu(session: &TestSession) -> Vec<TurnAction> {
    enumerate::legal_actions(session.state(), TEST_REGISTRIES.cards)
}

fn leo_in_play(state: &GameState) -> bool {
    state.investigators[&INV]
        .cards_in_play
        .iter()
        .any(|card| card.code.as_str() == LEO)
}

/// Assign the pending damage to `leo` for as long as he is offered it.
fn soak_onto(mut session: TestSession, leo: CardInstanceId) -> TestSession {
    let leo = OptionTarget::CardInstance(leo);
    while session.offered_at(&leo) > 0 {
        session = session.pick(leo.clone());
    }
    session
}

/// Run `NativeCx::lose_actions` for them, as Blinding Light 01066's *"lose 1
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

/// *"You may take an additional action during your turn."* Four actions, and
/// the menu still offers one once the three standard ones are spent.
#[test]
fn with_leo_in_play_an_investigator_has_four_actions() {
    let session = staged(with_leo()).session();
    assert_eq!(actions(session.state()), 4);

    let session = session.take(&RESOURCE).take(&RESOURCE).take(&RESOURCE);
    assert_eq!(actions(session.state()), 1);
    assert!(menu(&session).contains(&RESOURCE), "a fourth action");

    let session = session.take(&RESOURCE);
    assert_eq!(actions(session.state()), 0);
    assert!(!menu(&session).contains(&RESOURCE), "no fifth");
}

/// With Leo in play and "Skids" O'Toole 01003's grant bought, five actions,
/// and none of them asks which action pool pays for it.
#[test]
fn with_leo_and_skids_grant_an_investigator_has_five_actions() {
    let session = staged(seat(1).as_card(SKIDS).at(HERE).in_play([LEO])).session();
    let skids = session.state().investigators[&INV]
        .investigator_card
        .instance_id;
    let session = session.take(&TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(skids),
        address: AbilityAddress::Printed(0),
    });
    assert_eq!(actions(session.state()), 5);

    let mut session = session;
    for left in (0..5).rev() {
        session = session.take(&RESOURCE);
        session.assert_at_turn_menu(INV);
        assert_eq!(actions(session.state()), left);
    }
}

/// Leo's and "Skids" O'Toole 01003's grant are both unrestricted, so the
/// first action qualifies as either and nothing is asked. Leo's is used
/// first, since only his can leave play: defeated after that action, he takes
/// nothing with him, and the three standard actions and Skids' grant are left.
#[test]
fn with_leo_and_skids_grant_leos_is_used_first_without_asking() {
    let session = staged(seat(1).as_card(SKIDS).at(HERE).in_play([LEO])).session();
    let skids = session.state().investigators[&INV]
        .investigator_card
        .instance_id;
    let leo = session.state().in_play(INV, LEO);
    let session = session
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(skids),
            address: AbilityAddress::Printed(0),
        })
        .take(&RESOURCE);
    session.assert_at_turn_menu(INV);
    assert_eq!(standard_actions(session.state()), 3);

    let session = soak_onto(session.take_damage(INV, 2), leo);
    assert!(!leo_in_play(session.state()));
    assert_eq!(
        actions(session.state()),
        4,
        "three standard and Skids' grant"
    );
}

/// The ruling: *"If Leo De Luca is discarded mid-turn, you do not lose 1
/// action from your total pool of actions for the turn."* His action was the
/// first one spent; the attack of opportunity it provoked defeats him, and the
/// three standard actions are still there.
#[test]
fn leo_defeated_after_the_first_action_costs_no_action() {
    let session = staged(with_leo())
        .with_enemy_engaged(ghoul(), INV)
        .session();
    let leo = session.state().in_play(INV, LEO);
    let session = session.take(&RESOURCE);
    let session = soak_onto(session, leo);
    assert!(!leo_in_play(session.state()), "the attack defeated Leo");
    assert_eq!(standard_actions(session.state()), 3);
    assert_eq!(actions(session.state()), 3);
}

/// The grant is read off the card in play: defeated before any action, he
/// grants nothing.
#[test]
fn leo_defeated_before_any_action_takes_his_grant_with_him() {
    let session = staged(with_leo()).session();
    let leo = session.state().in_play(INV, LEO);
    let session = soak_onto(session.take_damage(INV, 2), leo);
    assert!(!leo_in_play(session.state()));
    assert_eq!(actions(session.state()), 3);
}

/// The ruling: *"If you then play a second copy of the card, you gain a
/// further additional action."* The first Leo's action used, the first Leo
/// defeated, the second played: five actions in the turn.
#[test]
fn a_second_leo_played_after_the_first_left_play_grants_a_further_action() {
    let session = staged(with_leo().hand([LEO]).resources(6)).session();
    let first = session.state().in_play(INV, LEO);
    let session = session.take(&RESOURCE);
    assert_eq!(
        standard_actions(session.state()),
        3,
        "Leo's action was used"
    );

    let session = soak_onto(session.take_damage(INV, 2), first);
    assert!(!leo_in_play(session.state()));
    assert_eq!(actions(session.state()), 3);

    let session = session.take(&PLAY);
    assert!(leo_in_play(session.state()), "the second Leo entered play");
    assert_eq!(standard_actions(session.state()), 2);
    assert_eq!(actions(session.state()), 3, "two standard and his");

    let session = session.take(&RESOURCE).take(&RESOURCE).take(&RESOURCE);
    assert_eq!(actions(session.state()), 0, "five actions in all");
}

/// FAQ 1.10 (`glossary/Action.md`): *"the investigator’s three standard actions
/// are the ones that are "lost" first. If an investigator only has additional
/// actions remaining, those are then lost"*.
#[test]
fn losing_actions_takes_the_standard_ones_before_leos() {
    let session = lose(staged(with_leo()).session(), 3);
    assert_eq!(standard_actions(session.state()), 0);
    assert_eq!(actions(session.state()), 1, "Leo's is left");

    let session = lose(session, 1);
    assert_eq!(actions(session.state()), 0);
}

/// *"during your turn"*: during another investigator's turn his controller has
/// no additional action from him.
#[test]
fn leo_grants_nothing_outside_his_controllers_turn() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(with_leo())
        .with_seat(seat(2).at(HERE))
        .with_location(test_support::test_location(HERE.0, "Here"))
        .open_turn(OTHER)
        .session();
    assert_eq!(actions(session.state()), standard_actions(session.state()));
}

/// Once per turn, every turn: used this turn, he grants again on the next.
#[test]
fn leo_grants_again_on_his_controllers_next_turn() {
    let session = staged(with_leo().deck([EMERGENCY_CACHE]))
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
    let session = session.take(&RESOURCE);
    assert_eq!(actions(session.state()), 3, "his action used this turn");

    let session = session.take(&TurnAction::EndTurn);
    session.assert_at_encounter_draw();
    let session = session.confirm();
    assert_eq!(session.state().phase, Phase::Investigation);
    session.assert_at_turn_menu(INV);
    assert_eq!(actions(session.state()), 4, "three standard and Leo's");
}

/// Horror soaks onto Leo too: his 2 sanity takes 2 horror, which defeats him,
/// and none reaches his controller.
#[test]
fn horror_soaks_onto_leo() {
    let session = staged(with_leo()).session();
    let leo = session.state().in_play(INV, LEO);
    let session = soak_onto(session.take_horror(INV, 2), leo);
    assert!(!leo_in_play(session.state()), "2 horror defeated Leo");
    assert_eq!(session.state().investigators[&INV].horror(), 0);
}

/// Leo is an Ally: he takes the Ally slot from the ally there before him. (An
/// attack's damage assigned to his 2 health is the defeat in
/// `leo_defeated_after_the_first_action_costs_no_action`.)
#[test]
fn leo_takes_the_ally_slot() {
    let session = staged(
        seat(1)
            .at(HERE)
            .in_play([BEAT_COP])
            .hand([LEO])
            .resources(6),
    )
    .session()
    .take(&PLAY);
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(
        inv.cards_in_play
            .iter()
            .map(|card| card.code.as_str())
            .collect::<Vec<_>>(),
        vec![LEO],
        "Beat Cop was displaced"
    );
    assert_eq!(inv.discard, vec![CardCode::new(BEAT_COP)]);
}
