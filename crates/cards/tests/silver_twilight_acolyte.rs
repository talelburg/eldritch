//! Silver Twilight Acolyte 01102 carries its owner (#977). Own process →
//! installs the real `cards::REGISTRY`.
//!
//! The card is a basic weakness enemy, printed verbatim:
//!
//! ```text
//! Prey - Bearer only.
//! Hunter.
//! Forced - After Silver Twilight Acolyte attacks: Place 1 doom on the current
//!   agenda.
//! ```
//!
//! Its owner is its bearer (`glossary/Weakness.md`: *"The bearer of a weakness
//! is the investigator who started the game with the weakness in his or her deck
//! or play area."*). No engine path spawns a weakness enemy from a draw yet —
//! that is #514 — so the board seats it with its owner set, and the test pins
//! that the owner is the enemy's to keep through its own attack and Forced.
//!
//! Defeated, it leaves play to its owner's pile. `glossary/Defeat.md`: *"If an
//! enemy has as much or more damage on it as it has health, that enemy is
//! defeated and placed on the encounter discard pile (or on its owner's discard
//! pile if it is a weakness)."* Printed health 3, fight 2.

use cards::REGISTRY;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::assert_event_sequence;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{self, TimingEvent};
use game_core::event::Event;
use game_core::state::{
    Agenda, CardCode, ChaosBag, ChaosToken, DiscardPile, EnemyId, GameStateBuilder, InvestigatorId,
    LocationId, Owner, TokenModifiers, Zone,
};
use game_core::test_support::{self, TestSession};

const ACOLYTE: &str = "01102";

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

#[test]
fn the_acolyte_keeps_its_bearer_as_owner_through_its_attack() {
    let bearer = InvestigatorId(1);
    let acolyte_id = EnemyId(7);
    let mut acolyte = test_support::test_enemy(7, "Silver Twilight Acolyte");
    acolyte.code = CardCode::new(ACOLYTE);
    acolyte.owner = Owner::Investigator(bearer);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([bearer])
        .with_enemy(acolyte)
        .build();
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01105"),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;

    let session = TestSession::new(state).fire_at(TimingEvent::EnemyAttacks {
        enemy: acolyte_id,
        investigator: bearer,
    });

    assert_eq!(
        session.state().agenda_doom,
        1,
        "its Forced placed 1 doom on the current agenda"
    );
    assert_eq!(
        session.state().enemies[&acolyte_id].owner,
        Owner::Investigator(bearer)
    );
}

/// **Defeated in a two-investigator game, it goes to its bearer's discard** —
/// not the encounter discard, and not the discard of the investigator who
/// defeated it. Before #982 a multiplayer defeat had no owner to route by
/// (#654) and left the card unplaced.
///
/// The bearer (investigator 2) is engaged with it; investigator 1, at the same
/// location, fights it with combat high enough that the test cannot fail
/// against the bag's single `Numeric(0)`, and the unarmed Fight's 1 damage is
/// its last.
#[test]
fn defeated_in_a_two_investigator_game_it_goes_to_its_bearers_discard() {
    let fighter = InvestigatorId(1);
    let bearer = InvestigatorId(2);
    let study = LocationId(10);
    let acolyte_id = EnemyId(7);

    let mut acolyte = test_support::test_enemy(7, "Silver Twilight Acolyte");
    acolyte.code = CardCode::new(ACOLYTE);
    acolyte.owner = Owner::Investigator(bearer);
    acolyte.fight = 2;
    acolyte.max_health = 3;
    acolyte.damage = 2;
    acolyte.engaged_with = Some(bearer);
    acolyte.current_location = Some(study);
    let mut fighting = test_support::test_investigator(1);
    fighting.skills.combat = 8;

    let state = GameStateBuilder::new()
        .with_round(0)
        .with_location(test_support::test_location(10, "Study"))
        .with_investigator_at(fighting, study)
        .with_investigator_at(test_support::test_investigator(2), study)
        .with_turn_order([fighter, bearer])
        .open_turn(fighter)
        .with_enemy(acolyte)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    let after_fight = test_support::take_turn_action(
        state,
        &TurnAction::Fight {
            investigator: fighter,
            enemy: acolyte_id,
        },
    );
    let result = engine::apply(
        after_fight.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );

    assert!(!result.state.enemies.contains_key(&acolyte_id));
    assert_eq!(
        result.state.investigators[&bearer].discard,
        vec![CardCode::new(ACOLYTE)],
        "its bearer's discard pile"
    );
    assert!(result.state.investigators[&fighter].discard.is_empty());
    assert!(result.state.encounter_discard.is_empty());
    assert_event_sequence!(
        result.events,
        Event::EnemyDefeated { enemy, .. } if *enemy == acolyte_id,
        Event::CardDiscarded {
            code,
            from: Zone::Enemy,
            to: DiscardPile::Investigator(to),
        } if code.as_str() == ACOLYTE && *to == bearer,
    );
}
