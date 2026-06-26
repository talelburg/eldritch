//! End-to-end reaction-window flow with a mock [`CardRegistry`] that
//! carries `Trigger::OnEvent` abilities.
//!
//! Lives at `crates/game-core/tests/` so it runs in its own integration-
//! test process (separate `OnceLock<CardRegistry>`), letting it install
//! a mock registry without colliding with game-core's in-crate tests or
//! with other `tests/*.rs` files. Mirrors `on_skill_test_resolution.rs`.
//!
//! Roland Banks (01001, #55) is the first real-card `Trigger::OnEvent`
//! consumer; that card's end-to-end test lives in
//! `crates/cards/tests/roland_banks.rs`. This file keeps mock cards to
//! exercise edge cases (multi-controller defeats, two abilities on one
//! card, `by_controller: false`) that no real Phase-3 card hits.

use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::dsl::{
    discover_clue, gain_resources, reaction_on_event, Ability, EventPattern, EventTiming,
    InvestigatorTarget, LocationTarget, SkillTestKind, TestOutcome,
};
use game_core::engine::{EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, Continuation, EnemyId,
    InvestigatorId, LocationId, Phase, TokenModifiers,
};
use game_core::test_support::{
    apply_no_commits, test_enemy, test_investigator, test_location, GameStateBuilder,
};
use game_core::{assert_event, assert_no_event, Action, InputResponse, PlayerAction, TurnAction};

/// Mock: optional reaction "after you defeat an enemy, discover 1 clue
/// at your location" — the Roland-shape canonical `OnEvent` test card.
const ROLAND_REACTION: &str = "MOCK-OE-ROLAND";

/// Mock: optional reaction "after any enemy is defeated, gain 1
/// resource" — the no-`by_controller` variant. Exercises the "any
/// defeat matches" branch of `trigger_matches`.
const BYSTANDER_REACTION: &str = "MOCK-OE-BYSTANDER";

/// Mock: a card with two `OnEvent` abilities. Exercises the
/// per-ability-index loop inside `scan_pending_triggers` so each
/// ability gets its own pending-trigger entry.
const TWO_REACTIONS: &str = "MOCK-OE-TWO";

/// Mock: optional reaction "after you successfully investigate, gain 1
/// resource" — the Dr. Milan Christopher 01033 shape (C6a #241).
const MILAN_REACTION: &str = "MOCK-OE-MILAN";

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        ROLAND_REACTION => Some(vec![reaction_on_event(
            EventPattern::EnemyDefeated {
                by_controller: true,
                code: None,
            },
            EventTiming::After,
            discover_clue(LocationTarget::YourLocation, 1),
        )]),
        BYSTANDER_REACTION => Some(vec![reaction_on_event(
            EventPattern::EnemyDefeated {
                by_controller: false,
                code: None,
            },
            EventTiming::After,
            gain_resources(InvestigatorTarget::You, 1),
        )]),
        TWO_REACTIONS => Some(vec![
            reaction_on_event(
                EventPattern::EnemyDefeated {
                    by_controller: true,
                    code: None,
                },
                EventTiming::After,
                discover_clue(LocationTarget::YourLocation, 1),
            ),
            reaction_on_event(
                EventPattern::EnemyDefeated {
                    by_controller: true,
                    code: None,
                },
                EventTiming::After,
                gain_resources(InvestigatorTarget::You, 1),
            ),
        ]),
        MILAN_REACTION => Some(vec![reaction_on_event(
            EventPattern::SkillTestResolved {
                outcome: TestOutcome::Success,
                kind: Some(SkillTestKind::Investigate),
            },
            EventTiming::After,
            gain_resources(InvestigatorTarget::You, 1),
        )]),
        _ => None,
    }
}

#[ctor::ctor]
fn install_mock_registry() {
    let _ = game_core::card_registry::install(CardRegistry {
        metadata_for: mock_metadata_for,
        abilities_for: mock_abilities_for,
        native_effect_for: |_| None,
        native_eligibility_for: |_| None,
    });
}

/// Build a Fight-ready scenario with the investigator at a location,
/// engaged with an enemy at 1/2 damage so a successful Fight defeats.
/// Optionally place `in_play_cards` in the investigator's
/// `cards_in_play`.
fn fight_to_defeat_scenario(
    in_play_cards: &[(&str, u32)],
) -> (InvestigatorId, EnemyId, LocationId, game_core::GameState) {
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 3;
    for (code, instance) in in_play_cards {
        inv.cards_in_play.push(CardInPlay::enter_play(
            CardCode::new(*code),
            CardInstanceId(*instance),
        ));
    }
    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 3;
    enemy.max_health = 2;
    enemy.damage = 1;
    enemy.engaged_with = Some(inv_id);
    enemy.current_location = Some(loc_id); // co-located: Fight is location-gated (#401)
    let mut loc = test_location(10, "Mock Location");
    loc.clues = 3;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (inv_id, enemy_id, loc_id, state)
}

/// Resolve the open-turn `Fight` action against `state` to its `OptionId`,
/// returning the `ResolveInput(PickSingle)` submit the enumeration round-trip
/// expects. The state must carry an `InvestigatorTurn` frame (so the Fight is
/// offered). Replaces the typed `PlayerAction::Fight` (removed in 2b, #447).
fn fight_action(state: &game_core::GameState, inv: InvestigatorId, enemy: EnemyId) -> Action {
    let target = TurnAction::Fight {
        investigator: inv,
        enemy,
    };
    let idx = game_core::engine::enumerate::legal_actions(state)
        .iter()
        .position(|a| a == &target)
        .expect("Fight must be a legal open-turn action");
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
    })
}

/// Output of [`fight_through_commit_window`]: merged state + events
/// across the two applies plus the second apply's terminal outcome.
struct DrivenResult {
    state: game_core::GameState,
    events: Vec<Event>,
    outcome: EngineOutcome,
}

/// Drive a Fight action through its commit window with empty commits,
/// concatenate the events from both applies, and return the merged
/// result. Stops at whatever outcome the second apply produces — `Done`
/// if no reaction window opened, `AwaitingInput` if one did. Used in
/// place of [`apply_no_commits`] for tests that want to inspect the
/// reaction-window paused state directly rather than drive past it.
fn fight_through_commit_window(state: game_core::GameState, action: Action) -> DrivenResult {
    let paused = game_core::engine::apply(state, action);
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "first apply must suspend at the commit window, got {:?}",
        paused.outcome,
    );
    let mut events = paused.events;
    let after = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    events.extend(after.events);
    DrivenResult {
        state: after.state,
        events,
        outcome: after.outcome,
    }
}

#[test]
fn no_in_play_reaction_means_no_window_opens() {
    // No cards in play → no triggers → no window. The Fight resolves
    // to Done and never suspends on a reaction window. Sanity check that
    // the in-play scan is the gate.
    let (inv_id, enemy_id, _loc_id, state) = fight_to_defeat_scenario(&[]);
    let action = fight_action(&state, inv_id, enemy_id);
    let result = apply_no_commits(state, action);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == enemy_id && *by == inv_id
    );
    // No AfterEnemyDefeated reaction window (none in play) — no reaction window
    // is left on the stack.
    assert!(result
        .state
        .continuations
        .last()
        .and_then(Continuation::pending_candidates)
        .is_none_or(Vec::is_empty));
}

#[test]
fn matching_reaction_opens_window_and_suspends() {
    // ROLAND_REACTION in play, Fight defeats the enemy → window opens
    // with one pending trigger; engine returns AwaitingInput; the
    // window record on state names the right kind + pending entry.
    let (inv_id, enemy_id, _loc_id, state) = fight_to_defeat_scenario(&[(ROLAND_REACTION, 1)]);

    // Drive Fight through the commit window with empty commits, then
    // inspect the second-apply result (which lands on the queued
    // reaction-window AwaitingInput).
    let action = fight_action(&state, inv_id, enemy_id);
    let result = fight_through_commit_window(state, action);

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "expected AwaitingInput for the queued reaction window, got {:?}",
        result.outcome,
    );
    assert_event!(
        result.events,
        Event::EnemyDefeated { enemy: e, .. } if *e == enemy_id
    );
    // The window opened and is still open + suspended — verified via the
    // AwaitingInput outcome above and the populated reaction window below.
    let window = result
        .state
        .continuations
        .last()
        .filter(|c| c.pending_candidates().is_some_and(|p| !p.is_empty()))
        .expect("reaction window must be populated while suspended");
    assert!(
        matches!(
            window.window_timing_event(),
            Some(game_core::engine::TimingEvent::EnemyDefeated { enemy, by: Some(by), .. })
                if *enemy == enemy_id && *by == inv_id
        ),
        "reaction window must be after the enemy defeat: {:?}",
        window.window_timing_event(),
    );
    assert_eq!(window.pending_candidates().unwrap().len(), 1);
    assert_eq!(window.pending_candidates().unwrap()[0].controller, inv_id);
    assert_eq!(
        window.pending_candidates().unwrap()[0].source,
        game_core::state::CandidateSource::InPlay(CardInstanceId(1))
    );
    assert_eq!(window.pending_candidates().unwrap()[0].ability_index, 0);
}

#[test]
fn pick_index_fires_pending_trigger_and_closes_window() {
    // Open the window, then PickSingle(OptionId(0)) → effect fires (clue
    // discovered), the entry drains, the window closes, and the
    // engine returns Done.
    let (inv_id, enemy_id, loc_id, state) = fight_to_defeat_scenario(&[(ROLAND_REACTION, 1)]);
    let action = fight_action(&state, inv_id, enemy_id);
    let paused = fight_through_commit_window(state, action);
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    let resumed = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );

    assert!(matches!(
        resumed.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        resumed.events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
    );
    assert_eq!(resumed.state.investigators[&inv_id].clues, 1);
    assert_eq!(resumed.state.locations[&loc_id].clues, 2);
    // The window closed: no reaction window left on the stack.
    assert!(resumed
        .state
        .continuations
        .last()
        .and_then(Continuation::pending_candidates)
        .is_none_or(Vec::is_empty));
}

#[test]
fn skip_closes_an_optional_only_window_without_firing() {
    // Open the window, then Skip → no effect fires; the window
    // closes; engine returns Done.
    let (inv_id, enemy_id, loc_id, state) = fight_to_defeat_scenario(&[(ROLAND_REACTION, 1)]);
    let action = fight_action(&state, inv_id, enemy_id);
    let paused = fight_through_commit_window(state, action);
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    let resumed = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );

    assert!(matches!(
        resumed.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_no_event!(resumed.events, Event::CluePlaced { .. });
    assert_eq!(resumed.state.investigators[&inv_id].clues, 0);
    assert_eq!(resumed.state.locations[&loc_id].clues, 3);
    // The window closed without firing: no reaction window left on the stack.
    assert!(resumed
        .state
        .continuations
        .last()
        .and_then(Continuation::pending_candidates)
        .is_none_or(Vec::is_empty));
}

#[test]
fn by_controller_filter_excludes_unrelated_investigators() {
    // ROLAND_REACTION belongs to a second investigator who is NOT the
    // attacker. The trigger is `by_controller: true`, so it must NOT
    // match the defeat (the active investigator made the kill, not
    // the controller of ROLAND_REACTION). No window opens.
    let attacker = InvestigatorId(1);
    let bystander = InvestigatorId(2);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut atk = test_investigator(1);
    atk.current_location = Some(loc_id);
    atk.skills.combat = 3;
    let mut byst = test_investigator(2);
    byst.current_location = Some(loc_id);
    byst.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(ROLAND_REACTION),
        CardInstanceId(1),
    ));
    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 3;
    enemy.max_health = 2;
    enemy.damage = 1;
    enemy.engaged_with = Some(attacker);
    enemy.current_location = Some(loc_id); // co-located: Fight is location-gated (#401)
    let mut loc = test_location(10, "Mock Location");
    loc.clues = 3;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(attacker)
        .with_turn_order([attacker, bystander])
        .with_investigator_turn(attacker)
        .with_investigator(atk)
        .with_investigator(byst)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    let action = fight_action(&state, attacker, enemy_id);
    let result = apply_no_commits(state, action);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    // No AfterEnemyDefeated window for the unrelated investigator — resolution
    // ran straight to Done with no reaction window left on the stack.
    assert!(result
        .state
        .continuations
        .last()
        .and_then(Continuation::pending_candidates)
        .is_none_or(Vec::is_empty));
}

#[test]
fn unqualified_pattern_matches_any_defeat() {
    // BYSTANDER_REACTION uses `by_controller: false`, so a defeat by
    // ANY investigator triggers it. Here the trigger's controller
    // (investigator 2) isn't the attacker, but the window still
    // opens and the effect fires on resolve.
    let attacker = InvestigatorId(1);
    let bystander = InvestigatorId(2);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut atk = test_investigator(1);
    atk.current_location = Some(loc_id);
    atk.skills.combat = 3;
    let mut byst = test_investigator(2);
    byst.current_location = Some(loc_id);
    byst.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(BYSTANDER_REACTION),
        CardInstanceId(1),
    ));
    let byst_resources_before = byst.resources;
    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 3;
    enemy.max_health = 2;
    enemy.damage = 1;
    enemy.engaged_with = Some(attacker);
    enemy.current_location = Some(loc_id); // co-located: Fight is location-gated (#401)
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(attacker)
        .with_turn_order([attacker, bystander])
        .with_investigator_turn(attacker)
        .with_investigator(atk)
        .with_investigator(byst)
        .with_enemy(enemy)
        .with_location(test_location(10, "Mock Location"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    let action = fight_action(&state, attacker, enemy_id);
    let paused = fight_through_commit_window(state, action);
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    let window = paused
        .state
        .continuations
        .last()
        .filter(|c| c.pending_candidates().is_some_and(|p| !p.is_empty()))
        .expect("window must open for an unqualified pattern");
    assert_eq!(window.pending_candidates().unwrap().len(), 1);
    assert_eq!(
        window.pending_candidates().unwrap()[0].controller,
        bystander
    );

    let resumed = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    assert!(matches!(
        resumed.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(
        resumed.state.investigators[&bystander].resources,
        byst_resources_before + 1,
        "BYSTANDER_REACTION's gain_resources fires on the trigger's controller, \
         not on the attacker",
    );
}

#[test]
fn pick_index_out_of_bounds_rejects_window_stays_open() {
    let (inv_id, enemy_id, _loc_id, state) = fight_to_defeat_scenario(&[(ROLAND_REACTION, 1)]);
    let action = fight_action(&state, inv_id, enemy_id);
    let paused = fight_through_commit_window(state, action);
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    let bad = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(99)),
        }),
    );
    match bad.outcome {
        EngineOutcome::Rejected { reason } => {
            assert!(
                reason.contains("out of bounds"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    assert!(
        bad.state
            .continuations
            .last()
            .and_then(Continuation::pending_candidates)
            .is_some_and(|p| !p.is_empty()),
        "window must survive a rejected pick so the client can retry"
    );
}

#[test]
fn multiple_pending_triggers_resolve_one_at_a_time() {
    // TWO_REACTIONS exposes two OnEvent abilities. The window opens
    // with both pending; PickSingle(OptionId(0)) fires the first (clue), engine
    // re-emits AwaitingInput with one entry remaining; PickSingle(OptionId(0))
    // again fires the second (resource); window closes.
    let (inv_id, enemy_id, loc_id, state) = fight_to_defeat_scenario(&[(TWO_REACTIONS, 1)]);
    let action = fight_action(&state, inv_id, enemy_id);
    let paused = fight_through_commit_window(state, action);
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(
        paused
            .state
            .continuations
            .last()
            .filter(|c| c.pending_candidates().is_some_and(|p| !p.is_empty()))
            .expect("window populated")
            .pending_candidates()
            .unwrap()
            .len(),
        2,
    );

    let after_first = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    assert!(
        matches!(after_first.outcome, EngineOutcome::AwaitingInput { .. }),
        "window stays open while triggers remain pending",
    );
    assert_event!(
        after_first.events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
    );
    assert_eq!(
        after_first
            .state
            .continuations
            .last()
            .filter(|c| c.pending_candidates().is_some_and(|p| !p.is_empty()))
            .expect("window still populated")
            .pending_candidates()
            .unwrap()
            .len(),
        1,
    );

    let resources_before = after_first.state.investigators[&inv_id].resources;
    let after_second = game_core::engine::apply(
        after_first.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    assert!(matches!(
        after_second.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        after_second.events,
        Event::ResourcesGained { investigator, amount: 1 } if *investigator == inv_id
    );
    assert_eq!(after_second.state.locations[&loc_id].clues, 2);
    assert_eq!(
        after_second.state.investigators[&inv_id].resources,
        resources_before + 1,
    );
    assert!(after_second
        .state
        .continuations
        .last()
        .and_then(Continuation::pending_candidates)
        .is_none_or(Vec::is_empty));
}

#[test]
fn fight_event_sequence_pins_window_between_enemy_defeated_and_skill_test_ended() {
    // Per the Rules Reference, "after… [reaction] abilities may be
    // used immediately after that triggering condition's impact upon
    // the game state has resolved" — i.e. mid-action. For a Fight
    // that defeats an enemy with Roland-shaped reaction in play, the
    // canonical event order is:
    //   SkillTestSucceeded → EnemyDamaged → EnemyDefeated
    //     → [AfterEnemyDefeated reaction window]
    //       → CluePlaced (Roland's clue)
    //       → LocationCluesChanged
    //     → [OnSkillTestResolution events if any committed]
    //     → CardDiscarded × committed
    //     → SkillTestEnded
    //
    // This pin protects the ordering: the reaction effect (CluePlaced) must
    // fire AFTER EnemyDefeated but BEFORE SkillTestEnded — i.e. mid-action.
    // Any future refactor that moves the reaction back to a post-action
    // position (the rules-incorrect "deferred" design that landed initially)
    // requires an intentional decision (and an update here).
    let (inv_id, enemy_id, _loc_id, state) = fight_to_defeat_scenario(&[(ROLAND_REACTION, 1)]);
    let action = fight_action(&state, inv_id, enemy_id);
    let paused = fight_through_commit_window(state, action);
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "Fight must suspend at the reaction window before SkillTestEnded fires",
    );

    // Drive past the window by firing the single pending trigger.
    let resumed = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    assert!(matches!(
        resumed.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    // Merge the events from both phases so we can pin the full
    // ordering across the suspend point.
    let mut events = paused.events;
    events.extend(resumed.events);

    // Walk the event list and assert ordering of the milestones. The reaction
    // effect (CluePlaced) fires between EnemyDefeated and SkillTestEnded — the
    // window opening/closing itself is observed via the suspend (the
    // `paused.outcome` AwaitingInput above), not a discrete event.
    let mut defeated_idx: Option<usize> = None;
    let mut clue_idx: Option<usize> = None;
    let mut ended_idx: Option<usize> = None;
    for (i, ev) in events.iter().enumerate() {
        match ev {
            Event::EnemyDefeated { enemy: e, .. } if *e == enemy_id => defeated_idx = Some(i),
            Event::CluePlaced { .. } => clue_idx = Some(i),
            Event::SkillTestEnded { .. } => ended_idx = Some(i),
            _ => {}
        }
    }
    let defeated = defeated_idx.expect("EnemyDefeated must fire");
    let clue = clue_idx.expect("CluePlaced must fire (Roland's reaction)");
    let ended = ended_idx.expect("SkillTestEnded must fire");

    assert!(
        defeated < clue,
        "CluePlaced (reaction effect) fires after the EnemyDefeated it reacts to"
    );
    assert!(
        clue < ended,
        "CluePlaced precedes SkillTestEnded — the rules-correct mid-action shape \
         (the reaction resolves before the test ends)"
    );
}

#[test]
fn reaction_window_closes_before_on_skill_test_resolution_fires() {
    // A Fight that both defeats an enemy AND has a committed
    // `OnSkillTestResolution` card pins the cross-step ordering:
    //   EnemyDefeated → [reaction window opens] → [Roland fires]
    //     → [reaction window closes]
    //     → OnSkillTestResolution effect events
    //     → CardDiscarded (committed) → SkillTestEnded
    //
    // Per the Rules Reference, OnSkillTestResolution is part of the
    // skill test's resolution machinery (not a reaction window), so
    // it sits inside the test's resolution but AFTER the
    // `EnemyDefeated` reaction window — that reaction is response to
    // the defeat impact, which already resolved by the time we get
    // to OnSkillTestResolution.
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    // Use a registry-known card with an OnSkillTestResolution
    // ability. We don't have one in the mock registry; instead just
    // exercise the "open window resumes through the
    // OnSkillTestResolution step" boundary. The OnSkillTestResolution
    // step is a no-op without an installed registry entry for the
    // committed card, but the event-order pinning still applies: the
    // window closes BEFORE the discard-and-end sequence.
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 3;
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(ROLAND_REACTION),
        CardInstanceId(1),
    ));
    inv.hand = vec![CardCode::new("COMMITTED")];
    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 3;
    enemy.max_health = 2;
    enemy.damage = 1;
    enemy.engaged_with = Some(inv_id);
    enemy.current_location = Some(loc_id); // co-located: Fight is location-gated (#401)
    let mut loc = test_location(10, "Mock Location");
    loc.clues = 3;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    // First apply: opens commit window.
    let action = fight_action(&state, inv_id, enemy_id);
    let paused_commit = game_core::engine::apply(state, action);
    assert!(matches!(
        paused_commit.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    // Second apply: commit the card → drives through follow-up →
    // queues window → suspends.
    let paused_reaction = game_core::engine::apply(
        paused_commit.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple {
                selected: vec![OptionId(0)],
            },
        }),
    );
    assert!(
        matches!(paused_reaction.outcome, EngineOutcome::AwaitingInput { .. }),
        "second apply must suspend at the reaction window",
    );

    // Third apply: fire the reaction → window closes → driver
    // resumes → OnSkillTestResolution step → discard → SkillTestEnded
    // → Done.
    let resumed = game_core::engine::apply(
        paused_reaction.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    assert!(matches!(
        resumed.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    // Verify the final event order from the resumed phase: the reaction effect
    // (CluePlaced, fired inside the now-closing window) precedes the
    // OnSkillTestResolution/discard step, which precedes SkillTestEnded. The
    // window closing itself is no longer a discrete event — the reaction
    // resolving before the discard is the observable mid-action shape.
    let mut clue_idx: Option<usize> = None;
    let mut discarded_idx: Option<usize> = None;
    let mut ended_idx: Option<usize> = None;
    for (i, ev) in resumed.events.iter().enumerate() {
        match ev {
            Event::CluePlaced { .. } => clue_idx = Some(i),
            Event::CardDiscarded { .. } => discarded_idx = Some(i),
            Event::SkillTestEnded { .. } => ended_idx = Some(i),
            _ => {}
        }
    }
    let clue = clue_idx.expect("CluePlaced must fire (Roland's reaction on resume)");
    let discarded = discarded_idx.expect("CardDiscarded must fire (committed card)");
    let ended = ended_idx.expect("SkillTestEnded must fire");
    assert!(
        clue < discarded,
        "the reaction effect resolves before the discard step (window closes before discard)",
    );
    assert!(
        discarded < ended,
        "CardDiscarded precedes SkillTestEnded (pre-existing pin from #63)",
    );
    // The committed card landed in discard.
    assert_eq!(
        resumed.state.investigators[&inv_id].discard,
        vec![CardCode::new("COMMITTED")],
    );
    // The clue Roland's reaction discovered landed.
    assert_eq!(resumed.state.locations[&loc_id].clues, 2);
    assert_eq!(resumed.state.investigators[&inv_id].clues, 1);
}

#[test]
fn pending_triggers_order_active_investigator_first_then_turn_order() {
    // Two investigators both carry a `by_controller: false` reaction
    // (so the defeat — credited to the active investigator — matches
    // for both). The pending list must put the active investigator's
    // trigger first, followed by the other investigator's, per
    // Arkham's active-investigator-first / turn-order priority for
    // reaction windows.
    let active = InvestigatorId(1);
    let other = InvestigatorId(2);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut atk = test_investigator(1);
    atk.current_location = Some(loc_id);
    atk.skills.combat = 3;
    atk.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(BYSTANDER_REACTION),
        CardInstanceId(1),
    ));
    let mut byst = test_investigator(2);
    byst.current_location = Some(loc_id);
    byst.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(BYSTANDER_REACTION),
        CardInstanceId(2),
    ));
    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 3;
    enemy.max_health = 2;
    enemy.damage = 1;
    enemy.engaged_with = Some(active);
    enemy.current_location = Some(loc_id); // co-located: Fight is location-gated (#401)
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(active)
        .with_turn_order([active, other])
        .with_investigator_turn(active)
        .with_investigator(atk)
        .with_investigator(byst)
        .with_enemy(enemy)
        .with_location(test_location(10, "Mock Location"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    let action = fight_action(&state, active, enemy_id);
    let paused = fight_through_commit_window(state, action);
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    let window = paused
        .state
        .continuations
        .last()
        .filter(|c| c.pending_candidates().is_some_and(|p| !p.is_empty()))
        .expect("window must populate when both investigators carry triggers");

    assert_eq!(window.pending_candidates().unwrap().len(), 2);
    assert_eq!(
        window.pending_candidates().unwrap()[0].controller,
        active,
        "active investigator's trigger must come first",
    );
    assert_eq!(
        window.pending_candidates().unwrap()[0].source,
        game_core::state::CandidateSource::InPlay(CardInstanceId(1))
    );
    assert_eq!(
        window.pending_candidates().unwrap()[1].controller,
        other,
        "non-active investigator's trigger comes after, in turn order",
    );
    assert_eq!(
        window.pending_candidates().unwrap()[1].source,
        game_core::state::CandidateSource::InPlay(CardInstanceId(2))
    );
}

#[test]
fn skip_after_firing_one_drops_remaining_optionals() {
    // TWO_REACTIONS has two optional triggers. Fire PickSingle(OptionId(0))
    // (consuming the first), then Skip. The second optional must NOT
    // fire — its effect (gain 1 resource) leaves resources unchanged
    // from the post-first-fire baseline. The window closes cleanly.
    let (inv_id, enemy_id, loc_id, state) = fight_to_defeat_scenario(&[(TWO_REACTIONS, 1)]);
    let action = fight_action(&state, inv_id, enemy_id);
    let paused = fight_through_commit_window(state, action);
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(
        paused
            .state
            .continuations
            .last()
            .filter(|c| c.pending_candidates().is_some_and(|p| !p.is_empty()))
            .expect("window populated")
            .pending_candidates()
            .unwrap()
            .len(),
        2,
    );

    let after_first = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    assert!(
        matches!(after_first.outcome, EngineOutcome::AwaitingInput { .. }),
        "one optional fired, one remains pending",
    );
    let resources_after_first = after_first.state.investigators[&inv_id].resources;

    let skipped = game_core::engine::apply(
        after_first.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );
    assert!(matches!(
        skipped.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    // The skipped optional was the resource-gain — its effect didn't fire.
    assert_eq!(
        skipped.state.investigators[&inv_id].resources, resources_after_first,
        "Skip must not fire the remaining optional's effect",
    );
    // Sanity: the first optional did fire — the location lost a clue
    // (TWO_REACTIONS' first ability discovers 1 clue at the
    // controller's location).
    assert_eq!(skipped.state.locations[&loc_id].clues, 2);
    assert_eq!(skipped.state.investigators[&inv_id].clues, 1);
    assert!(skipped
        .state
        .continuations
        .last()
        .and_then(Continuation::pending_candidates)
        .is_none_or(Vec::is_empty));
}

#[test]
fn reaction_trigger_in_threat_area_opens_window() {
    // The shared scan source spans cards_in_play + threat_area, so a
    // reaction ability on a threat-area card is offered just like one
    // in play. Build the standard fight-to-defeat scenario but seat
    // ROLAND_REACTION in the threat area.
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 3;
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode::new(ROLAND_REACTION),
        CardInstanceId(7),
    ));
    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 3;
    enemy.max_health = 2;
    enemy.damage = 1;
    enemy.engaged_with = Some(inv_id);
    enemy.current_location = Some(loc_id); // co-located: Fight is location-gated (#401)
    let mut loc = test_location(10, "Mock Location");
    loc.clues = 3;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    let action = fight_action(&state, inv_id, enemy_id);
    let result = fight_through_commit_window(state, action);

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "a threat-area reaction must open a window, got {:?}",
        result.outcome,
    );
    let window = result
        .state
        .continuations
        .last()
        .filter(|c| c.pending_candidates().is_some_and(|p| !p.is_empty()))
        .expect("threat-area reaction must populate the window");
    assert_eq!(window.pending_candidates().unwrap().len(), 1);
    assert_eq!(window.pending_candidates().unwrap()[0].controller, inv_id);
    assert_eq!(
        window.pending_candidates().unwrap()[0].source,
        game_core::state::CandidateSource::InPlay(CardInstanceId(7))
    );
}

#[test]
fn active_reaction_window_is_the_top_continuation_frame() {
    // Invariant the loop-driven dispatch relies on (Slice C-plumbing, #431): the
    // continuation stack is the resolution order, so an *active* reaction window
    // (one with pending candidates) is always `continuations.last()` — never
    // stranded beneath another frame. The engine dispatches the top frame and
    // operates on it directly (no `top_reaction_window_index` reach-down); this
    // pins the property that makes that correct. (Replaces the former
    // `close_reaction_window_at_removes_..._phase_gate_on_top` regression, which
    // hand-injected an empty gate *above* a pending reaction window — a shape the
    // framework never produces, because a pending window's `awaits_input()` gates
    // the framework from advancing to open one.)
    let (inv_id, enemy_id, _loc_id, state) = fight_to_defeat_scenario(&[(ROLAND_REACTION, 1)]);
    let action = fight_action(&state, inv_id, enemy_id);
    let paused = fight_through_commit_window(state, action);
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "Fight must suspend at the reaction window, got {:?}",
        paused.outcome,
    );
    let top = paused
        .state
        .continuations
        .last()
        .expect("a reaction window is open");
    assert!(
        top.pending_candidates().is_some_and(|c| !c.is_empty()),
        "the active reaction window must be the top frame, got {top:?}",
    );
}

#[test]
fn pick_index_fires_threat_area_reaction_and_closes_window() {
    // ROLAND_REACTION is seated in the investigator's threat_area
    // (instance id 7). The scan already finds it there; now verify
    // the fire path also resolves it: PickSingle(OptionId(0)) discovers 1 clue,
    // the window closes, and Done is returned.
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 3;
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode::new(ROLAND_REACTION),
        CardInstanceId(7),
    ));
    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 3;
    enemy.max_health = 2;
    enemy.damage = 1;
    enemy.engaged_with = Some(inv_id);
    enemy.current_location = Some(loc_id); // co-located: Fight is location-gated (#401)
    let mut loc = test_location(10, "Mock Location");
    loc.clues = 3;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    let action = fight_action(&state, inv_id, enemy_id);
    let paused = fight_through_commit_window(state, action);
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "threat-area reaction must open a window, got {:?}",
        paused.outcome,
    );

    let resumed = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );

    assert!(matches!(
        resumed.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        resumed.events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
    );
    assert_eq!(resumed.state.investigators[&inv_id].clues, 1);
    assert_eq!(resumed.state.locations[&loc_id].clues, 2);
    // The window closed: no reaction window left on the stack.
    assert!(resumed
        .state
        .continuations
        .last()
        .and_then(Continuation::pending_candidates)
        .is_none_or(Vec::is_empty));
}

// ------------------------------------------------------------------
// After-successful-investigate reaction window (Dr. Milan, C6a #241)
// ------------------------------------------------------------------

/// Build an investigator at a 1-clue location (shroud 2), intellect 3, with
/// `in_play_cards` in play, a `Numeric(0)` bag (so intellect 3 ≥ shroud 2 →
/// the Investigate succeeds and discovers the clue).
fn investigate_to_success_scenario(
    in_play_cards: &[(&str, u32)],
) -> (InvestigatorId, LocationId, game_core::GameState) {
    let id = InvestigatorId(1);
    let loc = LocationId(10);
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc);
    inv.skills.intellect = 3;
    for (code, instance) in in_play_cards {
        inv.cards_in_play.push(CardInPlay::enter_play(
            CardCode::new(*code),
            CardInstanceId(*instance),
        ));
    }
    let mut loc_meta = test_location(10, "Study");
    loc_meta.clues = 1;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_turn_order([id])
        .with_investigator_turn(id)
        .with_investigator(inv)
        .with_location(loc_meta)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (id, loc, state)
}

fn commit_nothing() -> Action {
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickMultiple { selected: vec![] },
    })
}

/// A successful Investigate opens the after-investigate window for the
/// in-play reaction (Dr. Milan), which the controller fires to gain a
/// resource; the skill test then resumes to completion.
#[test]
fn after_successful_investigate_fires_in_play_reaction() {
    let (id, loc, state) = investigate_to_success_scenario(&[(MILAN_REACTION, 1)]);
    let resources_before = state.investigators[&id].resources;

    let investigate = {
        let target = TurnAction::Investigate { investigator: id };
        let idx = game_core::engine::enumerate::legal_actions(&state)
            .iter()
            .position(|a| a == &target)
            .expect("Investigate must be legal");
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        })
    };
    let paused_commit = game_core::engine::apply(state, investigate);
    assert!(matches!(
        paused_commit.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    // Commit nothing → test succeeds → clue discovered → after-investigate
    // window opens → suspends.
    let paused_reaction = game_core::engine::apply(paused_commit.state, commit_nothing());
    assert!(
        matches!(paused_reaction.outcome, EngineOutcome::AwaitingInput { .. }),
        "after-investigate reaction window must suspend, got {:?}",
        paused_reaction.outcome,
    );

    // Fire the reaction → gain 1 resource → resume the test → Done.
    let resumed = game_core::engine::apply(
        paused_reaction.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    assert!(matches!(
        resumed.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(
        resumed.state.investigators[&id].resources,
        resources_before + 1,
        "Dr. Milan reaction gained a resource",
    );
    assert_eq!(
        resumed.state.locations[&loc].clues, 0,
        "clue was discovered"
    );
    assert!(resumed
        .state
        .continuations
        .last()
        .and_then(Continuation::pending_candidates)
        .is_none_or(Vec::is_empty));
}

/// With no after-investigate reaction in play, a successful Investigate
/// opens no window — it completes in the single commit apply (regression:
/// `queue_reaction_window` is a no-op when nothing reacts).
#[test]
fn after_successful_investigate_no_window_without_reaction() {
    let (id, loc, state) = investigate_to_success_scenario(&[]);

    let investigate = {
        let target = TurnAction::Investigate { investigator: id };
        let idx = game_core::engine::enumerate::legal_actions(&state)
            .iter()
            .position(|a| a == &target)
            .expect("Investigate must be legal");
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        })
    };
    let paused_commit = game_core::engine::apply(state, investigate);
    let resolved = game_core::engine::apply(paused_commit.state, commit_nothing());

    // No reaction card is in play, so no AfterSuccessfulInvestigate window can
    // open: the investigate resolves straight through to the open-turn menu in
    // one apply (the clue assertions below confirm it resolved).
    assert!(matches!(
        resolved.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(resolved.state.investigators[&id].clues, 1);
    assert_eq!(resolved.state.locations[&loc].clues, 0);
    // No AfterSuccessfulInvestigate reaction window (none in play): resolution
    // ran straight to Done above in a single apply, never suspending.
}
