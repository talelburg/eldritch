//! The one forced-trigger scan (#965, spec #962): a walk over the whole board
//! at every triggering condition, narrowed only by the card's pattern and the
//! condition's own data, binding each hit to one controller.
//!
//! Every fixture models a primitive (ADR 0016): a forced ability on one
//! pattern, on one kind of source, dealing 1 horror to "you" — so the
//! investigator who takes the horror is the controller the scan bound, and a
//! fixture that fires twice where it should fire once shows up as the lead's
//! ordering prompt (`TestSession::finish` refuses to finish at one).

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use card_dsl::dsl::{
    self, Ability, AttackerScope, EventPattern, EventTiming, InvestigatorTarget, SkillTestKind,
    TargetScope, TestOutcome, TestedLocationScope,
};
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{ApplyResult, Cx, EngineOutcome, OptionId, OptionTarget, TimingEvent};
use game_core::state::{
    Act, Agenda, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, EnemyId, GameState,
    GameStateBuilder, InvestigatorId, LocationId, Status, TokenModifiers,
};
use game_core::test_support::{self, MockRegistry, TestSession};

/// A forced ability on `pattern` in `cell`, dealing 1 horror to the
/// investigator it is bound to.
fn horror_on(pattern: EventPattern, cell: EventTiming) -> Vec<Ability> {
    vec![dsl::forced_on_event(
        pattern,
        cell,
        dsl::deal_horror(InvestigatorTarget::You, 1u8),
    )]
}

fn after_horror(pattern: EventPattern) -> Vec<Ability> {
    horror_on(pattern, EventTiming::After)
}

/// *"Forced - After an enemy is defeated"*, wherever it is printed.
const ON_DEFEAT: &str = "_ts_on_defeat";
/// *"Forced - After you enter this location"*.
const ON_ENTER: &str = "_ts_on_enter";
/// *"Forced - After an investigator leaves attached location"*.
const ON_LEAVE: &str = "_ts_on_leave";
/// *"Forced - After attached location is successfully investigated"*.
const ON_ATTACHED_INVESTIGATED: &str = "_ts_on_attached_investigated";
/// *"Forced - After this enemy attacks"*.
const ON_THIS_ATTACKS: &str = "_ts_on_this_attacks";
/// *"Forced - At the end of your turn"*.
const ON_END_OF_TURN: &str = "_ts_on_end_of_turn";
/// *"Forced - After this agenda advances"*.
const ON_AGENDA_ADVANCED: &str = "_ts_on_agenda_advanced";
/// A weakness printing *"Forced - When the game ends"*. Its effect is a native
/// marking the investigator it is bound to, because the eliminated investigator
/// it fires for can no longer take horror.
const GAME_END_WEAKNESS: &str = "_ts_game_end_weakness";
/// The same ability on a card that is not a weakness.
const GAME_END_NON_WEAKNESS: &str = "_ts_game_end_non_weakness";
/// *"Forced - At the end of the round"*, on whatever card prints it.
const ON_ROUND_END: &str = "_ts_on_round_end";
/// *"[reaction] After an enemy is defeated"*, wherever it is printed: deals 1
/// horror to the investigator who used it, so the horror names who was offered
/// the option they picked.
const REACT_ON_DEFEAT: &str = "_ts_react_on_defeat";
/// A cost-0 Fast event: *"Fast. Play after an enemy is defeated."*, dealing 1
/// horror to the investigator who played it.
const FAST_REACT_ON_DEFEAT: &str = "_ts_fast_react_on_defeat";

fn treachery(code: &'static str, weakness: bool) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: code.to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_test".to_owned(),
        weakness,
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    }
}

fn fast_event(code: &'static str) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: code.to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_test".to_owned(),
        weakness: false,
        kind: CardKind::Event {
            class: Class::Neutral,
            cost: Some(0),
            xp: Some(0),
            skill_icons: SkillIcons::default(),
            is_fast: true,
            deck_limit: 2,
            play_only_during_turn: false,
        },
    }
}

fn react_on_defeat() -> Vec<Ability> {
    vec![dsl::reaction_on_event(
        EventPattern::EnemyDefeated {
            by_controller: false,
            code: None,
        },
        EventTiming::After,
        dsl::deal_horror(InvestigatorTarget::You, 1u8),
    )]
}

const MARK: &str = "test:mark-the-bound-investigator";

/// Gives the investigator the ability is bound to 1 resource.
fn mark(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    if let Some(inv) = cx.state.investigators.get_mut(&ctx.controller) {
        inv.resources += 1;
    }
    EngineOutcome::Done
}

fn game_end_mark() -> Vec<Ability> {
    vec![dsl::forced_on_event(
        EventPattern::GameEnd,
        EventTiming::When,
        dsl::native(MARK),
    )]
}

#[ctor::ctor(unsafe)]
fn install_mock_registry() {
    MockRegistry::new()
        .with_abilities(ON_DEFEAT, || {
            after_horror(EventPattern::EnemyDefeated {
                by_controller: false,
                code: None,
            })
        })
        .with_abilities(ON_ENTER, || after_horror(EventPattern::EnteredLocation))
        .with_abilities(ON_LEAVE, || after_horror(EventPattern::LeftLocation))
        .with_abilities(ON_ATTACHED_INVESTIGATED, || {
            after_horror(EventPattern::SkillTestResolved {
                outcome: TestOutcome::Success,
                kind: Some(SkillTestKind::Investigate),
                by_controller: false,
                tested_location: TestedLocationScope::Attached,
            })
        })
        .with_abilities(ON_THIS_ATTACKS, || {
            after_horror(EventPattern::EnemyAttacks {
                attacker: AttackerScope::This,
                target: TargetScope::Any,
            })
        })
        .with_abilities(ON_END_OF_TURN, || {
            horror_on(EventPattern::EndOfTurn, EventTiming::At)
        })
        .with_abilities(ON_AGENDA_ADVANCED, || {
            after_horror(EventPattern::AgendaAdvanced)
        })
        .with_abilities(GAME_END_WEAKNESS, game_end_mark)
        .with_card(treachery(GAME_END_WEAKNESS, true))
        .with_abilities(GAME_END_NON_WEAKNESS, game_end_mark)
        .with_native_effect(MARK, mark)
        .with_card(treachery(GAME_END_NON_WEAKNESS, false))
        .with_abilities(ON_ROUND_END, || {
            horror_on(EventPattern::RoundEnded, EventTiming::At)
        })
        .with_abilities(REACT_ON_DEFEAT, react_on_defeat)
        .with_abilities(FAST_REACT_ON_DEFEAT, react_on_defeat)
        .with_card(fast_event(FAST_REACT_ON_DEFEAT))
        .install();
}

fn horror(state: &GameState, id: u32) -> u8 {
    state.investigators[&InvestigatorId(id)].horror()
}

/// Investigators 1 and 2 at location 10, with locations 10 and 11 on the map
/// and `turn_order` `[1, 2]` — so the lead proxy is investigator 1.
fn table() -> GameState {
    let at_10 = |id| {
        let mut inv = test_support::test_investigator(id);
        inv.current_location = Some(LocationId(10));
        inv
    };
    GameStateBuilder::new()
        .with_investigator(at_10(1))
        .with_investigator(at_10(2))
        .with_location(test_support::test_location(10, "Study"))
        .with_location(test_support::test_location(11, "Hallway"))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .build()
}

fn instance(code: &str, id: u32) -> CardInPlay {
    CardInPlay::enter_play(CardCode::new(code), CardInstanceId(id))
}

fn print_on_location(state: &mut GameState, location: u32, code: &str) {
    state.locations.get_mut(&LocationId(location)).unwrap().code = CardCode::new(code);
}

fn defeat(by: Option<u32>) -> TimingEvent {
    TimingEvent::EnemyDefeated {
        enemy: EnemyId(9),
        by: by.map(InvestigatorId),
        code: CardCode::new("test-defeated-enemy"),
    }
}

/// Fire `event` and drain it. Panics at an outstanding prompt, so a fixture
/// that fired twice where it should fire once — the lead's ordering prompt —
/// fails here.
fn fire(state: GameState, event: TimingEvent) -> ApplyResult {
    let result = TestSession::new(state).fire_at(event).finish();
    assert_eq!(result.outcome, EngineOutcome::Done);
    result
}

// ---------------------------------------------------------------------------
// #698: a forced ability on an enemy defeat fires from any card in play, and
// the forced-binding rule names who "you" is on each kind of source.
// ---------------------------------------------------------------------------

#[test]
fn an_agendas_forced_ability_fires_on_an_enemy_defeat_bound_to_the_lead_proxy() {
    let mut state = table();
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(ON_DEFEAT),
        doom_threshold: 10,
    }];
    let result = fire(state, defeat(Some(2)));
    assert_eq!(horror(&result.state, 1), 1, "the lead proxy takes it");
    assert_eq!(horror(&result.state, 2), 0);
}

#[test]
fn an_in_play_assets_forced_ability_fires_on_an_enemy_defeat_bound_to_its_controller() {
    let mut state = table();
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .unwrap()
        .cards_in_play
        .push(instance(ON_DEFEAT, 50));
    let result = fire(state, defeat(Some(1)));
    assert_eq!(horror(&result.state, 2), 1, "its controller takes it");
    assert_eq!(horror(&result.state, 1), 0);
}

#[test]
fn a_locations_forced_ability_fires_on_an_enemy_defeat_bound_to_the_defeating_investigator() {
    let mut state = table();
    print_on_location(&mut state, 11, ON_DEFEAT);
    let result = fire(state, defeat(Some(2)));
    assert_eq!(horror(&result.state, 2), 1, "the subject, `by`, takes it");
    assert_eq!(horror(&result.state, 1), 0);
}

#[test]
fn an_uncontrolled_cards_forced_ability_with_no_subject_binds_to_the_lead_proxy() {
    let mut state = table();
    print_on_location(&mut state, 11, ON_DEFEAT);
    let result = fire(state, defeat(None));
    assert_eq!(
        horror(&result.state, 1),
        1,
        "an uncredited defeat has no subject, so the lead proxy takes it"
    );
    assert_eq!(horror(&result.state, 2), 0);
}

#[test]
fn the_lead_proxy_is_the_first_active_seat() {
    let mut state = table();
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(ON_DEFEAT),
        doom_threshold: 10,
    }];
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .status = Status::Defeated;
    let result = fire(state, defeat(None));
    assert_eq!(horror(&result.state, 2), 1);
}

// ---------------------------------------------------------------------------
// Each condition's narrowing holds under the whole-board walk: the fixture
// fires for its own location, enemy, attachment or investigator and not for
// another one on the board.
// ---------------------------------------------------------------------------

#[test]
fn entering_a_location_fires_only_that_locations_ability() {
    let mut state = table();
    print_on_location(&mut state, 10, ON_ENTER);
    print_on_location(&mut state, 11, ON_ENTER);
    let result = fire(
        state,
        TimingEvent::EnteredLocation {
            investigator: InvestigatorId(2),
            location: LocationId(11),
        },
    );
    assert_eq!(horror(&result.state, 2), 1, "the entering investigator");
    assert_eq!(horror(&result.state, 1), 0);
}

#[test]
fn leaving_a_location_fires_only_its_attachments_ability() {
    let mut state = table();
    for (loc, inst) in [(10, 60), (11, 61)] {
        state
            .locations
            .get_mut(&LocationId(loc))
            .unwrap()
            .attachments
            .push(instance(ON_LEAVE, inst));
    }
    let result = fire(
        state,
        TimingEvent::LeftLocation {
            investigator: InvestigatorId(2),
            location: LocationId(10),
            destination: LocationId(11),
        },
    );
    assert_eq!(horror(&result.state, 2), 1, "the leaving investigator");
    assert_eq!(horror(&result.state, 1), 0);
}

#[test]
fn investigating_a_location_fires_only_its_attachments_ability() {
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.skills.intellect = 3;
    inv.actions_remaining = 1;
    let mut studied = test_support::test_location(10, "Study");
    studied.shroud = 0;
    studied.clues = 1;
    studied
        .attachments
        .push(instance(ON_ATTACHED_INVESTIGATED, 70));
    let mut elsewhere = test_support::test_location(11, "Hallway");
    elsewhere
        .attachments
        .push(instance(ON_ATTACHED_INVESTIGATED, 71));
    let state = GameStateBuilder::new()
        .open_turn(InvestigatorId(1))
        .with_investigator(inv)
        .with_location(studied)
        .with_location(elsewhere)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    let idx = enumerate::legal_actions(&state)
        .iter()
        .position(|a| {
            a == &TurnAction::Investigate {
                investigator: InvestigatorId(1),
            }
        })
        .expect("Investigate is on the turn menu");
    let result = test_support::apply_no_commits(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        }),
    );
    assert_eq!(
        horror(&result.state, 1),
        1,
        "only the investigated location's attachment fired — two would have \
         opened the lead's ordering prompt instead"
    );
}

#[test]
fn an_enemy_attacking_fires_only_its_own_ability() {
    let mut state = table();
    for id in [1, 2] {
        let mut enemy = test_support::test_enemy(id, "Acolyte");
        enemy.code = CardCode::new(ON_THIS_ATTACKS);
        enemy.current_location = Some(LocationId(10));
        enemy.attack_damage = 0;
        state.enemies.insert(EnemyId(id), enemy);
    }
    let result = fire(
        state,
        TimingEvent::EnemyAttacks {
            enemy: EnemyId(2),
            investigator: InvestigatorId(2),
        },
    );
    assert_eq!(horror(&result.state, 2), 1, "the attacked investigator");
    assert_eq!(horror(&result.state, 1), 0);
}

#[test]
fn the_end_of_a_turn_fires_only_the_ending_investigators_card() {
    let mut state = table();
    for id in [1, 2] {
        state
            .investigators
            .get_mut(&InvestigatorId(id))
            .unwrap()
            .threat_area
            .push(instance(ON_END_OF_TURN, 80 + id));
    }
    let result = fire(
        state,
        TimingEvent::EndOfTurn {
            investigator: InvestigatorId(2),
        },
    );
    assert_eq!(horror(&result.state, 2), 1);
    assert_eq!(horror(&result.state, 1), 0);
}

#[test]
fn an_advance_fires_only_the_advancing_cards_ability() {
    let mut state = table();
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(ON_AGENDA_ADVANCED),
        doom_threshold: 10,
    }];
    // The same pattern on a location never hears an agenda advance: it is not
    // the agenda the ability is printed on.
    print_on_location(&mut state, 11, ON_AGENDA_ADVANCED);
    let other = fire(
        state.clone(),
        TimingEvent::AgendaAdvanced {
            code: CardCode::new("another-agenda"),
        },
    );
    assert_eq!(
        horror(&other.state, 1),
        0,
        "another agenda advancing does not fire this one's reverse"
    );
    let own = fire(
        state,
        TimingEvent::AgendaAdvanced {
            code: CardCode::new(ON_AGENDA_ADVANCED),
        },
    );
    assert_eq!(horror(&own.state, 1), 1, "the lead proxy takes it");
}

#[test]
fn elimination_fires_only_the_eliminated_investigators_weaknesses() {
    let mut state = table();
    for id in [1, 2] {
        state
            .investigators
            .get_mut(&InvestigatorId(id))
            .unwrap()
            .threat_area
            .push(instance(GAME_END_WEAKNESS, 90 + id));
    }
    // Neither of these is "a weakness the eliminated investigator owns": a
    // non-weakness in their own play area, and the weakness's ability printed
    // on a location they stand in.
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .unwrap()
        .cards_in_play
        .push(instance(GAME_END_NON_WEAKNESS, 95));
    print_on_location(&mut state, 10, GAME_END_WEAKNESS);
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .unwrap()
        .status = Status::Defeated;
    let before = state.clone();
    let result = fire(
        state,
        TimingEvent::EliminationGameEnd {
            investigator: InvestigatorId(2),
        },
    );
    let gained = |id| {
        result.state.investigators[&InvestigatorId(id)].resources
            - before.investigators[&InvestigatorId(id)].resources
    };
    assert_eq!(gained(2), 1, "the eliminated investigator's own weakness");
    assert_eq!(gained(1), 0);
}

#[test]
fn an_eliminated_investigators_cards_are_not_walked() {
    let mut state = table();
    for id in [1, 2] {
        state
            .investigators
            .get_mut(&InvestigatorId(id))
            .unwrap()
            .threat_area
            .push(instance(ON_ROUND_END, 100 + id));
    }
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .unwrap()
        .status = Status::Defeated;
    let result = fire(state, TimingEvent::RoundEnded);
    assert_eq!(horror(&result.state, 1), 1);
    assert_eq!(horror(&result.state, 2), 0);
}

// ---------------------------------------------------------------------------
// The walk reaches every kind of source, in one fixed order.
// ---------------------------------------------------------------------------

#[test]
fn a_board_wide_condition_reaches_every_kind_of_source_in_walk_order() {
    let mut state = table();
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .unwrap()
        .threat_area
        .push(instance(ON_ROUND_END, 110));
    print_on_location(&mut state, 10, ON_ROUND_END);
    let mut enemy = test_support::test_enemy(3, "Ghoul");
    enemy.attachments.push(instance(ON_ROUND_END, 111));
    state.enemies.insert(EnemyId(3), enemy);
    state.act_deck = vec![Act {
        code: CardCode::new(ON_ROUND_END),
        clue_threshold: 10,
    }];
    let session = TestSession::new(state).fire_at(TimingEvent::RoundEnded);
    let offered: Vec<_> = session
        .prompt()
        .options
        .iter()
        .map(|o| o.target.clone())
        .collect();
    assert_eq!(
        offered,
        vec![
            Some(OptionTarget::CardInstance(CardInstanceId(110))),
            Some(OptionTarget::Location(LocationId(10))),
            Some(OptionTarget::CardInstance(CardInstanceId(111))),
            Some(OptionTarget::Act),
        ],
        "investigators, then locations, then enemies' attachments, then the act"
    );
}

// ---------------------------------------------------------------------------
// #966: a reaction is offered to each investigator who can reach its source
// (ADR 0010), and to nobody else.
// ---------------------------------------------------------------------------

/// The anchors of the options the reaction window offers after `event`.
fn offered(state: GameState, event: TimingEvent) -> Vec<Option<OptionTarget>> {
    TestSession::new(state)
        .fire_at(event)
        .prompt()
        .options
        .iter()
        .map(|o| o.target.clone())
        .collect()
}

/// Fire `event`, pick the `n`th reaction option, and return the state.
fn react_with(state: GameState, event: TimingEvent, n: u32) -> GameState {
    TestSession::new(state)
        .fire_at(event)
        .apply(Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(n)),
        }))
        .state()
        .clone()
}

#[test]
fn a_reaction_on_a_co_located_location_is_offered_to_each_investigator_there() {
    let mut state = table();
    print_on_location(&mut state, 10, REACT_ON_DEFEAT);
    assert_eq!(
        offered(state.clone(), defeat(None)),
        vec![
            Some(OptionTarget::Location(LocationId(10))),
            Some(OptionTarget::Location(LocationId(10))),
        ],
        "both investigators stand at the Study"
    );
    let first = react_with(state.clone(), defeat(None), 0);
    assert_eq!((horror(&first, 1), horror(&first, 2)), (1, 0));
    let second = react_with(state, defeat(None), 1);
    assert_eq!((horror(&second, 1), horror(&second, 2)), (0, 1));
}

#[test]
fn a_reaction_on_a_location_nobody_stands_at_is_not_offered() {
    let mut state = table();
    print_on_location(&mut state, 11, REACT_ON_DEFEAT);
    let result = fire(state, defeat(None));
    assert_eq!((horror(&result.state, 1), horror(&result.state, 2)), (0, 0));
}

#[test]
fn a_reaction_on_another_investigators_asset_is_offered_only_to_its_controller() {
    let mut state = table();
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .unwrap()
        .cards_in_play
        .push(instance(REACT_ON_DEFEAT, 80));
    assert_eq!(
        offered(state.clone(), defeat(None)),
        vec![Some(OptionTarget::CardInstance(CardInstanceId(80)))],
        "investigator 1 stands beside the asset but does not control it"
    );
    let after = react_with(state, defeat(None), 0);
    assert_eq!((horror(&after, 1), horror(&after, 2)), (0, 1));
}

#[test]
fn a_reaction_on_a_co_located_enemy_or_threat_area_card_is_offered_to_each_investigator_there() {
    let mut state = table();
    let mut enemy = test_support::test_enemy(3, "Ghoul");
    enemy.code = CardCode::new(REACT_ON_DEFEAT);
    enemy.current_location = Some(LocationId(10));
    state.enemies.insert(EnemyId(3), enemy);
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .unwrap()
        .threat_area
        .push(instance(REACT_ON_DEFEAT, 90));
    assert_eq!(
        offered(state, defeat(None)),
        vec![
            Some(OptionTarget::CardInstance(CardInstanceId(90))),
            Some(OptionTarget::CardInstance(CardInstanceId(90))),
            Some(OptionTarget::Enemy(EnemyId(3))),
            Some(OptionTarget::Enemy(EnemyId(3))),
        ],
    );
}

#[test]
fn a_reaction_on_an_enemy_elsewhere_is_not_offered() {
    let mut state = table();
    let mut enemy = test_support::test_enemy(3, "Ghoul");
    enemy.code = CardCode::new(REACT_ON_DEFEAT);
    enemy.current_location = Some(LocationId(11));
    state.enemies.insert(EnemyId(3), enemy);
    let result = fire(state, defeat(None));
    assert_eq!((horror(&result.state, 1), horror(&result.state, 2)), (0, 0));
}

#[test]
fn a_reaction_on_the_agenda_is_offered_once_bound_to_the_lead_proxy() {
    let mut state = table();
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(REACT_ON_DEFEAT),
        doom_threshold: 10,
    }];
    assert_eq!(
        offered(state.clone(), defeat(None)),
        vec![Some(OptionTarget::Agenda)]
    );
    let after = react_with(state, defeat(None), 0);
    assert_eq!((horror(&after, 1), horror(&after, 2)), (1, 0));
}

#[test]
fn reaction_options_are_grouped_by_investigator_then_board_then_act() {
    let mut state = table();
    for (id, inst) in [(2, 81), (1, 80)] {
        state
            .investigators
            .get_mut(&InvestigatorId(id))
            .unwrap()
            .cards_in_play
            .push(instance(REACT_ON_DEFEAT, inst));
    }
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .hand
        .push(CardCode::new(FAST_REACT_ON_DEFEAT));
    print_on_location(&mut state, 10, REACT_ON_DEFEAT);
    state.act_deck = vec![Act {
        code: CardCode::new(REACT_ON_DEFEAT),
        clue_threshold: 10,
    }];
    assert_eq!(
        offered(state, defeat(None)),
        vec![
            Some(OptionTarget::CardInstance(CardInstanceId(80))),
            Some(OptionTarget::HandCardByCode {
                investigator: InvestigatorId(1),
                code: CardCode::new(FAST_REACT_ON_DEFEAT),
            }),
            Some(OptionTarget::CardInstance(CardInstanceId(81))),
            Some(OptionTarget::Location(LocationId(10))),
            Some(OptionTarget::Location(LocationId(10))),
            Some(OptionTarget::Act),
        ],
        "each investigator's cards then hand, then the board, then the act"
    );
}
