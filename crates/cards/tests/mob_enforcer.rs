//! Mob Enforcer 01101, a basic weakness enemy that pursues its bearer only and
//! can be paid off. Runs against `cards::TEST_REGISTRIES`.
//!
//! Printed text (`data/arkhamdb-snapshot/pack/core/core.json`), verbatim:
//!
//! ```text
//! Prey - Bearer only.
//! Hunter.
//! [action] Spend 4 resources: Parley. Discard Mob Enforcer.
//! ```
//!
//! Fight 4, evade 3, health 3, damage 1, horror 0.
//!
//! `glossary/Prey.md`: *"If an enemy's prey instructions contain the word
//! "only," that enemy only moves towards and engages that investigator (as if
//! it were the only investigator in play), and ignores all other investigators
//! while moving and engaging. Other investigators may use the engage action or
//! card abilities to engage the enemy."*
//!
//! Ruling (`data/arkhamdb-faq/core/01101.md`, <https://arkhamdb.com/card/01101>):
//! *"Discarding an enemy is not the same as defeating it, so reactions to an
//! enemy being defeated (e.g. Roland Banks, Evidence!) do not trigger."*

use card_dsl::card_data::CardKind;
use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, DiscardPile, Enemy, EnemyId, GameStateBuilder,
    InvestigatorId, Location, LocationId, Owner, Zone,
};
use game_core::test_support::{self, seat, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

const MOB_ENFORCER: &str = "01101";
/// Roland Banks 01001: *"\[reaction\] After you defeat an enemy: Discover 1 clue
/// at your location. (Limit once per round.)"*
const ROLAND: &str = "01001";
/// Filler for the decks, so no draw runs a deck out.
const FILLER: &str = "01088";

const BEARER: InvestigatorId = InvestigatorId(1);
const OTHER: InvestigatorId = InvestigatorId(2);
const A: LocationId = LocationId(1);
const B: LocationId = LocationId(2);
const C: LocationId = LocationId(3);
const D: LocationId = LocationId(4);
const ENFORCER: EnemyId = EnemyId(50);

/// The one Mob Enforcer on the board.
fn the_enforcer(session: &TestSession) -> &Enemy {
    let mut found = session
        .state()
        .enemies
        .values()
        .filter(|e| e.code.as_str() == MOB_ENFORCER);
    let enforcer = found.next().expect("Mob Enforcer is in play");
    assert!(found.next().is_none(), "one Mob Enforcer");
    enforcer
}

/// Its bearer, alone at A with `resources` resources and Roland Banks in play,
/// has drawn it from their deck with the Draw action.
fn drawn_by_its_bearer(resources: u8) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(A.0, "Study"))
        .with_seat(
            seat(BEARER.0)
                .at(A)
                .resources(resources)
                .in_play([ROLAND])
                .deck([MOB_ENFORCER, FILLER]),
        )
        .open_turn(BEARER)
        .session()
        .take(&TurnAction::Draw {
            investigator: BEARER,
        })
}

/// The Parley, activated by `investigator` from the enemy's printed ability.
fn parley(investigator: InvestigatorId, enemy: EnemyId) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator,
        source: AbilitySource::Enemy(enemy),
        address: AbilityAddress::Printed(0),
    }
}

/// **Drawn by its bearer, it spawns engaged with them.** It prints no Spawn
/// instruction, so `glossary/Spawn.md`: *"If the enemy does not have a "Spawn
/// –" instruction, the investigator drawing the enemy spawns it engaged with
/// him/her, unless it is aloof."*
#[test]
fn drawn_by_its_bearer_it_spawns_engaged_with_them() {
    let session = drawn_by_its_bearer(0);

    session.assert_at_turn_menu(BEARER);
    let enforcer = the_enforcer(&session);
    assert_eq!(enforcer.current_location, Some(A));
    assert_eq!(enforcer.engaged_with, Some(BEARER));
    assert_eq!(enforcer.owner, Owner::Investigator(BEARER));
}

/// **Drawn in the Upkeep phase's step-4.4 draw, it spawns engaged with its
/// bearer too**, though another investigator stands with them and leads the
/// turn order: the drawer, not the lead, is who it engages.
#[test]
fn drawn_in_upkeep_it_spawns_engaged_with_its_bearer() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(A.0, "Study"))
        .with_seat(seat(OTHER.0).at(A).deck([FILLER, FILLER]))
        .with_seat(seat(BEARER.0).at(A).deck([MOB_ENFORCER, FILLER]))
        .with_turn_order([OTHER, BEARER])
        .ending_enemy_phase()
        .session();

    session.assert_at_encounter_draw();
    let enforcer = the_enforcer(&session);
    assert_eq!(enforcer.current_location, Some(A));
    assert_eq!(enforcer.engaged_with, Some(BEARER));
    assert_eq!(enforcer.owner, Owner::Investigator(BEARER));
    assert_event!(
        session.events(),
        Event::EnemySpawned { code, engaged_with, .. }
            if code.as_str() == MOB_ENFORCER && *engaged_with == Some(BEARER)
    );
}

/// **The Parley spends 4 resources and discards it to its bearer's discard.**
/// Discarding is not defeating (the ruling), so it logs no `EnemyDefeated` and
/// Roland's *"After you defeat an enemy"* reaction is not offered: the session
/// comes straight back to the turn menu. Leaving play ends the engagement with
/// no `EnemyDisengaged` (`event.rs`'s leave-play contract).
#[test]
fn the_parley_spends_4_resources_and_discards_it_to_its_bearers_discard() {
    let session = drawn_by_its_bearer(5);
    let enforcer = the_enforcer(&session).id;

    let session = session.take(&parley(BEARER, enforcer));

    session.assert_at_turn_menu(BEARER);
    let state = session.state();
    assert!(!state.enemies.contains_key(&enforcer));
    assert_eq!(state.investigators[&BEARER].resources, 1);
    assert_eq!(
        state.investigators[&BEARER].discard,
        vec![CardCode::new(MOB_ENFORCER)],
        "its bearer's discard pile"
    );
    assert!(state.encounter_discard.is_empty());
    assert_event!(
        session.events(),
        Event::CardDiscarded {
            code,
            from: Zone::Enemy,
            to: DiscardPile::Investigator(to),
        } if code.as_str() == MOB_ENFORCER && *to == BEARER
    );
    assert_no_event!(session.events(), Event::EnemyDefeated { .. });
    assert_no_event!(session.events(), Event::EnemyDisengaged { .. });
}

/// **The Parley is refused with fewer than 4 resources.** The menu does not
/// offer it, and the handler refuses it sent off the menu, changing nothing.
#[test]
fn the_parley_is_refused_with_fewer_than_4_resources() {
    let session = drawn_by_its_bearer(3);
    let enforcer = the_enforcer(&session).id;
    let action = parley(BEARER, enforcer);
    assert!(
        !enumerate::legal_actions(session.state(), TEST_REGISTRIES.cards).contains(&action),
        "not offered with 3 resources"
    );

    let reason = session.assert_rejects(|s| s.submit_off_menu(&action));
    assert!(reason.contains("resources"), "{reason}");
}

/// With exactly 4 resources the Parley is on the menu: the refusal above is the
/// cost, not a missing ability.
#[test]
fn the_parley_is_offered_with_4_resources() {
    let session = drawn_by_its_bearer(4);
    let action = parley(BEARER, the_enforcer(&session).id);
    assert!(enumerate::legal_actions(session.state(), TEST_REGISTRIES.cards).contains(&action));
}

/// Mob Enforcer on the board at `at`, unengaged and ready, owned by `BEARER` —
/// what a draw mints, with its printed keywords and prey read from the corpus.
fn on_the_board(at: LocationId) -> Enemy {
    let meta = cards::by_code(MOB_ENFORCER).expect("Mob Enforcer in the corpus");
    let CardKind::Enemy { hunter, prey, .. } = meta.kind else {
        panic!("01101 is an enemy")
    };
    let mut enemy = test_support::test_enemy(ENFORCER.0, "Mob Enforcer");
    enemy.code = CardCode::new(MOB_ENFORCER);
    enemy.hunter = hunter;
    enemy.prey = prey;
    enemy.owner = Owner::Investigator(BEARER);
    enemy.current_location = Some(at);
    enemy
}

/// A location named `name` at `id`, connected to `connections`.
fn linked(id: LocationId, name: &str, connections: &[LocationId]) -> Location {
    let mut location = test_support::test_location(id.0, name);
    location.connections = connections.to_vec();
    location
}

/// The line `A - B - C - D`, Mob Enforcer at `enforcer_at`, its bearer at
/// `bearer_at` and the other investigator at `other_at`, on `turn`'s turn.
fn two_seats(
    enforcer_at: LocationId,
    bearer_at: LocationId,
    other_at: LocationId,
    turn: InvestigatorId,
) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(linked(A, "A", &[B]))
        .with_location(linked(B, "B", &[A, C]))
        .with_location(linked(C, "C", &[B, D]))
        .with_location(linked(D, "D", &[C]))
        .with_seat(seat(BEARER.0).at(bearer_at).deck([FILLER, FILLER]))
        .with_seat(seat(OTHER.0).at(other_at).deck([FILLER, FILLER]))
        .with_turn_order([BEARER, OTHER])
        .with_enemy(on_the_board(enforcer_at))
        .open_turn(turn)
        .session()
}

/// **In the Enemy phase it steps toward its bearer, though the other
/// investigator is nearer, and does not engage the other investigator it
/// stands with.** It starts at B with the other investigator; its bearer is two
/// steps away at D. It ignores the investigator beside it, moves to C, and
/// engages no one there.
#[test]
fn in_the_enemy_phase_it_hunts_its_farther_bearer_past_the_other_investigator() {
    let session = two_seats(B, D, B, OTHER).take(&TurnAction::EndTurn);

    assert_event!(
        session.events(),
        Event::EnemyMoved { enemy, to } if *enemy == ENFORCER && *to == C
    );
    assert_no_event!(session.events(), Event::EnemyEngaged { .. });
    let enforcer = &session.state().enemies[&ENFORCER];
    assert_eq!(enforcer.current_location, Some(C));
    assert_eq!(enforcer.engaged_with, None);
}

/// From C, the next step reaches its bearer at D and engages them there.
#[test]
fn reaching_its_bearer_it_engages_them() {
    let session = two_seats(C, D, A, OTHER).take(&TurnAction::EndTurn);

    assert_event_sequence!(
        session.events(),
        Event::EnemyMoved { enemy, to } if *enemy == ENFORCER && *to == D,
        Event::EnemyEngaged { enemy, investigator }
            if *enemy == ENFORCER && *investigator == BEARER,
    );
}

/// **The other investigator can still take the Engage action against it.**
/// `glossary/Prey.md`: *"Other investigators may use the engage action or card
/// abilities to engage the enemy."*
#[test]
fn the_other_investigator_can_still_engage_it() {
    let session = two_seats(B, D, B, OTHER).take(&TurnAction::Engage {
        investigator: OTHER,
        enemy: ENFORCER,
    });

    assert_eq!(session.state().enemies[&ENFORCER].engaged_with, Some(OTHER));
    assert_event!(
        session.events(),
        Event::EnemyEngaged { enemy, investigator }
            if *enemy == ENFORCER && *investigator == OTHER
    );
}

/// **The other investigator walking in is not engaged by it.** Entering a
/// location engages its ready, unengaged enemies (`glossary/Enemy_Engagement.md`),
/// but a bearer-only enemy *"ignores all other investigators while moving and
/// engaging"*.
#[test]
fn the_other_investigator_walking_in_is_not_engaged() {
    let session = two_seats(B, D, A, OTHER).take(&TurnAction::Move {
        investigator: OTHER,
        destination: B,
    });

    assert_eq!(
        session.state().investigators[&OTHER].current_location,
        Some(B)
    );
    assert_eq!(session.state().enemies[&ENFORCER].engaged_with, None);
    assert_no_event!(session.events(), Event::EnemyEngaged { .. });
}
