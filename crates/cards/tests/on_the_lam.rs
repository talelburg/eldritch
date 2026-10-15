//! On the Lam 01010 ("Skids" O'Toole's signature event), end-to-end against
//! the real card corpus.
//!
//! Printed text, verbatim from the pinned snapshot:
//!
//! ```text
//! "Skids" O'Toole deck only.
//! Fast. Play after your turn begins.
//! Until the end of the round, non-[[Elite]] enemies cannot attack you.
//! ```
//!
//! Its one ruling (<https://arkhamdb.com/card/01010>) is the signature-deck
//! note, which bears on deckbuilding, not play.
//!
//! The three places an enemy attacks — an attack of opportunity, the Enemy
//! phase's step 3.3 and a Retaliate — each skip a non-Elite enemy while the
//! protection lasts. A skipped enemy makes no attack and so is not exhausted:
//! `Appendix_II_Timing_and_Gameplay.md` 3.3 exhausts only *"Upon completion of
//! dealing the attack"*.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    Act, CardCode, ChaosBag, ChaosToken, Enemy, EnemyId, GameState, GameStateBuilder,
    InvestigatorId, Location, LocationId, Phase, SkillKind, TokenModifiers,
};
use game_core::test_support::{self, seat, Seat, TestSession};
use game_core::{assert_event, assert_no_event};

/// On the Lam (01010).
const ON_THE_LAM: &str = "01010";
/// Beat Cop (01018): its `[fast]` ability keeps the Enemy phase's player
/// window standing, so a play there can be attempted.
const BEAT_COP: &str = "01018";
/// Ghoul Priest (01116): *Humanoid. Monster. Ghoul. Elite.*, Retaliate.
const GHOUL_PRIEST: &str = "01116";
/// Ghoul Minion (01160): *Humanoid. Monster. Ghoul.* — not Elite.
const GHOUL_MINION: &str = "01160";
/// Wizard of the Order (01170): *Humanoid. Cultist.*, Retaliate — not Elite.
const WIZARD_OF_THE_ORDER: &str = "01170";
/// A deck card for the round-ending cascade's Upkeep step-4.4 draw.
const FILLER: &str = "01088";

const INV: InvestigatorId = InvestigatorId(1);
const OTHER: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(101);
const HALLWAY: LocationId = LocationId(102);
const ATTACKER: EnemyId = EnemyId(7);
const SECOND_ATTACKER: EnemyId = EnemyId(8);

/// A ready enemy printed as `code`, dealing 1 damage.
fn enemy(id: EnemyId, code: &str) -> Enemy {
    let mut e = test_support::test_enemy(id.0, code);
    e.code = CardCode::new(code);
    e.attack_damage = 1;
    e.attack_horror = 0;
    e
}

/// Investigator 1 at the Study with On the Lam in hand and the resource to
/// play it.
fn skids() -> Seat {
    seat(1)
        .at(HERE)
        .resources(1)
        .hand([ON_THE_LAM])
        .deck([FILLER])
}

/// The Study (connected to the Hallway) and the Hallway, investigator 1 at the
/// Study engaged by each of `enemies`.
fn study_board(seat_one: Seat, enemies: Vec<Enemy>) -> GameStateBuilder {
    let mut study = test_support::test_location(HERE.0, "Study");
    study.connections = vec![HALLWAY];
    let mut hallway = test_support::test_location(HALLWAY.0, "Hallway");
    hallway.connections = vec![HERE];
    enemies.into_iter().fold(
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_location(study)
            .with_location(hallway)
            .with_seat(seat_one),
        |b, e| b.with_enemy_engaged(e, INV),
    )
}

/// Investigator 1's turn at the Study, engaged by each of `enemies`, with On
/// the Lam unplayed.
fn board(enemies: Vec<Enemy>) -> TestSession {
    study_board(skids(), enemies).open_turn(INV).session()
}

/// Investigator 2 at `other_at` takes the turn before investigator 1's. Ending
/// it begins investigator 1's turn, whose `TurnBegan` `after` cell is where On
/// the Lam is played.
fn turn_began(builder: GameStateBuilder, other: Seat) -> TestSession {
    turn_began_with(builder, other, |_| {})
}

/// [`turn_began`] on a board `edit` finishes, for what the builder cannot set.
fn turn_began_with(
    builder: GameStateBuilder,
    other: Seat,
    edit: impl FnOnce(&mut GameState),
) -> TestSession {
    let mut state = builder
        .with_seat(other.deck([FILLER]))
        .with_turn_order([OTHER, INV])
        .open_turn(OTHER)
        .build();
    edit(&mut state);
    TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::EndTurn)
}

/// [`board`], reached at the start of investigator 1's turn and with On the
/// Lam played there.
fn on_the_lam_played(enemies: Vec<Enemy>) -> TestSession {
    play_on_the_lam(turn_began(
        study_board(skids(), enemies),
        seat(2).at(HALLWAY),
    ))
}

/// The option playing investigator `who`'s On the Lam from hand in a
/// reaction window.
fn on_the_lam_of(who: InvestigatorId) -> OptionTarget {
    OptionTarget::HandCardByCode {
        investigator: who,
        code: CardCode::new(ON_THE_LAM),
    }
}

fn play_on_the_lam(session: TestSession) -> TestSession {
    session.pick(on_the_lam_of(INV))
}

/// The option playing investigator `who`'s first hand card in a Fast window.
fn hand_card(who: InvestigatorId) -> OptionTarget {
    OptionTarget::HandCard {
        investigator: who,
        hand_index: 0,
    }
}

fn move_to_hallway(session: TestSession) -> TestSession {
    session.take(&TurnAction::Move {
        investigator: INV,
        destination: HALLWAY,
    })
}

/// The events up to the end of the Enemy phase: the `EndTurn` cascade runs on
/// into Upkeep, which readies every enemy.
fn enemy_phase_events(session: &TestSession) -> Vec<Event> {
    session
        .events()
        .iter()
        .take_while(|e| {
            !matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Enemy
                }
            )
        })
        .cloned()
        .collect()
}

// ---- play timing ---------------------------------------------------------

/// The cell after investigator 1's turn begins, both investigators at the
/// Study, investigator 1 holding On the Lam and investigator 2 holding a copy
/// if `other_holds` (as if it were in their deck too).
fn after_skids_turn_begins(other_holds: bool) -> TestSession {
    let other = if other_holds {
        seat(2).at(HERE).hand([ON_THE_LAM]).resources(1)
    } else {
        seat(2).at(HERE)
    };
    turn_began(study_board(skids(), vec![]), other)
}

#[test]
fn played_after_the_turn_begins() {
    let window = after_skids_turn_begins(false);
    assert_eq!(
        window.offered_at(&on_the_lam_of(INV)),
        1,
        "the cell after the turn begins offers On the Lam: {:?}",
        window.prompt(),
    );
    let before = window.state().investigators[&INV].actions_remaining;
    let session = play_on_the_lam(window);
    assert_event!(
        session.events(),
        Event::CardPlayed { investigator: INV, code } if code.as_str() == ON_THE_LAM
    );
    assert_eq!(
        session.state().investigators[&INV].actions_remaining,
        before,
        "Fast: playing it costs no action",
    );
    session.assert_at_turn_menu(INV);
}

#[test]
fn not_playable_after_an_action() {
    // Passed at the start of the turn, then an action taken: the timing point
    // has gone, and the window 2.2.1 returns to is not its cell.
    let session = after_skids_turn_begins(false)
        .skip()
        .take(&TurnAction::Resource { investigator: INV });
    session.assert_at_turn_menu(INV);
    let play = TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    };
    let _ = session.assert_rejects(|s| s.submit_off_menu(&play));
}

#[test]
fn not_playable_in_another_investigators_turn() {
    let window = after_skids_turn_begins(true);
    assert_eq!(
        window.offered_at(&on_the_lam_of(OTHER)),
        0,
        "it is not investigator 2's turn: {:?}",
        window.prompt(),
    );
    let play = TurnAction::PlayCard {
        investigator: OTHER,
        source: PlaySource::Hand(0),
    };
    let _ = window.assert_rejects(|s| s.submit_off_menu(&play));
}

#[test]
fn not_playable_in_the_enemy_phase() {
    // Beat Cop's `[fast]` ability pauses the Enemy phase's step-3.3 window.
    let paused = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(skids().in_play([BEAT_COP]))
        .with_enemy_engaged(enemy(ATTACKER, GHOUL_MINION), INV)
        .open_turn(INV)
        .session()
        .take(&TurnAction::EndTurn);
    assert_eq!(paused.state().phase, Phase::Enemy);
    assert_eq!(
        paused.offered_at(&hand_card(INV)),
        0,
        "the Enemy phase window does not offer On the Lam: {:?}",
        paused.prompt(),
    );
    let play = TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    };
    let _ = paused.assert_rejects(|s| s.submit_off_menu(&play));
}

// ---- attacks of opportunity ---------------------------------------------

#[test]
fn a_non_elite_enemy_makes_no_attack_of_opportunity() {
    let session = move_to_hallway(on_the_lam_played(vec![enemy(ATTACKER, GHOUL_MINION)]));
    assert_no_event!(session.events(), Event::DamageTaken { .. });
    assert_eq!(
        session.state().investigators[&INV].current_location,
        Some(HALLWAY),
        "the move completes",
    );
    assert!(!session.state().enemies[&ATTACKER].exhausted);
}

#[test]
fn an_elite_enemy_still_makes_an_attack_of_opportunity() {
    let session = move_to_hallway(on_the_lam_played(vec![enemy(ATTACKER, GHOUL_PRIEST)]));
    assert_event!(
        session.events(),
        Event::DamageTaken {
            investigator: INV,
            amount: 1
        }
    );
}

#[test]
fn without_on_the_lam_a_non_elite_enemy_attacks_of_opportunity() {
    let session = move_to_hallway(board(vec![enemy(ATTACKER, GHOUL_MINION)]));
    assert_event!(
        session.events(),
        Event::DamageTaken {
            investigator: INV,
            amount: 1
        }
    );
}

// ---- enemy phase ---------------------------------------------------------

#[test]
fn a_non_elite_enemy_makes_no_enemy_phase_attack_and_stays_ready() {
    let session = on_the_lam_played(vec![enemy(ATTACKER, GHOUL_MINION)]).take(&TurnAction::EndTurn);
    let events = enemy_phase_events(&session);
    assert_event!(
        events,
        Event::PhaseStarted {
            phase: Phase::Enemy
        }
    );
    assert_no_event!(events, Event::DamageTaken { .. });
    assert_no_event!(events, Event::EnemyExhausted { .. });
}

#[test]
fn an_elite_enemy_still_attacks_in_the_enemy_phase() {
    // Engaged with both: the Elite attacks, the non-Elite does not.
    let session = on_the_lam_played(vec![
        enemy(ATTACKER, GHOUL_PRIEST),
        enemy(SECOND_ATTACKER, GHOUL_MINION),
    ])
    .take(&TurnAction::EndTurn);
    let events = enemy_phase_events(&session);
    let damage: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::DamageTaken { .. }))
        .collect();
    assert_eq!(damage.len(), 1, "only the Elite attacks: {events:?}");
    assert_event!(events, Event::EnemyExhausted { enemy: ATTACKER });
    assert_no_event!(
        events,
        Event::EnemyExhausted {
            enemy: SECOND_ATTACKER
        }
    );
}

// ---- retaliate -----------------------------------------------------------

/// A failed Fight against `code`, a Retaliate enemy: combat 1 + `Numeric(0)`
/// against fight 5.
fn failed_fight_against(code: &str, on_the_lam: bool) -> TestSession {
    let mut e = enemy(ATTACKER, code);
    e.fight = 5;
    e.max_health = 3;
    e.retaliate = true;
    let fighter = if on_the_lam {
        skids()
    } else {
        seat(1).at(HERE)
    };
    let builder = study_board(fighter.skill(SkillKind::Combat, 1), vec![e])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default());
    let session = if on_the_lam {
        play_on_the_lam(turn_began(builder, seat(2).at(HALLWAY)))
    } else {
        builder.open_turn(INV).session()
    };
    session
        .take(&TurnAction::Fight {
            investigator: INV,
            enemy: ATTACKER,
        })
        .commit(&[])
}

#[test]
fn retaliate_is_suppressed_for_a_non_elite_enemy() {
    let session = failed_fight_against(WIZARD_OF_THE_ORDER, true);
    assert_event!(session.events(), Event::SkillTestEnded { .. });
    assert_no_event!(session.events(), Event::DamageTaken { .. });
}

#[test]
fn without_on_the_lam_a_non_elite_enemy_retaliates() {
    let session = failed_fight_against(WIZARD_OF_THE_ORDER, false);
    assert_event!(
        session.events(),
        Event::DamageTaken {
            investigator: INV,
            amount: 1
        }
    );
}

#[test]
fn an_elite_enemy_still_retaliates() {
    let session = failed_fight_against(GHOUL_PRIEST, true);
    assert_event!(
        session.events(),
        Event::DamageTaken {
            investigator: INV,
            amount: 1
        }
    );
}

// ---- expiry --------------------------------------------------------------

/// The protection is gone by the first round-end ability: The Barrier 01109's
/// *"Objective - When the round ends, investigators in the hallway may, as a
/// group, spend the requisite number of clues to advance."* — investigator 1
/// stands in the Hallway (01112) holding its threshold, so ending the round's
/// last turn stops at that window.
#[test]
fn the_protection_is_gone_before_any_round_end_ability() {
    let builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(skids().at(HALLWAY).clues(3))
        .with_location(Location::new(
            HALLWAY,
            CardCode::new("01112"),
            "Hallway",
            1,
            0,
        ));
    let played = play_on_the_lam(turn_began_with(builder, seat(2).at(HALLWAY), |state| {
        state.act_deck = vec![Act {
            code: CardCode::new("01109"),
            clue_threshold: 3,
        }];
        state.act_index = 0;
    }));
    assert!(
        !played.state().lasting_effects.is_empty(),
        "On the Lam's protection lasts through the round",
    );

    let window = played.take(&TurnAction::EndTurn);
    assert_eq!(
        window.offered_at(&OptionTarget::Act),
        1,
        "the cascade stops at The Barrier's round-end window: {:?}",
        window.prompt(),
    );
    assert!(
        window.state().lasting_effects.is_empty(),
        "the protection expired before the round-end window: {:?}",
        window.state().lasting_effects,
    );
}
