//! The one forced-trigger scan: a walk over the whole board
//! at every triggering condition, narrowed only by the card's pattern and the
//! condition's own data, binding each hit to one controller.
//!
//! Every fixture models a primitive (ADR 0016): a forced ability on one
//! pattern, on one kind of source, dealing 1 horror to "you" — so the
//! investigator who takes the horror is the controller the scan bound, and a
//! fixture that fires twice where it should fire once shows up as the lead's
//! ordering prompt where the session should be back at the turn menu.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, WeaknessKind};
use card_dsl::dsl::{
    self, Ability, AttackerScope, EventPattern, EventTiming, InvestigatorTarget, SkillTestKind,
    TargetScope, TestOutcome, TestedLocationScope,
};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionTarget, Registries, TimingEvent};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    Act, Agenda, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, EnemyId, GameState,
    GameStateBuilder, InvestigatorId, Location, LocationId, Owner, Phase, Status, TokenModifiers,
};
use game_core::test_support::{self, seat, MockRegistry, Seat, SeatLookup as _, TestSession};

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
/// *"\[reaction\] After an enemy is defeated"*, wherever it is printed: deals 1
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
        weakness: weakness.then_some(WeaknessKind::Other),
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
        weakness: None,
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

fn game_end_mark() -> Vec<Ability> {
    vec![dsl::forced_on_event(
        EventPattern::GameEnd,
        EventTiming::When,
        dsl::gain_resources(InvestigatorTarget::You, 1),
    )]
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
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
                    margin: None,
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
            .with_card(treachery(ON_END_OF_TURN, false))
            .with_abilities(ON_AGENDA_ADVANCED, || {
                after_horror(EventPattern::AgendaAdvanced)
            })
            .with_abilities(GAME_END_WEAKNESS, game_end_mark)
            .with_card(treachery(GAME_END_WEAKNESS, true))
            .with_abilities(GAME_END_NON_WEAKNESS, game_end_mark)
            .with_card(treachery(GAME_END_NON_WEAKNESS, false))
            .with_abilities(ON_ROUND_END, || {
                horror_on(EventPattern::RoundEnded, EventTiming::At)
            })
            .with_abilities(REACT_ON_DEFEAT, react_on_defeat)
            .with_card(treachery(REACT_ON_DEFEAT, false))
            .with_abilities(FAST_REACT_ON_DEFEAT, react_on_defeat)
            .with_card(fast_event(FAST_REACT_ON_DEFEAT))
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

const ONE: InvestigatorId = InvestigatorId(1);
const TWO: InvestigatorId = InvestigatorId(2);
const STUDY: LocationId = LocationId(10);
const HALLWAY: LocationId = LocationId(11);

/// Location and enemy attachments: a seat cannot place them, so each has one
/// named id.
const STUDY_ATTACHMENT: CardInstanceId = CardInstanceId(60);
const HALLWAY_ATTACHMENT: CardInstanceId = CardInstanceId(61);
const GHOUL_ATTACHMENT: CardInstanceId = CardInstanceId(111);

fn horror(state: &GameState, id: InvestigatorId) -> u8 {
    state.investigators[&id].horror()
}

fn study() -> Location {
    test_support::test_location(STUDY.0, "Study")
}

fn hallway() -> Location {
    test_support::test_location(HALLWAY.0, "Hallway")
}

/// `location`, with `code`'s abilities printed on it.
fn printing(mut location: Location, code: &str) -> Location {
    location.code = CardCode::new(code);
    location
}

/// An encounter card `code` attached to something on the board, as `id`.
fn attachment(code: &str, id: CardInstanceId) -> CardInPlay {
    CardInPlay::enter_play(CardCode::new(code), id, Owner::EncounterDeck)
}

/// `location`, with `code` attached to it as `id`.
fn attaching(mut location: Location, code: &str, id: CardInstanceId) -> Location {
    location.attachments.push(attachment(code, id));
    location
}

/// Investigators `one` and `two` seated at the Study, with `study` and
/// `hallway` on the map and `turn_order` `[1, 2]` — so the lead proxy is
/// investigator 1.
fn board(one: Seat, two: Seat, study: Location, hallway: Location) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(one.at(STUDY))
        .with_seat(two.at(STUDY))
        .with_location(study)
        .with_location(hallway)
        .with_turn_order([ONE, TWO])
}

/// [`board`] with nothing seated or printed.
fn table() -> GameStateBuilder {
    board(seat(ONE.0), seat(TWO.0), study(), hallway())
}

/// Make `code` the current agenda of a built `state`; the agenda deck has no
/// builder setter.
fn with_agenda(mut state: GameState, code: &str) -> GameState {
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(code),
        doom_threshold: 10,
    }];
    state
}

/// Make `code` the current act of a built `state`; the act deck has no builder
/// setter.
fn with_act(mut state: GameState, code: &str) -> GameState {
    state.act_deck = vec![Act {
        code: CardCode::new(code),
        clue_threshold: 10,
    }];
    state
}

/// Mark `id` defeated on a built `state`.
fn defeated(mut state: GameState, id: InvestigatorId) -> GameState {
    state.investigators.get_mut(&id).unwrap().status = Status::Defeated;
    state
}

fn defeat(by: Option<InvestigatorId>) -> TimingEvent {
    TimingEvent::EnemyDefeated {
        enemy: EnemyId(9),
        by,
        code: CardCode::new("test-defeated-enemy"),
    }
}

/// Fire `event` over the open turn and drain it back to the turn menu. Panics
/// at an outstanding prompt, so a fixture that fired twice where it should fire
/// once — the lead's ordering prompt — fails here.
///
/// The condition is fired on its own rather than reached by play: the subject
/// is how the scan narrows on the condition's own data, and the fixtures name
/// subjects no basic action produces on demand — an uncredited defeat, a defeat
/// or a move by the investigator whose turn it is not, another agenda's
/// advance.
fn fire(state: GameState, event: TimingEvent) -> TestSession {
    let active = state.active_investigator.expect("an open turn");
    let session = TestSession::new(state, registries()).fire_at(event);
    session.assert_at_turn_menu(active);
    session
}

/// End the round for real — the end of the Upkeep phase, staged with
/// [`ending_upkeep_phase`](GameStateBuilder::ending_upkeep_phase) — so the
/// `RoundEnded` scan runs. Play goes on into the next round's Mythos phase.
fn end_the_round(state: GameState) -> TestSession {
    let session = TestSession::new(state, registries());
    assert_event!(
        session.events(),
        Event::PhaseEnded {
            phase: Phase::Upkeep
        }
    );
    session
}

// ---------------------------------------------------------------------------
// A forced ability on an enemy defeat fires from any card in play, and the
// forced-binding rule names who "you" is on each kind of source.
// ---------------------------------------------------------------------------

#[test]
fn an_agendas_forced_ability_fires_on_an_enemy_defeat_bound_to_the_lead_proxy() {
    let state = with_agenda(table().open_turn(ONE).build(), ON_DEFEAT);
    let session = fire(state, defeat(Some(TWO)));
    assert_eq!(horror(session.state(), ONE), 1, "the lead proxy takes it");
    assert_eq!(horror(session.state(), TWO), 0);
}

/// Regression for #698: an enemy defeat's forced scan reaches cards in play,
/// not only the current act.
#[test]
fn an_in_play_assets_forced_ability_fires_on_an_enemy_defeat_bound_to_its_controller() {
    let state = board(
        seat(ONE.0),
        seat(TWO.0).in_play([ON_DEFEAT]),
        study(),
        hallway(),
    )
    .open_turn(ONE)
    .build();
    let session = fire(state, defeat(Some(ONE)));
    assert_eq!(horror(session.state(), TWO), 1, "its controller takes it");
    assert_eq!(horror(session.state(), ONE), 0);
}

#[test]
fn a_locations_forced_ability_fires_on_an_enemy_defeat_bound_to_the_defeating_investigator() {
    let state = board(
        seat(ONE.0),
        seat(TWO.0),
        study(),
        printing(hallway(), ON_DEFEAT),
    )
    .open_turn(ONE)
    .build();
    let session = fire(state, defeat(Some(TWO)));
    assert_eq!(
        horror(session.state(), TWO),
        1,
        "the subject, `by`, takes it"
    );
    assert_eq!(horror(session.state(), ONE), 0);
}

#[test]
fn an_uncontrolled_cards_forced_ability_with_no_subject_binds_to_the_lead_proxy() {
    let state = board(
        seat(ONE.0),
        seat(TWO.0),
        study(),
        printing(hallway(), ON_DEFEAT),
    )
    .open_turn(ONE)
    .build();
    let session = fire(state, defeat(None));
    assert_eq!(
        horror(session.state(), ONE),
        1,
        "an uncredited defeat has no subject, so the lead proxy takes it"
    );
    assert_eq!(horror(session.state(), TWO), 0);
}

#[test]
fn the_lead_proxy_is_the_first_active_seat() {
    // Investigator 1 is defeated, so the open turn is investigator 2's.
    let state = defeated(with_agenda(table().open_turn(TWO).build(), ON_DEFEAT), ONE);
    let session = fire(state, defeat(None));
    assert_eq!(horror(session.state(), TWO), 1);
}

// ---------------------------------------------------------------------------
// Each condition's narrowing holds under the whole-board walk: the fixture
// fires for its own location, enemy, attachment or investigator and not for
// another one on the board.
// ---------------------------------------------------------------------------

#[test]
fn entering_a_location_fires_only_that_locations_ability() {
    let state = board(
        seat(ONE.0),
        seat(TWO.0),
        printing(study(), ON_ENTER),
        printing(hallway(), ON_ENTER),
    )
    .open_turn(ONE)
    .build();
    let session = fire(
        state,
        TimingEvent::EnteredLocation {
            investigator: TWO,
            location: HALLWAY,
        },
    );
    assert_eq!(horror(session.state(), TWO), 1, "the entering investigator");
    assert_eq!(horror(session.state(), ONE), 0);
}

#[test]
fn leaving_a_location_fires_only_its_attachments_ability() {
    let state = board(
        seat(ONE.0),
        seat(TWO.0),
        attaching(study(), ON_LEAVE, STUDY_ATTACHMENT),
        attaching(hallway(), ON_LEAVE, HALLWAY_ATTACHMENT),
    )
    .open_turn(ONE)
    .build();
    let session = fire(
        state,
        TimingEvent::LeftLocation {
            investigator: TWO,
            location: STUDY,
            destination: HALLWAY,
        },
    );
    assert_eq!(horror(session.state(), TWO), 1, "the leaving investigator");
    assert_eq!(horror(session.state(), ONE), 0);
}

#[test]
fn investigating_a_location_fires_only_its_attachments_ability() {
    let mut studied = attaching(study(), ON_ATTACHED_INVESTIGATED, STUDY_ATTACHMENT);
    studied.shroud = 0;
    studied.clues = 1;
    let session = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(ONE.0).at(STUDY).actions(1))
        .with_location(studied)
        .with_location(attaching(
            hallway(),
            ON_ATTACHED_INVESTIGATED,
            HALLWAY_ATTACHMENT,
        ))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(ONE)
        .session()
        .take(&TurnAction::Investigate { investigator: ONE })
        .commit(&[]);
    assert_eq!(
        horror(session.state(), ONE),
        1,
        "only the investigated location's attachment fired — two would have \
         opened the lead's ordering prompt instead"
    );
}

#[test]
fn an_enemy_attacking_fires_only_its_own_ability() {
    let acolyte = |id| {
        let mut enemy = test_support::test_enemy(id, "Acolyte");
        enemy.code = CardCode::new(ON_THIS_ATTACKS);
        enemy.current_location = Some(STUDY);
        enemy.attack_damage = 0;
        enemy
    };
    let state = table()
        .with_enemy(acolyte(1))
        .with_enemy(acolyte(2))
        .open_turn(ONE)
        .build();
    let session = fire(
        state,
        TimingEvent::EnemyAttacks {
            enemy: EnemyId(2),
            investigator: TWO,
        },
    );
    assert_eq!(horror(session.state(), TWO), 1, "the attacked investigator");
    assert_eq!(horror(session.state(), ONE), 0);
}

#[test]
fn the_end_of_a_turn_fires_only_the_ending_investigators_card() {
    let state = board(
        seat(ONE.0).threat_area([ON_END_OF_TURN]),
        seat(TWO.0).threat_area([ON_END_OF_TURN]),
        study(),
        hallway(),
    )
    .open_turn(ONE)
    .build();
    let session = fire(state, TimingEvent::EndOfTurn { investigator: TWO });
    assert_eq!(horror(session.state(), TWO), 1);
    assert_eq!(horror(session.state(), ONE), 0);
}

#[test]
fn an_advance_fires_only_the_advancing_cards_ability() {
    // The same pattern on a location never hears an agenda advance: it is not
    // the agenda the ability is printed on.
    let state = with_agenda(
        board(
            seat(ONE.0),
            seat(TWO.0),
            study(),
            printing(hallway(), ON_AGENDA_ADVANCED),
        )
        .open_turn(ONE)
        .build(),
        ON_AGENDA_ADVANCED,
    );
    let other = fire(
        state.clone(),
        TimingEvent::AgendaAdvanced {
            code: CardCode::new("another-agenda"),
        },
    );
    assert_eq!(
        horror(other.state(), ONE),
        0,
        "another agenda advancing does not fire this one's reverse"
    );
    let own = fire(
        state,
        TimingEvent::AgendaAdvanced {
            code: CardCode::new(ON_AGENDA_ADVANCED),
        },
    );
    assert_eq!(horror(own.state(), ONE), 1, "the lead proxy takes it");
}

#[test]
fn elimination_fires_only_the_eliminated_investigators_weaknesses() {
    // Neither of investigator 2's other game-end cards is "a weakness the
    // eliminated investigator owns": a non-weakness in their own play area,
    // and the weakness's ability printed on a location they stand in. The
    // weaknesses are owned by their holders (the registry prints them as
    // weaknesses).
    let state = defeated(
        board(
            seat(ONE.0).threat_area([GAME_END_WEAKNESS]),
            seat(TWO.0)
                .threat_area([GAME_END_WEAKNESS])
                .in_play([GAME_END_NON_WEAKNESS]),
            printing(study(), GAME_END_WEAKNESS),
            hallway(),
        )
        .open_turn(ONE)
        .build(),
        TWO,
    );
    let before = state.clone();
    // Fired on its own over a defeated seat: a real elimination goes on to
    // take the eliminated investigator's resources, which the marker counts.
    let session = fire(state, TimingEvent::EliminationGameEnd { investigator: TWO });
    let gained =
        |id| session.state().investigators[&id].resources - before.investigators[&id].resources;
    assert_eq!(gained(TWO), 1, "the eliminated investigator's own weakness");
    assert_eq!(gained(ONE), 0);
}

#[test]
fn an_eliminated_investigators_cards_are_not_walked() {
    let state = defeated(
        board(
            seat(ONE.0).threat_area([ON_ROUND_END]),
            seat(TWO.0).threat_area([ON_ROUND_END]),
            study(),
            hallway(),
        )
        .ending_upkeep_phase()
        .build(),
        TWO,
    );
    let session = end_the_round(state);
    assert_eq!(horror(session.state(), ONE), 1);
    assert_eq!(horror(session.state(), TWO), 0);
}

// ---------------------------------------------------------------------------
// The walk reaches every kind of source, in one fixed order.
// ---------------------------------------------------------------------------

#[test]
fn a_board_wide_condition_reaches_every_kind_of_source_in_walk_order() {
    let mut ghoul = test_support::test_enemy(3, "Ghoul");
    ghoul
        .attachments
        .push(attachment(ON_ROUND_END, GHOUL_ATTACHMENT));
    let state = with_act(
        board(
            seat(ONE.0),
            seat(TWO.0).threat_area([ON_ROUND_END]),
            printing(study(), ON_ROUND_END),
            hallway(),
        )
        .with_enemy(ghoul)
        .ending_upkeep_phase()
        .build(),
        ON_ROUND_END,
    );
    let threat = state.in_play(TWO, ON_ROUND_END);
    let session = end_the_round(state);
    let offered: Vec<_> = session
        .prompt()
        .options
        .iter()
        .map(|o| o.target.clone())
        .collect();
    assert_eq!(
        offered,
        vec![
            Some(OptionTarget::CardInstance(threat)),
            Some(OptionTarget::Location(STUDY)),
            Some(OptionTarget::CardInstance(GHOUL_ATTACHMENT)),
            Some(OptionTarget::Act),
        ],
        "investigators, then locations, then enemies' attachments, then the act"
    );
}

// ---------------------------------------------------------------------------
// A reaction is offered to each investigator who can reach its source
// (ADR 0010), and to nobody else.
// ---------------------------------------------------------------------------

/// The window `event` opens over the open turn. Fired on its own, as
/// [`fire`] says why.
fn window(state: GameState, event: TimingEvent) -> TestSession {
    TestSession::new(state, registries()).fire_at(event)
}

/// The anchors of the options the reaction window offers after `event`.
fn offered(state: GameState, event: TimingEvent) -> Vec<Option<OptionTarget>> {
    window(state, event)
        .prompt()
        .options
        .iter()
        .map(|o| o.target.clone())
        .collect()
}

/// Fire `event`, pick the reaction anchored to `target`, and return the state.
fn react_with(state: GameState, event: TimingEvent, target: OptionTarget) -> GameState {
    window(state, event).pick(target).state().clone()
}

#[test]
fn a_reaction_on_a_co_located_location_is_offered_to_each_investigator_there() {
    let state = board(
        seat(ONE.0),
        seat(TWO.0),
        printing(study(), REACT_ON_DEFEAT),
        hallway(),
    )
    .open_turn(ONE)
    .build();
    assert_eq!(
        offered(state.clone(), defeat(None)),
        vec![
            Some(OptionTarget::Location(STUDY)),
            Some(OptionTarget::Location(STUDY)),
        ],
        "both investigators stand at the Study"
    );
    // The two options share the Study's anchor and label; nothing in them names
    // the investigator each is offered to. So they are picked by position, and
    // the horror assertions pin whose option each position is.
    // TODO(#1024): name the investigator an option is offered to.
    let react_nth = |n| {
        window(state.clone(), defeat(None))
            .pick_nth_at(OptionTarget::Location(STUDY), n)
            .state()
            .clone()
    };
    let first = react_nth(0);
    assert_eq!((horror(&first, ONE), horror(&first, TWO)), (1, 0));
    let second = react_nth(1);
    assert_eq!((horror(&second, ONE), horror(&second, TWO)), (0, 1));
}

#[test]
fn a_reaction_on_a_location_nobody_stands_at_is_not_offered() {
    let state = board(
        seat(ONE.0),
        seat(TWO.0),
        study(),
        printing(hallway(), REACT_ON_DEFEAT),
    )
    .open_turn(ONE)
    .build();
    let session = fire(state, defeat(None));
    assert_eq!(
        (horror(session.state(), ONE), horror(session.state(), TWO)),
        (0, 0)
    );
}

#[test]
fn a_reaction_on_another_investigators_asset_is_offered_only_to_its_controller() {
    let state = board(
        seat(ONE.0),
        seat(TWO.0).in_play([REACT_ON_DEFEAT]),
        study(),
        hallway(),
    )
    .open_turn(ONE)
    .build();
    let asset = state.card(TWO, REACT_ON_DEFEAT);
    assert_eq!(
        offered(state.clone(), defeat(None)),
        vec![Some(asset.clone())],
        "investigator 1 stands beside the asset but does not control it"
    );
    let after = react_with(state, defeat(None), asset);
    assert_eq!((horror(&after, ONE), horror(&after, TWO)), (0, 1));
}

#[test]
fn a_reaction_on_a_co_located_enemy_or_threat_area_card_is_offered_to_each_investigator_there() {
    let mut ghoul = test_support::test_enemy(3, "Ghoul");
    ghoul.code = CardCode::new(REACT_ON_DEFEAT);
    ghoul.current_location = Some(STUDY);
    let state = board(
        seat(ONE.0),
        seat(TWO.0).threat_area([REACT_ON_DEFEAT]),
        study(),
        hallway(),
    )
    .with_enemy(ghoul)
    .open_turn(ONE)
    .build();
    let threat = state.card(TWO, REACT_ON_DEFEAT);
    assert_eq!(
        offered(state, defeat(None)),
        vec![
            Some(threat.clone()),
            Some(threat),
            Some(OptionTarget::Enemy(EnemyId(3))),
            Some(OptionTarget::Enemy(EnemyId(3))),
        ],
    );
}

#[test]
fn a_reaction_on_an_enemy_elsewhere_is_not_offered() {
    let mut ghoul = test_support::test_enemy(3, "Ghoul");
    ghoul.code = CardCode::new(REACT_ON_DEFEAT);
    ghoul.current_location = Some(HALLWAY);
    let state = table().with_enemy(ghoul).open_turn(ONE).build();
    let session = fire(state, defeat(None));
    assert_eq!(
        (horror(session.state(), ONE), horror(session.state(), TWO)),
        (0, 0)
    );
}

#[test]
fn a_reaction_on_the_agenda_is_offered_once_bound_to_the_lead_proxy() {
    let state = with_agenda(table().open_turn(ONE).build(), REACT_ON_DEFEAT);
    assert_eq!(
        offered(state.clone(), defeat(None)),
        vec![Some(OptionTarget::Agenda)]
    );
    let after = react_with(state, defeat(None), OptionTarget::Agenda);
    assert_eq!((horror(&after, ONE), horror(&after, TWO)), (1, 0));
}

/// The board walk's order — each investigator's cards, the board, the act —
/// and then the Fast events in hand, which are not on the board.
#[test]
fn reaction_options_follow_the_board_walk_then_the_hands() {
    let state = with_act(
        board(
            seat(ONE.0)
                .in_play([REACT_ON_DEFEAT])
                .hand([FAST_REACT_ON_DEFEAT]),
            seat(TWO.0).in_play([REACT_ON_DEFEAT]),
            printing(study(), REACT_ON_DEFEAT),
            hallway(),
        )
        .open_turn(ONE)
        .build(),
        REACT_ON_DEFEAT,
    );
    let (mine, theirs) = (
        state.in_play(ONE, REACT_ON_DEFEAT),
        state.in_play(TWO, REACT_ON_DEFEAT),
    );
    assert_eq!(
        offered(state, defeat(None)),
        vec![
            Some(OptionTarget::CardInstance(mine)),
            Some(OptionTarget::CardInstance(theirs)),
            Some(OptionTarget::Location(STUDY)),
            Some(OptionTarget::Location(STUDY)),
            Some(OptionTarget::Act),
            Some(OptionTarget::HandCardByCode {
                investigator: ONE,
                code: CardCode::new(FAST_REACT_ON_DEFEAT),
            }),
        ],
        "each investigator's cards, then the board, then the act, then the hands"
    );
}
