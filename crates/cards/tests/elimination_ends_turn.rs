//! A defeated active investigator's turn ends (#764), driven through the public
//! `apply` loop with the real card corpus installed.
//!
//! Before this, `apply_investigator_defeat` flipped the status and ran the
//! elimination steps but left the `InvestigatorTurn` frame on top with
//! `ending: false` — so `drive` re-enumerated the open turn and prompted the
//! investigator who had just been killed, with only `EndTurn` surviving
//! enumeration.
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
//! an attack of opportunity provoked by a non-fast play — and `max_health()`
//! reads the investigator's capacity from the installed corpus registry (#448).
//!
//! ## Verified card text (`data/arkhamdb-snapshot`, 2026-08-24)
//!
//! **Emergency Cache (01088):** "Gain 3 resources." A non-fast event, so playing
//! it is an action and provokes an attack of opportunity (RR p.5). No rulings —
//! it is listed in `data/arkhamdb-faq/no-rulings.txt`.

use game_core::event::Event;
use game_core::state::{
    CardCode, Continuation, Enemy, InvestigationResume, InvestigatorId, LocationId, Phase, Status,
};
use game_core::test_support::{
    take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder,
};
use game_core::{EngineOutcome, TurnAction};

/// Emergency Cache (01088): non-fast event → playing it provokes.
const EMERGENCY_CACHE: &str = "01088";
/// Skids O'Toole (01003): health 8 / sanity 6.
const SKIDS: &str = "01003";

const DYING: InvestigatorId = InvestigatorId(1);
const SURVIVOR: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(101);
const ELSEWHERE: LocationId = LocationId(102);

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// An enemy engaged with `inv`, ready, hitting for 3 damage.
fn engaged_attacker(inv: InvestigatorId) -> Enemy {
    let mut e = test_enemy(7, "Attacker");
    e.attack_damage = 3;
    e.attack_horror = 0;
    e.max_health = 5;
    e.current_location = Some(HERE);
    e.engaged_with = Some(inv);
    e
}

/// Investigator 1 on 7 damage (one attack of opportunity from dead), engaged,
/// holding Emergency Cache, with the open turn. `turn_order` decides whether the
/// rotation has anyone left to hand the turn to.
fn board(turn_order: &[InvestigatorId]) -> game_core::GameState {
    let mut dying = test_investigator(1);
    dying.investigator_card.code = CardCode::new(SKIDS);
    dying.investigator_card.accumulated_damage = 7; // 7 + 3 ≥ 8 = max_health
    dying.current_location = Some(HERE);
    dying.hand = vec![CardCode::new(EMERGENCY_CACHE)];

    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(HERE.0, "Study"))
        .with_location(test_location(ELSEWHERE.0, "Hallway"))
        .with_investigator(dying)
        .with_active_investigator(DYING)
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(DYING)
        .with_enemy(engaged_attacker(DYING))
        .build();

    if turn_order.contains(&SURVIVOR) {
        let mut survivor = test_investigator(2);
        survivor.investigator_card.code = CardCode::new(SKIDS);
        survivor.current_location = Some(ELSEWHERE);
        state.investigators.insert(SURVIVOR, survivor);
    }
    state.turn_order = turn_order.to_vec();
    state
}

/// Play Emergency Cache, provoking the lethal attack of opportunity.
fn play_into_the_lethal_aoo(state: game_core::GameState) -> game_core::ApplyResult {
    take_turn_action(
        state,
        &TurnAction::PlayCard {
            investigator: DYING,
            hand_index: 0,
        },
    )
}

#[test]
fn defeat_mid_turn_hands_the_turn_to_the_next_investigator() {
    let result = play_into_the_lethal_aoo(board(&[DYING, SURVIVOR]));
    let state = &result.state;

    assert_eq!(state.investigators[&DYING].status, Status::Killed);
    // 2.2.2 "return to 2.2": the next investigator who has not yet taken a turn.
    assert_eq!(
        state.active_investigator,
        Some(SURVIVOR),
        "the turn passes on rather than sticking with the dead investigator"
    );
    assert!(
        matches!(
            state.continuations.last(),
            Some(Continuation::InvestigatorTurn {
                investigator,
                ending: false,
            }) if *investigator == SURVIVOR
        ),
        "the survivor's open turn is on top, not the dead investigator's: {:?}",
        state.continuations,
    );
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, Event::TurnEnded { investigator } if *investigator == DYING)),
        "the turn ending is announced: {:?}",
        result.events,
    );
}

#[test]
fn a_defeated_investigator_is_never_prompted_for_an_action() {
    let result = play_into_the_lethal_aoo(board(&[DYING, SURVIVOR]));

    // The acceptance criterion, stated positively so it cannot vacate: the engine
    // *does* ask for something, and the investigator it is addressed to is the
    // survivor rather than the corpse.
    let EngineOutcome::AwaitingInput { request, .. } = &result.outcome else {
        panic!(
            "the survivor's turn must prompt for an action, got {:?}",
            result.outcome
        );
    };
    assert_eq!(
        result.state.active_investigator,
        Some(SURVIVOR),
        "the open turn belongs to the survivor, so the menu is addressed to them"
    );
    assert_eq!(
        request.prompt, "Choose an action",
        "it is a turn menu — the thing #764 wrongly showed the dead investigator"
    );
    assert_ne!(
        result.state.active_investigator,
        Some(DYING),
        "a defeated investigator must never be re-prompted: {request:?}"
    );
}

#[test]
fn defeat_on_the_last_turn_of_the_round_ends_the_investigation_phase() {
    // The survivor has already taken their turn (they come first in
    // `turn_order`), so 2.2.2 finds nobody left and proceeds to 2.3 → Enemy.
    let result = play_into_the_lethal_aoo(board(&[SURVIVOR, DYING]));

    assert_eq!(result.state.investigators[&DYING].status, Status::Killed);
    assert_ne!(
        result.state.phase,
        Phase::Investigation,
        "2.3: the investigation phase ends rather than idling on a dead turn"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::PhaseEnded {
                phase: Phase::Investigation
            }
        )),
        "PhaseEnded(Investigation) is emitted: {:?}",
        result.events,
    );
}

#[test]
fn solo_defeat_ends_the_scenario_instead_of_rotating() {
    // ADR 0004: the `Lost` latch cancels the armed turn frame rather than
    // resuming it, so the arming never rotates a table with nobody left.
    let result = play_into_the_lethal_aoo(board(&[DYING]));

    assert_eq!(result.state.investigators[&DYING].status, Status::Killed);
    assert!(
        matches!(
            result.state.resolution,
            Some(game_core::scenario::ScenarioEnding::NoResolution)
        ),
        "RR p.10 step 6: no remaining players ⇒ the scenario ends"
    );
    assert!(
        !result
            .state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::InvestigatorTurn { .. })),
        "the armed turn frame is cancelled, not resumed: {:?}",
        result.state.continuations,
    );
    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "nothing is asked of a table with no investigators left"
    );
}
