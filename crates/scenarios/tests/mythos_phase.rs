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
//! intentionally do **not** install the scenario registry. Under the
//! push-model latch the synthetic fixture only resolves when an
//! act/agenda resolution point is reached, which these Mythos-phase
//! draws never trigger — so the scenario resolution hook stays
//! dormant regardless.

use std::sync::Once;

use game_core::card_data::CardType;
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{CardCode, InvestigatorId, LocationId, Phase, WindowKind};
use game_core::{assert_event, assert_event_sequence, Action, InputResponse, PlayerAction};
use scenarios::test_fixtures::synth_cards::{
    SYNTH_ENEMY_CODE, SYNTH_FAST_EVENT_CODE, SYNTH_SURGE_TREACHERY_CODE, SYNTH_TREACHERY_CODE,
    TEST_REGISTRY,
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

#[test]
fn mythos_phase_multi_investigator_spawn_suspends_then_resumes_chain() {
    // Two investigators co-located at the synth spawn location: the
    // drawn enemy ties under Prey::Default, so the draw suspends for the
    // lead's PickInvestigator (#128, option A). Resolving the pick
    // engages the chosen investigator and resumes inv1's Mythos draw
    // chain — which, the enemy being non-surge, advances the cursor to
    // inv2 and stays in Mythos.
    install_test_registry();
    let mut base = synthetic::setup();
    base.investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));
    let mut inv2 = game_core::test_support::test_investigator(2);
    inv2.current_location = Some(LocationId(10));
    base.investigators.insert(InvestigatorId(2), inv2);
    base.turn_order.push(InvestigatorId(2));

    // inv1 draws the enemy; inv2 draws a plain treachery afterward.
    synthetic::with_encounter_deck(
        &mut base,
        vec![
            CardCode(SYNTH_ENEMY_CODE.into()),
            CardCode(SYNTH_TREACHERY_CODE.into()),
        ],
    );

    // Drive both investigators through setup into Mythos (mirrors
    // `mythos_phase_multi_investigator_player_order`; `setup_at_mythos_draw`
    // only mulligans inv1 and so can't seat a second investigator).
    let (state, _) = drive(
        base,
        vec![
            Action::Player(PlayerAction::StartScenario),
            Action::Player(PlayerAction::Mulligan {
                investigator: InvestigatorId(1),
                indices_to_redraw: vec![],
            }),
            Action::Player(PlayerAction::Mulligan {
                investigator: InvestigatorId(2),
                indices_to_redraw: vec![],
            }),
            Action::Player(PlayerAction::EndTurn),
            Action::Player(PlayerAction::EndTurn),
        ],
    );
    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(state.mythos_draw_pending, Some(InvestigatorId(1)));

    // Draw → spawn tie → suspend.
    let suspended = apply(state, Action::Player(PlayerAction::DrawEncounterCard));
    assert!(
        matches!(suspended.outcome, EngineOutcome::AwaitingInput { .. }),
        "spawn tie must suspend, got {:?}",
        suspended.outcome,
    );
    assert!(suspended.state.spawn_engage_pending.is_some());
    let enemy = suspended
        .state
        .enemies
        .values()
        .next()
        .expect("enemy placed");
    assert_eq!(enemy.engaged_with, None, "engagement deferred");
    // The cursor is unchanged — still mid-chain for inv1.
    assert_eq!(suspended.state.mythos_draw_pending, Some(InvestigatorId(1)));

    // Lead picks inv2 → engage + resume the chain. The enemy is
    // non-surge, so no further card draws; the chain advances to inv2.
    let resumed = apply(
        suspended.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickInvestigator(InvestigatorId(2)),
        }),
    );
    assert_eq!(resumed.outcome, EngineOutcome::Done);
    assert!(resumed.state.spawn_engage_pending.is_none());
    let enemy = resumed
        .state
        .enemies
        .values()
        .next()
        .expect("enemy still placed");
    assert_eq!(
        enemy.engaged_with,
        Some(InvestigatorId(2)),
        "the lead's pick is now engaged",
    );
    // Chain resumed and advanced to inv2; still in Mythos.
    assert_eq!(resumed.state.phase, Phase::Mythos);
    assert_eq!(resumed.state.mythos_draw_pending, Some(InvestigatorId(2)));
    assert_event!(
        resumed.events,
        Event::EnemyEngaged { investigator, .. }
            if *investigator == InvestigatorId(2)
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
// Multi-investigator surge isolation
// ------------------------------------------------------------------

#[test]
fn mythos_phase_multi_investigator_surge_does_not_spill() {
    // Verifies that a surge in inv1's draw chain resolves entirely within
    // inv1's DrawEncounterCard apply — consuming two cards from the shared
    // encounter deck — without disrupting inv2's subsequent draw.
    //
    // Encounter deck (top → bottom):
    //   [SYNTH_SURGE_TREACHERY, SYNTH_TREACHERY, SYNTH_TREACHERY]
    //
    // Drive: StartScenario → mulligans → EndTurn(inv1) → EndTurn(inv2)
    //        → DrawEncounterCard(inv1) → DrawEncounterCard(inv2)
    //
    // Expected:
    //   - inv1's DrawEncounterCard: draws the surge treachery, which
    //     triggers an immediate chain-draw of the next card (plain
    //     treachery). Both cards resolve, 2× CardRevealed, discard grows
    //     by 2. Still Mythos after; mythos_draw_pending = Some(inv2).
    //   - inv2's DrawEncounterCard: draws the third card (plain treachery),
    //     1× CardRevealed, discard grows by 1 more (3 total).
    //     Phase transitions to Investigation; mythos_draw_pending = None.
    install_test_registry();

    let mut base = synthetic::setup();
    let mut inv2 = game_core::test_support::test_investigator(2);
    inv2.current_location = Some(LocationId(10));
    base.investigators.insert(InvestigatorId(2), inv2);
    base.turn_order.push(InvestigatorId(2));

    // Deck: surge on top, then two plain treacheries.
    synthetic::with_encounter_deck(
        &mut base,
        vec![
            CardCode(SYNTH_SURGE_TREACHERY_CODE.into()),
            CardCode(SYNTH_TREACHERY_CODE.into()),
            CardCode(SYNTH_TREACHERY_CODE.into()),
        ],
    );

    let inv1 = InvestigatorId(1);
    let inv2 = InvestigatorId(2);

    // StartScenario + mulligan both investigators + both EndTurns.
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
            // inv2 is last in turn_order → auto-advances into Mythos.
            Action::Player(PlayerAction::EndTurn),
        ],
    );

    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(state.mythos_draw_pending, Some(inv1), "inv1 draws first");

    // inv1 draws: surge chain pulls TWO cards (surge + plain treachery).
    let result1 = apply(state, Action::Player(PlayerAction::DrawEncounterCard));
    assert_eq!(result1.outcome, EngineOutcome::Done);
    // The surge chain resolves within inv1's single apply; still Mythos
    // because inv2 still needs to draw.
    assert_eq!(result1.state.phase, Phase::Mythos);
    assert_eq!(
        result1.state.mythos_draw_pending,
        Some(inv2),
        "cursor advances to inv2 after inv1's chain completes"
    );
    assert_eq!(
        result1.state.encounter_discard.len(),
        2,
        "surge + plain treachery both discarded after inv1's chain"
    );
    assert_eq!(
        result1.state.encounter_deck.len(),
        1,
        "one card remains for inv2"
    );
    // Both cards emitted CardRevealed attributed to inv1.
    assert_event!(
        result1.events,
        Event::CardRevealed { investigator, code, .. }
            if *investigator == inv1
                && *code == CardCode(SYNTH_SURGE_TREACHERY_CODE.into())
    );
    assert_event!(
        result1.events,
        Event::CardRevealed { investigator, code, .. }
            if *investigator == inv1
                && *code == CardCode(SYNTH_TREACHERY_CODE.into())
    );

    // inv2 draws: one plain treachery, no surge.
    let result2 = apply(
        result1.state,
        Action::Player(PlayerAction::DrawEncounterCard),
    );
    assert_eq!(result2.outcome, EngineOutcome::Done);
    assert_eq!(result2.state.phase, Phase::Investigation);
    assert_eq!(result2.state.mythos_draw_pending, None);
    assert!(result2.state.encounter_deck.is_empty());
    assert_eq!(
        result2.state.encounter_discard.len(),
        3,
        "all three cards in discard after both investigators draw"
    );
    assert_event!(
        result2.events,
        Event::CardRevealed { investigator, code, .. }
            if *investigator == inv2
                && *code == CardCode(SYNTH_TREACHERY_CODE.into())
    );
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

// ------------------------------------------------------------------
// Fast-window push-then-scan fix (defect A + B from pre-PR review)
// ------------------------------------------------------------------

/// Regression test for the push-then-scan ordering fix in
/// `open_fast_window`. Before the fix, `any_fast_play_eligible` was
/// called BEFORE the `MythosAfterDraws` window was pushed onto
/// `state.open_windows`, so `check_play_card`'s `permissive_window`
/// check saw an empty stack and evaluated every Fast card as
/// ineligible. The window would auto-skip even when the player had a
/// Fast event in hand — silently denying plays in the post-1.4 window.
///
/// This test puts a synthetic Fast event in inv1's hand, drives
/// `DrawEncounterCard` to trigger `open_fast_window`, and asserts that
/// the window STAYS OPEN (not auto-skipped): `WindowOpened` is emitted
/// but `WindowClosed` is NOT, and the window is on `state.open_windows`.
#[test]
fn mythos_after_draws_window_stays_open_when_fast_event_in_hand() {
    install_test_registry();
    let base = synthetic::setup();

    let mut state = setup_at_mythos_draw(base);
    // Insert the synthetic Fast event into inv1's hand AFTER setup so it
    // doesn't interact with the player-deck draw during StartScenario.
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .expect("inv1 must be present")
        .hand
        .push(CardCode(SYNTH_FAST_EVENT_CODE.into()));

    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(state.mythos_draw_pending, Some(InvestigatorId(1)));

    let result = apply(state, Action::Player(PlayerAction::DrawEncounterCard));

    // DrawEncounterCard should complete normally (Done) — the window
    // opens but doesn't block the draw action's completion.
    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "DrawEncounterCard should return Done even when window stays open"
    );

    // The window must remain on the stack — it was NOT auto-skipped.
    assert!(
        !result.state.open_windows.is_empty(),
        "MythosAfterDraws window must stay on stack when Fast event is in hand; \
         pre-fix this would have been empty (window auto-skipped)"
    );
    assert!(
        matches!(
            result.state.open_windows.last(),
            Some(w) if w.kind == WindowKind::MythosAfterDraws
        ),
        "top open window must be MythosAfterDraws; got {:?}",
        result.state.open_windows.last()
    );

    // Phase must still be Mythos — the window continuation (mythos_phase_end)
    // has NOT fired yet.
    assert_eq!(
        result.state.phase,
        Phase::Mythos,
        "phase must still be Mythos while MythosAfterDraws window is open"
    );

    // WindowOpened must be in the event stream.
    assert_event!(
        result.events,
        Event::WindowOpened {
            kind: WindowKind::MythosAfterDraws
        }
    );

    // WindowClosed must NOT be in the event stream — the window is still open.
    assert!(
        !result.events.iter().any(|e| matches!(
            e,
            Event::WindowClosed {
                kind: WindowKind::MythosAfterDraws
            }
        )),
        "WindowClosed(MythosAfterDraws) must not fire while window is open; \
         pre-fix this would have been emitted (window incorrectly auto-skipped)"
    );
}

/// Continuation of the push-then-scan regression: after `DrawEncounterCard`
/// leaves the `MythosAfterDraws` window open (because a Fast event is in
/// hand), `ResolveInput::Skip` must close the window, run
/// `mythos_phase_end`, and transition to Investigation.
///
/// Before the fix for defect B, `resolve_input`'s Skip arm used
/// `top_reaction_window_index()` which filters out empty-`pending_triggers`
/// windows. A pure-Fast `MythosAfterDraws` window would not be found, and
/// `Skip` would reject with "no `AwaitingInput` prompt is currently
/// outstanding" — leaving the window stuck on the stack forever.
#[test]
fn mythos_after_draws_window_closed_by_skip_and_transitions_to_investigation() {
    install_test_registry();
    let base = synthetic::setup();

    let mut state = setup_at_mythos_draw(base);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .expect("inv1 must be present")
        .hand
        .push(CardCode(SYNTH_FAST_EVENT_CODE.into()));

    // Advance through the draw to land in the open-window state.
    let draw_result = apply(state, Action::Player(PlayerAction::DrawEncounterCard));
    assert_eq!(draw_result.outcome, EngineOutcome::Done);
    assert!(
        !draw_result.state.open_windows.is_empty(),
        "window must be open before Skip test"
    );

    // Now close the window with Skip (player decides not to play the Fast card).
    let skip_result = apply(
        draw_result.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );

    assert_eq!(
        skip_result.outcome,
        EngineOutcome::Done,
        "Skip must return Done after closing MythosAfterDraws"
    );

    // MythosAfterDraws is closed; investigation_phase then opens
    // InvestigationBegins. Because there's a Fast card in hand,
    // InvestigationBegins does NOT auto-skip — it stays on the stack.
    // (Defect B was: MythosAfterDraws could NOT be closed via Skip at all;
    // that defect is still fixed — only the old "open_windows empty" shape
    // changes now that investigation_phase opens its own window.)
    assert_eq!(
        skip_result.state.open_windows.len(),
        1,
        "InvestigationBegins window must be open (Fast card is eligible); \
         MythosAfterDraws must be gone"
    );
    assert!(
        skip_result
            .state
            .open_windows
            .last()
            .is_some_and(|w| w.kind == WindowKind::InvestigationBegins),
        "top window must be InvestigationBegins; got {:?}",
        skip_result.state.open_windows.last()
    );

    // mythos_phase_end ran: phase transitioned to Investigation.
    // active_investigator is None until InvestigationBegins closes
    // (its continuation begin_investigator_turn rotates to the lead).
    assert_eq!(
        skip_result.state.phase,
        Phase::Investigation,
        "phase must be Investigation after MythosAfterDraws window closes"
    );
    assert_eq!(
        skip_result.state.active_investigator, None,
        "active investigator not yet set — InvestigationBegins window is still open"
    );
    assert_eq!(
        skip_result.state.round, 2,
        "round stays 2 — it bumped on Mythos entry"
    );

    // WindowClosed event for MythosAfterDraws must be in the stream.
    assert_event!(
        skip_result.events,
        Event::WindowClosed {
            kind: WindowKind::MythosAfterDraws
        }
    );
    // WindowOpened event for InvestigationBegins must also be present.
    assert_event!(
        skip_result.events,
        Event::WindowOpened {
            kind: WindowKind::InvestigationBegins
        }
    );
}
