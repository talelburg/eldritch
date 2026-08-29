//! End-to-end `queue_forced_triggers` flow with a mock `CardRegistry`
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

use game_core::action::InputResponse;
use game_core::assert_event;
use game_core::assert_event_sequence;
use game_core::assert_no_event;
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

/// Mock act code: one `EventPattern::PhaseEnded { phase: Upkeep }` forced
/// ability dealing **1** horror — step 4.6's timing point.
const UPKEEP_END_ACT: &str = "test-upkeep-end-act";

/// Mock agenda code: one `EventPattern::RoundEnded` forced ability dealing **2**
/// horror in the `at` cell — the round end that follows step 4.6. The differing
/// amounts make the two distinguishable in the event log, which is what pins
/// their order.
const ROUND_END_AGENDA: &str = "test-round-end-agenda";

/// Mock location attachment: TWO `EventPattern::LeftLocation` forced abilities,
/// each dealing 1 horror to the leaving investigator. The Barricade-01038 shape
/// doubled, so leaving opens a lead-ordered run that *suspends* mid-move (#569).
const DOUBLE_LEFT_LOCATION: &str = "test-double-left-location";

/// Mock location attachment: one `EventPattern::LeftLocation` forced ability in
/// the **`when`** cell, dealing 1 horror to the leaving investigator. The
/// Barricade-01038 shape as the card actually prints it — *"Forced - When an
/// investigator leaves attached location"* — declarable since #721 made
/// `LeftLocation` coordinator-owned.
const WHEN_LEFT_LOCATION: &str = "test-when-left-location";

/// The same, doubled: two `when`-cell `LeftLocation` forced abilities, so
/// leaving opens a lead-ordered run that suspends *before the departure has
/// landed*. The suspension is what makes the mid-sequence board state
/// observable.
const DOUBLE_WHEN_LEFT_LOCATION: &str = "test-double-when-left-location";

/// Mock threat-area card: one `EventPattern::SkillTestResolved { Success,
/// Some(Investigate) }` forced ability dealing 1 horror to the controller. The
/// Obscuring-Fog-shape (C4c), minus the location attachment.
const AFTER_INVESTIGATE_CARD: &str = "test-after-investigate";

/// Mock locations exercising the forced scan's **eligibility gate** (#786): the
/// `EnteredLocation` horror ability of [`HORROR_ATTIC`], carrying an
/// `Ability::eligibility` tag that respectively passes, fails, and has no
/// registered predicate at all. RR p.2: an ability with no potential to change
/// the game state does not initiate — so the last two must fire nothing, the
/// unresolvable one because a host that cannot evaluate its own gate must not
/// guess in the ability's favour.
const GATED_ELIGIBLE: &str = "test-gated-eligible";
const GATED_INELIGIBLE: &str = "test-gated-ineligible";
const GATED_UNKNOWN_TAG: &str = "test-gated-unknown-tag";

/// Tags resolved by [`mock_native_eligibility_for`]. `UNKNOWN_TAG` is
/// deliberately absent from it.
const ALWAYS_TAG: &str = "test:always";
const NEVER_TAG: &str = "test:never";
const UNKNOWN_TAG: &str = "test:unregistered";

/// Returns metadata for `TEST_INV` (used by `test_investigator`) so that
/// capacity reads (`max_health()` / `max_sanity()`) work when this registry
/// is installed. All other codes return `None`.
fn mock_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    game_core::test_support::metadata_for_test_inv(code)
}

/// The eligibility-gated half of [`mock_abilities_for`] (#786): the
/// [`HORROR_ATTIC`] on-enter ability carrying one of the three tags.
fn gated_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    let tag = match code.as_str() {
        GATED_ELIGIBLE => ALWAYS_TAG,
        GATED_INELIGIBLE => NEVER_TAG,
        GATED_UNKNOWN_TAG => UNKNOWN_TAG,
        _ => return None,
    };
    Some(vec![forced_on_event(
        EventPattern::EnteredLocation,
        EventTiming::After,
        deal_horror(InvestigatorTarget::You, 1u8),
    )
    .with_eligibility(tag)])
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
    } else if code.as_str() == UPKEEP_END_ACT {
        Some(vec![forced_on_event(
            EventPattern::PhaseEnded {
                phase: DslPhase::Upkeep,
            },
            EventTiming::After,
            deal_horror(InvestigatorTarget::You, 1u8),
        )])
    } else if code.as_str() == ROUND_END_AGENDA {
        Some(vec![forced_on_event(
            EventPattern::RoundEnded,
            EventTiming::At,
            deal_horror(InvestigatorTarget::You, 2u8),
        )])
    } else if code.as_str() == DOUBLE_LEFT_LOCATION {
        Some(vec![
            forced_on_event(
                EventPattern::LeftLocation,
                EventTiming::After,
                deal_horror(InvestigatorTarget::You, 1u8),
            ),
            forced_on_event(
                EventPattern::LeftLocation,
                EventTiming::After,
                deal_horror(InvestigatorTarget::You, 1u8),
            ),
        ])
    } else if code.as_str() == WHEN_LEFT_LOCATION {
        Some(vec![forced_on_event(
            EventPattern::LeftLocation,
            EventTiming::When,
            deal_horror(InvestigatorTarget::You, 1u8),
        )])
    } else if code.as_str() == DOUBLE_WHEN_LEFT_LOCATION {
        Some(vec![
            forced_on_event(
                EventPattern::LeftLocation,
                EventTiming::When,
                deal_horror(InvestigatorTarget::You, 1u8),
            ),
            forced_on_event(
                EventPattern::LeftLocation,
                EventTiming::When,
                deal_horror(InvestigatorTarget::You, 1u8),
            ),
        ])
    } else if code.as_str() == END_OF_TURN_CARD {
        Some(vec![forced_on_event(
            EventPattern::EndOfTurn,
            EventTiming::After,
            deal_horror(InvestigatorTarget::You, 1u8),
        )])
    } else if let Some(abilities) = gated_abilities_for(code) {
        Some(abilities)
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

fn mock_native_eligibility_for(tag: &str) -> Option<game_core::card_registry::EligibilityFn> {
    fn always(_: &game_core::GameState, _: &game_core::engine::EvalContext) -> bool {
        true
    }
    fn never(_: &game_core::GameState, _: &game_core::engine::EvalContext) -> bool {
        false
    }
    match tag {
        ALWAYS_TAG => Some(always as game_core::card_registry::EligibilityFn),
        NEVER_TAG => Some(never as game_core::card_registry::EligibilityFn),
        _ => None,
    }
}

#[ctor::ctor(unsafe)]
fn install_mock_registry() {
    let _ = game_core::card_registry::install(CardRegistry {
        metadata_for: mock_metadata_for,
        abilities_for: mock_abilities_for,
        native_effect_for: |_| None,
        native_eligibility_for: mock_native_eligibility_for,
        back_abilities_for: |_| None,
        native_condition_for: |_| None,
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
    }];
    state.agenda_index = 0;
    state
}

#[test]
fn forced_on_enemy_phase_end_fires_agenda_ability() {
    let mut state = state_with_doom_agenda();
    let mut events = Vec::new();
    let outcome =
        fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy, EventTiming::After);

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
    // The agenda ability is keyed to Enemy; firing Mythos should be a no-op.
    let mut state = state_with_doom_agenda();
    let mut events = Vec::new();
    let outcome =
        fire_forced_on_phase_end(&mut state, &mut events, Phase::Mythos, EventTiming::After);

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
    for phase in [Phase::Mythos, Phase::Investigation, Phase::Upkeep] {
        let mut state = state_with_doom_agenda();
        let mut events = Vec::new();
        let outcome = fire_forced_on_phase_end(&mut state, &mut events, phase, EventTiming::After);

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
    let inv = test_investigator(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .build();
    // "plain-agenda" → None from mock registry.
    state.agenda_deck = vec![Agenda {
        code: CardCode("plain-agenda".into()),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;

    let mut events = Vec::new();
    let outcome =
        fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy, EventTiming::After);

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror(), 0);
    assert!(events.is_empty(), "no events for agenda with no abilities");
}

#[test]
fn forced_on_phase_end_no_op_when_no_act_or_agenda() {
    // Empty decks — common fixture shape for tests not modeling scenarios.
    let inv = test_investigator(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .build();
    // state.agenda_deck / act_deck are empty by default from GameStateBuilder.

    let mut events = Vec::new();
    let outcome =
        fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy, EventTiming::After);

    assert_eq!(outcome, EngineOutcome::Done);
    assert!(events.is_empty(), "no events when decks are empty");
}

#[test]
fn forced_on_phase_end_no_op_when_no_lead_investigator() {
    // No turn_order set → no lead investigator → early return.
    let mut state = state_with_doom_agenda();
    state.turn_order.clear();

    let mut events = Vec::new();
    let outcome =
        fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy, EventTiming::After);

    assert_eq!(outcome, EngineOutcome::Done);
    assert!(events.is_empty(), "no events without a lead investigator");
}

#[test]
fn forced_on_phase_end_fires_act_ability() {
    let inv = test_investigator(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .build();
    // Set current act to DOOM_ACT, no matching agenda (plain code → None).
    state.act_deck = vec![Act {
        code: CardCode(DOOM_ACT.into()),
        clue_threshold: 3,
    }];
    state.act_index = 0;
    state.agenda_deck = vec![Agenda {
        code: CardCode("plain-agenda".into()),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;

    let mut events = Vec::new();
    let outcome =
        fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy, EventTiming::After);

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
    let outcome = fire_forced_at_end_of_turn(
        &mut state,
        &mut events,
        InvestigatorId(1),
        EventTiming::After,
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror(), 1);
    assert_event!(
        events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
    );
}

#[test]
fn fire_forced_at_end_of_turn_no_op_without_threat_area_card() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .build();

    let mut events = Vec::new();
    let outcome = fire_forced_at_end_of_turn(
        &mut state,
        &mut events,
        InvestigatorId(1),
        EventTiming::After,
    );

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

// ── #569: an emit queues, so every site with post-emit work rides a frame ─────

/// Both mock board cards carry a `PhaseEnded { Enemy }` forced ability, so step
/// 3.4 has two simultaneous hits and the lead orders them (#213) — a suspension
/// that outlives the `apply()`.
///
/// Regression (#569): `enemy_phase_end` used to pop the Enemy anchor *before*
/// emitting and push the Upkeep anchor only after, so the ordering run closed
/// onto an empty continuation stack: `phase` stuck at `Enemy` forever, every
/// later input rejected. Now the anchor is re-parked beneath the run and the
/// transition runs on its re-exposure.
#[test]
fn two_forced_at_enemy_phase_end_resolve_and_the_phase_still_transitions() {
    let state = board_with_two_phase_end_forced();

    // EndTurn cascades Investigation → Enemy → step 3.4 (no enemies, so the
    // attack loop drains and the final window auto-skips into `enemy_phase_end`).
    let paused = end_turn(state);

    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "2+ forced at the enemy phase end must present the lead a choice; got {:?}",
        paused.outcome,
    );
    assert_eq!(
        paused.state.investigators[&InvestigatorId(1)].horror(),
        0,
        "no forced effect resolves until the lead orders them",
    );
    // The load-bearing bit: the phase's own frame is still there to resume onto.
    assert!(
        paused
            .state
            .continuations
            .iter()
            .any(|c| matches!(c, game_core::state::Continuation::EnemyPhase { .. })),
        "the Enemy anchor must survive beneath the ordering run; stack = {:?}",
        paused.state.continuations,
    );

    // Order them: each pick resolves one, and the second closes the run.
    let first = resolve_pick(paused.state, 0);
    assert_eq!(first.state.investigators[&InvestigatorId(1)].horror(), 1);
    let second = resolve_pick(first.state, 0);

    assert_eq!(
        second.state.investigators[&InvestigatorId(1)].horror(),
        2,
        "both forced abilities resolve once ordered",
    );
    assert_ne!(
        second.state.phase,
        Phase::Enemy,
        "the Enemy → Upkeep transition must run once the forced run closes, \
         not stall the phase; stack = {:?}",
        second.state.continuations,
    );
    assert!(
        !second.state.continuations.is_empty(),
        "the game must never be left with an empty continuation stack",
    );
}

/// A `LeftLocation` forced that suspends (two attachment abilities → an ordering
/// run) must not cost the move its entered-location half.
///
/// Regression (#569): `move_primary_effect` returned the suspension, and nothing
/// resumed it — `engage_ready_enemies_on_enter` and the `EnteredLocation` emit
/// were skipped permanently. Both now ride the `MoveEnter` frame parked beneath
/// the emit, which also fixes their order: leaving resolves before entering.
#[test]
fn suspending_left_location_forced_still_engages_and_fires_entered_location() {
    use game_core::state::{CardInPlay, CardInstanceId, EnemyId};

    let mut from = test_location(10, "Hallway");
    from.connections = vec![LocationId(11)];
    // Two `LeftLocation` forced abilities on the left location's attachment.
    from.attachments.push(CardInPlay::enter_play(
        CardCode(DOUBLE_LEFT_LOCATION.into()),
        CardInstanceId(7),
    ));
    // The destination has its own on-enter forced (1 horror) and a ready enemy.
    let mut attic = test_location(11, "Attic");
    attic.code = CardCode(HORROR_ATTIC.into());

    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.actions_remaining = 3;

    let mut enemy = game_core::test_support::test_enemy(1, "Lurker");
    enemy.current_location = Some(LocationId(11));

    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(from)
        .with_location(attic)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        .with_investigator_turn(InvestigatorId(1))
        .build();
    state.enemies.insert(EnemyId(1), enemy);

    let paused = move_action(state, InvestigatorId(1), LocationId(11));
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "the two LeftLocation forced abilities must open an ordering run",
    );
    assert_eq!(
        paused.state.investigators[&InvestigatorId(1)].horror(),
        0,
        "leaving resolves nothing until the lead orders the two abilities",
    );

    let first = resolve_pick(paused.state, 0);
    let done = resolve_pick(first.state, 0);

    assert_eq!(
        done.state.investigators[&InvestigatorId(1)].horror(),
        3,
        "2 horror from leaving, then 1 from the entered location's forced — the \
         enter half is not skipped by the suspension",
    );
    assert_eq!(
        done.state.enemies[&EnemyId(1)].engaged_with,
        Some(InvestigatorId(1)),
        "the destination's ready enemy engages on entry, after the move resumes",
    );
    // Ordering: leaving fully resolves before entering fires. This last `apply`
    // holds the tail of the sequence — the second LeftLocation ability, then the
    // engage, then the entered location's forced.
    let horror_positions: Vec<usize> = done
        .events
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e, Event::HorrorTaken { .. }))
        .map(|(i, _)| i)
        .collect();
    let engaged = done
        .events
        .iter()
        .position(|e| matches!(e, Event::EnemyEngaged { .. }))
        .expect("the auto-engage must emit EnemyEngaged");
    assert_eq!(
        horror_positions.len(),
        2,
        "this apply resolves the second LeftLocation ability and the entered \
         location's forced; events = {:?}",
        done.events,
    );
    assert!(
        horror_positions[0] < engaged && engaged < horror_positions[1],
        "the last LeftLocation ability resolves, then the engage, then the \
         EnteredLocation forced; events = {:?}",
        done.events,
    );
}

/// A `when`-cell `LeftLocation` forced resolves **before** the departure lands,
/// and the entered-location half still resolves after it (#721 over #569).
///
/// `glossary/When.md`: *"the moment immediately after the specified timing point
/// or triggering condition initiates, but before its impact upon the game state
/// resolves"* — and the departure's impact is `InvestigatorMoved`. Barricade
/// 01038's *"Forced - When an investigator leaves attached location: Discard
/// Barricade."* is the real card this shape stands in for; its own end-to-end
/// coverage is `cards/tests/barricade.rs`.
#[test]
fn a_when_cell_left_location_forced_resolves_before_the_departure_lands() {
    use game_core::state::{CardInPlay, CardInstanceId, EnemyId};

    let mut from = test_location(10, "Hallway");
    from.connections = vec![LocationId(11)];
    from.attachments.push(CardInPlay::enter_play(
        CardCode(WHEN_LEFT_LOCATION.into()),
        CardInstanceId(7),
    ));
    // The destination has its own on-enter forced (1 horror) and a ready enemy,
    // so the whole tail of the move is visible in one event log.
    let mut attic = test_location(11, "Attic");
    attic.code = CardCode(HORROR_ATTIC.into());

    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.actions_remaining = 3;

    let mut enemy = game_core::test_support::test_enemy(1, "Lurker");
    enemy.current_location = Some(LocationId(11));

    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(from)
        .with_location(attic)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        .with_investigator_turn(InvestigatorId(1))
        .build();
    state.enemies.insert(EnemyId(1), enemy);

    let r = move_action(state, InvestigatorId(1), LocationId(11));
    assert!(
        !matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "the `when` cell is walked on a coordinator-owned condition: {:?}",
        r.outcome,
    );
    // The whole sequence, in order: the interrupt, then the departure's own
    // impact, then the arrival's engage, then the entered location's forced.
    assert_event_sequence!(
        r.events,
        Event::HorrorTaken { .. },
        Event::InvestigatorMoved { .. },
        Event::EnemyEngaged { .. },
        Event::HorrorTaken { .. },
    );
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].horror(),
        2,
        "1 horror from leaving, 1 from the entered location's forced",
    );
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].current_location,
        Some(LocationId(11)),
        "the departure still lands",
    );
}

/// The destination reveal is the **arrival's** business, not the departure's:
/// it rides the `MoveEnter` frame parked beneath the emit, so it lands after
/// the whole `LeftLocation` sequence and before the entered location's own
/// forced ability.
///
/// #721 moved it there. Left in `move_primary_effect` it would have revealed
/// the destination before the investigator had even left the location they
/// were on — Rules Reference p.14 reveals a location when an investigator
/// *enters* it.
#[test]
fn the_destination_reveal_belongs_to_the_arrival_not_the_departure() {
    use game_core::state::{CardInPlay, CardInstanceId};

    let mut from = test_location(10, "Hallway");
    from.connections = vec![LocationId(11)];
    from.attachments.push(CardInPlay::enter_play(
        CardCode(WHEN_LEFT_LOCATION.into()),
        CardInstanceId(7),
    ));
    let mut attic = test_location(11, "Attic");
    attic.code = CardCode(HORROR_ATTIC.into());
    attic.revealed = false;

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

    let r = move_action(state, InvestigatorId(1), LocationId(11));
    assert_event_sequence!(
        r.events,
        Event::HorrorTaken { .. },
        Event::InvestigatorMoved { .. },
        Event::LocationRevealed { .. },
        Event::HorrorTaken { .. },
    );
    assert!(
        r.state.locations[&LocationId(11)].revealed,
        "the destination is still revealed by the move",
    );
}

/// The mid-sequence board, observed directly: with two `when`-cell
/// `LeftLocation` abilities the lead has to order them, and the move suspends
/// with the departure **not yet resolved** — the investigator still standing on
/// the location they are leaving, the enemy engaged with them still beside them.
///
/// This is the state-level counterpart to the event-order assertion above, and
/// the one an event log cannot make: before #721 the caller had already moved
/// both by the time any ability ran. It also pins that the engaged enemy still
/// rides along (`glossary/Enemy_Engagement.md`: such an enemy *"remains engaged
/// and moves to the new location simultaneously with the investigator"*) when the
/// departure finally lands.
#[test]
fn a_suspended_when_cell_sees_the_investigator_still_at_the_location_they_are_leaving() {
    use game_core::state::{CardInPlay, CardInstanceId, EnemyId};

    let mut from = test_location(10, "Hallway");
    from.connections = vec![LocationId(11)];
    from.attachments.push(CardInPlay::enter_play(
        CardCode(DOUBLE_WHEN_LEFT_LOCATION.into()),
        CardInstanceId(7),
    ));
    let mut attic = test_location(11, "Attic");
    attic.connections = vec![LocationId(10)];

    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.actions_remaining = 3;

    // Engaged, and exhausted so the departure is not followed by a re-engage at
    // the destination muddying the reading.
    let mut enemy = game_core::test_support::test_enemy(1, "Lurker");
    enemy.current_location = Some(LocationId(10));
    enemy.engaged_with = Some(InvestigatorId(1));
    enemy.exhausted = true;

    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(from)
        .with_location(attic)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        .with_investigator_turn(InvestigatorId(1))
        .build();
    state.enemies.insert(EnemyId(1), enemy);

    let paused = move_action(state, InvestigatorId(1), LocationId(11));
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "the two `when`-cell abilities must open an ordering run",
    );
    assert_eq!(
        paused.state.investigators[&InvestigatorId(1)].current_location,
        Some(LocationId(10)),
        "the `when` cell interrupts the departure: it has not landed yet",
    );
    assert_eq!(
        paused.state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(10)),
        "nor has the engaged enemy been dragged along yet",
    );
    assert_no_event!(paused.events, Event::InvestigatorMoved { .. });

    let done = resolve_pick(resolve_pick(paused.state, 0).state, 0);
    assert_eq!(
        done.state.investigators[&InvestigatorId(1)].current_location,
        Some(LocationId(11)),
        "the departure lands once the `when` cell is done",
    );
    let enemy = &done.state.enemies[&EnemyId(1)];
    assert_eq!(
        enemy.current_location,
        Some(LocationId(11)),
        "the engaged enemy rides along at the resolve step",
    );
    assert_eq!(enemy.engaged_with, Some(InvestigatorId(1)), "still engaged");
}

/// Step 4.6's `PhaseEnded { Upkeep }` forced ability resolves before the round
/// end that follows it.
///
/// Regression (#569): `upkeep_phase_end` emitted the phase end, read the `Done`
/// as "resolved", and emitted `RoundEnded` — pushing the round-end coordinator
/// *above* the phase-end ability it had just queued, so the round end resolved
/// first. The round-end emit now lives in the anchor's `AfterPhaseEndForced`
/// resume arm, which the loop reaches only once that ability has resolved.
#[test]
fn upkeep_phase_end_forced_resolves_before_the_round_end() {
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_location(10, "Study"))
        .with_phase(Phase::Upkeep)
        .with_turn_order([InvestigatorId(1)])
        .with_phase_anchor(game_core::state::Continuation::UpkeepPhase {
            resume: game_core::state::UpkeepResume::Begins,
        })
        .build();
    state.act_deck = vec![Act {
        code: CardCode(UPKEEP_END_ACT.into()),
        clue_threshold: 3,
    }];
    state.act_index = 0;
    state.agenda_deck = vec![Agenda {
        code: CardCode(ROUND_END_AGENDA.into()),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;

    let mut events = Vec::new();
    let _ = game_core::test_support::run_upkeep_round_end(&mut state, &mut events);

    let phase_end = events
        .iter()
        .position(|e| matches!(e, Event::HorrorTaken { amount: 1, .. }))
        .expect("the PhaseEnded(Upkeep) forced ability must resolve");
    let round_end = events
        .iter()
        .position(|e| matches!(e, Event::HorrorTaken { amount: 2, .. }))
        .expect("the RoundEnded forced ability must resolve");
    assert!(
        phase_end < round_end,
        "step 4.6's phase-end ability resolves before the round end it precedes; \
         events = {events:?}",
    );
}

/// Investigation-phase board with one investigator out of actions and both mock
/// board cards keyed to `PhaseEnded { Enemy }` — the two-hit step-3.4 fixture.
fn board_with_two_phase_end_forced() -> game_core::state::GameState {
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.actions_remaining = 0;
    // A card to draw at Upkeep 4.4, so the empty-deck horror penalty doesn't
    // muddy the horror assertions downstream of the transition.
    inv.deck = vec![CardCode("filler-card".into())];

    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_location(10, "Study"))
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        .with_phase_anchor(game_core::state::Continuation::InvestigationPhase {
            resume: game_core::state::InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(InvestigatorId(1))
        .build();
    state.act_deck = vec![Act {
        code: CardCode(DOOM_ACT.into()),
        clue_threshold: 3,
    }];
    state.act_index = 0;
    state.agenda_deck = vec![Agenda {
        code: CardCode(DOOM_AGENDA.into()),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;
    state
}

/// Submit the open-turn `EndTurn` action through the enumeration round-trip.
fn end_turn(state: game_core::state::GameState) -> game_core::ApplyResult {
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
}

/// Resolve an open prompt by picking `option`.
fn resolve_pick(state: game_core::state::GameState, option: u32) -> game_core::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(option)),
        }),
    )
}

// (Removed `two_simultaneous_forced_triggers_resolve_in_order`, Slice D #423: it
// was a pre-#213 stand-in that fired 2+ forced directly through
// `queue_forced_triggers` in a fixed order. The production route for 2+
// simultaneous forced is the lead-ordered run, covered by
// `two_simultaneous_forced_triggers_present_a_choice` +
// `two_simultaneous_forced_triggers_resolved_in_lead_chosen_order` through `apply`.)

// #786: the forced scan asks the same eligibility question the reaction-offer
// scan does, so a forced ability's `eligibility` tag gates *initiation*. Rules
// Reference, "Ability" -> "Forced Abilities" (p.2):
//
//   "If a forced ability does not have the potential to change the game state,
//    the ability does not initiate."
//
// Three tags, one per outcome: a passing predicate fires, a failing one does
// not, and one with no registered predicate is suppressed rather than resolved
// — the reaction side's posture, so a half-installed host never fires a gate it
// cannot evaluate.

/// Enter `code`'s location and return the entering investigator's horror, so a
/// gated `EnteredLocation` forced is measured by whether its 1 horror landed.
fn horror_after_entering(code: &str) -> u8 {
    let mut loc = test_location(10, "Attic");
    loc.code = CardCode(code.into());
    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_investigator(1), LocationId(10))
        .with_location(loc)
        .with_active_investigator(InvestigatorId(1))
        .build();

    let mut events = Vec::new();
    let outcome = fire_forced_on_enter(&mut state, &mut events, InvestigatorId(1), LocationId(10));
    assert_eq!(outcome, EngineOutcome::Done);
    state.investigators[&InvestigatorId(1)].horror()
}

#[test]
fn a_forced_with_a_satisfied_eligibility_tag_still_fires() {
    assert_eq!(horror_after_entering(GATED_ELIGIBLE), 1);
}

#[test]
fn a_forced_with_an_unmet_eligibility_tag_does_not_initiate() {
    assert_eq!(
        horror_after_entering(GATED_INELIGIBLE),
        0,
        "a false eligibility predicate keeps the forced out of the candidate list",
    );
}

#[test]
fn a_forced_whose_eligibility_tag_has_no_predicate_is_suppressed() {
    assert_eq!(
        horror_after_entering(GATED_UNKNOWN_TAG),
        0,
        "an unresolvable gate suppresses rather than guessing in the ability's favour",
    );
}
