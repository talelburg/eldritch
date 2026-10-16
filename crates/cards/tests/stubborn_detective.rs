//! Stubborn Detective 01103, a basic weakness enemy whose constant ability
//! blanks the text box of each investigator at his location (#1127). Runs
//! against `cards::TEST_REGISTRY`, with one probe granter composed in for the
//! granted-ability case.
//!
//! Printed text (`data/arkhamdb-snapshot/pack/core/core.json`), verbatim:
//!
//! ```text
//! Prey - Bearer only.
//! Hunter.
//! While Stubborn Detective is at your location, treat your investigator as if
//! his or her printed text box were blank (except for [[Traits]]).
//! ```
//!
//! Fight 3, evade 2, health 2, damage 1, horror 0.
//!
//! Rulings (`data/arkhamdb-faq/core/01103.md`, <https://arkhamdb.com/card/01103>):
//! *"His static ability affects each investigator in his location (not just
//! the bearer)."* and *"Roland's ability can be used as normal after you
//! defeat this enemy."*
//!
//! `glossary/Blank.md`: *"Text and/or icons gained from another source are not
//! blanked."*
//!
//! Roland Banks 01001 is the investigator under test: *"[reaction] After you
//! defeat an enemy: Discover 1 clue at your location. (Limit once per round.)
//! / [elder_sign] effect: +1 for each clue on your location."*

use card_dsl::card_data::CardKind;
use card_dsl::dsl::{self, Ability, GrantTarget, InvestigatorTarget, SkillTestKind};
use cards::TEST_REGISTRY;
use game_core::card_registry::CardRegistry;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::{
    modified_value, ModifiedQuantity, ModifierTarget, OptionTarget, ReadContext, Registries,
};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, ChaosBag, ChaosToken, Enemy, EnemyId, GameState,
    GameStateBuilder, InvestigatorId, Location, LocationId, Owner, SkillKind, TokenModifiers,
};
use game_core::test_support::{self, seat, Seat, TestSession};
use game_core::{assert_event, assert_no_event};

const STUBBORN_DETECTIVE: &str = "01103";
const ROLAND: &str = "01001";
/// Filler for the decks, so no draw runs a deck out.
const FILLER: &str = "01088";
/// A location that grants Roland Banks a one-action activation, so a granted
/// ability can be told from a printed one. A probe of the grant primitive, not
/// a printed card (ADR 0016).
const GRANTS_ROLAND: &str = "GRANTS_ROLAND";

const INV: InvestigatorId = InvestigatorId(1);
const OTHER: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(1);
const THERE: LocationId = LocationId(2);
const C: LocationId = LocationId(3);
const D: LocationId = LocationId(4);
const DETECTIVE: EnemyId = EnemyId(50);
const GHOUL: EnemyId = EnemyId(60);

fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    if code.as_str() == GRANTS_ROLAND {
        return Some(vec![dsl::constant(dsl::grant(
            GrantTarget::Card(ROLAND.to_owned()),
            None,
            vec![dsl::activated(
                1,
                Vec::new(),
                dsl::gain_resources(InvestigatorTarget::You, 1),
            )],
        ))]);
    }
    (TEST_REGISTRY.abilities_for)(code)
}

/// [`cards::TEST_REGISTRY`] with this binary's probe granter composed in.
const CARDS: CardRegistry = CardRegistry {
    abilities_for,
    ..TEST_REGISTRY
};

const REGISTRIES: Registries = Registries {
    cards: &CARDS,
    scenarios: &ScenarioRegistry::EMPTY,
};

/// Stubborn Detective at `at`, owned by `bearer`, with its printed keywords
/// and prey read from the corpus (the fixture's fight and evade stand in for
/// his printed 3 and 2; no test here turns on them). Exhausted, so nothing engages him by
/// accident; his constant reads where he is, not whether he is ready.
fn the_detective(at: LocationId, bearer: InvestigatorId) -> Enemy {
    let meta = cards::by_code(STUBBORN_DETECTIVE).expect("Stubborn Detective in the corpus");
    let CardKind::Enemy { hunter, prey, .. } = meta.kind else {
        panic!("01103 is an enemy")
    };
    let mut enemy = test_support::test_enemy(DETECTIVE.0, "Stubborn Detective");
    enemy.code = CardCode::new(STUBBORN_DETECTIVE);
    enemy.hunter = hunter;
    enemy.prey = prey;
    enemy.owner = Owner::Investigator(bearer);
    enemy.current_location = Some(at);
    enemy.exhausted = true;
    enemy
}

/// A 1-health enemy Roland's Fight defeats on a `0` token.
fn a_ghoul() -> Enemy {
    let mut ghoul = test_support::test_enemy(GHOUL.0, "Ghoul");
    ghoul.fight = 1;
    ghoul.max_health = 1;
    ghoul
}

/// A location named `name` at `id` with `clues` on it, connected to
/// `connections`.
fn location(id: LocationId, name: &str, clues: u8, connections: &[LocationId]) -> Location {
    let mut location = test_support::test_location(id.0, name);
    location.clues = clues;
    location.connections = connections.to_vec();
    location
}

/// HERE and THERE, connected, two clues on each; a chaos bag of `token`.
fn two_rooms(token: ChaosToken) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(REGISTRIES)
        .with_location(location(HERE, "Study", 2, &[THERE]))
        .with_location(location(THERE, "Hallway", 2, &[HERE]))
        .with_chaos_bag(ChaosBag::new([token]))
        .with_token_modifiers(TokenModifiers::default())
}

/// Roland at HERE, engaged with the ghoul, Stubborn Detective at `detective_at`.
fn roland_engaged_with_a_ghoul(detective_at: LocationId) -> GameState {
    two_rooms(ChaosToken::Numeric(0))
        .with_seat(seat(INV.0).as_card(ROLAND).at(HERE))
        .with_enemy_engaged(a_ghoul(), INV)
        .with_enemy(the_detective(detective_at, INV))
        .open_turn(INV)
        .build()
}

/// `investigator` fights `enemy`, committing nothing.
fn fight(session: TestSession, investigator: InvestigatorId, enemy: EnemyId) -> TestSession {
    session
        .take(&TurnAction::Fight {
            investigator,
            enemy,
        })
        .commit(&[])
}

/// Roland's investigator card, as the target a reaction is offered on.
fn rolands_card(session: &TestSession, roland: InvestigatorId) -> OptionTarget {
    OptionTarget::CardInstance(
        session.state().investigators[&roland]
            .investigator_card
            .instance_id,
    )
}

/// A Willpower (3) test against difficulty 3 on the elder sign, committing
/// nothing. Roland's Willpower is 3, so the margin is whatever his elder sign
/// adds.
fn elder_sign_test(session: TestSession) -> TestSession {
    session.test_skill(INV, SkillKind::Willpower, 3).commit(&[])
}

// ---- the blank -------------------------------------------------------

/// **With the Detective at his location, Roland's reaction is not offered
/// after he defeats a different enemy.** The ghoul is defeated, and the
/// session comes straight back to the turn menu with no clue discovered.
#[test]
fn rolands_reaction_is_not_offered_with_the_detective_at_his_location() {
    let session = fight(
        TestSession::new(roland_engaged_with_a_ghoul(HERE), REGISTRIES),
        INV,
        GHOUL,
    );

    assert_event!(
        session.events(),
        Event::EnemyDefeated { enemy, .. } if *enemy == GHOUL
    );
    session.assert_at_turn_menu(INV);
    assert_no_event!(session.events(), Event::CluePlaced { .. });
}

/// The control: with the Detective in the next room, the same fight offers the
/// reaction.
#[test]
fn rolands_reaction_is_offered_with_the_detective_elsewhere() {
    let at_window = fight(
        TestSession::new(roland_engaged_with_a_ghoul(THERE), REGISTRIES),
        INV,
        GHOUL,
    );
    let card = rolands_card(&at_window, INV);
    let session = at_window.pick(card);

    assert_event!(
        session.events(),
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV
    );
}

/// **His elder sign contributes nothing while blanked.** Two clues on his
/// location would make it `+2`; the test succeeds by 0.
#[test]
fn rolands_elder_sign_contributes_nothing_with_the_detective_at_his_location() {
    let session = elder_sign_test(
        two_rooms(ChaosToken::ElderSign)
            .with_seat(seat(INV.0).as_card(ROLAND).at(HERE))
            .with_enemy(the_detective(HERE, INV))
            .open_turn(INV)
            .session(),
    );

    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { margin: 0, .. }
    );
}

/// Roland at HERE with the Detective, and `there_enemy` waiting at THERE.
fn roland_beside_the_detective(token: ChaosToken, there_enemy: Option<Enemy>) -> TestSession {
    let mut builder = two_rooms(token)
        .with_seat(seat(INV.0).as_card(ROLAND).at(HERE))
        .with_enemy(the_detective(HERE, INV));
    if let Some(mut enemy) = there_enemy {
        enemy.current_location = Some(THERE);
        builder = builder.with_enemy(enemy);
    }
    builder.open_turn(INV).session()
}

fn move_to_there(session: TestSession) -> TestSession {
    session.take(&TurnAction::Move {
        investigator: INV,
        destination: THERE,
    })
}

/// **Once Roland moves away, his reaction works again.** The ghoul waiting in
/// the next room engages him as he enters, and defeating it offers the
/// reaction.
#[test]
fn once_roland_moves_away_his_reaction_is_offered_again() {
    let moved = move_to_there(roland_beside_the_detective(
        ChaosToken::Numeric(0),
        Some(a_ghoul()),
    ));
    assert_eq!(moved.state().enemies[&GHOUL].engaged_with, Some(INV));

    let at_window = fight(moved, INV, GHOUL);
    let card = rolands_card(&at_window, INV);
    let session = at_window.pick(card);

    assert_event!(
        session.events(),
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV
    );
}

/// **Once Roland moves away, his elder sign works again**: `+2` for the two
/// clues on his new location.
#[test]
fn once_roland_moves_away_his_elder_sign_works_again() {
    let session = elder_sign_test(move_to_there(roland_beside_the_detective(
        ChaosToken::ElderSign,
        None,
    )));

    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { margin: 2, .. }
    );
}

/// **A second investigator at the Detective's location is blanked too**, not
/// just his bearer (the ruling). The bearer is in the next room; Roland, at
/// the Detective's location, defeats a ghoul and is not offered his reaction.
#[test]
fn a_second_investigator_at_his_location_is_blanked_too() {
    let state = two_rooms(ChaosToken::Numeric(0))
        .with_seat(seat(INV.0).at(THERE).deck([FILLER]))
        .with_seat(seat(OTHER.0).as_card(ROLAND).at(HERE))
        .with_turn_order([INV, OTHER])
        .with_enemy_engaged(a_ghoul(), OTHER)
        .with_enemy(the_detective(HERE, INV))
        .open_turn(OTHER)
        .build();

    let session = fight(TestSession::new(state, REGISTRIES), OTHER, GHOUL);

    assert_event!(
        session.events(),
        Event::EnemyDefeated { enemy, .. } if *enemy == GHOUL
    );
    session.assert_at_turn_menu(OTHER);
    assert_no_event!(session.events(), Event::CluePlaced { .. });
}

/// **A granted ability on a blanked investigator still works.** HERE grants
/// Roland a one-action "gain 1 resource"; with the Detective beside him it is
/// still on the menu and still resolves.
#[test]
fn a_granted_ability_on_a_blanked_investigator_still_works() {
    let mut here = location(HERE, "Granting Study", 0, &[THERE]);
    here.code = CardCode::new(GRANTS_ROLAND);
    let session = GameStateBuilder::new()
        .with_registries(REGISTRIES)
        .with_location(here)
        .with_seat(seat(INV.0).as_card(ROLAND).at(HERE))
        .with_enemy(the_detective(HERE, INV))
        .open_turn(INV)
        .session();
    let activation = TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(
            session.state().investigators[&INV]
                .investigator_card
                .instance_id,
        ),
        address: AbilityAddress::Granted {
            granter: CardCode::new(GRANTS_ROLAND),
            ability: 0,
            sub: 0,
        },
    };
    assert!(enumerate::legal_actions(session.state(), &CARDS).contains(&activation));
    let before = session.state().investigators[&INV].resources;

    let session = session.take(&activation);

    assert_eq!(session.state().investigators[&INV].resources, before + 1);
}

/// **Skills, health, sanity and traits read unchanged while blanked.** They are
/// not text-box content the blank reaches: Roland's printed Combat 4,
/// Willpower 3, health 9 and sanity 5 read as printed beside the Detective, and
/// his traits are still *"Agency. Detective."*.
#[test]
fn skills_health_sanity_and_traits_read_unchanged_while_blanked() {
    let state = roland_engaged_with_a_ghoul(HERE);
    let read = |quantity| {
        modified_value(
            &state,
            &CARDS,
            ModifierTarget::Investigator(INV),
            quantity,
            ReadContext::DuringTest(SkillTestKind::Plain),
        )
        .total()
    };

    assert_eq!(read(ModifiedQuantity::Skill(SkillKind::Combat)), 4);
    assert_eq!(read(ModifiedQuantity::Skill(SkillKind::Willpower)), 3);
    assert_eq!(read(ModifiedQuantity::MaxHealth), 9);
    assert_eq!(read(ModifiedQuantity::MaxSanity), 5);
    let card = &state.investigators[&INV].investigator_card.code;
    assert_eq!(
        (CARDS.metadata_for)(card)
            .expect("Roland in the corpus")
            .traits,
        vec!["Agency".to_owned(), "Detective".to_owned()]
    );
}

/// **Roland's reaction is offered after he defeats the Detective himself** (the
/// ruling). The Detective has left play before the after-window is scanned, so
/// nothing blanks Roland by then. He is engaged with Roland and one damage from
/// defeat.
#[test]
fn rolands_reaction_is_offered_after_he_defeats_the_detective() {
    let mut detective = the_detective(HERE, INV);
    detective.exhausted = false;
    detective.damage = 1;
    let state = two_rooms(ChaosToken::Numeric(0))
        .with_seat(seat(INV.0).as_card(ROLAND).at(HERE))
        .with_enemy_engaged(detective, INV)
        .open_turn(INV)
        .build();

    let at_window = fight(TestSession::new(state, REGISTRIES), INV, DETECTIVE);
    assert_event!(
        at_window.events(),
        Event::EnemyDefeated { enemy, .. } if *enemy == DETECTIVE
    );
    let card = rolands_card(&at_window, INV);
    let session = at_window.pick(card);

    assert_event!(
        session.events(),
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV
    );
}

// ---- bearer only -----------------------------------------------------

/// **Drawn by its bearer, it spawns engaged with them.** It prints no Spawn
/// instruction, so `glossary/Spawn.md`: *"If the enemy does not have a "Spawn
/// –" instruction, the investigator drawing the enemy spawns it engaged with
/// him/her, unless it is aloof."*
#[test]
fn drawn_by_its_bearer_it_spawns_engaged_with_them() {
    let session = GameStateBuilder::new()
        .with_registries(REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat(INV.0).at(HERE).deck([STUBBORN_DETECTIVE, FILLER]))
        .open_turn(INV)
        .session()
        .take(&TurnAction::Draw { investigator: INV });

    session.assert_at_turn_menu(INV);
    let mut found = session
        .state()
        .enemies
        .values()
        .filter(|e| e.code.as_str() == STUBBORN_DETECTIVE);
    let detective = found.next().expect("Stubborn Detective is in play");
    assert!(found.next().is_none(), "one Stubborn Detective");
    assert_eq!(detective.current_location, Some(HERE));
    assert_eq!(detective.engaged_with, Some(INV));
    assert_eq!(detective.owner, Owner::Investigator(INV));
}

/// The line `HERE - THERE - C - D`, the Detective ready at THERE beside the
/// other investigator, his bearer two steps away at D.
fn hunting(bearer: Seat, other: Seat) -> TestSession {
    let mut detective = the_detective(THERE, INV);
    detective.exhausted = false;
    GameStateBuilder::new()
        .with_registries(REGISTRIES)
        .with_location(location(HERE, "A", 0, &[THERE]))
        .with_location(location(THERE, "B", 0, &[HERE, C]))
        .with_location(location(C, "C", 0, &[THERE, D]))
        .with_location(location(D, "D", 0, &[C]))
        .with_seat(bearer.at(D).deck([FILLER, FILLER]))
        .with_seat(other.at(THERE).deck([FILLER, FILLER]))
        .with_turn_order([INV, OTHER])
        .with_enemy(detective)
        .open_turn(OTHER)
        .session()
}

/// **It hunts only its bearer.** In the Enemy phase it ignores the
/// investigator beside it, steps toward its farther bearer, and engages no
/// one.
#[test]
fn in_the_enemy_phase_it_hunts_only_its_bearer() {
    let session = hunting(seat(INV.0), seat(OTHER.0)).take(&TurnAction::EndTurn);

    assert_event!(
        session.events(),
        Event::EnemyMoved { enemy, to } if *enemy == DETECTIVE && *to == C
    );
    assert_no_event!(session.events(), Event::EnemyEngaged { .. });
    let detective = &session.state().enemies[&DETECTIVE];
    assert_eq!(detective.current_location, Some(C));
    assert_eq!(detective.engaged_with, None);
}
