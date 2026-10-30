use super::*;
use crate::engine::enumerate::TurnAction;
use crate::engine::outcome::EngineOutcome;
use crate::event::Event;
use crate::state::{EnemyId, GameStateBuilder, InvestigatorId, LocationId, Phase, Status};
use crate::test_support::{self, enemy, location, BoardLookup as _, TestSession, TEST_REGISTRIES};
use crate::{assert_event, assert_event_sequence, assert_no_event};

#[test]
fn hand_size_discard_prompt_is_player_facing() {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).hand(["filler0"; 10]))
        .build();
    let mut events = Vec::new();
    let outcome = park_hand_size_discard(
        &mut Cx::for_test(&mut state, &mut events),
        vec![InvestigatorId(1)],
    );
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("park_hand_size_discard suspends on the discard prompt");
    };
    for forbidden in [
        "InputResponse",
        "option ids",
        "InvestigatorId(",
        "OptionId",
        "PickSingle(",
    ] {
        assert!(
            !request.prompt.contains(forbidden),
            "prompt must be player-facing, found {forbidden:?} in: {}",
            request.prompt
        );
    }
    assert!(
        request.prompt.contains("choose 2 cards to discard"),
        "{}",
        request.prompt
    );
    assert!(request.prompt.contains("Weaknesses can't be chosen"));
}

#[test]
fn upkeep_phase_emits_phase_started_and_auto_skips_to_mythos() {
    // No Fast-eligible cards / no reactions installed → the post-4.1
    // window auto-skips inline, the continuation runs, and the
    // cascade lands in Mythos.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        // Give the investigator a card so the step-4.4 draw pulls it normally
        // rather than decking out: 4.4 draws through `draw_one_with_deckout`,
        // so an empty deck applies the deckout horror penalty, which this test
        // isn't about.
        .with_seat(test_support::seat(1).deck(["filler0"]))
        .with_seating([id])
        .with_phase(Phase::Enemy)
        .build();

    let mut events = Vec::new();
    step_phase(&mut Cx::for_test(&mut state, &mut events)); // Enemy → Upkeep, cascades to Mythos

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
    let enemy = enemy(1).exhausted();
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).in_play(["filler0"]))
        .with_enemy(enemy)
        .build();
    let card = state.in_play(inv_id, "filler0");
    // A seat places cards ready; exhaust this one on the built board.
    state.investigators.get_mut(&inv_id).unwrap().cards_in_play[0].exhausted = true;
    let mut events = Vec::new();

    ready_exhausted_cards(&mut Cx::for_test(&mut state, &mut events));

    assert!(
        !state.investigators[&inv_id].cards_in_play[0].exhausted,
        "card readied"
    );
    assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
    assert!(events.iter().any(|e| matches!(
        e, Event::CardReadied { investigator, instance_id, .. }
        if *investigator == inv_id && *instance_id == card)));
    assert!(events.iter().any(|e| matches!(
        e, Event::EnemyReadied { enemy } if *enemy == enemy_id)));
}

#[test]
fn ready_exhausted_cards_reengages_co_located_unengaged_enemy() {
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(1);
    let loc = location(10);
    // exhausted + disengaged, e.g. survived a successful Evade
    let enemy = enemy(1).exhausted().at(LocationId(10));
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(LocationId(10)))
        .with_location(loc)
        .with_enemy(enemy)
        .with_seating([inv_id])
        .build();
    let mut events = Vec::new();

    ready_exhausted_cards(&mut Cx::for_test(&mut state, &mut events));

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
    // already ready
    let enemy = enemy(1);
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_enemy(enemy)
        .build();
    let mut events = Vec::new();

    ready_exhausted_cards(&mut Cx::for_test(&mut state, &mut events));

    assert!(
        events.is_empty(),
        "no readying events for already-ready cards"
    );
}

#[test]
fn ready_exhausted_cards_no_engage_when_no_co_located_investigator() {
    let enemy_id = EnemyId(1);
    let inv_id = InvestigatorId(1);
    let loc = location(10);
    let enemy = enemy(1).exhausted().at(LocationId(10));
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1)) // current_location stays None — NOT co-located
        .with_location(loc)
        .with_enemy(enemy)
        .with_seating([inv_id])
        .build();
    let mut events = Vec::new();

    ready_exhausted_cards(&mut Cx::for_test(&mut state, &mut events));

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
    // exhausted but still engaged (e.g. attacked last Enemy phase)
    let enemy = enemy(1).exhausted().engaged_with(inv_id);
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_enemy(enemy)
        .build();
    let mut events = Vec::new();

    ready_exhausted_cards(&mut Cx::for_test(&mut state, &mut events));

    assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
    assert_eq!(
        state.enemies[&enemy_id].engaged_with,
        Some(inv_id),
        "an already-engaged enemy keeps its engagement"
    );
    assert_no_event!(events, Event::EnemyEngaged { .. });
}

/// Stage the post-4.1 player window open over the Upkeep anchor, then close it,
/// so steps 4.2–4.6 run through the drive loop.
fn run_upkeep(builder: GameStateBuilder) -> TestSession {
    builder
        .with_phase(Phase::Upkeep)
        .with_phase_anchor(UpkeepPhaseFrame {
            resume: UpkeepResume::Begins,
        })
        .with_open_window(FastWindowKind::Phase(PhaseStep::UpkeepBegins))
        .session()
        .skip()
}

#[test]
fn upkeep_draws_and_grants_a_resource_to_each_active_investigator_only() {
    let (a, b, c) = (InvestigatorId(1), InvestigatorId(2), InvestigatorId(3));
    let builder = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).deck(["filler0"]))
        .with_seat(test_support::seat(2).deck(["filler1"]))
        // Resigned, so skipped.
        .with_seat(
            test_support::seat(3)
                .deck(["filler2"])
                .status(Status::Resigned),
        )
        .with_seating([a, b, c]);
    let before = builder.clone().build();
    let res_a = before.investigators[&a].resources;
    let res_b = before.investigators[&b].resources;
    let res_c = before.investigators[&c].resources;
    let hand_a = before.investigators[&a].hand.len();

    let session = run_upkeep(builder);
    let state = session.state();

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
fn upkeep_draws_in_player_order_then_grants_resources() {
    // Appendix II 4.4: "In player order, each investigator draws 1 card. Once
    // those cards have been drawn, each investigator gains 1 resource."
    let (a, b) = (InvestigatorId(1), InvestigatorId(2));
    let session = run_upkeep(
        GameStateBuilder::default()
            .with_registries(TEST_REGISTRIES)
            .with_seat(test_support::seat(1).deck(["filler0"]))
            .with_seat(test_support::seat(2).deck(["filler1"]))
            // Investigator 2 leads, so draws first.
            .with_seating([b, a]),
    );

    assert_event_sequence!(
        session.events(),
        Event::CardsDrawn { investigator, .. } if *investigator == b,
        Event::CardsDrawn { investigator, .. } if *investigator == a,
        Event::ResourcesGained { investigator, .. } if *investigator == b,
        Event::ResourcesGained { investigator, .. } if *investigator == a,
    );
}

#[test]
fn reset_actions_sets_active_to_per_turn_and_skips_eliminated() {
    let (a, b) = (InvestigatorId(1), InvestigatorId(2));
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).actions(0))
        .with_seat(test_support::seat(2).actions(0).status(Status::Defeated))
        .with_seating([a, b])
        .build();
    let mut events = Vec::new();

    reset_actions(&mut Cx::for_test(&mut state, &mut events));

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
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).actions(ACTIONS_PER_TURN))
        .with_seating([id])
        .build();
    let mut events = Vec::new();

    reset_actions(&mut Cx::for_test(&mut state, &mut events));

    assert_eq!(state.investigators[&id].actions_remaining, ACTIONS_PER_TURN);
    assert!(events.is_empty(), "no event when value is unchanged");
}

#[test]
fn rotate_to_active_does_not_refresh_actions() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).actions(1))
        .build();
    let mut events = Vec::new();

    rotate_to_active(&mut Cx::for_test(&mut state, &mut events), id);

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
    // The bump happens at the Mythos phase's step 1.1; the test asserts the
    // observable round number.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_seating([id])
        .with_phase(Phase::Upkeep)
        .with_round(4)
        .build();

    let mut events = Vec::new();
    step_phase(&mut Cx::for_test(&mut state, &mut events)); // Upkeep → ... → Mythos via the cascade

    assert_eq!(state.round, 5, "round bumps on Mythos entry");
    assert_eq!(state.phase, Phase::Mythos);
}

#[test]
fn end_turn_cascades_through_upkeep_to_mythos_draw_prompt() {
    // Single investigator, non-empty deck, an exhausted in-play card.
    // After EndTurn: card readied, hand +1, resources +1, landed in
    // Mythos paused at the encounter-draw prompt and round bumped.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            test_support::seat(1)
                .actions(0)
                .deck(["filler0", "filler1"])
                .in_play(["filler2"]),
        )
        .with_round(1)
        .open_turn(id)
        .build();
    // A seat places cards ready; exhaust this one on the built board.
    state.investigators.get_mut(&id).unwrap().cards_in_play[0].exhausted = true;
    let res_before = state.investigators[&id].resources;
    let hand_before = state.investigators[&id].hand.len();

    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::EndTurn);

    // The round-ending EndTurn cascades into Mythos and pauses at the
    // step-1.4 encounter-draw prompt.
    session.assert_at_encounter_draw();
    let result = session.state();
    assert_eq!(result.phase, Phase::Mythos);
    assert_eq!(result.round, 2, "round bumped on Mythos entry");
    assert_eq!(result.current_encounter_drawer(), Some(id));
    assert_eq!(result.active_investigator, None);
    assert!(
        !result.investigators[&id].cards_in_play[0].exhausted,
        "readied"
    );
    assert_eq!(
        result.investigators[&id].resources,
        res_before + 1,
        "gained 1"
    );
    assert_eq!(
        result.investigators[&id].hand.len(),
        hand_before + 1,
        "drew 1"
    );
}
