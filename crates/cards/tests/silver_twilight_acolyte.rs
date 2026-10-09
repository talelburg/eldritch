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

use cards::REGISTRY;
use game_core::engine::TimingEvent;
use game_core::state::{Agenda, CardCode, EnemyId, GameStateBuilder, InvestigatorId, Owner};
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
