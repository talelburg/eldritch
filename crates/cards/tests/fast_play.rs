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
//! via Beat Cop's (01018) `[fast]` *"Discard Beat Cop: Deal 1 damage to an
//! enemy at your location."*. Beat Cop rather than Hyperawareness (01034)
//! because a `for this skill test` buff is refused outside a test (#676),
//! which would mask the window gate this file is about.
//!
//! Note: we use `Phase::Mythos` (a non-Investigation phase) in the
//! "owner during permissive window" test so the open-window branch is
//! the load-bearing condition for permission — Investigation phase alone
//! is enough to play under the active-investigator branch and would mask
//! the actual rule being tested.
//!
//! The file grew a #710 section at the end: the same zero-action gate, asked of
//! an **engine-opened** player window rather than a builder-seeded one, so that
//! the corpus's existing `[fast]` abilities are checked for *being offered* and
//! not only for being accepted.
//!
//! Why this file exists at the `cards/tests/` layer: it needs real card
//! metadata + abilities from the `cards` corpus, which `game-core` itself
//! cannot reach by crate-dependency direction. Each `tests/*.rs` is its
//! own process so `install(cards::REGISTRY)` does not collide with the
//! other integration test binaries.

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{EngineOutcome, InputKind, OptionTarget};
use game_core::state::{
    AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, Continuation,
    EnemyId, FastActorScope, FastWindowKind, InvestigatorId, LocationId, MythosResume, Phase,
    PhaseStep, SkillKind,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, perform_skill_test, test_enemy, test_investigator,
    test_location, GameStateBuilder,
};

use game_core::{apply, Action, TurnAction};

/// Beat Cop 01018: *"You get +1 \[combat\]."* / *"\[fast\] Discard Beat Cop:
/// Deal 1 damage to an enemy at your location."*
const BEAT_COP: &str = "01018";

#[ctor::ctor(unsafe)]
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

/// Board for the activated-ability gate: investigator B controls Beat Cop
/// (instance 1) at a location with a co-located enemy, so the ability has a
/// live target; A is the active investigator. `open_window` adds a permissive
/// Mythos player window — the condition the two tests below differ on.
fn board_with_beat_cop(open_window: bool) -> game_core::GameState {
    let loc = LocationId(101);
    let a = test_investigator(1);
    let mut b = test_investigator(2);
    b.current_location = Some(loc);
    b.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(BEAT_COP),
        CardInstanceId(1),
    ));
    let mut enemy = test_enemy(100, "Ghoul");
    enemy.max_health = 3;
    enemy.current_location = Some(loc);
    let mut builder = GameStateBuilder::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_location(test_location(101, "Study"))
        .with_enemy(enemy)
        .with_phase(Phase::Mythos)
        .with_active_investigator(InvestigatorId(1));
    if open_window {
        builder = builder
            .with_phase_anchor(Continuation::MythosPhase {
                resume: MythosResume::AfterDraws,
            })
            .with_open_window(
                FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
                FastActorScope::Any,
            );
    }
    builder.build()
}

/// Beat Cop's `[fast]` ability is index 1 (index 0 is its constant
/// `+1 [combat]`).
const BEAT_COP_FAST: TurnAction = TurnAction::ActivateAbility {
    investigator: InvestigatorId(2),
    source: AbilitySource::InPlay(CardInstanceId(1)),
    ability_index: 1,
};

#[test]
fn fast_activated_ability_usable_by_non_active_investigator_when_window_permits() {
    let result = dispatch_turn_action_unchecked(board_with_beat_cop(true), &BEAT_COP_FAST);
    // The ability activates (its DiscardSelf cost is paid and the damage
    // lands). The #476 fast window may re-prompt afterwards, so assert the
    // activation executed rather than the exact post-activation outcome.
    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "Beat Cop's [fast] ability should activate from a non-active investigator: {:?}",
        result.outcome,
    );
    assert_eq!(
        result.state.enemies[&EnemyId(100)].damage,
        1,
        "the ability's damage landed",
    );
    assert!(
        result.state.investigators[&InvestigatorId(2)]
            .cards_in_play
            .is_empty(),
        "Beat Cop paid its own discard as the cost",
    );
}

#[test]
fn fast_activated_ability_rejected_when_no_permissive_window() {
    // Same board, no open window: B is not the active investigator, so
    // nothing permits the fast activation.
    let result = dispatch_turn_action_unchecked(board_with_beat_cop(false), &BEAT_COP_FAST);
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

// ---- the corpus regression for #710 --------------------------------------
//
// #710 widened *which sources* a player window reaches, by making the window's
// enumerator consult the one reachability predicate the turn menu consults
// (`engine::ability_source`). The cards that already print a zero-action
// ability all sit under the first rules bullet — *"a card in play and under his
// or her control"* — so this is the half most at risk of a silent regression,
// and the one the ticket asks be checked against the real corpus.
//
// Beat Cop 01018 (verbatim, <https://arkhamdb.com/card/01018>):
//
// > You get +1 [combat].
// > [fast] Discard Beat Cop: Deal 1 damage to an enemy at your location.
//
// with the ruling that makes the window the load-bearing part, verbatim:
//
// > You cannot use Beat Cop's ability after you assign lethal damage/horror to
// > him (there is no Player Window to use [fast]free abilities in between
// > assigning and applying damage).
//
// The two tests above submit the activation directly against a builder-seeded
// window. This one goes through an **engine-opened** window — a skill test's
// ST.1 player window — and asserts the ability is *offered* there, which is
// the seam #710 is about: the Parlor's Resign missing from the menu was as
// much the defect as its being refused on submission, and the same is true of
// a window's option list.

/// Beat Cop's zero-action ability is offered by an engine-opened player window,
/// anchored to the card instance carrying it, and resolves from the pick — the
/// existing corpus behaviour, unchanged by the source widening.
#[test]
fn corpus_zero_action_ability_is_offered_by_an_engine_opened_player_window() {
    // No seeded window: the skill test opens its own, so the option list under
    // assertion is the one a real client would be shown. Mythos phase with
    // investigator 1 active, so nothing but the window can permit the
    // activation.
    let mut state = board_with_beat_cop(false);
    // The window opens before any token is revealed, but the test refuses to
    // start against an empty bag.
    state.chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)]);
    let result = perform_skill_test(state, InvestigatorId(2), SkillKind::Willpower, 4);
    let EngineOutcome::AwaitingInput { ref request, .. } = result.outcome else {
        panic!(
            "the skill test should park at its ST.1 player window, got {:?}",
            result.outcome,
        );
    };
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert!(
        request.skippable,
        "a player window is skippable: {request:?}"
    );
    let targets: Vec<_> = request.options.iter().map(|o| o.target.clone()).collect();
    assert_eq!(
        targets,
        vec![OptionTarget::CardInstance(CardInstanceId(1))],
        "Beat Cop's [fast] ability is the one thing eligible on this board, anchored to \
         the instance that carries it",
    );

    let option = request.options[0].id;
    let picked = apply(
        result.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(option),
        }),
    );
    assert_eq!(
        picked.state.enemies[&EnemyId(100)].damage,
        1,
        "picking the offered option resolved the ability",
    );
    assert!(
        picked.state.investigators[&InvestigatorId(2)]
            .cards_in_play
            .is_empty(),
        "Beat Cop paid its own discard as the cost",
    );
}
