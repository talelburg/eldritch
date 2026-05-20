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

use std::sync::OnceLock;

use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::dsl::{
    discover_clue, gain_resources, on_event, Ability, EventPattern, EventTiming,
    InvestigatorTarget, LocationTarget,
};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, EnemyId, InvestigatorId,
    LocationId, Phase, TokenModifiers, WindowKind,
};
use game_core::test_support::{
    apply_no_commits, test_enemy, test_investigator, test_location, TestGame,
};
use game_core::{
    assert_event, assert_event_count, assert_no_event, Action, InputResponse, PlayerAction,
};

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

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        ROLAND_REACTION => Some(vec![on_event(
            EventPattern::EnemyDefeated {
                by_controller: true,
            },
            EventTiming::After,
            discover_clue(LocationTarget::ControllerLocation, 1),
        )]),
        BYSTANDER_REACTION => Some(vec![on_event(
            EventPattern::EnemyDefeated {
                by_controller: false,
            },
            EventTiming::After,
            gain_resources(InvestigatorTarget::Controller, 1),
        )]),
        TWO_REACTIONS => Some(vec![
            on_event(
                EventPattern::EnemyDefeated {
                    by_controller: true,
                },
                EventTiming::After,
                discover_clue(LocationTarget::ControllerLocation, 1),
            ),
            on_event(
                EventPattern::EnemyDefeated {
                    by_controller: true,
                },
                EventTiming::After,
                gain_resources(InvestigatorTarget::Controller, 1),
            ),
        ]),
        _ => None,
    }
}

fn install_mock_registry() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = game_core::card_registry::install(CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
        });
    });
}

/// Build a Fight-ready scenario with the investigator at a location,
/// engaged with an enemy at 1/2 damage so a successful Fight defeats.
/// Optionally place `in_play_cards` in the investigator's
/// `cards_in_play`.
fn fight_to_defeat_scenario(
    in_play_cards: &[(&str, u32)],
) -> (InvestigatorId, EnemyId, LocationId, game_core::GameState) {
    install_mock_registry();
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
    let mut loc = test_location(10, "Mock Location");
    loc.clues = 3;
    let state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (inv_id, enemy_id, loc_id, state)
}

fn fight_action(inv: InvestigatorId, enemy: EnemyId) -> Action {
    Action::Player(PlayerAction::Fight {
        investigator: inv,
        enemy,
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
            response: InputResponse::CommitCards { indices: vec![] },
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
    // to Done with no WindowOpened / WindowClosed pair on the event
    // log. Sanity check that the in-play scan is the gate.
    let (inv_id, enemy_id, _loc_id, state) = fight_to_defeat_scenario(&[]);
    let result = apply_no_commits(state, fight_action(inv_id, enemy_id));

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == enemy_id && *by == inv_id
    );
    assert_no_event!(result.events, Event::WindowOpened { .. });
    assert_no_event!(result.events, Event::WindowClosed { .. });
    assert!(result.state.in_flight_reaction_window.is_none());
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
    let result = fight_through_commit_window(state, fight_action(inv_id, enemy_id));

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "expected AwaitingInput for the queued reaction window, got {:?}",
        result.outcome,
    );
    assert_event!(
        result.events,
        Event::EnemyDefeated { enemy: e, .. } if *e == enemy_id
    );
    assert_event!(
        result.events,
        Event::WindowOpened {
            kind: WindowKind::AfterEnemyDefeated { enemy: e, by: Some(by) },
        } if *e == enemy_id && *by == inv_id
    );
    // WindowClosed must NOT fire yet — the window is open.
    assert_no_event!(result.events, Event::WindowClosed { .. });

    let window = result
        .state
        .in_flight_reaction_window
        .as_ref()
        .expect("reaction window must be populated while suspended");
    assert_eq!(
        window.kind,
        WindowKind::AfterEnemyDefeated {
            enemy: enemy_id,
            by: Some(inv_id),
        },
    );
    assert_eq!(window.pending.len(), 1);
    assert_eq!(window.pending[0].controller, inv_id);
    assert_eq!(window.pending[0].instance_id, CardInstanceId(1));
    assert_eq!(window.pending[0].ability_index, 0);
    assert!(!window.pending[0].forced);
}

#[test]
fn pick_index_fires_pending_trigger_and_closes_window() {
    // Open the window, then PickIndex(0) → effect fires (clue
    // discovered), the entry drains, the window closes, and the
    // engine returns Done.
    let (inv_id, enemy_id, loc_id, state) = fight_to_defeat_scenario(&[(ROLAND_REACTION, 1)]);
    let paused = fight_through_commit_window(state, fight_action(inv_id, enemy_id));
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    let resumed = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickIndex(0),
        }),
    );

    assert_eq!(resumed.outcome, EngineOutcome::Done);
    assert_event!(
        resumed.events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
    );
    assert_event!(
        resumed.events,
        Event::WindowClosed {
            kind: WindowKind::AfterEnemyDefeated { enemy: e, .. },
        } if *e == enemy_id
    );
    assert_eq!(resumed.state.investigators[&inv_id].clues, 1);
    assert_eq!(resumed.state.locations[&loc_id].clues, 2);
    assert!(resumed.state.in_flight_reaction_window.is_none());
}

#[test]
fn skip_closes_an_optional_only_window_without_firing() {
    // Open the window, then Skip → no effect fires; the window
    // closes; engine returns Done.
    let (inv_id, enemy_id, loc_id, state) = fight_to_defeat_scenario(&[(ROLAND_REACTION, 1)]);
    let paused = fight_through_commit_window(state, fight_action(inv_id, enemy_id));
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

    assert_eq!(resumed.outcome, EngineOutcome::Done);
    assert_no_event!(resumed.events, Event::CluePlaced { .. });
    assert_event!(
        resumed.events,
        Event::WindowClosed {
            kind: WindowKind::AfterEnemyDefeated { enemy: e, .. },
        } if *e == enemy_id
    );
    assert_eq!(resumed.state.investigators[&inv_id].clues, 0);
    assert_eq!(resumed.state.locations[&loc_id].clues, 3);
    assert!(resumed.state.in_flight_reaction_window.is_none());
}

#[test]
fn by_controller_filter_excludes_unrelated_investigators() {
    // ROLAND_REACTION belongs to a second investigator who is NOT the
    // attacker. The trigger is `by_controller: true`, so it must NOT
    // match the defeat (the active investigator made the kill, not
    // the controller of ROLAND_REACTION). No window opens.
    install_mock_registry();
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
    let mut loc = test_location(10, "Mock Location");
    loc.clues = 3;
    let state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(attacker)
        .with_turn_order([attacker, bystander])
        .with_investigator(atk)
        .with_investigator(byst)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    let result = apply_no_commits(state, fight_action(attacker, enemy_id));

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_no_event!(result.events, Event::WindowOpened { .. });
    assert!(result.state.in_flight_reaction_window.is_none());
}

#[test]
fn unqualified_pattern_matches_any_defeat() {
    // BYSTANDER_REACTION uses `by_controller: false`, so a defeat by
    // ANY investigator triggers it. Here the trigger's controller
    // (investigator 2) isn't the attacker, but the window still
    // opens and the effect fires on resolve.
    install_mock_registry();
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
    let state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(attacker)
        .with_turn_order([attacker, bystander])
        .with_investigator(atk)
        .with_investigator(byst)
        .with_enemy(enemy)
        .with_location(test_location(10, "Mock Location"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    let paused = fight_through_commit_window(state, fight_action(attacker, enemy_id));
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    let window = paused
        .state
        .in_flight_reaction_window
        .as_ref()
        .expect("window must open for an unqualified pattern");
    assert_eq!(window.pending.len(), 1);
    assert_eq!(window.pending[0].controller, bystander);

    let resumed = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickIndex(0),
        }),
    );
    assert_eq!(resumed.outcome, EngineOutcome::Done);
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
    let paused = fight_through_commit_window(state, fight_action(inv_id, enemy_id));
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    let bad = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickIndex(99),
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
        bad.state.in_flight_reaction_window.is_some(),
        "window must survive a rejected pick so the client can retry"
    );
}

#[test]
fn non_resolve_input_action_rejects_while_window_open() {
    // While a reaction window is open, every non-ResolveInput player
    // action rejects. Mirrors the commit-window and mulligan-window
    // guards.
    let (inv_id, enemy_id, _loc_id, state) = fight_to_defeat_scenario(&[(ROLAND_REACTION, 1)]);
    let paused = fight_through_commit_window(state, fight_action(inv_id, enemy_id));
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    let rejected = game_core::engine::apply(paused.state, Action::Player(PlayerAction::EndTurn));
    match rejected.outcome {
        EngineOutcome::Rejected { reason } => {
            assert!(
                reason.contains("reaction window"),
                "unexpected reason: {reason}",
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    assert!(rejected.state.in_flight_reaction_window.is_some());
}

#[test]
fn multiple_pending_triggers_resolve_one_at_a_time() {
    // TWO_REACTIONS exposes two OnEvent abilities. The window opens
    // with both pending; PickIndex(0) fires the first (clue), engine
    // re-emits AwaitingInput with one entry remaining; PickIndex(0)
    // again fires the second (resource); window closes.
    let (inv_id, enemy_id, loc_id, state) = fight_to_defeat_scenario(&[(TWO_REACTIONS, 1)]);
    let paused = fight_through_commit_window(state, fight_action(inv_id, enemy_id));
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(
        paused
            .state
            .in_flight_reaction_window
            .as_ref()
            .expect("window populated")
            .pending
            .len(),
        2,
    );

    let after_first = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickIndex(0),
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
    assert_no_event!(after_first.events, Event::WindowClosed { .. });
    assert_eq!(
        after_first
            .state
            .in_flight_reaction_window
            .as_ref()
            .expect("window still populated")
            .pending
            .len(),
        1,
    );

    let resources_before = after_first.state.investigators[&inv_id].resources;
    let after_second = game_core::engine::apply(
        after_first.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickIndex(0),
        }),
    );
    assert_eq!(after_second.outcome, EngineOutcome::Done);
    assert_event!(
        after_second.events,
        Event::ResourcesGained { investigator, amount: 1 } if *investigator == inv_id
    );
    assert_event!(
        after_second.events,
        Event::WindowClosed {
            kind: WindowKind::AfterEnemyDefeated { .. }
        }
    );
    assert_eq!(after_second.state.locations[&loc_id].clues, 2);
    assert_eq!(
        after_second.state.investigators[&inv_id].resources,
        resources_before + 1,
    );
    assert!(after_second.state.in_flight_reaction_window.is_none());
}

#[test]
fn fight_event_sequence_pins_window_between_enemy_defeated_and_skill_test_ended() {
    // Per the Rules Reference, "after… [reaction] abilities may be
    // used immediately after that triggering condition's impact upon
    // the game state has resolved" — i.e. mid-action. For a Fight
    // that defeats an enemy with Roland-shaped reaction in play, the
    // canonical event order is:
    //   SkillTestSucceeded → EnemyDamaged → EnemyDefeated
    //     → WindowOpened (AfterEnemyDefeated)
    //       → CluePlaced (Roland's clue)
    //       → LocationCluesChanged
    //     → WindowClosed
    //     → [OnSkillTestResolution events if any committed]
    //     → CardDiscarded × committed
    //     → SkillTestEnded
    //
    // This pin protects the ordering: any future refactor that moves
    // the window back to a post-action position (the rules-incorrect
    // "deferred" design that landed initially) requires an intentional
    // decision (and an update here).
    let (inv_id, enemy_id, _loc_id, state) = fight_to_defeat_scenario(&[(ROLAND_REACTION, 1)]);
    let paused = fight_through_commit_window(state, fight_action(inv_id, enemy_id));
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "Fight must suspend at the reaction window before SkillTestEnded fires",
    );

    // Drive past the window by firing the single pending trigger.
    let resumed = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickIndex(0),
        }),
    );
    assert_eq!(resumed.outcome, EngineOutcome::Done);

    // Merge the events from both phases so we can pin the full
    // ordering across the suspend point.
    let mut events = paused.events;
    events.extend(resumed.events);

    // Walk the event list and assert ordering of the milestones.
    let mut defeated_idx: Option<usize> = None;
    let mut opened_idx: Option<usize> = None;
    let mut clue_idx: Option<usize> = None;
    let mut closed_idx: Option<usize> = None;
    let mut ended_idx: Option<usize> = None;
    for (i, ev) in events.iter().enumerate() {
        match ev {
            Event::EnemyDefeated { enemy: e, .. } if *e == enemy_id => defeated_idx = Some(i),
            Event::WindowOpened {
                kind: WindowKind::AfterEnemyDefeated { .. },
            } => opened_idx = Some(i),
            Event::CluePlaced { .. } => clue_idx = Some(i),
            Event::WindowClosed {
                kind: WindowKind::AfterEnemyDefeated { .. },
            } => closed_idx = Some(i),
            Event::SkillTestEnded { .. } => ended_idx = Some(i),
            _ => {}
        }
    }
    let defeated = defeated_idx.expect("EnemyDefeated must fire");
    let opened = opened_idx.expect("WindowOpened must fire");
    let clue = clue_idx.expect("CluePlaced must fire (Roland's reaction)");
    let closed = closed_idx.expect("WindowClosed must fire");
    let ended = ended_idx.expect("SkillTestEnded must fire");

    assert!(defeated < opened, "EnemyDefeated precedes WindowOpened");
    assert!(
        opened < clue,
        "CluePlaced (reaction effect) fires inside the open window"
    );
    assert!(clue < closed, "CluePlaced precedes WindowClosed");
    assert!(
        closed < ended,
        "WindowClosed precedes SkillTestEnded — the rules-correct mid-action shape"
    );
    assert_event_count!(events, 1, Event::WindowOpened { .. });
    assert_event_count!(events, 1, Event::WindowClosed { .. });
}

#[test]
fn reaction_window_closes_before_on_skill_test_resolution_fires() {
    // A Fight that both defeats an enemy AND has a committed
    // `OnSkillTestResolution` card pins the cross-step ordering:
    //   EnemyDefeated → WindowOpened → [Roland fires] → WindowClosed
    //     → OnSkillTestResolution effect events
    //     → CardDiscarded (committed) → SkillTestEnded
    //
    // Per the Rules Reference, OnSkillTestResolution is part of the
    // skill test's resolution machinery (not a reaction window), so
    // it sits inside the test's resolution but AFTER the
    // `EnemyDefeated` reaction window — that reaction is response to
    // the defeat impact, which already resolved by the time we get
    // to OnSkillTestResolution.
    install_mock_registry();
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
    let mut loc = test_location(10, "Mock Location");
    loc.clues = 3;
    let state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    // First apply: opens commit window.
    let paused_commit = game_core::engine::apply(state, fight_action(inv_id, enemy_id));
    assert!(matches!(
        paused_commit.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    // Second apply: commit the card → drives through follow-up →
    // queues window → suspends.
    let paused_reaction = game_core::engine::apply(
        paused_commit.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::CommitCards { indices: vec![0] },
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
            response: InputResponse::PickIndex(0),
        }),
    );
    assert_eq!(resumed.outcome, EngineOutcome::Done);

    // Verify the final event order from the resumed phase: it
    // begins with WindowClosed, then proceeds with CardDiscarded
    // (for the committed card) and SkillTestEnded.
    let mut closed_idx: Option<usize> = None;
    let mut discarded_idx: Option<usize> = None;
    let mut ended_idx: Option<usize> = None;
    for (i, ev) in resumed.events.iter().enumerate() {
        match ev {
            Event::WindowClosed { .. } => closed_idx = Some(i),
            Event::CardDiscarded { .. } => discarded_idx = Some(i),
            Event::SkillTestEnded { .. } => ended_idx = Some(i),
            _ => {}
        }
    }
    let closed = closed_idx.expect("WindowClosed must fire on resume");
    let discarded = discarded_idx.expect("CardDiscarded must fire (committed card)");
    let ended = ended_idx.expect("SkillTestEnded must fire");
    assert!(
        closed < discarded,
        "WindowClosed precedes CardDiscarded (reaction window closes before discard step)",
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
    install_mock_registry();
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
    let state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(active)
        .with_turn_order([active, other])
        .with_investigator(atk)
        .with_investigator(byst)
        .with_enemy(enemy)
        .with_location(test_location(10, "Mock Location"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    let paused = fight_through_commit_window(state, fight_action(active, enemy_id));
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    let window = paused
        .state
        .in_flight_reaction_window
        .as_ref()
        .expect("window must populate when both investigators carry triggers");

    assert_eq!(window.pending.len(), 2);
    assert_eq!(
        window.pending[0].controller, active,
        "active investigator's trigger must come first",
    );
    assert_eq!(window.pending[0].instance_id, CardInstanceId(1));
    assert_eq!(
        window.pending[1].controller, other,
        "non-active investigator's trigger comes after, in turn order",
    );
    assert_eq!(window.pending[1].instance_id, CardInstanceId(2));
}

#[test]
fn skip_after_firing_one_drops_remaining_optionals() {
    // TWO_REACTIONS has two optional triggers. Fire PickIndex(0)
    // (consuming the first), then Skip. The second optional must NOT
    // fire — its effect (gain 1 resource) leaves resources unchanged
    // from the post-first-fire baseline. The window closes cleanly.
    let (inv_id, enemy_id, loc_id, state) = fight_to_defeat_scenario(&[(TWO_REACTIONS, 1)]);
    let paused = fight_through_commit_window(state, fight_action(inv_id, enemy_id));
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(
        paused
            .state
            .in_flight_reaction_window
            .as_ref()
            .expect("window populated")
            .pending
            .len(),
        2,
    );

    let after_first = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickIndex(0),
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
    assert_eq!(skipped.outcome, EngineOutcome::Done);
    assert_event!(
        skipped.events,
        Event::WindowClosed {
            kind: WindowKind::AfterEnemyDefeated { .. }
        }
    );
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
    assert!(skipped.state.in_flight_reaction_window.is_none());
}
