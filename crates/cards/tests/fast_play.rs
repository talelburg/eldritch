//! Integration tests for the Fast play-card and activated-ability gates
//! introduced in #103.
//!
//! Per the Arkham Horror LCG Rules Reference (page 11):
//!
//! - "A fast event card may be played from a player's hand any time its
//!   play instructions specify." → permitted by any investigator a
//!   window's `fast_actors` scope allows.
//! - "A fast asset may be played by an investigator during any player
//!   window on his or her turn." → restricted to the OWNER (the active
//!   investigator); non-owner plays remain illegal even in a window.
//! - "The ⚡ icon indicates a free triggered ability that does not cost
//!   an action and may be used during any player window." → activated
//!   abilities have no owner restriction.
//!
//! These tests cover the asset gate via Magnifying Glass (01030), the
//! event gate via Working a Hunch (01037), and the activated-ability gate
//! via Hyperawareness (01034).
//!
//! Note: we use `Phase::Mythos` (a non-Investigation phase) in the
//! "owner during permissive window" test so the open-window branch is
//! the load-bearing condition for permission — Investigation phase alone
//! is enough to play under the active-investigator branch and would mask
//! the actual rule being tested.
//!
//! Why this file exists at the `cards/tests/` layer: it needs real card
//! metadata + abilities from the `cards` corpus, which `game-core` itself
//! cannot reach by crate-dependency direction. Each `tests/*.rs` is its
//! own process so `install(cards::REGISTRY)` does not collide with the
//! other integration test binaries.

use game_core::engine::EngineOutcome;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, Continuation, FastActorScope, FastWindowKind,
    InvestigatorId, LocationId, MythosResume, Phase, PhaseStep,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, test_investigator, test_location, GameStateBuilder,
};
use game_core::TurnAction;

#[ctor::ctor]
fn install_cards_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

#[test]
fn fast_asset_playable_by_owner_during_permissive_window() {
    // Owner-as-active-investigator with a real permissive window (MythosAfterDraws)
    // open during a non-Investigation phase: the strict pre-#103 gate would reject
    // (phase != Investigation), but the loosened gate must accept because the
    // window permits and the owner IS the active investigator. This is the
    // rules-correct positive case for Fast assets. (#476: the window now surfaces
    // the play as a choice and auto-closes once nothing remains, so the post-play
    // drive cascades through the MythosPhase anchor to the next phase — hence the
    // realistic anchor; the assertion is that the play executed, not the exact
    // post-cascade outcome.)
    let mut a = test_investigator(1);
    a.resources = 5;
    a.hand.push(CardCode::new("01030")); // Magnifying Glass — Fast.
    let state = GameStateBuilder::new()
        .with_investigator(a)
        .with_phase(Phase::Mythos)
        .with_active_investigator(InvestigatorId(1))
        .with_phase_anchor(Continuation::MythosPhase {
            resume: MythosResume::AfterDraws,
        })
        .with_open_window(
            FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
            FastActorScope::Any,
        )
        .build();
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: InvestigatorId(1),
            hand_index: 0,
        },
    );
    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "Magnifying Glass plays Fast for the owner (= active investigator) during a \
         permissive window in Mythos: {:?}",
        result.outcome,
    );
    let a_after = result.state.investigators.get(&InvestigatorId(1)).unwrap();
    assert_eq!(a_after.hand.len(), 0, "card should have left hand");
    assert!(
        a_after
            .cards_in_play
            .iter()
            .any(|c| c.code == CardCode::new("01030")),
        "Magnifying Glass should be in play",
    );
}

#[test]
fn fast_asset_rejected_by_non_owner_even_with_permissive_window() {
    // Per Rules Reference p. 11: a Fast asset may only be played by its
    // owner (i.e. on the owner's turn — the active investigator). A
    // non-owner attempting the Fast play remains illegal even if an
    // open window's `fast_actors` scope permits the actor.
    let a = test_investigator(1);
    let mut b = test_investigator(2);
    b.resources = 5;
    b.hand.push(CardCode::new("01030")); // Magnifying Glass — Fast.
    let state = GameStateBuilder::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_open_window(
            FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
            FastActorScope::Any,
        )
        .build();
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: InvestigatorId(2),
            hand_index: 0,
        },
    );
    let reason = match result.outcome {
        EngineOutcome::Rejected { reason } => reason,
        other => panic!(
            "Fast asset by NON-owner must reject per Rules Reference p. 11, even in a \
             permissive window: {other:?}",
        ),
    };
    assert!(
        reason.contains("owner")
            || reason.contains("asset")
            || reason.contains("active")
            || reason.contains("Fast"),
        "expected gate rejection citing Fast-asset owner restriction; got: {reason}",
    );
}

#[test]
fn non_fast_asset_still_rejected_when_not_active_investigator() {
    let a = test_investigator(1);
    let mut b = test_investigator(2);
    b.resources = 5;
    b.hand.push(CardCode::new("01059")); // Holy Rosary — non-Fast asset, cost 2.
    let state = GameStateBuilder::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_open_window(
            FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
            FastActorScope::Any,
        )
        .build();
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: InvestigatorId(2),
            hand_index: 0,
        },
    );
    let reason = match result.outcome {
        EngineOutcome::Rejected { reason } => reason,
        other => {
            panic!("Holy Rosary is not Fast — non-active investigator must not play it: {other:?}")
        }
    };
    // Make sure the rejection cites the timing-window gate,
    // not (for instance) the missing-from-hand or resource-shortage paths.
    assert!(
        reason.contains("non-Fast")
            || reason.contains("Investigation")
            || reason.contains("active")
            || reason.contains("timing"),
        "expected non-Fast gate rejection; got: {reason}",
    );
}

#[test]
fn fast_asset_still_playable_by_active_investigator_during_investigation() {
    let mut a = test_investigator(1);
    a.resources = 5;
    a.hand.push(CardCode::new("01030")); // Magnifying Glass — Fast.
    let state = GameStateBuilder::new()
        .with_investigator(a)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .build();
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: InvestigatorId(1),
            hand_index: 0,
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Done),
        "Magnifying Glass plays normally for active investigator (Phase-3 behavior preserved): {:?}",
        result.outcome,
    );
}

#[test]
fn fast_activated_ability_usable_by_non_active_investigator_when_window_permits() {
    let a = test_investigator(1);
    let mut b = test_investigator(2);
    b.resources = 5; // Hyperawareness's [fast] cost is 1 resource per use.
                     // Place Hyperawareness (01034) into play for investigator B.
    b.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new("01034"),
        CardInstanceId(1),
    ));
    let state = GameStateBuilder::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_phase(Phase::Mythos)
        .with_active_investigator(InvestigatorId(1))
        .with_phase_anchor(Continuation::MythosPhase {
            resume: MythosResume::AfterDraws,
        })
        .with_open_window(
            FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
            FastActorScope::Any,
        )
        .build();
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: InvestigatorId(2),
            instance_id: CardInstanceId(1),
            ability_index: 0,
        },
    );
    // The ability activates (resource spent). After it, Hyperawareness is still
    // 0-cost-eligible (B has resources left), so the #476 fast window re-prompts
    // rather than reaching Done — assert the activation executed, not the exact
    // post-activation outcome.
    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "Hyperawareness [fast] ability should activate from non-active investigator: {:?}",
        result.outcome,
    );
    let b_after = result.state.investigators.get(&InvestigatorId(2)).unwrap();
    assert_eq!(b_after.resources, 4, "1 resource should have been spent");
}

#[test]
fn fast_activated_ability_rejected_when_no_permissive_window() {
    let a = test_investigator(1);
    let mut b = test_investigator(2);
    b.resources = 5;
    b.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new("01034"),
        CardInstanceId(1),
    ));
    let state = GameStateBuilder::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        // No open window.
        .build();
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: InvestigatorId(2),
            instance_id: CardInstanceId(1),
            ability_index: 0,
        },
    );
    let reason = match result.outcome {
        EngineOutcome::Rejected { reason } => reason,
        other => panic!("non-active investigator with no permissive window must reject: {other:?}"),
    };
    assert!(
        reason.contains("Fast") || reason.contains("active") || reason.contains("Investigation"),
        "expected gate-rejection wording; got: {reason}",
    );
}

#[test]
fn fast_event_play_only_during_turn_rejected_outside_investigation() {
    // Working a Hunch (01037): "Fast. Play only during your turn. Discover 1
    // clue at your location." The `play_only_during_turn` metadata flag (#322)
    // tightens the Fast gate to the active investigator's Investigation turn,
    // so even a permissive window in the Mythos phase is rejected — per the FAQ,
    // "'your turn' is within the Investigation phase." (Was previously, wrongly,
    // accepted while the clause was unenforced.)
    let loc = LocationId(101);
    let mut a = test_investigator(1);
    a.resources = 5;
    a.current_location = Some(loc);
    a.hand.push(CardCode::new("01037"));
    let mut location = test_location(101, "Study");
    location.clues = 1;
    let state = GameStateBuilder::new()
        .with_investigator(a)
        .with_location(location)
        .with_phase(Phase::Mythos)
        .with_active_investigator(InvestigatorId(1))
        .with_open_window(
            FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
            FastActorScope::Any,
        )
        .build();
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: InvestigatorId(1),
            hand_index: 0,
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "'Play only during your turn' is rejected outside the Investigation phase: {:?}",
        result.outcome,
    );
    // Unchanged: still in hand, clue not taken.
    assert_eq!(
        result.state.investigators[&InvestigatorId(1)].hand.len(),
        1,
        "card stays in hand on reject",
    );
    assert_eq!(result.state.locations[&loc].clues, 1, "no clue taken");
}

#[test]
fn fast_event_play_only_during_turn_rejected_for_non_owner() {
    // Working a Hunch (01037): "Fast. Play only during your turn." During
    // investigator 1's turn (Investigation, active = inv 1), investigator 2
    // cannot play it — it is not inv 2's turn. The `play_only_during_turn`
    // gate (#322) requires the *active* investigator, so a non-owner is
    // rejected even in a permissive window.
    let a = test_investigator(1);
    let loc = LocationId(101);
    let mut b = test_investigator(2);
    b.resources = 5;
    b.current_location = Some(loc);
    b.hand.push(CardCode::new("01037"));
    let mut location = test_location(101, "Study");
    location.clues = 1;
    let state = GameStateBuilder::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_location(location)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_open_window(
            FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
            FastActorScope::Any,
        )
        .build();
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: InvestigatorId(2),
            hand_index: 0,
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "non-owner cannot play a 'Play only during your turn' event on another's turn: {:?}",
        result.outcome,
    );
}

#[test]
fn fast_asset_rejected_by_owner_outside_investigation_with_no_window() {
    // Fast assets need EITHER active_during_investigation OR
    // (owner_is_active && permissive_window). Owner-during-non-
    // Investigation with no window meets neither — must reject.
    //
    // Magnifying Glass (01030) text: "Fast.\nYou get +1 [intellect]
    // while investigating."
    let mut a = test_investigator(1);
    a.resources = 5;
    a.hand.push(CardCode::new("01030")); // Magnifying Glass — Fast asset.
    let state = GameStateBuilder::new()
        .with_investigator(a)
        .with_phase(Phase::Mythos)
        .with_active_investigator(InvestigatorId(1))
        // No open window.
        .build();
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: InvestigatorId(1),
            hand_index: 0,
        },
    );
    let reason = match result.outcome {
        EngineOutcome::Rejected { reason } => reason,
        other => panic!(
            "Fast asset by owner outside Investigation with no window must reject: {other:?}"
        ),
    };
    assert!(
        reason.contains("Fast")
            || reason.contains("active")
            || reason.contains("Investigation")
            || reason.contains("window"),
        "expected gate-rejection wording; got: {reason}",
    );
}
