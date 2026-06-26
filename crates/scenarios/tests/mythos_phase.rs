//! Integration tests for #69 Mythos phase content.
//!
//! Drives full apply cycles through `seat_and_open` → `Mulligan` →
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

use game_core::action::RosterEntry;
use game_core::card_data::CardType;
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::seat_and_open;
use game_core::state::{
    CardCode, Continuation, FastWindowKind, InvestigatorId, LocationId, Phase, PhaseStep,
};
use game_core::test_support::{take_turn_action, TEST_INV};
use game_core::{
    assert_event, assert_event_sequence, Action, InputResponse, PlayerAction, TurnAction,
};
use scenarios::test_fixtures::synth_cards::{
    SYNTH_ENEMY_CODE, SYNTH_FAST_EVENT_CODE, SYNTH_SURGE_TREACHERY_CODE, SYNTH_TREACHERY_CODE,
    TEST_REGISTRY,
};
use scenarios::test_fixtures::synthetic;

#[ctor::ctor]
fn install_test_registry() {
    let _ = game_core::card_registry::install(TEST_REGISTRY);
}

/// Build the standard single-investigator sequence up to the point
/// where `DrawEncounterCard` is the next expected action.
///
/// Returns the state after `EndTurn` has ticked through all phases
/// and landed in Mythos with `mythos_draw_pending = Some(InvestigatorId(1))`.
fn setup_at_mythos_draw(state: game_core::state::GameState) -> game_core::state::GameState {
    let roster = vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck: vec![],
    }];
    // seat_and_open opens the mulligan prompt; close it (keep hand).
    let mut state = seat_and_open(state, &roster).state;
    state = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
    .state;
    // Sole investigator ends their turn → auto-advance through
    // Investigation → Enemy → Upkeep → Mythos (round 2).
    // Pauses with mythos_draw_pending = Some(inv1).
    take_turn_action(state, &TurnAction::EndTurn).state
}

// ------------------------------------------------------------------
// Single-treachery happy path
// ------------------------------------------------------------------

#[test]
fn mythos_phase_resolves_single_treachery() {
    let mut base = synthetic::setup();
    // Deck: exactly one synth treachery (already seeded by setup()).
    // Ensure discard is empty.
    base.encounter_discard.clear();

    let state = setup_at_mythos_draw(base);
    assert_eq!(state.phase, Phase::Mythos, "must be in Mythos before draw");
    assert_eq!(state.current_encounter_drawer(), Some(InvestigatorId(1)));

    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
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
        result.state.current_encounter_drawer(),
        None,
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
    let base = synthetic::setup();

    let mut state = setup_at_mythos_draw(base);
    assert_eq!(state.phase, Phase::Mythos);

    // Seed the controlled draw order *after* seat_and_open's shuffle:
    // surge treachery on top, plain treachery below.
    synthetic::with_encounter_deck(
        &mut state,
        vec![
            CardCode(SYNTH_SURGE_TREACHERY_CODE.into()),
            CardCode(SYNTH_TREACHERY_CODE.into()),
        ],
    );

    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
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
    // seat_and_open (called inside setup_at_mythos_draw) places the investigator
    // at starting_location = LocationId(10), so the spawned enemy engages them.
    let mut base = synthetic::setup();
    // Deck: synth enemy (placed at LocationId(10) = synth loc via SYNTH_LOC_CODE).
    synthetic::with_encounter_deck(&mut base, vec![CardCode(SYNTH_ENEMY_CODE.into())]);

    let state = setup_at_mythos_draw(base);
    assert_eq!(state.phase, Phase::Mythos);

    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
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
#[allow(clippy::too_many_lines)] // end-to-end multi-investigator spawn-suspend walkthrough
fn mythos_phase_multi_investigator_spawn_suspends_then_resumes_chain() {
    // Two investigators co-located at the synth spawn location: the
    // drawn enemy ties under Prey::Default, so the draw suspends for the
    // lead's PickSingle (#128, option A). Resolving the pick
    // engages the chosen investigator and resumes inv1's Mythos draw
    // chain — which, the enemy being non-surge, advances the cursor to
    // inv2 and stays in Mythos.
    // Both investigators are seated at LocationId(10) (starting_location) by
    // seat_and_open; no pre-seating needed.
    let base = synthetic::setup();

    // Seat both investigators and drive through setup into Mythos.
    let roster = vec![
        RosterEntry {
            investigator: CardCode::new(TEST_INV),
            deck: vec![],
        },
        RosterEntry {
            investigator: CardCode::new(TEST_INV),
            deck: vec![],
        },
    ];
    let mut state = seat_and_open(base, &roster).state;
    // Close mulligan for both investigators.
    state = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
    .state;
    state = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
    .state;
    // inv1 ends turn → rotates to inv2.
    state = take_turn_action(state, &TurnAction::EndTurn).state;
    // inv2 is the last in turn_order → ticks through phases into Mythos.
    state = take_turn_action(state, &TurnAction::EndTurn).state;
    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(state.current_encounter_drawer(), Some(InvestigatorId(1)));

    // Seed the controlled draw order *after* seat_and_open's shuffle:
    // inv1 draws the enemy; inv2 draws a plain treachery afterward.
    synthetic::with_encounter_deck(
        &mut state,
        vec![
            CardCode(SYNTH_ENEMY_CODE.into()),
            CardCode(SYNTH_TREACHERY_CODE.into()),
        ],
    );

    // Draw → spawn tie → suspend.
    let suspended = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert!(
        matches!(suspended.outcome, EngineOutcome::AwaitingInput { .. }),
        "spawn tie must suspend, got {:?}",
        suspended.outcome,
    );
    assert!(matches!(
        suspended.state.continuations.last(),
        Some(game_core::state::Continuation::SpawnEngage(_))
    ));
    let enemy = suspended
        .state
        .enemies
        .values()
        .next()
        .expect("enemy placed");
    assert_eq!(enemy.engaged_with, None, "engagement deferred");
    // The cursor is unchanged — still mid-chain for inv1.
    assert_eq!(
        suspended.state.current_encounter_drawer(),
        Some(InvestigatorId(1))
    );

    // Lead picks inv2 (by its offered option id) → engage + resume the chain.
    // The enemy is non-surge, so no further card draws; the chain advances to inv2.
    let pick = {
        let EngineOutcome::AwaitingInput { request, .. } = &suspended.outcome else {
            unreachable!("asserted AwaitingInput above");
        };
        request
            .options
            .iter()
            .find(|o| o.label == format!("{:?}", InvestigatorId(2)))
            .expect("InvestigatorId(2) among offered options")
            .id
    };
    let resumed = apply(
        suspended.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(pick),
        }),
    );
    // Picking engages inv2 and re-enters inv1's chain, which completes; the
    // loop then drains inv1 and re-prompts inv2 (AwaitingInput).
    assert!(matches!(
        resumed.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert!(!matches!(
        resumed.state.continuations.last(),
        Some(game_core::state::Continuation::SpawnEngage(_))
    ));
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
    assert_eq!(
        resumed.state.current_encounter_drawer(),
        Some(InvestigatorId(2))
    );
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
    // Build a two-investigator state from setup() (both seated at
    // starting_location by seat_and_open).
    let mut base = synthetic::setup();

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

    // Seat both investigators and mulligan each.
    let roster = vec![
        RosterEntry {
            investigator: CardCode::new(TEST_INV),
            deck: vec![],
        },
        RosterEntry {
            investigator: CardCode::new(TEST_INV),
            deck: vec![],
        },
    ];
    let mut state = seat_and_open(base, &roster).state;
    state = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
    .state;
    state = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
    .state;
    // inv1 ends turn → rotates to inv2.
    state = take_turn_action(state, &TurnAction::EndTurn).state;
    // inv2 is the last in turn_order → ticks through phases into Mythos.
    state = take_turn_action(state, &TurnAction::EndTurn).state;

    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(
        state.current_encounter_drawer(),
        Some(inv1),
        "inv1 draws first"
    );

    // inv1 draws their card → the loop re-prompts inv2 (AwaitingInput).
    let result1 = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert!(matches!(
        result1.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    // Still in Mythos; inv2 must draw next.
    assert_eq!(result1.state.phase, Phase::Mythos);
    assert_eq!(result1.state.current_encounter_drawer(), Some(inv2));

    // inv2 draws their card → completes the phase.
    let result2 = apply(
        result1.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert!(matches!(
        result2.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(result2.state.current_encounter_drawer(), None);
    assert_eq!(result2.state.phase, Phase::Investigation);
    assert!(result2.state.encounter_deck.is_empty());
    assert_eq!(result2.state.encounter_discard.len(), 2);
}

// ------------------------------------------------------------------
// Full round chain (round counter bump)
// ------------------------------------------------------------------

#[test]
fn mythos_phase_full_round_chain() {
    let base = synthetic::setup();
    // Deck already seeded with one synth treachery by setup().

    let state = setup_at_mythos_draw(base);
    // Confirm the round bumped on Mythos entry.
    assert_eq!(state.round, 2);
    assert_eq!(state.phase, Phase::Mythos);

    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
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
#[allow(clippy::too_many_lines)] // end-to-end multi-investigator surge-isolation walkthrough
fn mythos_phase_multi_investigator_surge_does_not_spill() {
    // Verifies that a surge in inv1's draw chain resolves entirely within
    // inv1's DrawEncounterCard apply — consuming two cards from the shared
    // encounter deck — without disrupting inv2's subsequent draw.
    //
    // Encounter deck (top → bottom):
    //   [SYNTH_SURGE_TREACHERY, SYNTH_TREACHERY, SYNTH_TREACHERY]
    //
    // Drive: seat_and_open → mulligans → EndTurn(inv1) → EndTurn(inv2)
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

    // Both investigators seated at starting_location by seat_and_open.
    let base = synthetic::setup();

    let inv1 = InvestigatorId(1);
    let inv2 = InvestigatorId(2);

    // Seat both investigators and mulligan each.
    let roster = vec![
        RosterEntry {
            investigator: CardCode::new(TEST_INV),
            deck: vec![],
        },
        RosterEntry {
            investigator: CardCode::new(TEST_INV),
            deck: vec![],
        },
    ];
    let mut state = seat_and_open(base, &roster).state;
    state = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
    .state;
    state = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
    .state;
    // inv1 ends turn → rotates to inv2.
    state = take_turn_action(state, &TurnAction::EndTurn).state;
    // inv2 is last in turn_order → auto-advances into Mythos.
    state = take_turn_action(state, &TurnAction::EndTurn).state;

    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(
        state.current_encounter_drawer(),
        Some(inv1),
        "inv1 draws first"
    );

    // Seed the controlled draw order *after* seat_and_open's shuffle:
    // surge on top, then two plain treacheries.
    synthetic::with_encounter_deck(
        &mut state,
        vec![
            CardCode(SYNTH_SURGE_TREACHERY_CODE.into()),
            CardCode(SYNTH_TREACHERY_CODE.into()),
            CardCode(SYNTH_TREACHERY_CODE.into()),
        ],
    );

    // inv1 draws: surge chain pulls TWO cards (surge + plain treachery), then
    // the loop re-prompts inv2 (AwaitingInput).
    let result1 = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert!(matches!(
        result1.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    // The surge chain resolves within inv1's single apply; still Mythos
    // because inv2 still needs to draw.
    assert_eq!(result1.state.phase, Phase::Mythos);
    assert_eq!(
        result1.state.current_encounter_drawer(),
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
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert!(matches!(
        result2.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(result2.state.phase, Phase::Investigation);
    assert_eq!(result2.state.current_encounter_drawer(), None);
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
    let mut base = synthetic::setup();
    // Drain the seeded deck and ensure discard is also empty.
    base.encounter_deck.clear();
    base.encounter_discard.clear();

    let state = setup_at_mythos_draw(base);
    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(state.current_encounter_drawer(), Some(InvestigatorId(1)));

    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );

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
        result.state.current_encounter_drawer(),
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
/// the window STAYS OPEN (not auto-skipped): it remains on
/// `state.open_windows` and the phase has not yet advanced.
#[test]
fn mythos_after_draws_window_stays_open_when_fast_event_in_hand() {
    let base = synthetic::setup();

    let mut state = setup_at_mythos_draw(base);
    // Insert the synthetic Fast event into inv1's hand AFTER setup so it
    // doesn't interact with the player-deck draw during seat_and_open.
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .expect("inv1 must be present")
        .hand
        .push(CardCode(SYNTH_FAST_EVENT_CODE.into()));

    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(state.current_encounter_drawer(), Some(InvestigatorId(1)));

    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );

    // DrawEncounterCard should complete normally (Done) — the window
    // opens but doesn't block the draw action's completion.
    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "DrawEncounterCard should return Done even when window stays open"
    );

    // The window must remain on the stack — it was NOT auto-skipped.
    assert!(
        !result.state.open_windows().is_empty(),
        "MythosAfterDraws window must stay on stack when Fast event is in hand; \
         pre-fix this would have been empty (window auto-skipped)"
    );
    assert!(
        matches!(
            result.state.open_windows().last(),
            Some(Continuation::FastWindow {
                kind: FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
                ..
            })
        ),
        "top open window must be MythosAfterDraws; got {:?}",
        result.state.open_windows().last()
    );

    // Phase must still be Mythos — the window continuation (mythos_phase_end)
    // has NOT fired yet. (The window staying open + the phase not advancing is
    // the observable signal that it was not auto-skipped.)
    assert_eq!(
        result.state.phase,
        Phase::Mythos,
        "phase must still be Mythos while MythosAfterDraws window is open"
    );
}

/// Continuation of the push-then-scan regression: after `DrawEncounterCard`
/// leaves the `MythosAfterDraws` window open (because a Fast event is in
/// hand), `ResolveInput::Skip` must close the window, run
/// `mythos_phase_end`, and transition to Investigation.
///
/// `resolve_input`'s Skip arm closes the top frame: a pure-Fast
/// `MythosAfterDraws` gate (empty `pending_triggers`) on top is closed via
/// `close_reaction_window`. (Historically this routed through an empty-skipping
/// `top_reaction_window_index`, which failed to find the pure-Fast window and
/// left it stuck — Slice C-plumbing replaced that with top-frame dispatch.)
#[test]
fn mythos_after_draws_window_closed_by_skip_and_transitions_to_investigation() {
    let base = synthetic::setup();

    let mut state = setup_at_mythos_draw(base);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .expect("inv1 must be present")
        .hand
        .push(CardCode(SYNTH_FAST_EVENT_CODE.into()));

    // Advance through the draw to land in the open-window state.
    let draw_result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert_eq!(draw_result.outcome, EngineOutcome::Done);
    assert!(
        !draw_result.state.open_windows().is_empty(),
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
        skip_result.state.open_windows().len(),
        1,
        "InvestigationBegins window must be open (Fast card is eligible); \
         MythosAfterDraws must be gone"
    );
    assert!(
        matches!(
            skip_result.state.open_windows().last(),
            Some(Continuation::FastWindow {
                kind: FastWindowKind::Phase(PhaseStep::InvestigationBegins),
                ..
            })
        ),
        "top window must be InvestigationBegins; got {:?}",
        skip_result.state.open_windows().last()
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
    // MythosAfterDraws closed and InvestigationBegins opened — both observable
    // via the open-window stack + phase transition asserted above.
}
