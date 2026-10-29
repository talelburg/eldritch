//! Regression for #764: a defeated active investigator's turn ends, driven
//! through the public `apply` loop against the real card corpus.
//!
//! The `InvestigatorTurn` frame does not stay on top after the elimination
//! steps run, so `drive` never re-enumerates the open turn and prompts the
//! investigator who has just been defeated.
//!
//! ## The rule
//!
//! The Elimination entry (Rules Reference p.10) says nothing about turns. The
//! basis is Appendix II step **2.2.1**, *"If the investigator does not or cannot
//! take an action, proceed to 2.2.2."* — an eliminated investigator cannot take
//! one. Step **2.2.2** is then the rotation: *"If there is an investigator who
//! has not yet taken a turn this round, return to 2.2. If each investigator has
//! taken a turn this round, proceed to 2.3."*
//!
//! Lives in `crates/cards/tests/` because the defeat is reached the real way —
//! an attack of opportunity provoked by a non-fast play.
//!
//! ## Verified card text (`data/arkhamdb-snapshot`, 2026-08-24)
//!
//! **Emergency Cache (01088):** "Gain 3 resources." A non-fast event, so playing
//! it is an action and provokes an attack of opportunity (RR p.5). No rulings —
//! it is listed in `data/arkhamdb-faq/no-rulings.txt`.

use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::{EngineOutcome, OptionTarget};
use game_core::event::Event;
use game_core::scenario::ScenarioEnding;
use game_core::state::{
    Continuation, Enemy, GameStateBuilder, InvestigatorId, LocationId, Phase, Status,
};
use game_core::test_support::{self, seat, TestSession};

/// Emergency Cache (01088): non-fast event → playing it provokes.
const EMERGENCY_CACHE: &str = "01088";

const DYING: InvestigatorId = InvestigatorId(1);
const SURVIVOR: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(101);
const ELSEWHERE: LocationId = LocationId(102);

/// A ready enemy hitting for 3 damage.
fn ready_attacker() -> Enemy {
    let mut e = test_support::test_enemy(7, "Attacker");
    e.attack_damage = 3;
    e.attack_horror = 0;
    e.max_health = 5;
    e
}

/// Investigator 1 on 7 damage (one attack of opportunity from dead), engaged,
/// holding Emergency Cache, with the open turn. `seating` decides whether the
/// rotation has anyone left to hand the turn to; the survivor is seated only
/// when it names them.
fn board(seating: &[InvestigatorId]) -> GameStateBuilder {
    let builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_location(test_support::test_location(ELSEWHERE.0, "Hallway"))
        // 7 + 3 ≥ 8 = max_health
        .with_seat(seat(1).at(HERE).hand([EMERGENCY_CACHE]).harm(7, 0));
    let builder = if seating.contains(&SURVIVOR) {
        builder.with_seat(seat(2).at(ELSEWHERE))
    } else {
        builder
    };
    builder
        .with_seating(seating.iter().copied())
        .open_turn(DYING)
        .with_enemy_engaged(ready_attacker(), DYING)
}

/// Play Emergency Cache, provoking the lethal attack of opportunity.
fn play_into_the_lethal_aoo(board: GameStateBuilder) -> TestSession {
    board.session().take(&TurnAction::PlayCard {
        investigator: DYING,
        source: PlaySource::Hand(0),
    })
}

#[test]
fn defeat_mid_turn_hands_the_turn_to_the_next_investigator() {
    let session = play_into_the_lethal_aoo(board(&[DYING, SURVIVOR]));
    let state = session.state();

    assert_eq!(state.investigators[&DYING].status, Status::Defeated);
    // 2.2.2 "return to 2.2": the next investigator who has not yet taken a turn.
    assert_eq!(
        state.active_investigator,
        Some(SURVIVOR),
        "the turn passes on rather than sticking with the dead investigator"
    );
    // The survivor's open turn is where play rests, not the dead investigator's.
    session.assert_at_turn_menu(SURVIVOR);
    assert_event!(
        session.events(),
        Event::TurnEnded { investigator } if *investigator == DYING
    );
}

#[test]
fn a_defeated_investigator_is_never_prompted_for_an_action() {
    let session = play_into_the_lethal_aoo(board(&[DYING, SURVIVOR]));

    // The acceptance criterion, stated positively so it cannot vacate: the engine
    // *does* ask for something, and the investigator it is addressed to is the
    // survivor rather than the corpse.
    let request = session.prompt();
    assert_eq!(
        session.state().active_investigator,
        Some(SURVIVOR),
        "the open turn belongs to the survivor, so the menu is addressed to them"
    );
    assert_eq!(
        request.target,
        Some(OptionTarget::TurnControl(SURVIVOR)),
        "it is the survivor's turn menu — the thing #764 wrongly showed the dead \
         investigator: {request:?}"
    );
    assert_ne!(
        session.state().active_investigator,
        Some(DYING),
        "a defeated investigator must never be re-prompted: {request:?}"
    );
}

#[test]
fn defeat_on_the_last_turn_of_the_round_ends_the_investigation_phase() {
    // The survivor has already taken their turn (they come first in
    // player order), so 2.2.2 finds nobody left and proceeds to 2.3 → Enemy.
    let session = play_into_the_lethal_aoo(board(&[SURVIVOR, DYING]));

    assert_eq!(
        session.state().investigators[&DYING].status,
        Status::Defeated
    );
    assert_ne!(
        session.state().phase,
        Phase::Investigation,
        "2.3: the investigation phase ends rather than idling on a dead turn"
    );
    assert_event!(
        session.events(),
        Event::PhaseEnded {
            phase: Phase::Investigation
        }
    );
}

#[test]
fn solo_defeat_ends_the_scenario_instead_of_rotating() {
    // ADR 0004: the `Lost` latch cancels the armed turn frame rather than
    // resuming it, so the arming never rotates a table with nobody left.
    let result = play_into_the_lethal_aoo(board(&[DYING])).finish();

    assert_eq!(result.state.investigators[&DYING].status, Status::Defeated);
    assert!(
        matches!(result.state.ending, Some(ScenarioEnding::NoResolution)),
        "RR p.10 step 6: no remaining players ⇒ the scenario ends"
    );
    // A stack invariant, so read off the stack: no turn frame is stranded
    // beneath the ending.
    assert!(
        !result
            .state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::InvestigatorTurn(_))),
        "the armed turn frame is cancelled, not resumed: {:?}",
        result.state.continuations,
    );
    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "nothing is asked of a table with no investigators left"
    );
}
