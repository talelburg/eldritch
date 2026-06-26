//! PR-8 (#306) integration: Dynamite Blast 01024's `Choose either your
//! location or a connecting location. Deal 3 damage to each enemy and to each
//! investigator at the chosen location.` end-to-end against the real
//! `cards::REGISTRY`.
//!
//! Exercises (a) the location choice — auto when there's one candidate, a
//! suspend/resume `Continuation::Choice` when there are 2+; (b) the area
//! damage over both enemies and investigators at the chosen location (and *not* the other
//! location); (c) self-damage when blasting your own location; (d) the
//! played-event being discarded on *completion* of a suspending `OnPlay`
//! (`pending_played_event`, RR Appendix I step 4) rather than stranded in hand.
//!
//! Own process → installs `cards::REGISTRY`.

use game_core::engine::EngineOutcome;
use game_core::state::{CardCode, EnemyId, InvestigatorId, LocationId, Phase};
use game_core::test_support::{
    take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder,
};
use game_core::{apply, Action, InputResponse, OptionId, PlayerAction, TurnAction};

const DYNAMITE: &str = "01024";
const INV: InvestigatorId = InvestigatorId(1);
const INV2: InvestigatorId = InvestigatorId(2);
const LOC_A: LocationId = LocationId(10);
const LOC_B: LocationId = LocationId(11);
const ENEMY_A: EnemyId = EnemyId(100);
const ENEMY_B: EnemyId = EnemyId(101);

#[ctor::ctor]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

fn play(state: game_core::GameState) -> game_core::engine::ApplyResult {
    take_turn_action(
        state,
        &TurnAction::PlayCard {
            investigator: INV,
            hand_index: 0,
        },
    )
}

fn pick(state: game_core::GameState, option: u32) -> game_core::engine::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(option)),
        }),
    )
}

/// Two connected locations. The controller (Dynamite in hand) and a 3-health
/// enemy sit at `LOC_A`; a second investigator and a 5-health enemy sit at the
/// connecting `LOC_B`.
fn board() -> game_core::GameState {
    let mut inv = test_investigator(1);
    inv.current_location = Some(LOC_A);
    inv.hand = vec![CardCode::new(DYNAMITE)];
    // Use Skids O'Toole (01003, 8 health / 6 sanity) — real code so
    // max_health()/max_sanity() can read from the installed cards registry
    // (#448 cp2a), no implemented abilities so no reaction windows fire.
    inv.investigator_card.code = CardCode::new("01003");

    let mut inv2 = test_investigator(2);
    inv2.current_location = Some(LOC_B);
    // Same reason as inv above.
    inv2.investigator_card.code = CardCode::new("01003");

    let mut loc_a = test_location(10, "Cellar");
    loc_a.connections = vec![LOC_B];
    let mut loc_b = test_location(11, "Hallway");
    loc_b.connections = vec![LOC_A];

    let mut enemy_a = test_enemy(100, "Ghoul A");
    enemy_a.current_location = Some(LOC_A);
    enemy_a.max_health = 3; // 3 damage defeats it
    let mut enemy_b = test_enemy(101, "Ghoul B");
    enemy_b.current_location = Some(LOC_B);
    enemy_b.max_health = 5; // survives, to assert it's untouched

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_investigator(inv2)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_enemy(enemy_a)
        .with_enemy(enemy_b)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .build()
}

#[test]
fn blasts_only_the_chosen_location_then_discards_the_event() {
    // Two candidates (your location + the connection) → suspend.
    let r = play(board());
    assert!(
        matches!(r.outcome, EngineOutcome::AwaitingInput { .. }),
        "2 candidate locations → choice suspends",
    );
    // The event has left hand ("commences being played") but isn't discarded yet.
    assert!(
        r.state.investigators[&INV].hand.is_empty(),
        "event left hand"
    );
    assert!(
        r.state.investigators[&INV].discard.is_empty(),
        "not discarded until the effect completes",
    );
    assert!(r.state.pending_played_event.is_some(), "event mid-play");

    // candidate_locations = [LOC_A, LOC_B] → OptionId(0) blasts LOC_A.
    let r = pick(r.state, 0);
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));

    // LOC_A: enemy defeated (3 dmg ≥ 3 health), controller took 3 (self-damage).
    assert!(
        !r.state.enemies.contains_key(&ENEMY_A),
        "enemy at the blasted location was defeated and removed",
    );
    assert_eq!(
        r.state.investigators[&INV].damage(),
        3,
        "the controller blasted its own location and took 3",
    );
    // LOC_B (not chosen): untouched.
    assert_eq!(
        r.state.enemies[&ENEMY_B].damage, 0,
        "enemy at LOC_B untouched"
    );
    assert_eq!(
        r.state.investigators[&INV2].damage(),
        0,
        "investigator at LOC_B untouched",
    );

    // Discard-on-completion (RR Appendix I step 4): the event is now discarded.
    assert_eq!(
        r.state.investigators[&INV].discard,
        vec![CardCode::new(DYNAMITE)],
        "event discarded when its effect completed",
    );
    assert!(
        r.state.pending_played_event.is_none(),
        "pending played-event flushed",
    );
}

#[test]
fn auto_targets_and_discards_when_your_location_is_the_only_candidate() {
    // A single location with no connections → one candidate → auto, no suspend.
    let mut inv = test_investigator(1);
    inv.current_location = Some(LOC_A);
    inv.hand = vec![CardCode::new(DYNAMITE)];
    // Real investigator code so max_health() reads from the cards registry (#448 cp2a).
    inv.investigator_card.code = CardCode::new("01003"); // Skids O'Toole: 8/6

    let loc_a = test_location(10, "Cellar"); // no connections
    let mut enemy_a = test_enemy(100, "Ghoul A");
    enemy_a.current_location = Some(LOC_A);
    enemy_a.max_health = 5; // survives, to assert the 3-damage amount

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_location(loc_a)
        .with_enemy(enemy_a)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .build();

    let r = play(state);
    // Returns to the open-turn menu; the damage assertions below prove the
    // blast resolved fully (a single candidate auto-binds — no target-pick
    // suspend).
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(r.state.enemies[&ENEMY_A].damage, 3, "enemy took 3");
    assert_eq!(r.state.investigators[&INV].damage(), 3, "controller took 3");
    assert_eq!(
        r.state.investigators[&INV].discard,
        vec![CardCode::new(DYNAMITE)],
        "event discarded",
    );
}
