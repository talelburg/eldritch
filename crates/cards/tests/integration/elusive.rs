//! Elusive 01050 end-to-end against the real corpus.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01050):
//!
//! > Fast. Play only during your turn.
//! > Disengage from each enemy engaged with you and move to a revealed location
//! > with no enemies.
//!
//! Rulings (<https://arkhamdb.com/card/01050>):
//!
//! > - Disengaging does not exhaust an enemy.
//! > - You can play Elusive to move to another location even if you're not
//! >   engaged with any enemies.
//! > - You can move to any revealed location with no enemies, not necessarily a
//! >   connecting one.
//! > - You must move when you play Elusive, as it is not a 'may' effect, unless
//! >   there is no legal place to move to or a rule effect forbids movement.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{self, PlaySource, TurnAction};
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, EnemyId, FastWindowKind, GameStateBuilder,
    InvestigationPhaseFrame, InvestigationResume, InvestigatorId, LocationId, Owner, Phase,
    PhaseStep,
};
use game_core::test_support::{enemy, location, seat, StagedEnemy, StagedLocation, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

use crate::common::with_attic;

const ELUSIVE: &str = "01050";
const INV: InvestigatorId = InvestigatorId(1);
/// A second investigator, left behind at `HERE`.
const INV2: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
/// A revealed location with no enemies, not connected to `HERE`.
const THERE: LocationId = LocationId(11);
/// A revealed location with no enemies, connected to `HERE`.
const NEXT_DOOR: LocationId = LocationId(12);
/// A location that has not been revealed.
const UNREVEALED: LocationId = LocationId(13);
/// A revealed location with an enemy at it.
const OCCUPIED: LocationId = LocationId(14);

const BARRICADE: &str = "01038";
/// Beat Cop (01018): its `[fast]` ability keeps the Enemy phase's player
/// window standing.
const BEAT_COP: &str = "01018";
/// Magnifying Glass (01030): a Fast asset, so its owner's window offers it.
const MAGNIFYING_GLASS: &str = "01030";

const PLAY: TurnAction = TurnAction::PlayCard {
    investigator: INV,
    source: PlaySource::Hand(0),
};

/// `INV` at `HERE` with Elusive in hand and the resources to play it, the
/// given extra locations in play, and `engaged` enemies engaged with `INV`.
fn board(
    locations: impl IntoIterator<Item = StagedLocation>,
    engaged: impl IntoIterator<Item = StagedEnemy>,
) -> GameStateBuilder {
    let mut builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE).hand([ELUSIVE]).resources(2).actions(3))
        .with_location(location(HERE));
    for location in locations {
        builder = builder.with_location(location);
    }
    for enemy in engaged {
        builder = builder.with_enemy_engaged(enemy, INV);
    }
    builder
}

fn session(builder: GameStateBuilder) -> TestSession {
    builder.open_turn(INV).session()
}

fn location_of(session: &TestSession) -> Option<LocationId> {
    session.state().investigators[&INV].current_location
}

/// `builder` plus the two locations Elusive cannot move to: an unrevealed one,
/// and a revealed one with an (exhausted, so it stays put) unengaged enemy at
/// it.
fn with_non_destinations(builder: GameStateBuilder) -> GameStateBuilder {
    let unrevealed = location(UNREVEALED).unrevealed();
    let lurker = enemy(200).at(OCCUPIED).exhausted();
    builder
        .with_location(unrevealed)
        .with_location(location(OCCUPIED))
        .with_enemy(lurker)
}

fn is_playable(session: &TestSession) -> bool {
    enumerate::legal_actions(session.state(), TEST_REGISTRIES.cards).contains(&PLAY)
}

#[test]
fn both_engaged_enemies_are_disengaged_and_stay_ready() {
    let (a, b) = (EnemyId(100), EnemyId(101));
    let session = session(board([location(THERE)], [enemy(a.0), enemy(b.0)])).take(&PLAY);

    session.assert_at_turn_menu(INV);
    for id in [a, b] {
        let e = &session.state().enemies[&id];
        assert_eq!(e.engaged_with, None, "{id:?} disengaged");
        assert!(
            !e.exhausted,
            "{id:?} still ready: disengaging does not exhaust"
        );
        assert_eq!(e.current_location, Some(HERE), "{id:?} stays behind");
        assert_event!(
            session.events(),
            Event::EnemyDisengaged { enemy, investigator } if *enemy == id && *investigator == INV
        );
    }
    assert_eq!(
        location_of(&session),
        Some(THERE),
        "moved to the lone destination"
    );
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.damage(), 0, "no attack of opportunity");
    assert_eq!(inv.actions_remaining, 3, "a Fast play spends no action");
    assert_eq!(inv.resources, 0, "cost 2 paid");
    assert_eq!(inv.discard, vec![CardCode::new(ELUSIVE)]);
}

#[test]
fn several_destinations_offer_each_revealed_enemy_free_location_connecting_or_not() {
    // `HERE` connects to `NEXT_DOOR` only; `THERE` does not connect at all.
    let here = location(HERE).connections(vec![NEXT_DOOR]);
    let session = session(with_non_destinations(board(
        [here, location(THERE), location(NEXT_DOOR)],
        [enemy(100)],
    )))
    .take(&PLAY);

    assert_eq!(session.offered_at(&OptionTarget::Location(NEXT_DOOR)), 1);
    assert_eq!(
        session.offered_at(&OptionTarget::Location(THERE)),
        1,
        "a non-connecting revealed location is offered too"
    );
    for (ruled_out, why) in [
        (UNREVEALED, "unrevealed"),
        (OCCUPIED, "has an enemy"),
        (HERE, "where you are"),
    ] {
        assert_eq!(
            session.offered_at(&OptionTarget::Location(ruled_out)),
            0,
            "{why}: not offered"
        );
    }

    let session = session.pick(OptionTarget::Location(THERE));
    session.assert_at_turn_menu(INV);
    assert_eq!(location_of(&session), Some(THERE));
}

#[test]
fn with_no_destination_you_stay_and_the_disengaged_enemies_engage_you_again() {
    let (a, b) = (EnemyId(100), EnemyId(101));
    let session = session(with_non_destinations(board([], [enemy(a.0), enemy(b.0)]))).take(&PLAY);

    session.assert_at_turn_menu(INV);
    assert_eq!(
        location_of(&session),
        Some(HERE),
        "nowhere to move: no move"
    );
    assert_no_event!(session.events(), Event::InvestigatorMoved { .. });
    for id in [a, b] {
        // Disengaged by the card, then re-engaged by the engagement rule once
        // it has resolved: a ready unengaged enemy at your location engages you.
        assert_event_sequence!(
            session.events(),
            Event::EnemyDisengaged { enemy, .. } if *enemy == id,
            Event::EnemyEngaged { enemy, investigator } if *enemy == id && *investigator == INV,
        );
        let e = &session.state().enemies[&id];
        assert_eq!(e.engaged_with, Some(INV), "{id:?} engages you again");
        assert!(!e.exhausted, "{id:?} still ready");
    }
    assert_eq!(
        session.state().investigators[&INV].discard,
        vec![CardCode::new(ELUSIVE)],
        "the card still resolved"
    );
}

#[test]
fn playable_with_no_engaged_enemies_and_moves_you() {
    let session = session(board([location(THERE)], []));
    assert!(is_playable(&session));

    let session = session.take(&PLAY);
    session.assert_at_turn_menu(INV);
    assert_eq!(location_of(&session), Some(THERE));
    assert_no_event!(session.events(), Event::EnemyDisengaged { .. });
}

#[test]
fn not_playable_when_it_can_neither_disengage_nor_move() {
    // No enemy engaged with you, and nowhere Elusive could move you: the effect
    // cannot change the game state, so the play is not offered.
    let session = session(with_non_destinations(board([], [])));
    assert!(
        !is_playable(&session),
        "Elusive must not be on the turn menu"
    );
}

/// The option playing investigator 1's Elusive in a Fast window.
const ELUSIVE_IN_HAND: OptionTarget = OptionTarget::HandCard {
    investigator: INV,
    hand_index: 0,
};

/// *"Play only during your turn"*: in investigator 2's turn-begins window,
/// standing on their Magnifying Glass 01030 (a Fast asset), Elusive is not
/// offered to investigator 1 even with a destination to move to.
#[test]
fn not_playable_in_another_investigators_turn() {
    let other = InvestigatorId(2);
    let window = board([location(THERE)], [])
        .with_seat(seat(2).at(HERE).hand([MAGNIFYING_GLASS]).resources(1))
        .with_seating([INV, other])
        .with_phase(Phase::Investigation)
        .with_active_investigator(other)
        .with_phase_anchor(InvestigationPhaseFrame {
            resume: InvestigationResume::TurnBegins,
        })
        .with_open_window(FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins))
        .session();
    let glass = OptionTarget::HandCard {
        investigator: other,
        hand_index: 0,
    };
    assert_eq!(window.offered_at(&glass), 1, "the window stands");
    assert_eq!(
        window.offered_at(&ELUSIVE_IN_HAND),
        0,
        "it is not investigator 1's turn: {:?}",
        window.prompt(),
    );
    let _ = window.assert_rejects(|s| s.submit_off_menu(&PLAY));
}

/// *"Play only during your turn"*: the Enemy phase is no one's turn. Beat Cop
/// 01018's `[fast]` ability keeps the phase's player window standing.
#[test]
fn not_playable_in_the_enemy_phase() {
    let paused = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .hand([ELUSIVE])
                .resources(2)
                .in_play([BEAT_COP]),
        )
        .with_location(location(HERE))
        .with_location(location(THERE))
        .with_enemy_engaged(enemy(100), INV)
        .open_turn(INV)
        .session()
        .take(&TurnAction::EndTurn);
    assert_eq!(paused.state().phase, Phase::Enemy);
    assert_eq!(
        paused.offered_at(&ELUSIVE_IN_HAND),
        0,
        "the Enemy phase window does not offer Elusive: {:?}",
        paused.prompt(),
    );
    let _ = paused.assert_rejects(|s| s.submit_off_menu(&PLAY));
}

#[test]
fn the_move_fires_leave_and_enter_timing() {
    // Barricade 01038 at `HERE`: "Forced - When an investigator leaves attached
    // location: Discard Barricade." The Attic 01113 as the lone destination:
    // "Forced - After you enter the Attic: Take 1 horror."
    let here = location(HERE).attachments([CardInPlay::enter_play(
        CardCode::new(BARRICADE),
        CardInstanceId(900),
        Owner::Investigator(INV),
    )]);
    let (session, attic) = with_attic(session(board([here], [enemy(100)])), &[]);
    let session = session.take(&PLAY);

    session.assert_at_turn_menu(INV);
    assert_eq!(location_of(&session), Some(attic));
    assert_event_sequence!(
        session.events(),
        Event::CardDiscarded { code, .. } if code.as_str() == BARRICADE,
        Event::InvestigatorMoved { to, .. } if *to == attic,
    );
    assert!(
        session.state().locations[&HERE].attachments.is_empty(),
        "Barricade discarded on leaving"
    );
    assert_eq!(
        session.state().investigators[&INV].horror(),
        1,
        "the Attic's after-enter Forced resolved"
    );
}

/// The enemy Elusive disengages engages the investigator left behind before
/// the Attic 01113's *"**Forced** - After you enter the Attic: Take 1
/// horror."* resolves. The engagement is the move's ENGAGE step, part of the
/// effect, and `glossary/Effects.md` reads: *"All aspects of an effect have
/// timing priority over all "after..." triggering conditions that might arise
/// as a consequence of that effect."* A second investigator has to stay at
/// `HERE`: with no one left behind, ENGAGE has nobody to engage.
#[test]
fn the_disengaged_enemy_engages_whoever_stays_before_the_attics_horror() {
    let ghoul = EnemyId(100);
    let (session, attic) = with_attic(
        session(
            board([], [enemy(ghoul.0)])
                .with_seat(seat(2).at(HERE))
                .with_seating([INV, INV2]),
        ),
        &[],
    );
    let session = session.take(&PLAY);

    session.assert_at_turn_menu(INV);
    assert_eq!(location_of(&session), Some(attic));
    assert_eq!(session.state().enemies[&ghoul].engaged_with, Some(INV2));
    assert_eq!(session.state().investigators[&INV].horror(), 1);
    assert_event_sequence!(
        session.events(),
        Event::EnemyDisengaged { enemy, investigator } if *enemy == ghoul && *investigator == INV,
        Event::InvestigatorMoved { to, .. } if *to == attic,
        Event::EnemyEngaged { enemy, investigator } if *enemy == ghoul && *investigator == INV2,
        Event::HorrorPlacedOnInvestigator { investigator, amount: 1 } if *investigator == INV,
    );
}
