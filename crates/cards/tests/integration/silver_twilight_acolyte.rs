//! Silver Twilight Acolyte 01102 carries its owner, drawn from its bearer's
//! deck.
//!
//! Runs against `cards::TEST_REGISTRIES`.
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
//! or play area."*). Every test draws it from the bearer's deck with the Draw
//! action, which resolves it *"as if it were just drawn from the encounter
//! deck"* (`glossary/Weakness.md`), and pins that the owner is the enemy's to
//! keep through its own attack and Forced, and through its defeat.
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
    AbilityAddress, AbilitySource, CardCode, ChaosBag, ChaosToken, DiscardPile, Enemy, EnemyId,
    GameStateBuilder, InvestigatorId, LocationId, Owner, SkillKind, Zone,
};
use game_core::test_support::{enemy, location, seat, BoardLookup as _, TestSession};

const ACOLYTE: &str = "01102";
const MACHETE: &str = "01020";
/// Filler for the decks, so no draw runs a deck out.
const FILLER: &str = "01088";
const HERE: LocationId = LocationId(10);
const THERE: LocationId = LocationId(11);

/// The one Silver Twilight Acolyte on the board.
fn spawned_acolyte(session: &TestSession) -> &Enemy {
    let mut acolytes = session
        .state()
        .enemies
        .values()
        .filter(|e| e.code.as_str() == ACOLYTE);
    let acolyte = acolytes.next().expect("the Acolyte is in play");
    assert!(acolytes.next().is_none(), "one Acolyte");
    acolyte
}

/// `bearer` takes the Draw action with the Acolyte on top of their deck.
fn draw_the_acolyte(session: TestSession, bearer: InvestigatorId) -> TestSession {
    session.take(&TurnAction::Draw {
        investigator: bearer,
    })
}

/// **Drawn from its bearer's deck, it spawns engaged with them, owned by
/// them.** `glossary/Weakness.md`: *"When an investigator draws a weakness with
/// an encounter cardtype (for example, an enemy or a treachery weakness),
/// resolve that card as if it were just drawn from the encounter deck."* It
/// prints no Spawn instruction, so `glossary/Spawn.md` applies: *"If the enemy
/// does not have a "Spawn –" instruction, the investigator drawing the enemy
/// spawns it engaged with him/her, unless it is aloof."*
#[test]
fn drawn_by_the_draw_action_it_spawns_engaged_with_its_bearer() {
    let bearer = InvestigatorId(1);
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(seat(1).at(HERE).actions(3).deck([ACOLYTE, FILLER]))
        .open_turn(bearer)
        .session();
    let session = draw_the_acolyte(session, bearer);

    session.assert_at_turn_menu(bearer);
    assert!(
        !session.state().investigators[&bearer]
            .hand
            .iter()
            .any(|c| c.as_str() == ACOLYTE),
        "it does not sit in hand"
    );
    let acolyte = spawned_acolyte(&session);
    assert_eq!(acolyte.current_location, Some(HERE));
    assert_eq!(acolyte.engaged_with, Some(bearer));
    assert_eq!(acolyte.owner, Owner::Investigator(bearer));
}

#[test]
fn the_acolyte_keeps_its_bearer_as_owner_through_its_attack() {
    let bearer = InvestigatorId(1);
    // The bearer draws it, then ends their turn engaged with it: that runs
    // into the Enemy phase, where the Acolyte attacks. Filler cards so Upkeep
    // 4.4's draw does not deal empty-deck horror.
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(seat(1).at(HERE).deck([ACOLYTE, FILLER, FILLER]))
        .open_turn(bearer)
        .with_agenda_deck(["01105"])
        .session();

    let session = draw_the_acolyte(session, bearer).take(&TurnAction::EndTurn);

    // 2, not 1: the `EndTurn` cascade runs on through Upkeep into the next
    // round's Mythos, whose step 1.2 places a doom of its own before parking on
    // the encounter-draw prompt.
    assert_eq!(
        session.state().agenda_doom,
        2,
        "its Forced placed 1 doom on the current agenda (plus Mythos step 1.2's)"
    );
    assert_eq!(spawned_acolyte(&session).owner, Owner::Investigator(bearer));
}

/// **Defeated in a two-investigator game, it goes to its bearer's discard** —
/// not the encounter discard, and not the discard of the investigator who
/// defeated it. Its bearer owns it, so the defeat routes it there.
///
/// The bearer (investigator 1) draws it alone at the Study, so it spawns
/// engaged with them, and hits it once. The fighter (investigator 2) starts
/// next door, Moves in, and Fights it twice with Machete 01020, dealing the
/// last point. Combat is high enough that no test can fail against the bag's
/// single `Numeric(0)`.
///
/// Machete's *"this attack deals +1 damage"* does not apply: its condition is
/// *"If the attacked enemy is the only enemy engaged with you"*, and the
/// Acolyte is engaged with the bearer. Its ruling
/// (<https://arkhamdb.com/card/01020>): *"Machete will not provide a damage
/// bonus for attacking … an enemy engaged to another player."* So each of the
/// three attacks deals 1 damage against its printed health 3.
#[test]
fn defeated_in_a_two_investigator_game_it_goes_to_its_bearers_discard() {
    let bearer = InvestigatorId(1);
    let fighter = InvestigatorId(2);

    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE).connections([THERE]))
        .with_location(location(THERE).connections([HERE]))
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Combat, 8)
                .deck([ACOLYTE, FILLER]),
        )
        .with_seat(
            seat(2)
                .at(THERE)
                .skill(SkillKind::Combat, 8)
                .in_play([MACHETE]),
        )
        .with_seating([bearer, fighter])
        .open_turn(bearer)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .session();
    let session = draw_the_acolyte(session, bearer);
    let acolyte = spawned_acolyte(&session).id;
    let session = session
        .take(&TurnAction::Fight {
            investigator: bearer,
            enemy: acolyte,
        })
        .commit(&[])
        .take(&TurnAction::EndTurn);
    session.assert_at_turn_menu(fighter);
    assert_eq!(spawned_acolyte(&session).damage, 1, "the bearer's hit");

    let machete_fight = |session: TestSession| {
        let fight = TurnAction::ActivateAbility {
            investigator: fighter,
            source: AbilitySource::InPlay(session.state().in_play(fighter, MACHETE)),
            address: AbilityAddress::Printed(0),
        };
        session.take(&fight).commit(&[])
    };
    let session = session.take(&TurnAction::Move {
        investigator: fighter,
        destination: HERE,
    });
    let session = machete_fight(session);
    assert_eq!(
        spawned_acolyte(&session).damage,
        2,
        "1 damage: no Machete bonus against an enemy engaged with another investigator"
    );
    let session = machete_fight(session);

    session.assert_at_turn_menu(fighter);
    let state = session.state();
    assert!(!state.enemies.contains_key(&acolyte));
    assert_eq!(
        state.investigators[&bearer].discard,
        vec![CardCode::new(ACOLYTE)],
        "its bearer's discard pile"
    );
    assert!(state.investigators[&fighter].discard.is_empty());
    assert!(state.encounter_discard.is_empty());
    assert_event_sequence!(
        session.events(),
        Event::EnemyDefeated { enemy, .. } if *enemy == acolyte,
        Event::CardDiscarded {
            code,
            from: Zone::Enemy,
            to: DiscardPile::Investigator(to),
        } if code.as_str() == ACOLYTE && *to == bearer,
    );
}

/// **Readied beside only the other investigator, it engages no one**.
/// `glossary/Prey.md`: *"If an enemy's prey instructions contain the word
/// "only," that enemy only moves towards and engages that investigator (as if it
/// were the only investigator in play), and ignores all other investigators
/// while moving and engaging."*
///
/// The bearer draws it at the Study, where the other investigator also stands,
/// so it spawns engaged with the bearer. The bearer evades it (exhausting it and
/// breaking the engagement) and walks to the Hallway. It readies in Upkeep
/// beside the other investigator, which would engage them under any other prey.
#[test]
fn readied_beside_only_the_other_investigator_it_engages_no_one() {
    let bearer = InvestigatorId(1);
    let other = InvestigatorId(2);

    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE).connections([THERE]))
        .with_location(location(THERE).connections([HERE]))
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Agility, 8)
                .deck([ACOLYTE, FILLER, FILLER]),
        )
        .with_seat(seat(2).at(HERE).deck([FILLER, FILLER]))
        .with_seating([bearer, other])
        .open_turn(bearer)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .session();
    let session = draw_the_acolyte(session, bearer);
    let acolyte = spawned_acolyte(&session).id;
    assert_eq!(spawned_acolyte(&session).engaged_with, Some(bearer));

    let session = session
        .take(&TurnAction::Evade {
            investigator: bearer,
            enemy: acolyte,
        })
        .commit(&[])
        .take(&TurnAction::Move {
            investigator: bearer,
            destination: THERE,
        })
        .take(&TurnAction::EndTurn);
    session.assert_at_turn_menu(other);
    // Its Enemy phase (exhausted, it does not hunt) and Upkeep (it readies).
    let session = session.take(&TurnAction::EndTurn);

    let enemy = spawned_acolyte(&session);
    assert!(!enemy.exhausted, "readied in Upkeep");
    assert_eq!(enemy.current_location, Some(HERE));
    assert_eq!(
        enemy.engaged_with, None,
        "the other investigator is ignored"
    );
    let engagements: Vec<_> = session
        .events()
        .iter()
        .filter_map(|e| match e {
            Event::EnemyEngaged {
                enemy,
                investigator,
            } if *enemy == acolyte => Some(*investigator),
            _ => None,
        })
        .collect();
    assert_eq!(
        engagements,
        vec![bearer],
        "engaged only on spawn, never after the evade"
    );
}

/// **In the Enemy phase it hunts its bearer, not the investigator beside it**.
/// It stands at the Study with the other investigator, its bearer at
/// the Hallway next door: the other investigator does not hold it in place, and
/// it moves to its bearer and engages them. The Acolyte is placed on the board
/// as a draw mints it: the printed card, owned by its bearer.
#[test]
fn in_the_enemy_phase_it_hunts_its_bearer_past_the_investigator_beside_it() {
    let bearer = InvestigatorId(1);
    let other = InvestigatorId(2);
    let acolyte = EnemyId(50);

    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE).connections([THERE]))
        .with_location(location(THERE).connections([HERE]))
        .with_seat(seat(1).at(THERE).deck([FILLER, FILLER]))
        .with_seat(seat(2).at(HERE).deck([FILLER, FILLER]))
        .with_seating([bearer, other])
        .with_enemy(
            enemy(acolyte)
                .as_card(ACOLYTE)
                .owner(Owner::Investigator(bearer))
                .at(HERE),
        )
        .open_turn(other)
        .session()
        .take(&TurnAction::EndTurn);

    assert_event_sequence!(
        session.events(),
        Event::EnemyMoved { enemy, to } if *enemy == acolyte && *to == THERE,
        Event::EnemyEngaged { enemy, investigator }
            if *enemy == acolyte && *investigator == bearer,
    );
}
