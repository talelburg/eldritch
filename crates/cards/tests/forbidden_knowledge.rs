//! Forbidden Knowledge 01058: its secrets **move** to the resource pool, paid
//! for with an exhaust and a horror.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01058):
//! > Uses (4 secrets). If Forbidden Knowledge has no secrets, discard it.
//! > [fast] Exhaust Forbidden Knowledge and take 1 horror: Move 1 secret from
//! > Forbidden Knowledge to your resource pool, as a resource.
//!
//! Ruling (<https://arkhamdb.com/card/01058>):
//! > Moving secrets from Forbidden Knowledge to your resource pool **does not**
//! > count as 'gaining' resources, so can be used in situations where the
//! > investigator with Forbidden Knowledge may not otherwise gain resources
//! > (such as being at Trapper's Cabin or being engaged with O'Bannion's Thug).
//!
//! So the move announces itself as [`Event::UsesMovedToResourcePool`], never as
//! [`Event::ResourcesGained`]; and since spending is a cost
//! (`glossary/Uses.md`), not as [`Event::UsesSpent`] either.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{self, PlaySource, TurnAction};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, DiscardPile, EnemyId, GameState, GameStateBuilder,
    InvestigatorId, LocationId, UseKind, Zone,
};
use game_core::test_support::{self, seat, uses, Seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

const FORBIDDEN_KNOWLEDGE: &str = "01058";
const AGNES: &str = "01004";

const INV: InvestigatorId = InvestigatorId(1);
const ENEMY: EnemyId = EnemyId(100);
const HERE: LocationId = LocationId(10);

/// A seat at `HERE` at its turn menu, on a board with only `HERE`.
fn board(seat: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat.at(HERE).actions(3).resources(0))
        .open_turn(INV)
        .session()
}

/// The seat holding Forbidden Knowledge with `secrets` secrets.
fn holding(secrets: u8) -> Seat {
    seat(1).in_play([(FORBIDDEN_KNOWLEDGE, uses(UseKind::Secrets, secrets))])
}

fn activate(state: &GameState) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(state.in_play(INV, FORBIDDEN_KNOWLEDGE)),
        address: AbilityAddress::Printed(0),
    }
}

fn use_it(session: TestSession) -> TestSession {
    let action = activate(session.state());
    session.take(&action)
}

fn secrets(session: &TestSession) -> Option<u8> {
    session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.code.as_str() == FORBIDDEN_KNOWLEDGE)
        .map(|c| c.uses.get(&UseKind::Secrets).copied().unwrap_or(0))
}

/// *"Uses (4 secrets)."* Played from hand (cost 0), it enters with 4.
#[test]
fn played_from_hand_it_enters_with_four_secrets() {
    let session = board(seat(1).hand([FORBIDDEN_KNOWLEDGE])).take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    });
    session.assert_at_turn_menu(INV);
    assert_eq!(secrets(&session), Some(4));
}

/// Exhaust it and take 1 horror: 1 secret leaves the card and arrives in the
/// resource pool, announced as a move — not a gain, not a spend.
#[test]
fn using_it_moves_one_secret_to_the_resource_pool_for_one_horror() {
    let session = use_it(board(holding(4)));
    session.assert_at_turn_menu(INV);

    let investigator = &session.state().investigators[&INV];
    assert_eq!(investigator.horror(), 1, "the cost's horror is taken");
    assert_eq!(investigator.resources, 1, "the secret is now a resource");
    assert_eq!(investigator.actions_remaining, 3, "a fast ability");
    assert!(
        investigator
            .cards_in_play
            .iter()
            .any(|c| c.code.as_str() == FORBIDDEN_KNOWLEDGE && c.exhausted),
        "exhausted as a cost"
    );
    assert_eq!(secrets(&session), Some(3));

    let fk = session.state().in_play(INV, FORBIDDEN_KNOWLEDGE);
    assert_event_sequence!(
        session.events(),
        Event::HorrorTaken {
            investigator: INV,
            amount: 1
        },
        Event::UsesMovedToResourcePool {
            investigator: INV,
            instance_id,
            kind: UseKind::Secrets,
            amount: 1,
        } if *instance_id == fk,
    );
    // Nothing keyed on gaining resources — or on spending uses — sees it.
    assert_no_event!(session.events(), Event::ResourcesGained { .. });
    assert_no_event!(session.events(), Event::UsesSpent { .. });
}

/// *"If Forbidden Knowledge has no secrets, discard it."* Moving the last one
/// discards it at once: the move is announced, then the discard.
#[test]
fn moving_the_last_secret_discards_it() {
    let session = use_it(board(holding(1)));
    session.assert_at_turn_menu(INV);

    assert_eq!(secrets(&session), None, "Forbidden Knowledge left play");
    let investigator = &session.state().investigators[&INV];
    assert_eq!(investigator.resources, 1);
    assert_eq!(
        investigator.discard,
        vec![CardCode::new(FORBIDDEN_KNOWLEDGE)]
    );
    assert_event_sequence!(
        session.events(),
        Event::UsesMovedToResourcePool {
            investigator: INV,
            kind: UseKind::Secrets,
            amount: 1,
            ..
        },
        Event::CardDiscarded {
            code,
            from: Zone::InPlay,
            to: DiscardPile::Investigator(INV),
        } if code.as_str() == FORBIDDEN_KNOWLEDGE,
    );
}

/// The cost's horror is placed on Agnes Baker 01004 like any other, so it
/// offers her *"\[reaction\] After 1 or more horror is placed on Agnes Baker:
/// Deal 1 damage to an enemy at your location."* — which resolves before the
/// secret moves, since the cost is paid before the effect.
#[test]
fn its_cost_horror_on_agnes_offers_her_reaction() {
    let mut enemy = test_support::test_enemy(ENEMY.0, "Mock Ghoul");
    enemy.max_health = 3;
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(holding(4).as_card(AGNES).at(HERE).actions(3).resources(0))
        .with_enemy_engaged(enemy, INV)
        .open_turn(INV)
        .session();
    let session = use_it(session);

    let agnes = session.state().investigators[&INV].card_anchor();
    assert_eq!(session.offered_at(&agnes), 1, "her reaction is offered");
    assert_eq!(
        session.state().investigators[&INV].resources,
        0,
        "the secret has not moved yet"
    );

    let session = session.pick(agnes);
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().enemies[&ENEMY].damage, 1);
    assert_eq!(session.state().investigators[&INV].resources, 1);
    assert_event_sequence!(
        session.events(),
        Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == ENEMY,
        Event::UsesMovedToResourcePool { .. },
    );
}

/// Exhausted, it cannot pay its cost again: the ability is not offered.
#[test]
fn exhausted_it_is_not_offered_again() {
    let session = use_it(board(holding(4)));
    let action = activate(session.state());
    assert_event!(session.events(), Event::CardExhausted { .. });
    assert!(
        !enumerate::legal_actions(session.state(), TEST_REGISTRIES.cards).contains(&action),
        "an exhausted Forbidden Knowledge cannot pay its cost"
    );
}
