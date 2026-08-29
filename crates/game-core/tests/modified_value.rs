//! The modified-value sweep, end to end over all six collections a
//! modifying card can sit in.
//!
//! Lives at `crates/game-core/tests/` so it runs in its own
//! integration-test process (separate `OnceLock<CardRegistry>`), letting
//! it install a mock registry without colliding with game-core's
//! in-crate tests or with other `tests/*.rs` files. Mirrors
//! `activate_ability.rs`.
//!
//! No corpus card declares a non-controller audience yet — the fourteen
//! that will (Lita Chantler 01117, Whippoorwill 02090, Whateley Ruins
//! 02250, The Ritual Begins 01144, …) are card work of their own — so
//! mock cards shaped after them are the only way to exercise the sweep.
//! Each mock below names the printed card it is shaped after.

use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::dsl::{constant, modify_for, Ability, ModifierAudience, ModifierScope, Stat};
use game_core::event::Event;
use game_core::state::{
    Act, Agenda, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, EnemyId, GameState,
    InvestigatorId, LocationId, Phase, SkillKind, TokenModifiers,
};
use game_core::test_support::{
    apply_no_commits, perform_skill_test_no_commits, take_turn_action, test_enemy,
    test_investigator, test_location, GameStateBuilder, TestSession,
};
use game_core::{
    assert_event, modified_value, Action, ContributionSource, InputResponse, ModifiedQuantity,
    ModifierTarget, PlayerAction, ReadContext, TurnAction,
};

/// Shaped after Lita Chantler 01117: *"Each investigator at your
/// location gets +1 [combat]."* An asset one investigator controls that
/// reaches everyone standing with them.
const LITA: &str = "MOCK-LITA";

/// Shaped after Whateley Ruins 02250: *"Each investigator in Whateley
/// Ruins gets -1 [willpower]."* A location card modifying from its own
/// place on the board.
const WHATELEY: &str = "MOCK-WHATELEY";

/// Shaped after Obscuring Fog 01168: *"Attached location gets +2
/// shroud."*
const FOG: &str = "MOCK-FOG";

/// Shaped after Whippoorwill 02090: *"Each investigator at
/// Whippoorwill's location gets -1 [willpower], -1 [intellect], -1
/// [combat], and -1 [agility]."* Only the intellect clause is modelled
/// here; one clause is enough to prove the enemy is swept.
const WHIPPOORWILL: &str = "MOCK-WHIPPOORWILL";

/// Shaped after Towering Beasts 02256: *"Attached enemy gets +1 fight
/// and +1 health."* Only the fight clause is modelled.
const TOWERING_BEASTS: &str = "MOCK-TOWERING";

/// Shaped after The Ritual Begins 01144: *"Each enemy gets +1 fight and
/// +1 evade."*
const RITUAL_BEGINS: &str = "MOCK-RITUAL";

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        LITA => Some(vec![constant(modify_for(
            ModifierAudience::EachInvestigatorAtSourceLocation,
            Stat::Combat,
            1,
            ModifierScope::WhileInPlay,
        ))]),
        WHATELEY => Some(vec![constant(modify_for(
            ModifierAudience::EachInvestigatorAtSourceLocation,
            Stat::Willpower,
            -1,
            ModifierScope::WhileInPlay,
        ))]),
        FOG => Some(vec![constant(modify_for(
            ModifierAudience::AttachedCard,
            Stat::Shroud,
            2,
            ModifierScope::WhileInPlay,
        ))]),
        WHIPPOORWILL => Some(vec![constant(modify_for(
            ModifierAudience::EachInvestigatorAtSourceLocation,
            Stat::Intellect,
            -1,
            ModifierScope::WhileInPlay,
        ))]),
        TOWERING_BEASTS => Some(vec![constant(modify_for(
            ModifierAudience::AttachedCard,
            Stat::Fight,
            1,
            ModifierScope::WhileInPlay,
        ))]),
        RITUAL_BEGINS => Some(vec![
            constant(modify_for(
                ModifierAudience::EachEnemy,
                Stat::Fight,
                1,
                ModifierScope::WhileInPlay,
            )),
            constant(modify_for(
                ModifierAudience::EachEnemy,
                Stat::Evade,
                1,
                ModifierScope::WhileInPlay,
            )),
        ]),
        _ => None,
    }
}

#[ctor::ctor(unsafe)]
fn install_mock_registry() {
    let _ = game_core::card_registry::install(CardRegistry {
        metadata_for: mock_metadata_for,
        abilities_for: mock_abilities_for,
        back_abilities_for: |_| None,
        native_effect_for: |_| None,
        native_eligibility_for: |_| None,
        native_condition_for: |_| None,
    });
}

const ME: InvestigatorId = InvestigatorId(1);
const TEAMMATE: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
const THERE: LocationId = LocationId(11);

/// A two-investigator board: me at `HERE`, my teammate wherever
/// `teammate_at` says, a chaos bag of a single `Numeric(0)` so the token
/// never moves a total.
fn board(teammate_at: LocationId) -> GameStateBuilder {
    let mut me = test_investigator(1);
    me.current_location = Some(HERE);
    let mut teammate = test_investigator(2);
    teammate.current_location = Some(teammate_at);
    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(ME)
        .with_turn_order([ME, TEAMMATE])
        .with_investigator_turn(ME)
        .with_investigator(me)
        .with_investigator(teammate)
        .with_location(test_location(10, "Study"))
        .with_location(test_location(11, "Hallway"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
}

fn in_play(code: &str, instance: u32) -> CardInPlay {
    CardInPlay::enter_play(CardCode::new(code), CardInstanceId(instance))
}

// ---- 1. another investigator's cards in play --------------------

/// Lita Chantler's *"Each investigator at your location gets +1
/// [combat]"* reaches a teammate. My combat is 3; against difficulty 4
/// the test fails by 1 on my own, and passes with Lita standing next to
/// me.
#[test]
fn a_card_another_investigator_controls_reaches_me() {
    let mut state = board(HERE).build();
    state
        .investigators
        .get_mut(&TEAMMATE)
        .unwrap()
        .cards_in_play
        .push(in_play(LITA, 1));

    let result = perform_skill_test_no_commits(state, ME, SkillKind::Combat, 4);
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, margin: 0, .. } if *investigator == ME
    );
}

/// The same card, with its controller standing somewhere else, reaches
/// nobody — the audience is *at your location*, not *everywhere*.
#[test]
fn a_card_another_investigator_controls_elsewhere_does_not_reach_me() {
    let mut state = board(THERE).build();
    state
        .investigators
        .get_mut(&TEAMMATE)
        .unwrap()
        .cards_in_play
        .push(in_play(LITA, 1));

    let result = perform_skill_test_no_commits(state, ME, SkillKind::Combat, 4);
    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, by: 1, .. } if *investigator == ME
    );
}

// ---- 2. a location's own card -----------------------------------

/// Whateley Ruins' *"Each investigator in Whateley Ruins gets -1
/// [willpower]"* applies to whoever is standing in it. Willpower 3 − 1
/// against difficulty 3 fails by 1.
#[test]
fn a_modifier_on_a_location_reaches_investigators_in_it() {
    let mut ruins = test_location(10, "Whateley Ruins");
    ruins.code = CardCode::new(WHATELEY);
    let state = board(THERE).with_location(ruins).build();

    let result = perform_skill_test_no_commits(state, ME, SkillKind::Willpower, 3);
    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, by: 1, .. } if *investigator == ME
    );
}

/// An investigator at a *different* location is untouched by it.
#[test]
fn a_modifier_on_a_location_does_not_reach_investigators_elsewhere() {
    let mut ruins = test_location(11, "Whateley Ruins");
    ruins.code = CardCode::new(WHATELEY);
    let state = board(THERE).with_location(ruins).build();

    let result = perform_skill_test_no_commits(state, ME, SkillKind::Willpower, 3);
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, margin: 0, .. } if *investigator == ME
    );
}

// ---- 3. a location's attachments --------------------------------

/// Obscuring Fog's *"Attached location gets +2 shroud"* reaches the
/// location it is attached to, and no other.
#[test]
fn a_modifier_on_a_location_attachment_reaches_that_location() {
    let mut fogged = test_location(10, "Study");
    fogged.attachments.push(in_play(FOG, 1));
    let state = board(THERE).with_location(fogged).build();

    assert_eq!(shroud(&state, HERE), 4, "printed 2 + 2 from the attachment");
    assert_eq!(
        shroud(&state, THERE),
        2,
        "an unfogged location is untouched"
    );
}

// ---- 4. an enemy's own card -------------------------------------

/// Whippoorwill is an *enemy* modifying every investigator at its
/// location. Intellect 3 − 1 against difficulty 3 fails by 1.
#[test]
fn a_modifier_on_an_enemy_reaches_investigators_at_its_location() {
    let mut bird = test_enemy(7, "Whippoorwill");
    bird.code = CardCode::new(WHIPPOORWILL);
    bird.current_location = Some(HERE);
    let state = board(THERE).with_enemy(bird).build();

    let result = perform_skill_test_no_commits(state, ME, SkillKind::Intellect, 3);
    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, by: 1, .. } if *investigator == ME
    );
}

/// The same enemy at another location leaves me alone.
#[test]
fn a_modifier_on_an_enemy_elsewhere_does_not_reach_me() {
    let mut bird = test_enemy(7, "Whippoorwill");
    bird.code = CardCode::new(WHIPPOORWILL);
    bird.current_location = Some(THERE);
    let state = board(THERE).with_enemy(bird).build();

    let result = perform_skill_test_no_commits(state, ME, SkillKind::Intellect, 3);
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, margin: 0, .. } if *investigator == ME
    );
}

// ---- 5. an enemy's attachments ----------------------------------

/// Towering Beasts' *"Attached enemy gets +1 fight"* reaches the enemy
/// it is attached to. Asked of the query directly; that this value *is*
/// the difficulty of a Fight action against that enemy is driven through
/// a real action below.
#[test]
fn a_modifier_on_an_enemy_attachment_reaches_that_enemy() {
    let mut brood = test_enemy(7, "Brood of Yog-Sothoth");
    brood.current_location = Some(HERE);
    brood.attachments.push(in_play(TOWERING_BEASTS, 1));
    let mut other = test_enemy(8, "Ghoul");
    other.current_location = Some(HERE);
    let state = board(THERE).with_enemy(brood).with_enemy(other).build();

    assert_eq!(
        fight(&state, EnemyId(7)),
        3,
        "printed 2 + 1 from the attachment"
    );
    assert_eq!(
        fight(&state, EnemyId(8)),
        2,
        "an unattached enemy is untouched"
    );
}

// ---- 6. the current act and agenda -------------------------------

/// The Ritual Begins' *"Each enemy gets +1 fight and +1 evade"* reaches
/// every enemy in play, wherever it stands, from the agenda.
#[test]
fn a_modifier_on_the_current_agenda_reaches_every_enemy() {
    let mut near = test_enemy(7, "Ghoul");
    near.current_location = Some(HERE);
    let mut far = test_enemy(8, "Ghoul");
    far.current_location = Some(THERE);
    let mut state = board(THERE).with_enemy(near).with_enemy(far).build();
    state.agenda_deck = vec![agenda("MOCK-QUIET"), agenda(RITUAL_BEGINS)];

    state.agenda_index = 0;
    assert_eq!(
        fight(&state, EnemyId(7)),
        2,
        "a different agenda is showing"
    );

    state.agenda_index = 1;
    assert_eq!(fight(&state, EnemyId(7)), 3);
    assert_eq!(fight(&state, EnemyId(8)), 3);
    assert_eq!(evade(&state, EnemyId(7)), 3);
}

/// The act is swept on the same footing as the agenda, and only the
/// *current* one of each contributes.
#[test]
fn a_modifier_on_the_current_act_reaches_every_enemy() {
    let mut ghoul = test_enemy(7, "Ghoul");
    ghoul.current_location = Some(HERE);
    let mut state = board(THERE).with_enemy(ghoul).build();
    state.act_deck = vec![act("MOCK-QUIET"), act(RITUAL_BEGINS)];

    state.act_index = 0;
    assert_eq!(fight(&state, EnemyId(7)), 2, "a different act is showing");

    state.act_index = 1;
    assert_eq!(fight(&state, EnemyId(7)), 3);
}

/// The AC's read-time property, isolated: the same query over the same
/// board gives two different answers either side of a change to that
/// board, with nothing invalidated, recomputed or re-registered in
/// between. Whippoorwill walks out of the room and the debuff is simply
/// gone at the next read.
#[test]
fn a_modifier_answers_differently_either_side_of_a_board_change() {
    let mut bird = test_enemy(7, "Whippoorwill");
    bird.code = CardCode::new(WHIPPOORWILL);
    bird.current_location = Some(HERE);
    let mut state = board(THERE).with_enemy(bird).build();

    let intellect = |state: &GameState| {
        modified_value(
            state,
            game_core::card_registry::current(),
            ModifierTarget::Investigator(ME),
            ModifiedQuantity::Skill(SkillKind::Intellect),
            ReadContext::OutsideTest,
        )
        .total()
    };

    assert_eq!(intellect(&state), 2, "the enemy is here");
    state.enemies.get_mut(&EnemyId(7)).unwrap().current_location = Some(THERE);
    assert_eq!(intellect(&state), 3, "it left, and nothing had to be told");
    state.enemies.get_mut(&EnemyId(7)).unwrap().current_location = Some(HERE);
    assert_eq!(intellect(&state), 2, "it came back");
}

// ---- an enemy's stats as a test's difficulty ---------------------

/// A Fight action's difficulty **is** the attacked enemy's modified
/// fight value (#677) — `glossary/Skill_Tests.md`, "Variable Difficulty
/// Skill Tests", quoted on `DifficultyBasis`.
///
/// Combat 2 against a Ghoul (printed fight 2) succeeds by 0; with The
/// Ritual Begins' *"Each enemy gets +1 fight"* showing, the same attack
/// fails by 1.
#[test]
fn a_modifier_on_an_enemys_fight_raises_a_fight_actions_difficulty() {
    let attack = |ritual: bool| {
        let mut ghoul = test_enemy(7, "Ghoul");
        ghoul.current_location = Some(HERE);
        let mut state = board(THERE).with_enemy(ghoul).build();
        state.investigators.get_mut(&ME).unwrap().skills.combat = 2;
        if ritual {
            state.agenda_deck = vec![agenda(RITUAL_BEGINS)];
            state.agenda_index = 0;
        }
        TestSession::new(state)
            .take(&TurnAction::Fight {
                investigator: ME,
                enemy: EnemyId(7),
            })
            .resolve_choices(|c| {
                c.commit_cards(&[]);
            })
            .run()
    };

    let plain = attack(false);
    assert_event!(plain.events, Event::SkillTestStarted { difficulty: 2, .. });
    assert_event!(
        plain.events,
        Event::SkillTestSucceeded { investigator, margin: 0, .. } if *investigator == ME
    );

    let harder = attack(true);
    assert_event!(harder.events, Event::SkillTestStarted { difficulty: 3, .. });
    assert_event!(
        harder.events,
        Event::SkillTestFailed { investigator, by: 1, .. } if *investigator == ME
    );
}

/// The same for Evade: the difficulty is the enemy's modified evade
/// value. Agility 2 against a Ghoul (printed evade 2) succeeds by 0, and
/// fails by 1 under The Ritual Begins' *"+1 evade"*.
#[test]
fn a_modifier_on_an_enemys_evade_raises_an_evade_actions_difficulty() {
    let dodge = |ritual: bool| {
        let mut ghoul = test_enemy(7, "Ghoul");
        ghoul.current_location = Some(HERE);
        ghoul.engaged_with = Some(ME);
        let mut state = board(THERE).with_enemy(ghoul).build();
        state.investigators.get_mut(&ME).unwrap().skills.agility = 2;
        if ritual {
            state.agenda_deck = vec![agenda(RITUAL_BEGINS)];
            state.agenda_index = 0;
        }
        TestSession::new(state)
            .take(&TurnAction::Evade {
                investigator: ME,
                enemy: EnemyId(7),
            })
            .resolve_choices(|c| {
                c.commit_cards(&[]);
            })
            .run()
    };

    let plain = dodge(false);
    assert_event!(plain.events, Event::SkillTestStarted { difficulty: 2, .. });
    assert_event!(
        plain.events,
        Event::SkillTestSucceeded { investigator, margin: 0, .. } if *investigator == ME
    );

    let harder = dodge(true);
    assert_event!(harder.events, Event::SkillTestStarted { difficulty: 3, .. });
    assert_event!(
        harder.events,
        Event::SkillTestFailed { investigator, by: 1, .. } if *investigator == ME
    );
}

/// The difficulty is read at ST.6, not banked at ST.1: an enemy whose
/// fight rises **after** the test started is harder to hit than the test
/// announced. Driven by advancing the agenda deck's index mid-test,
/// which is the smallest stand-in for a card that changes an enemy's
/// fight while a test is in flight.
#[test]
fn a_fights_difficulty_is_re_read_after_the_test_started() {
    let mut ghoul = test_enemy(7, "Ghoul");
    ghoul.current_location = Some(HERE);
    let mut state = board(THERE).with_enemy(ghoul).build();
    state.investigators.get_mut(&ME).unwrap().skills.combat = 2;
    state.agenda_deck = vec![agenda("MOCK-QUIET"), agenda(RITUAL_BEGINS)];
    state.agenda_index = 0;

    // ST.1: announced against the unmodified fight of 2.
    let started = take_turn_action(
        state,
        &TurnAction::Fight {
            investigator: ME,
            enemy: EnemyId(7),
        },
    );
    assert_event!(
        started.events,
        Event::SkillTestStarted { difficulty: 2, .. }
    );

    // The ritual begins while the test is paused at its commit window.
    let mut state = started.state;
    state.agenda_index = 1;

    let resolved = apply_no_commits(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple {
                selected: Vec::new(),
            },
        }),
    );
    assert_event!(
        resolved.events,
        Event::SkillTestFailed { investigator, by: 1, .. } if *investigator == ME
    );
}

// ---- the breakdown ----------------------------------------------

/// A value with two contributions reads back its base and both, each
/// attributed to the card that produced it.
#[test]
fn a_breakdown_reads_back_its_base_and_every_contribution() {
    let mut whateley = test_location(10, "Whateley Ruins");
    whateley.code = CardCode::new(WHATELEY);
    let mut state = board(HERE).with_location(whateley).build();
    // A second copy of the same audience from a wholly different place
    // on the board: a Whippoorwill standing in the ruins.
    let mut bird = test_enemy(7, "Whippoorwill");
    bird.code = CardCode::new(WHIPPOORWILL);
    bird.current_location = Some(HERE);
    state.enemies.insert(EnemyId(7), bird);

    let willpower = modified_value(
        &state,
        game_core::card_registry::current(),
        ModifierTarget::Investigator(ME),
        ModifiedQuantity::Skill(SkillKind::Willpower),
        ReadContext::OutsideTest,
    );
    assert_eq!(willpower.base, 3, "the printed willpower");
    assert_eq!(willpower.total(), 2, "one of the two touches willpower");
    assert_eq!(willpower.contributions.len(), 1);
    assert_eq!(
        willpower.contributions[0].source,
        ContributionSource::Card {
            code: CardCode::new(WHATELEY),
            instance: None,
        },
        "the location card has no in-play instance of its own",
    );

    let intellect = modified_value(
        &state,
        game_core::card_registry::current(),
        ModifierTarget::Investigator(ME),
        ModifiedQuantity::Skill(SkillKind::Intellect),
        ReadContext::OutsideTest,
    );
    assert_eq!(intellect.contributions.len(), 1);
    assert_eq!(
        intellect.contributions[0].source,
        ContributionSource::Card {
            code: CardCode::new(WHIPPOORWILL),
            instance: None,
        },
    );
    assert_eq!(intellect.contributions[0].delta, -1);
}

// ---- helpers ----------------------------------------------------

fn agenda(code: &str) -> Agenda {
    Agenda {
        code: CardCode::new(code),
        doom_threshold: 3,
    }
}

fn act(code: &str) -> Act {
    Act {
        code: CardCode::new(code),
        clue_threshold: 2,
    }
}

fn read(state: &GameState, target: ModifierTarget, quantity: ModifiedQuantity) -> i32 {
    modified_value(
        state,
        game_core::card_registry::current(),
        target,
        quantity,
        ReadContext::from_state(state),
    )
    .total()
}

fn shroud(state: &GameState, location: LocationId) -> i32 {
    read(
        state,
        ModifierTarget::Location(location),
        ModifiedQuantity::Shroud,
    )
}

fn fight(state: &GameState, enemy: EnemyId) -> i32 {
    read(state, ModifierTarget::Enemy(enemy), ModifiedQuantity::Fight)
}

fn evade(state: &GameState, enemy: EnemyId) -> i32 {
    read(state, ModifierTarget::Enemy(enemy), ModifiedQuantity::Evade)
}
