use super::*;
use crate::engine::enumerate::TurnAction;
use crate::engine::outcome::EngineOutcome;
use crate::event::Event;
use crate::state::{
    CardCode, CardInPlay, CardInstanceId, EnemyId, GameStateBuilder, InvestigationPhaseFrame,
    InvestigatorId, LocationId, Phase, Status,
};
use crate::{assert_event, assert_event_sequence, assert_no_event, test_support};

#[test]
fn hand_size_discard_prompt_is_player_facing() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let mut events = Vec::new();
    let outcome = park_hand_size_discard(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        vec![InvestigatorId(1)],
    );
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("park_hand_size_discard suspends on the discard prompt");
    };
    for forbidden in ["InputResponse", "option ids", "InvestigatorId("] {
        assert!(
            !request.prompt.contains(forbidden),
            "prompt must be player-facing, found {forbidden:?} in: {}",
            request.prompt
        );
    }
    assert!(request.prompt.contains("discard down to 8"));
}

#[test]
fn upkeep_phase_emits_phase_started_and_auto_skips_to_mythos() {
    // No Fast-eligible cards / no reactions installed → the post-4.1
    // window auto-skips inline, the continuation runs, and the
    // cascade lands in Mythos.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_phase(Phase::Enemy)
        .build();
    state.turn_order = vec![id];
    state.active_investigator = None;
    // Give the investigator a card so the step-4.4 draw pulls it normally
    // rather than decking out: since #509 routed 4.4 through
    // `draw_one_with_deckout`, an empty deck applies the deckout horror
    // penalty, whose capacity read requires a CardRegistry this registry-free
    // unit test cannot install.
    state.investigators.get_mut(&id).unwrap().deck = vec![CardCode("filler0".into())];

    let mut events = Vec::new();
    step_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    }); // Enemy → Upkeep, cascades to Mythos

    let pos = |pred: &dyn Fn(&Event) -> bool| events.iter().position(pred);
    let started = pos(&|e| {
        matches!(
            e,
            Event::PhaseStarted {
                phase: Phase::Upkeep
            }
        )
    })
    .expect("PhaseStarted(Upkeep)");
    let ended = pos(&|e| {
        matches!(
            e,
            Event::PhaseEnded {
                phase: Phase::Upkeep
            }
        )
    })
    .expect("PhaseEnded(Upkeep)");
    let mythos = pos(&|e| {
        matches!(
            e,
            Event::PhaseStarted {
                phase: Phase::Mythos
            }
        )
    })
    .expect("PhaseStarted(Mythos)");
    assert!(
        started < ended && ended < mythos,
        "upkeep sub-step events must be ordered 4.1 → 4.6 → Mythos 1.1; \
         events = {events:?}"
    );
    assert_eq!(state.phase, Phase::Mythos, "cascade lands in Mythos");
    assert!(
        state.open_windows().is_empty(),
        "UpkeepBegins must not persist on the stack"
    );
}

#[test]
fn ready_exhausted_cards_readies_investigator_cards_and_enemies() {
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(1);
    let mut inv = test_support::test_investigator(1);
    let mut card = CardInPlay::enter_play(CardCode("filler0".into()), CardInstanceId(1));
    card.exhausted = true;
    inv.cards_in_play = vec![card];
    let mut enemy = test_support::test_enemy(1, "Test Enemy");
    enemy.exhausted = true;
    let mut state = GameStateBuilder::default()
        .with_investigator(inv)
        .with_enemy(enemy)
        .build();
    let mut events = Vec::new();

    ready_exhausted_cards(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert!(
        !state.investigators[&inv_id].cards_in_play[0].exhausted,
        "card readied"
    );
    assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
    assert!(events.iter().any(|e| matches!(
        e, Event::CardReadied { investigator, instance_id, .. }
        if *investigator == inv_id && *instance_id == CardInstanceId(1))));
    assert!(events.iter().any(|e| matches!(
        e, Event::EnemyReadied { enemy } if *enemy == enemy_id)));
}

#[test]
fn ready_exhausted_cards_reengages_co_located_unengaged_enemy() {
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(1);
    let loc = test_support::test_location(10, "Synth Loc");
    let mut enemy = test_support::test_enemy(1, "Test Enemy");
    enemy.exhausted = true; // exhausted + disengaged, e.g. survived a successful Evade
    enemy.current_location = Some(LocationId(10));
    let mut state = GameStateBuilder::default()
        .with_investigator_at(test_support::test_investigator(1), LocationId(10))
        .with_location(loc)
        .with_enemy(enemy)
        .with_turn_order([inv_id])
        .build();
    let mut events = Vec::new();

    ready_exhausted_cards(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
    assert_eq!(
        state.enemies[&enemy_id].engaged_with,
        Some(inv_id),
        "readied enemy re-engages the co-located investigator (RR p.10)"
    );
    assert_event!(events, Event::EnemyReadied { enemy } if *enemy == enemy_id);
    assert_event!(events, Event::EnemyEngaged { investigator, .. } if *investigator == inv_id);
    assert_event_sequence!(
        events,
        Event::EnemyReadied { .. },
        Event::EnemyEngaged { .. },
    );
}

#[test]
fn ready_exhausted_cards_leaves_ready_cards_untouched() {
    let mut enemy = test_support::test_enemy(1, "Test Enemy");
    enemy.exhausted = false; // already ready
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_enemy(enemy)
        .build();
    let mut events = Vec::new();

    ready_exhausted_cards(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert!(
        events.is_empty(),
        "no readying events for already-ready cards"
    );
}

#[test]
fn ready_exhausted_cards_no_engage_when_no_co_located_investigator() {
    let enemy_id = EnemyId(1);
    let inv_id = InvestigatorId(1);
    let loc = test_support::test_location(10, "Synth Loc");
    let mut enemy = test_support::test_enemy(1, "Test Enemy");
    enemy.exhausted = true;
    enemy.current_location = Some(LocationId(10));
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1)) // current_location stays None — NOT co-located
        .with_location(loc)
        .with_enemy(enemy)
        .with_turn_order([inv_id])
        .build();
    let mut events = Vec::new();

    ready_exhausted_cards(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
    assert_eq!(
        state.enemies[&enemy_id].engaged_with, None,
        "no investigator at the enemy's location → no engagement"
    );
    assert_no_event!(events, Event::EnemyEngaged { .. });
}

#[test]
fn ready_exhausted_cards_keeps_existing_engagement_no_duplicate() {
    let enemy_id = EnemyId(1);
    let inv_id = InvestigatorId(1);
    let mut enemy = test_support::test_enemy(1, "Test Enemy");
    enemy.exhausted = true; // exhausted but still engaged (e.g. attacked last Enemy phase)
    enemy.engaged_with = Some(inv_id);
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_enemy(enemy)
        .build();
    let mut events = Vec::new();

    ready_exhausted_cards(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
    assert_eq!(
        state.enemies[&enemy_id].engaged_with,
        Some(inv_id),
        "an already-engaged enemy keeps its engagement"
    );
    assert_no_event!(events, Event::EnemyEngaged { .. });
}

#[test]
fn upkeep_draw_and_resource_draws_and_grants_per_active_investigator() {
    let (a, b, c) = (InvestigatorId(1), InvestigatorId(2), InvestigatorId(3));
    let mut inv_a = test_support::test_investigator(1);
    inv_a.deck = vec![CardCode::new("filler0")];
    let mut inv_b = test_support::test_investigator(2);
    inv_b.deck = vec![CardCode::new("filler1")];
    let mut inv_c = test_support::test_investigator(3);
    inv_c.status = Status::Resigned; // eliminated → skipped
    inv_c.deck = vec![CardCode::new("filler2")];
    let res_a = inv_a.resources;
    let res_b = inv_b.resources;
    let res_c = inv_c.resources;
    let hand_a = inv_a.hand.len();
    let mut state = GameStateBuilder::default()
        .with_investigator(inv_a)
        .with_investigator(inv_b)
        .with_investigator(inv_c)
        .build();
    state.turn_order = vec![a, b, c];
    let mut events = Vec::new();

    upkeep_draw_and_resource(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert_eq!(state.investigators[&a].resources, res_a + 1);
    assert_eq!(state.investigators[&b].resources, res_b + 1);
    assert_eq!(
        state.investigators[&c].resources, res_c,
        "eliminated investigator skipped"
    );
    assert_eq!(state.investigators[&a].hand.len(), hand_a + 1);
    assert_eq!(
        state.investigators[&c].deck.len(),
        1,
        "eliminated investigator did not draw"
    );
}

#[test]
fn upkeep_draw_and_resource_two_pass_ordering() {
    // All CardsDrawn events precede all ResourcesGained events.
    let (a, b) = (InvestigatorId(1), InvestigatorId(2));
    let mut inv_a = test_support::test_investigator(1);
    inv_a.deck = vec![CardCode::new("filler0")];
    let mut inv_b = test_support::test_investigator(2);
    inv_b.deck = vec![CardCode::new("filler1")];
    let mut state = GameStateBuilder::default()
        .with_investigator(inv_a)
        .with_investigator(inv_b)
        .build();
    state.turn_order = vec![a, b];
    let mut events = Vec::new();

    upkeep_draw_and_resource(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    let last_draw = events
        .iter()
        .rposition(|e| matches!(e, Event::CardsDrawn { .. }))
        .expect("draws");
    let first_gain = events
        .iter()
        .position(|e| matches!(e, Event::ResourcesGained { .. }))
        .expect("gains");
    assert!(
        last_draw < first_gain,
        "all draws must precede all resource gains"
    );
}

#[test]
fn reset_actions_sets_active_to_per_turn_and_skips_eliminated() {
    let (a, b) = (InvestigatorId(1), InvestigatorId(2));
    let mut inv_a = test_support::test_investigator(1);
    inv_a.actions_remaining = 0;
    let mut inv_b = test_support::test_investigator(2);
    inv_b.actions_remaining = 0;
    inv_b.status = Status::Defeated;
    let mut state = GameStateBuilder::default()
        .with_investigator(inv_a)
        .with_investigator(inv_b)
        .build();
    state.turn_order = vec![a, b];
    let mut events = Vec::new();

    reset_actions(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert_eq!(state.investigators[&a].actions_remaining, ACTIONS_PER_TURN);
    assert_eq!(
        state.investigators[&b].actions_remaining, 0,
        "eliminated skipped"
    );
    assert!(events.iter().any(|e| matches!(
        e, Event::ActionsRemainingChanged { investigator, new_count }
        if *investigator == a && *new_count == ACTIONS_PER_TURN)));
    assert!(!events.iter().any(|e| matches!(
        e, Event::ActionsRemainingChanged { investigator, .. } if *investigator == b)));
}

#[test]
fn reset_actions_emits_nothing_for_already_full() {
    // Emit-on-change semantics: when actions_remaining already equals
    // ACTIONS_PER_TURN, reset_actions makes no state change and emits
    // no ActionsRemainingChanged event.
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.actions_remaining = ACTIONS_PER_TURN;
    let mut state = GameStateBuilder::default().with_investigator(inv).build();
    state.turn_order = vec![id];
    let mut events = Vec::new();

    reset_actions(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert_eq!(state.investigators[&id].actions_remaining, ACTIONS_PER_TURN);
    assert!(events.is_empty(), "no event when value is unchanged");
}

#[test]
fn rotate_to_active_does_not_refresh_actions() {
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.actions_remaining = 1;
    let mut state = GameStateBuilder::default().with_investigator(inv).build();
    let mut events = Vec::new();

    rotate_to_active(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        id,
    );

    assert_eq!(state.active_investigator, Some(id));
    assert_eq!(
        state.investigators[&id].actions_remaining, 1,
        "rotate must not refresh actions"
    );
    assert!(
        events.is_empty(),
        "rotate no longer emits ActionsRemainingChanged"
    );
}

#[test]
fn round_increments_on_mythos_entry_via_driver() {
    // After the Upkeep→Mythos cascade, state.round bumps by 1.
    // The bump now lives in mythos_phase step 1.1 (this task);
    // the test asserts observable behavior, which is unchanged.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_phase(Phase::Upkeep)
        .build();
    state.turn_order = vec![id];
    state.active_investigator = None;
    state.round = 4;

    let mut events = Vec::new();
    step_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    }); // Upkeep → ... → Mythos via the cascade

    assert_eq!(state.round, 5, "round bumps on Mythos entry");
    assert_eq!(state.phase, Phase::Mythos);
}

#[test]
fn end_turn_cascades_through_upkeep_to_mythos_draw_prompt() {
    // Single investigator, non-empty deck, an exhausted in-play card.
    // After EndTurn: card readied, hand +1, resources +1, landed in
    // Mythos paused at the encounter-draw prompt and round bumped.
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.actions_remaining = 0;
    inv.deck = vec![CardCode::new("filler0"), CardCode::new("filler1")];
    let mut card = CardInPlay::enter_play(CardCode::new("filler2"), CardInstanceId(1));
    card.exhausted = true;
    inv.cards_in_play = vec![card];
    let res_before = inv.resources;
    let hand_before = inv.hand.len();
    let state = GameStateBuilder::default()
        .with_investigator(inv)
        .with_phase(Phase::Investigation)
        .with_turn_order([id])
        .with_active_investigator(id)
        .with_round(1)
        // Mid-Investigation invariant: the InvestigationPhase anchor (slice
        // 1a) + the open-turn frame (slice 2a-i) the driver leaves mid-turn.
        .with_phase_anchor(Continuation::InvestigationPhase(InvestigationPhaseFrame {
            resume: InvestigationResume::TurnBegins,
        }))
        .with_investigator_turn(id)
        .build();

    let result = test_support::take_turn_action(state, &TurnAction::EndTurn);

    // The round-ending EndTurn cascades into Mythos and pauses at the
    // step-1.4 encounter-draw prompt (AwaitingInput).
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "round-ending EndTurn pauses at the Mythos draw prompt, got {:?}",
        result.outcome
    );
    assert_eq!(result.state.phase, Phase::Mythos);
    assert_eq!(result.state.round, 2, "round bumped on Mythos entry");
    assert_eq!(result.state.current_encounter_drawer(), Some(id));
    assert_eq!(result.state.active_investigator, None);
    assert!(
        !result.state.investigators[&id].cards_in_play[0].exhausted,
        "readied"
    );
    assert_eq!(
        result.state.investigators[&id].resources,
        res_before + 1,
        "gained 1"
    );
    assert_eq!(
        result.state.investigators[&id].hand.len(),
        hand_before + 1,
        "drew 1"
    );
}
