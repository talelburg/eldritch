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

use std::sync::Once;

use game_core::action::{Action, PlayerAction};
use game_core::engine::{apply, EngineOutcome};
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, FastActorScope, InvestigatorId, LocationId, Phase,
    WindowKind,
};
use game_core::test_support::{test_investigator, test_location, TestGame};

fn install_cards_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

#[test]
fn fast_asset_playable_by_owner_during_permissive_window() {
    // Owner-as-active-investigator with a window open during a
    // non-Investigation phase: the strict pre-#103 gate would reject
    // (phase != Investigation), but the loosened gate must accept
    // because the window permits and the owner IS the active
    // investigator. This is the rules-correct positive case for
    // Fast assets.
    install_cards_registry();
    let mut a = test_investigator(1);
    a.resources = 5;
    a.hand.push(CardCode::new("01030")); // Magnifying Glass — Fast.
    let state = TestGame::new()
        .with_investigator(a)
        .with_phase(Phase::Mythos)
        .with_active_investigator(InvestigatorId(1))
        .with_open_window(
            WindowKind::BetweenPhases {
                from: Phase::Mythos,
                to: Phase::Investigation,
            },
            FastActorScope::Any,
        )
        .build();
    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: InvestigatorId(1),
            hand_index: 0,
        }),
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Done),
        "Magnifying Glass plays Fast for the owner (= active investigator) during a \
         permissive window in Mythos: {:?}",
        result.outcome,
    );
    let a_after = result.state.investigators.get(&InvestigatorId(1)).unwrap();
    assert_eq!(a_after.hand.len(), 0, "card should have left hand");
    assert_eq!(a_after.cards_in_play.len(), 1, "card should be in play");
}

#[test]
fn fast_asset_rejected_by_non_owner_even_with_permissive_window() {
    // Per Rules Reference p. 11: a Fast asset may only be played by its
    // owner (i.e. on the owner's turn — the active investigator). A
    // non-owner attempting the Fast play remains illegal even if an
    // open window's `fast_actors` scope permits the actor.
    install_cards_registry();
    let a = test_investigator(1);
    let mut b = test_investigator(2);
    b.resources = 5;
    b.hand.push(CardCode::new("01030")); // Magnifying Glass — Fast.
    let state = TestGame::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_open_window(
            WindowKind::BetweenPhases {
                from: Phase::Mythos,
                to: Phase::Investigation,
            },
            FastActorScope::Any,
        )
        .build();
    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: InvestigatorId(2),
            hand_index: 0,
        }),
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
    install_cards_registry();
    let a = test_investigator(1);
    let mut b = test_investigator(2);
    b.resources = 5;
    b.hand.push(CardCode::new("01059")); // Holy Rosary — non-Fast asset, cost 2.
    let state = TestGame::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_open_window(
            WindowKind::BetweenPhases {
                from: Phase::Mythos,
                to: Phase::Investigation,
            },
            FastActorScope::Any,
        )
        .build();
    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: InvestigatorId(2),
            hand_index: 0,
        }),
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
    install_cards_registry();
    let mut a = test_investigator(1);
    a.resources = 5;
    a.hand.push(CardCode::new("01030")); // Magnifying Glass — Fast.
    let state = TestGame::new()
        .with_investigator(a)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .build();
    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: InvestigatorId(1),
            hand_index: 0,
        }),
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Done),
        "Magnifying Glass plays normally for active investigator (Phase-3 behavior preserved): {:?}",
        result.outcome,
    );
}

#[test]
fn fast_activated_ability_usable_by_non_active_investigator_when_window_permits() {
    install_cards_registry();
    let a = test_investigator(1);
    let mut b = test_investigator(2);
    b.resources = 5; // Hyperawareness's [fast] cost is 1 resource per use.
                     // Place Hyperawareness (01034) into play for investigator B.
    b.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new("01034"),
        CardInstanceId(1),
    ));
    let state = TestGame::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_open_window(
            WindowKind::BetweenPhases {
                from: Phase::Mythos,
                to: Phase::Investigation,
            },
            FastActorScope::Any,
        )
        .build();
    let result = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: InvestigatorId(2),
            instance_id: CardInstanceId(1),
            ability_index: 0,
        }),
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Done),
        "Hyperawareness [fast] ability should activate from non-active investigator: {:?}",
        result.outcome,
    );
    // Verify the resource was spent.
    let b_after = result.state.investigators.get(&InvestigatorId(2)).unwrap();
    assert_eq!(b_after.resources, 4, "1 resource should have been spent");
}

#[test]
fn fast_activated_ability_rejected_when_no_permissive_window() {
    install_cards_registry();
    let a = test_investigator(1);
    let mut b = test_investigator(2);
    b.resources = 5;
    b.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new("01034"),
        CardInstanceId(1),
    ));
    let state = TestGame::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        // No open window.
        .build();
    let result = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: InvestigatorId(2),
            instance_id: CardInstanceId(1),
            ability_index: 0,
        }),
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
fn fast_event_playable_by_active_investigator_outside_investigation_in_permissive_window() {
    // Working a Hunch (01037): Fast event, "Play only during your turn.
    // Discover 1 clue at your location." Rules Reference page 11:
    // "A fast event card may be played from a player's hand any time
    // its play instructions specify." The card-level "Play only during
    // your turn" constraint is a *card-level* restriction not yet
    // modeled in the DSL; this test exercises the *engine gate* which
    // permits Fast events when the open window's fast_actors permits
    // the actor. Pre-#103 the strict gate rejected for `phase !=
    // Investigation` regardless of windows; the loosened gate accepts.
    install_cards_registry();
    let loc = LocationId(101);
    let mut a = test_investigator(1);
    a.resources = 5;
    a.current_location = Some(loc);
    a.hand.push(CardCode::new("01037"));
    let state = TestGame::new()
        .with_investigator(a)
        .with_location(test_location(101, "Study"))
        .with_phase(Phase::Mythos)
        .with_active_investigator(InvestigatorId(1))
        .with_open_window(
            WindowKind::BetweenPhases {
                from: Phase::Mythos,
                to: Phase::Investigation,
            },
            FastActorScope::Any,
        )
        .build();
    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: InvestigatorId(1),
            hand_index: 0,
        }),
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Done),
        "Working a Hunch should play Fast for the active investigator during a permissive \
         non-Investigation window: {:?}",
        result.outcome,
    );
    let a_after = result.state.investigators.get(&InvestigatorId(1)).unwrap();
    assert_eq!(a_after.hand.len(), 0, "card should have left hand");
    assert_eq!(
        a_after.discard.len(),
        1,
        "event lands in discard after OnPlay resolves",
    );
}
