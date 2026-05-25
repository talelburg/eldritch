//! Integration tests for #69 Mythos phase content.
//!
//! Drives full apply cycles through `StartScenario` → `Mulligan` →
//! `EndTurn` → `DrawEncounterCard`, verifying the per-card 5-step
//! sub-sequence, surge chain, and post-1.4 window behavior end-to-end.
//!
//! Lives in `crates/scenarios/tests/` because:
//!
//! - The `cards`-crate dependency direction prevents `game-core` unit
//!   tests from constructing real card-shaped registries.
//! - `card_registry::install` is process-global — an integration test
//!   binary gets its own process, so this install doesn't collide with
//!   `cards::REGISTRY` installs in other test binaries.
//!
//! We install [`TEST_REGISTRY`] (the synthetic card registry) but
//! intentionally do **not** install the scenario registry. The
//! synthetic `setup()` state carries `scenario_id = "synthetic"`, but
//! `apply()` short-circuits the resolution hook when no registry is
//! installed — preventing the synthetic `detect_resolution` (which
//! fires on round >= 1, phase Investigation) from triggering on round
//! 2's Investigation entry and obscuring the assertions.

use std::sync::Once;

use game_core::card_data::CardType;
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{CardCode, InvestigatorId, LocationId, Phase};
use game_core::{assert_event, assert_event_sequence, Action, PlayerAction};
use scenarios::test_fixtures::synth_cards::{
    SYNTH_ENEMY_CODE, SYNTH_SURGE_TREACHERY_CODE, SYNTH_TREACHERY_CODE, TEST_REGISTRY,
};
use scenarios::test_fixtures::synthetic;

static INSTALL: Once = Once::new();

fn install_test_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

/// Drive a sequence of actions from an initial state, collecting all
/// events. Returns the final state and the concatenation of all event
/// vecs.
fn drive(
    initial_state: game_core::state::GameState,
    actions: Vec<Action>,
) -> (game_core::state::GameState, Vec<Event>) {
    let mut state = initial_state;
    let mut all_events = Vec::new();
    for action in actions {
        let result = apply(state, action);
        all_events.extend(result.events);
        state = result.state;
    }
    (state, all_events)
}

/// Build the standard single-investigator sequence up to the point
/// where `DrawEncounterCard` is the next expected action.
///
/// Returns the state after `EndTurn` has ticked through all phases
/// and landed in Mythos with `mythos_draw_pending = Some(InvestigatorId(1))`.
fn setup_at_mythos_draw(state: game_core::state::GameState) -> game_core::state::GameState {
    let inv1 = InvestigatorId(1);
    let (state, _) = drive(
        state,
        vec![
            // Round 1 begins; mulligan window opens.
            Action::Player(PlayerAction::StartScenario),
            // Close mulligan window (empty redraw = keep hand).
            Action::Player(PlayerAction::Mulligan {
                investigator: inv1,
                indices_to_redraw: vec![],
            }),
            // Sole investigator ends their turn → auto-advance through
            // Investigation → Enemy → Upkeep → Mythos (round 2).
            // Pauses with mythos_draw_pending = Some(inv1).
            Action::Player(PlayerAction::EndTurn),
        ],
    );
    state
}

// ------------------------------------------------------------------
// Single-treachery happy path
// ------------------------------------------------------------------

#[test]
fn mythos_phase_resolves_single_treachery() {
    install_test_registry();
    let mut base = synthetic::setup();
    // Deck: exactly one synth treachery (already seeded by setup()).
    // Ensure discard is empty.
    base.encounter_discard.clear();

    let state = setup_at_mythos_draw(base);
    assert_eq!(state.phase, Phase::Mythos, "must be in Mythos before draw");
    assert_eq!(state.mythos_draw_pending, Some(InvestigatorId(1)));

    let result = apply(state, Action::Player(PlayerAction::DrawEncounterCard));

    assert_eq!(result.outcome, EngineOutcome::Done);
    // Mythos → Investigation transition completes inline (MythosAfterDraws
    // auto-closes because no fast-play-eligible cards are in any hand).
    assert_eq!(result.state.phase, Phase::Investigation);
    assert_eq!(result.state.round, 2);
    assert!(
        result.state.encounter_deck.is_empty(),
        "deck must be empty after draw"
    );
    assert!(
        result
            .state
            .encounter_discard
            .contains(&CardCode(SYNTH_TREACHERY_CODE.into())),
        "treachery must be in discard after Revelation resolves",
    );
    assert_eq!(
        result.state.mythos_draw_pending, None,
        "cursor must be cleared once all investigators have drawn",
    );
    assert_eq!(
        result.state.active_investigator,
        Some(InvestigatorId(1)),
        "investigation_phase rotates to the lead investigator",
    );

    // CardRevealed fires for the synth treachery.
    assert_event!(
        result.events,
        Event::CardRevealed { investigator, code, card_type }
            if *investigator == InvestigatorId(1)
                && *code == CardCode(SYNTH_TREACHERY_CODE.into())
                && *card_type == CardType::Treachery
    );
}

// ------------------------------------------------------------------
// Surge chain
// ------------------------------------------------------------------

#[test]
fn mythos_phase_surge_chains_into_next_card() {
    install_test_registry();
    let mut base = synthetic::setup();
    // Deck: surge treachery on top, plain treachery below.
    synthetic::with_encounter_deck(
        &mut base,
        vec![
            CardCode(SYNTH_SURGE_TREACHERY_CODE.into()),
            CardCode(SYNTH_TREACHERY_CODE.into()),
        ],
    );

    let state = setup_at_mythos_draw(base);
    assert_eq!(state.phase, Phase::Mythos);

    let result = apply(state, Action::Player(PlayerAction::DrawEncounterCard));

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert!(
        result.state.encounter_deck.is_empty(),
        "both cards consumed by surge chain"
    );
    assert_eq!(
        result.state.encounter_discard.len(),
        2,
        "both treacheries must be in discard after surge chain",
    );
    assert_eq!(result.state.phase, Phase::Investigation);

    // Both cards were revealed; surge treachery first.
    assert_event_sequence!(
        result.events,
        Event::CardRevealed { code, .. }
            if *code == CardCode(SYNTH_SURGE_TREACHERY_CODE.into()),
        Event::CardRevealed { code, .. }
            if *code == CardCode(SYNTH_TREACHERY_CODE.into()),
    );
}

// ------------------------------------------------------------------
// Spawn enemy via Mythos
// ------------------------------------------------------------------

#[test]
fn mythos_phase_resolves_single_spawn_enemy() {
    install_test_registry();
    let mut base = synthetic::setup();
    // Place the investigator at the synth location so the spawn engages them.
    base.investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));
    // Deck: synth enemy (already placed at LocationId(10) = synth loc via SYNTH_LOC_CODE).
    synthetic::with_encounter_deck(&mut base, vec![CardCode(SYNTH_ENEMY_CODE.into())]);

    let state = setup_at_mythos_draw(base);
    assert_eq!(state.phase, Phase::Mythos);

    let result = apply(state, Action::Player(PlayerAction::DrawEncounterCard));

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(
        result.state.enemies.len(),
        1,
        "one enemy must be in play after spawn",
    );
    let enemy = result.state.enemies.values().next().unwrap();
    assert_eq!(enemy.current_location, Some(LocationId(10)));
    assert_eq!(enemy.engaged_with, Some(InvestigatorId(1)));
    assert!(
        !result
            .state
            .encounter_discard
            .contains(&CardCode(SYNTH_ENEMY_CODE.into())),
        "spawned enemy must not be in encounter_discard",
    );
    assert_eq!(result.state.phase, Phase::Investigation);

    // EnemySpawned event fired.
    assert_event!(
        result.events,
        Event::EnemySpawned { code, location, engaged_with, .. }
            if *code == CardCode(SYNTH_ENEMY_CODE.into())
                && *location == LocationId(10)
                && *engaged_with == Some(InvestigatorId(1))
    );
}

// ------------------------------------------------------------------
// Multi-investigator player order
// ------------------------------------------------------------------

#[test]
fn mythos_phase_multi_investigator_player_order() {
    install_test_registry();
    // Build a two-investigator state manually, starting from setup()
    // (which gives us inv1 + the synth location).
    let mut base = synthetic::setup();
    let mut inv2 = game_core::test_support::test_investigator(2);
    inv2.current_location = Some(LocationId(10));
    base.investigators.insert(InvestigatorId(2), inv2);
    base.turn_order.push(InvestigatorId(2));

    // Two treacheries — one per investigator.
    synthetic::with_encounter_deck(
        &mut base,
        vec![
            CardCode(SYNTH_TREACHERY_CODE.into()),
            CardCode(SYNTH_TREACHERY_CODE.into()),
        ],
    );

    let inv1 = InvestigatorId(1);
    let inv2 = InvestigatorId(2);

    // StartScenario + mulligan both investigators.
    let (state, _) = drive(
        base,
        vec![
            Action::Player(PlayerAction::StartScenario),
            Action::Player(PlayerAction::Mulligan {
                investigator: inv1,
                indices_to_redraw: vec![],
            }),
            Action::Player(PlayerAction::Mulligan {
                investigator: inv2,
                indices_to_redraw: vec![],
            }),
            // inv1 ends turn → rotates to inv2.
            Action::Player(PlayerAction::EndTurn),
            // inv2 is the last in turn_order → ticks through phases into Mythos.
            Action::Player(PlayerAction::EndTurn),
        ],
    );

    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(state.mythos_draw_pending, Some(inv1), "inv1 draws first");

    // inv1 draws their card.
    let result1 = apply(state, Action::Player(PlayerAction::DrawEncounterCard));
    assert_eq!(result1.outcome, EngineOutcome::Done);
    // Still in Mythos; inv2 must draw next.
    assert_eq!(result1.state.phase, Phase::Mythos);
    assert_eq!(result1.state.mythos_draw_pending, Some(inv2));

    // inv2 draws their card → completes the phase.
    let result2 = apply(
        result1.state,
        Action::Player(PlayerAction::DrawEncounterCard),
    );
    assert_eq!(result2.outcome, EngineOutcome::Done);
    assert_eq!(result2.state.mythos_draw_pending, None);
    assert_eq!(result2.state.phase, Phase::Investigation);
    assert!(result2.state.encounter_deck.is_empty());
    assert_eq!(result2.state.encounter_discard.len(), 2);
}

// ------------------------------------------------------------------
// Full round chain (round counter bump)
// ------------------------------------------------------------------

#[test]
fn mythos_phase_full_round_chain() {
    install_test_registry();
    let base = synthetic::setup();
    // Deck already seeded with one synth treachery by setup().

    let state = setup_at_mythos_draw(base);
    // Confirm the round bumped on Mythos entry.
    assert_eq!(state.round, 2);
    assert_eq!(state.phase, Phase::Mythos);

    let result = apply(state, Action::Player(PlayerAction::DrawEncounterCard));

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(
        result.state.round, 2,
        "round stays 2 — it bumps on Mythos *entry*"
    );
    assert_eq!(result.state.phase, Phase::Investigation);
    assert_eq!(result.state.active_investigator, Some(InvestigatorId(1)));
}

// ------------------------------------------------------------------
// Empty-deck rejection
// ------------------------------------------------------------------

#[test]
fn mythos_draw_rejects_when_initial_deck_and_discard_both_empty() {
    install_test_registry();
    let mut base = synthetic::setup();
    // Drain the seeded deck and ensure discard is also empty.
    base.encounter_deck.clear();
    base.encounter_discard.clear();

    let state = setup_at_mythos_draw(base);
    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(state.mythos_draw_pending, Some(InvestigatorId(1)));

    let result = apply(state, Action::Player(PlayerAction::DrawEncounterCard));

    match result.outcome {
        EngineOutcome::Rejected { reason } => {
            assert!(
                reason.contains("encounter deck and discard both empty"),
                "unexpected reject reason: {reason:?}",
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }

    // Validate-first: state must be unchanged — still Mythos, cursor preserved.
    assert_eq!(
        result.state.phase,
        Phase::Mythos,
        "phase must not change on Rejected",
    );
    assert_eq!(
        result.state.mythos_draw_pending,
        Some(InvestigatorId(1)),
        "cursor must be preserved on initial Rejected (validate-first)",
    );
    assert!(
        result.events.is_empty(),
        "no events should fire on empty-deck reject; got {:?}",
        result.events,
    );
}
