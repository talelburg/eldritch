//! End-to-end `fire_forced_triggers` flow with a mock `CardRegistry`
//! covering a single `EventPattern::EnteredLocation` forced ability.
//!
//! Lives at `crates/game-core/tests/` (a separate integration-test
//! binary, hence its own process and its own `OnceLock<CardRegistry>`)
//! so installing a mock registry here doesn't collide with game-core's
//! in-crate tests or with `card_registry::tests::install_is_idempotent`.
//! Mirrors `activate_ability.rs` / `on_skill_test_resolution.rs`.
//!
//! No real card carries `EventPattern::EnteredLocation` yet — the
//! first consumer will land when a scenario-structure card with a
//! location-entry forced ability is implemented. Until then, mock
//! cards are the only way to exercise the full path.

use std::sync::OnceLock;

use game_core::action::InputResponse;
use game_core::assert_event;
use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::dsl::Phase as DslPhase;
use game_core::dsl::{
    deal_horror, forced_on_event, Ability, EventPattern, EventTiming, InvestigatorTarget,
    SkillTestKind, TestOutcome,
};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{Act, Agenda, CardCode, InvestigatorId, LocationId, Phase};
use game_core::test_support::{
    fire_forced_at_end_of_turn, fire_forced_on_enter, fire_forced_on_phase_end, test_investigator,
    test_location, GameStateBuilder,
};
use game_core::{apply, Action, PlayerAction};

/// Mock location code: one `EventPattern::EnteredLocation` forced ability
/// that deals 1 horror to the entering investigator.
const HORROR_ATTIC: &str = "test-attic";

/// Mock agenda code: one `EventPattern::PhaseEnded { phase: Enemy }` forced
/// ability that deals 1 horror to the controller (lead investigator).
const DOOM_AGENDA: &str = "test-agenda";

/// Mock act code: one `EventPattern::PhaseEnded { phase: Enemy }` forced
/// ability that deals 1 horror to the controller (lead investigator).
const DOOM_ACT: &str = "test-act";

/// Mock location code: TWO `EventPattern::EnteredLocation` forced abilities,
/// both dealing 1 horror to the entering investigator. Used to test that a
/// single timing point with 2+ simultaneous forced triggers rejects loudly
/// instead of silently choosing an order.
const DOUBLE_FORCED: &str = "test-double-forced";

/// Mock threat-area card: one `EventPattern::EndOfTurn` forced ability
/// dealing 1 horror to the controller. The Frozen-in-Fear-shape (C4c),
/// minus the skill test (kept non-suspending for the C4a firing path).
const END_OF_TURN_CARD: &str = "test-end-of-turn";

/// Mock threat-area card: one `EventPattern::SkillTestResolved { Success,
/// Some(Investigate) }` forced ability dealing 1 horror to the controller. The
/// Obscuring-Fog-shape (C4c), minus the location attachment.
const AFTER_INVESTIGATE_CARD: &str = "test-after-investigate";

/// Returns metadata for `TEST_INV` (used by `test_investigator`) so that
/// capacity reads (`max_health()` / `max_sanity()`) work when this registry
/// is installed. All other codes return `None`.
fn mock_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    game_core::test_support::metadata_for_test_inv(code)
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    if code.as_str() == HORROR_ATTIC {
        Some(vec![forced_on_event(
            EventPattern::EnteredLocation,
            EventTiming::After,
            deal_horror(InvestigatorTarget::You, 1u8),
        )])
    } else if code.as_str() == DOOM_AGENDA || code.as_str() == DOOM_ACT {
        Some(vec![forced_on_event(
            EventPattern::PhaseEnded {
                phase: DslPhase::Enemy,
            },
            EventTiming::After,
            deal_horror(InvestigatorTarget::You, 1u8),
        )])
    } else if code.as_str() == DOUBLE_FORCED {
        // Two distinct forced `EnteredLocation` abilities at the same timing
        // point — exercises ordered multi-resolution (both fire in order).
        Some(vec![
            forced_on_event(
                EventPattern::EnteredLocation,
                EventTiming::After,
                deal_horror(InvestigatorTarget::You, 1u8),
            ),
            forced_on_event(
                EventPattern::EnteredLocation,
                EventTiming::After,
                deal_horror(InvestigatorTarget::You, 1u8),
            ),
        ])
    } else if code.as_str() == END_OF_TURN_CARD {
        Some(vec![forced_on_event(
            EventPattern::EndOfTurn,
            EventTiming::After,
            deal_horror(InvestigatorTarget::You, 1u8),
        )])
    } else if code.as_str() == AFTER_INVESTIGATE_CARD {
        Some(vec![forced_on_event(
            EventPattern::SkillTestResolved {
                outcome: TestOutcome::Success,
                kind: Some(SkillTestKind::Investigate),
            },
            EventTiming::After,
            deal_horror(InvestigatorTarget::You, 1u8),
        )])
    } else {
        None
    }
}

fn install_mock_registry() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = game_core::card_registry::install(CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
            native_effect_for: |_| None,
        });
    });
}

/// Submit the open-turn `Move` action via the enumeration round-trip (the typed
/// `PlayerAction::Move` removed in 2b, #447). The state must carry an
/// `InvestigatorTurn` frame so the move is offered by `legal_actions`.
fn move_action(
    state: game_core::state::GameState,
    investigator: InvestigatorId,
    destination: LocationId,
) -> game_core::ApplyResult {
    use game_core::engine::enumerate::legal_actions;
    use game_core::engine::OptionId;
    use game_core::TurnAction;

    let target = TurnAction::Move {
        investigator,
        destination,
    };
    let idx = legal_actions(&state)
        .iter()
        .position(|a| a == &target)
        .expect("Move must be a legal open-turn action");
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        }),
    )
}

#[test]
fn forced_on_enter_resolves_immediately() {
    install_mock_registry();

    let mut loc = test_location(10, "Attic");
    loc.code = CardCode(HORROR_ATTIC.into());

    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_investigator(1), LocationId(10))
        .with_location(loc)
        .with_active_investigator(InvestigatorId(1))
        .build();

    let mut events = Vec::new();
    let outcome = fire_forced_on_enter(&mut state, &mut events, InvestigatorId(1), LocationId(10));

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror(), 1);
    assert_event!(
        events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
    );
}

#[test]
fn move_into_forced_location_fires_its_effect() {
    install_mock_registry();

    // Location A (id 10) — plain starting location, connected to B.
    let mut from = test_location(10, "Hallway");
    from.connections = vec![LocationId(11)];

    // Location B (id 11) — has the forced on-enter horror ability.
    let mut attic = test_location(11, "Attic");
    attic.code = CardCode(HORROR_ATTIC.into());

    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.actions_remaining = 3;

    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(from)
        .with_location(attic)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        .with_investigator_turn(InvestigatorId(1))
        .build();

    let result = move_action(state, InvestigatorId(1), LocationId(11));

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "outcome was {:?}",
        result.outcome
    );
    assert_eq!(
        result.state.investigators[&InvestigatorId(1)].current_location,
        Some(LocationId(11))
    );
    assert_eq!(result.state.investigators[&InvestigatorId(1)].horror(), 1);
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::InvestigatorMoved {
                investigator: InvestigatorId(1),
                to: LocationId(11),
                ..
            }
        )),
        "expected InvestigatorMoved to 11 in events; got {:?}",
        result.events
    );
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, Event::HorrorTaken { amount: 1, .. })),
        "expected HorrorTaken {{ amount: 1 }} in events; got {:?}",
        result.events
    );
}

#[test]
fn forced_on_enter_no_op_when_location_has_no_abilities() {
    install_mock_registry();

    // "plain-loc" is not HORROR_ATTIC — mock registry returns None.
    let mut loc = test_location(10, "Plain Room");
    loc.code = CardCode("plain-loc".into());

    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_investigator(1), LocationId(10))
        .with_location(loc)
        .with_active_investigator(InvestigatorId(1))
        .build();

    let mut events = Vec::new();
    let outcome = fire_forced_on_enter(&mut state, &mut events, InvestigatorId(1), LocationId(10));

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror(), 0);
    assert!(
        events.is_empty(),
        "no events for a location with no forced abilities"
    );
}

// ── PhaseEnded tests ──────────────────────────────────────────────────────────

/// Build a `GameState` with the mock agenda (`test-agenda`, Enemy-phase
/// forced horror) as the current agenda and `InvestigatorId(1)` as the lead.
fn state_with_doom_agenda() -> game_core::state::GameState {
    let inv = test_investigator(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.agenda_deck = vec![Agenda {
        code: CardCode(DOOM_AGENDA.into()),
        doom_threshold: 3,
        resolution: None,
    }];
    state.agenda_index = 0;
    state
}

#[test]
fn forced_on_enemy_phase_end_fires_agenda_ability() {
    install_mock_registry();

    let mut state = state_with_doom_agenda();
    let mut events = Vec::new();
    let outcome = fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy);

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&InvestigatorId(1)].horror(),
        1,
        "lead investigator should have taken 1 horror from agenda forced ability"
    );
    assert_event!(
        events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
    );
}

#[test]
fn forced_on_phase_end_wrong_phase_fires_nothing() {
    install_mock_registry();

    // The agenda ability is keyed to Enemy; firing Mythos should be a no-op.
    let mut state = state_with_doom_agenda();
    let mut events = Vec::new();
    let outcome = fire_forced_on_phase_end(&mut state, &mut events, Phase::Mythos);

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&InvestigatorId(1)].horror(),
        0,
        "no horror for a non-matching phase"
    );
    assert!(
        events.is_empty(),
        "no events when phase doesn't match agenda ability"
    );
}

/// The three non-Enemy phases do not fire an Enemy-keyed forced ability —
/// exercises the `dsl_phase` mapping's negative side.
#[test]
fn dsl_phase_mapping_non_enemy_phases_produce_no_hits() {
    install_mock_registry();

    for phase in [Phase::Mythos, Phase::Investigation, Phase::Upkeep] {
        let mut state = state_with_doom_agenda();
        let mut events = Vec::new();
        let outcome = fire_forced_on_phase_end(&mut state, &mut events, phase);

        assert_eq!(
            outcome,
            EngineOutcome::Done,
            "expected Done for phase {phase:?}"
        );
        assert_eq!(
            state.investigators[&InvestigatorId(1)].horror(),
            0,
            "no horror for phase {phase:?} (agenda keyed to Enemy only)"
        );
        assert!(
            events.is_empty(),
            "no events for phase {phase:?}; got {events:?}"
        );
    }
}

#[test]
fn forced_on_phase_end_no_op_when_agenda_has_no_abilities() {
    install_mock_registry();

    let inv = test_investigator(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .build();
    // "plain-agenda" → None from mock registry.
    state.agenda_deck = vec![Agenda {
        code: CardCode("plain-agenda".into()),
        doom_threshold: 3,
        resolution: None,
    }];
    state.agenda_index = 0;

    let mut events = Vec::new();
    let outcome = fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy);

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror(), 0);
    assert!(events.is_empty(), "no events for agenda with no abilities");
}

#[test]
fn forced_on_phase_end_no_op_when_no_act_or_agenda() {
    install_mock_registry();

    // Empty decks — common fixture shape for tests not modeling scenarios.
    let inv = test_investigator(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .build();
    // state.agenda_deck / act_deck are empty by default from GameStateBuilder.

    let mut events = Vec::new();
    let outcome = fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy);

    assert_eq!(outcome, EngineOutcome::Done);
    assert!(events.is_empty(), "no events when decks are empty");
}

#[test]
fn forced_on_phase_end_no_op_when_no_lead_investigator() {
    install_mock_registry();

    // No turn_order set → no lead investigator → early return.
    let mut state = state_with_doom_agenda();
    state.turn_order.clear();

    let mut events = Vec::new();
    let outcome = fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy);

    assert_eq!(outcome, EngineOutcome::Done);
    assert!(events.is_empty(), "no events without a lead investigator");
}

#[test]
fn forced_on_phase_end_fires_act_ability() {
    install_mock_registry();

    let inv = test_investigator(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .build();
    // Set current act to DOOM_ACT, no matching agenda (plain code → None).
    state.act_deck = vec![Act {
        code: CardCode(DOOM_ACT.into()),
        clue_threshold: 3,
        resolution: None,
    }];
    state.act_index = 0;
    state.agenda_deck = vec![Agenda {
        code: CardCode("plain-agenda".into()),
        doom_threshold: 3,
        resolution: None,
    }];
    state.agenda_index = 0;

    let mut events = Vec::new();
    let outcome = fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy);

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&InvestigatorId(1)].horror(),
        1,
        "lead investigator should have taken 1 horror from act forced ability"
    );
    assert_event!(
        events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
    );
}

// ── EndOfTurn tests ───────────────────────────────────────────────────────────

#[test]
fn fire_forced_at_end_of_turn_resolves_threat_area_ability() {
    use game_core::state::{CardInPlay, CardInstanceId};

    install_mock_registry();
    let mut inv = test_investigator(1);
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode(END_OF_TURN_CARD.into()),
        CardInstanceId(1),
    ));
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .build();

    let mut events = Vec::new();
    let outcome = fire_forced_at_end_of_turn(&mut state, &mut events, InvestigatorId(1));

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror(), 1);
    assert_event!(
        events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
    );
}

#[test]
fn fire_forced_at_end_of_turn_no_op_without_threat_area_card() {
    install_mock_registry();
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .build();

    let mut events = Vec::new();
    let outcome = fire_forced_at_end_of_turn(&mut state, &mut events, InvestigatorId(1));

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror(), 0);
    assert!(events.is_empty());
}

#[test]
fn end_turn_fires_end_of_turn_forced_for_the_ending_investigator() {
    // End-to-end: EndTurn for a lone investigator with an EndOfTurn
    // threat-area card fires its forced effect as part of ending the
    // turn.
    use game_core::state::{CardInPlay, CardInstanceId};

    install_mock_registry();
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.actions_remaining = 0;
    // Give the investigator a non-empty deck so Upkeep 4.4
    // draw_one_with_deckout doesn't fire its "draw from empty deck"
    // horror penalty and muddy the horror assertion.
    inv.deck = vec![CardCode("filler-card".into())];
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode(END_OF_TURN_CARD.into()),
        CardInstanceId(1),
    ));
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_location(10, "Study"))
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        // Mid-Investigation invariant (slice 1a): the EndTurn cascade pops the
        // InvestigationPhase anchor at investigation_phase_end.
        .with_phase_anchor(game_core::state::Continuation::InvestigationPhase {
            resume: game_core::state::InvestigationResume::TurnBegins,
        })
        // Open-turn invariant (slice 2a-i, #393): the InvestigatorTurn frame the
        // EndTurn pops (or strands a skill test below, then pops on resume).
        .with_investigator_turn(InvestigatorId(1))
        .build();

    let result = {
        use game_core::engine::enumerate::legal_actions;
        use game_core::engine::OptionId;
        use game_core::TurnAction;
        let idx = legal_actions(&state)
            .iter()
            .position(|a| a == &TurnAction::EndTurn)
            .expect("EndTurn must be a legal open-turn action");
        apply(
            state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
            }),
        )
    };

    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, Event::HorrorTaken { amount: 1, .. })),
        "EndOfTurn forced effect must fire during EndTurn; events = {:?}",
        result.events
    );
    assert_eq!(result.state.investigators[&InvestigatorId(1)].horror(), 1);
}

// ── AfterLocationInvestigated tests ───────────────────────────────────────────

#[test]
fn fire_forced_after_investigate_resolves_threat_area_ability() {
    use game_core::state::{CardInPlay, CardInstanceId};
    use game_core::test_support::fire_forced_after_location_investigated;

    install_mock_registry();
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode(AFTER_INVESTIGATE_CARD.into()),
        CardInstanceId(1),
    ));
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_location(10, "Study"))
        .with_turn_order([InvestigatorId(1)])
        .build();

    let mut events = Vec::new();
    let outcome =
        fire_forced_after_location_investigated(&mut state, &mut events, InvestigatorId(1));

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror(), 1);
    assert_event!(
        events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
    );
}

#[test]
fn fire_forced_after_investigate_no_op_without_threat_area_card() {
    use game_core::test_support::fire_forced_after_location_investigated;

    install_mock_registry();
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_location(10, "Study"))
        .with_turn_order([InvestigatorId(1)])
        .build();

    let mut events = Vec::new();
    let outcome =
        fire_forced_after_location_investigated(&mut state, &mut events, InvestigatorId(1));

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror(), 0);
    assert!(events.is_empty());
}

#[test]
fn successful_investigate_fires_after_location_investigated_forced() {
    // End-to-end: drive a successful Investigate (shroud 0, intellect 3,
    // Numeric(0) token → always succeeds) and confirm the threat-area
    // AfterLocationInvestigated forced effect fires.
    use game_core::state::{CardInPlay, CardInstanceId, ChaosBag, ChaosToken, TokenModifiers};
    use game_core::test_support::apply_no_commits;

    install_mock_registry();
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.skills.intellect = 3;
    inv.actions_remaining = 1;
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode(AFTER_INVESTIGATE_CARD.into()),
        CardInstanceId(1),
    ));
    let mut loc = test_location(10, "Study");
    loc.shroud = 0;
    loc.clues = 1;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        .with_investigator_turn(InvestigatorId(1))
        .with_investigator(inv)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    let result = {
        use game_core::engine::enumerate::legal_actions;
        use game_core::engine::OptionId;
        use game_core::TurnAction;
        let idx = legal_actions(&state)
            .iter()
            .position(|a| {
                a == &TurnAction::Investigate {
                    investigator: InvestigatorId(1),
                }
            })
            .expect("Investigate must be a legal open-turn action");
        apply_no_commits(
            state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
            }),
        )
    };

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, Event::HorrorTaken { amount: 1, .. })),
        "AfterLocationInvestigated forced effect must fire on a successful \
         investigate; events = {:?}",
        result.events
    );
    assert_eq!(result.state.investigators[&InvestigatorId(1)].horror(), 1);
}

#[test]
fn two_simultaneous_forced_triggers_present_a_choice() {
    // Axis-B T5b (#213): 2+ forced abilities at the same timing point let the
    // lead investigator choose the order — dispatch suspends with
    // `AwaitingInput` instead of auto-resolving both in a fixed order. Driven
    // through `apply` (Move into a location with two forced on-enter abilities,
    // a terminal emit site) so the suspension round-trips.
    install_mock_registry();

    let mut from = test_location(10, "Hallway");
    from.connections = vec![LocationId(11)];
    let mut double = test_location(11, "Double-Forced Room");
    double.code = CardCode(DOUBLE_FORCED.into());

    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.actions_remaining = 3;

    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(from)
        .with_location(double)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        .with_investigator_turn(InvestigatorId(1))
        .build();

    let result = move_action(state, InvestigatorId(1), LocationId(11));

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "2+ simultaneous forced triggers must present the lead a choice; got {:?}",
        result.outcome,
    );
    assert_eq!(
        result.state.investigators[&InvestigatorId(1)].horror(),
        0,
        "no forced effect resolves until the lead orders them",
    );
}

#[test]
fn two_simultaneous_forced_triggers_resolved_in_lead_chosen_order() {
    // Resume the choice: pick each forced trigger in turn; both resolve, the
    // move completes (terminal site → Done), total 2 horror.
    install_mock_registry();

    let mut from = test_location(10, "Hallway");
    from.connections = vec![LocationId(11)];
    let mut double = test_location(11, "Double-Forced Room");
    double.code = CardCode(DOUBLE_FORCED.into());

    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.actions_remaining = 3;

    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(from)
        .with_location(double)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        .with_investigator_turn(InvestigatorId(1))
        .build();

    let paused = move_action(state, InvestigatorId(1), LocationId(11));
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    // Pick the first forced trigger.
    let after_first = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    // One forced resolved; the second is still pending (another choice or
    // its resolution), so the move isn't done yet.
    assert_eq!(
        after_first.state.investigators[&InvestigatorId(1)].horror(),
        1
    );
    assert!(matches!(
        after_first.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    // Pick the remaining forced trigger.
    let done = apply(
        after_first.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    assert!(matches!(done.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(done.state.investigators[&InvestigatorId(1)].horror(), 2);
}

// (Removed `two_simultaneous_forced_triggers_resolve_in_order`, Slice D #423: it
// was a pre-#213 stand-in that fired 2+ forced directly through
// `fire_forced_triggers` in a fixed order. The production route for 2+
// simultaneous forced is the lead-ordered run, covered by
// `two_simultaneous_forced_triggers_present_a_choice` +
// `two_simultaneous_forced_triggers_resolved_in_lead_chosen_order` through `apply`.)
