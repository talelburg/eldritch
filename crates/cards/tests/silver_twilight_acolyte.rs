//! Silver Twilight Acolyte 01102 carries its owner (#977). Runs against
//! `cards::TEST_REGISTRIES`.
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
use cards::TEST_REGISTRIES;
use game_core::assert_event_sequence;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{
    Agenda, CardCode, ChaosBag, ChaosToken, DiscardPile, Enemy, EnemyId, GameStateBuilder,
    InvestigatorId, LocationId, Owner, SkillKind, TokenModifiers, Zone,
};
use game_core::test_support::{self, seat, TestSession};

const ACOLYTE: &str = "01102";
const ACOLYTE_ID: EnemyId = EnemyId(7);
const HERE: LocationId = LocationId(10);

/// A Silver Twilight Acolyte borne by `bearer`.
fn acolyte_of(bearer: InvestigatorId) -> Enemy {
    let mut acolyte = test_support::test_enemy(ACOLYTE_ID.0, "Silver Twilight Acolyte");
    acolyte.code = CardCode::new(ACOLYTE);
    acolyte.owner = Owner::Investigator(bearer);
    acolyte
}

#[test]
fn the_acolyte_keeps_its_bearer_as_owner_through_its_attack() {
    let bearer = InvestigatorId(1);
    // The bearer's turn, engaged with the Acolyte: ending it runs into the
    // Enemy phase, where the Acolyte attacks. A filler deck so Upkeep 4.4's
    // draw does not deal empty-deck horror.
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat(1).at(HERE).deck(["01088"]))
        .with_enemy_engaged(acolyte_of(bearer), bearer)
        .open_turn(bearer)
        .build();
    // The builder stages no agenda.
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01105"),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;

    let session =
        TestSession::new_with_registries(state, TEST_REGISTRIES).take(&TurnAction::EndTurn);

    // 2, not 1: the `EndTurn` cascade runs on through Upkeep into the next
    // round's Mythos, whose step 1.2 places a doom of its own before parking on
    // the encounter-draw prompt.
    assert_eq!(
        session.state().agenda_doom,
        2,
        "its Forced placed 1 doom on the current agenda (plus Mythos step 1.2's)"
    );
    assert_eq!(
        session.state().enemies[&ACOLYTE_ID].owner,
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

    let mut acolyte = acolyte_of(bearer);
    acolyte.fight = 2;
    acolyte.max_health = 3;
    acolyte.damage = 2;

    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat(1).at(HERE).skill(SkillKind::Combat, 8))
        .with_seat(seat(2).at(HERE))
        .with_turn_order([fighter, bearer])
        .open_turn(fighter)
        .with_enemy_engaged(acolyte, bearer)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .session()
        .take(&TurnAction::Fight {
            investigator: fighter,
            enemy: ACOLYTE_ID,
        })
        .commit(&[]);

    session.assert_at_turn_menu(fighter);
    let state = session.state();
    assert!(!state.enemies.contains_key(&ACOLYTE_ID));
    assert_eq!(
        state.investigators[&bearer].discard,
        vec![CardCode::new(ACOLYTE)],
        "its bearer's discard pile"
    );
    assert!(state.investigators[&fighter].discard.is_empty());
    assert!(state.encounter_discard.is_empty());
    assert_event_sequence!(
        session.events(),
        Event::EnemyDefeated { enemy, .. } if *enemy == ACOLYTE_ID,
        Event::CardDiscarded {
            code,
            from: Zone::Enemy,
            to: DiscardPile::Investigator(to),
        } if code.as_str() == ACOLYTE && *to == bearer,
    );
}
