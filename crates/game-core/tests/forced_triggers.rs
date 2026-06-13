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

use game_core::assert_event;
use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::dsl::Phase as DslPhase;
use game_core::dsl::{
    deal_horror, on_event, Ability, EventPattern, EventTiming, InvestigatorTarget,
};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{Act, Agenda, CardCode, InvestigatorId, LocationId, Phase};
use game_core::test_support::{
    fire_forced_on_enter, fire_forced_on_phase_end, test_investigator, test_location,
    GameStateBuilder,
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

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    if code.as_str() == HORROR_ATTIC {
        Some(vec![on_event(
            EventPattern::EnteredLocation,
            EventTiming::After,
            deal_horror(InvestigatorTarget::You, 1),
        )])
    } else if code.as_str() == DOOM_AGENDA || code.as_str() == DOOM_ACT {
        Some(vec![on_event(
            EventPattern::PhaseEnded {
                phase: DslPhase::Enemy,
            },
            EventTiming::After,
            deal_horror(InvestigatorTarget::You, 1),
        )])
    } else if code.as_str() == DOUBLE_FORCED {
        // Two distinct forced `EnteredLocation` abilities at the same timing
        // point — exercises the 2+-simultaneous reject path.
        Some(vec![
            on_event(
                EventPattern::EnteredLocation,
                EventTiming::After,
                deal_horror(InvestigatorTarget::You, 1),
            ),
            on_event(
                EventPattern::EnteredLocation,
                EventTiming::After,
                deal_horror(InvestigatorTarget::You, 1),
            ),
        ])
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
    assert_eq!(state.investigators[&InvestigatorId(1)].horror, 1);
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
        .build();

    let result = apply(
        state,
        Action::Player(PlayerAction::Move {
            investigator: InvestigatorId(1),
            destination: LocationId(11),
        }),
    );

    assert!(
        matches!(result.outcome, EngineOutcome::Done),
        "outcome was {:?}",
        result.outcome
    );
    assert_eq!(
        result.state.investigators[&InvestigatorId(1)].current_location,
        Some(LocationId(11))
    );
    assert_eq!(result.state.investigators[&InvestigatorId(1)].horror, 1);
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
    assert_eq!(state.investigators[&InvestigatorId(1)].horror, 0);
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
        state.investigators[&InvestigatorId(1)].horror,
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
        state.investigators[&InvestigatorId(1)].horror,
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
            state.investigators[&InvestigatorId(1)].horror,
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
    assert_eq!(state.investigators[&InvestigatorId(1)].horror, 0);
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
        round_end_advance: None,
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
        state.investigators[&InvestigatorId(1)].horror,
        1,
        "lead investigator should have taken 1 horror from act forced ability"
    );
    assert_event!(
        events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
    );
}

#[test]
fn two_simultaneous_forced_triggers_reject_loudly() {
    install_mock_registry();

    let mut loc = test_location(10, "Double-Forced Room");
    loc.code = CardCode(DOUBLE_FORCED.into());

    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_investigator(1), LocationId(10))
        .with_location(loc)
        .with_active_investigator(InvestigatorId(1))
        .build();

    let mut events = Vec::new();
    let outcome = fire_forced_on_enter(&mut state, &mut events, InvestigatorId(1), LocationId(10));

    // 2+ simultaneous forced triggers reject loudly — no order is chosen.
    // `fire_forced_triggers` counts hits first, before calling `apply_effect`,
    // so the reject happens before any effect is resolved.
    assert!(
        matches!(outcome, EngineOutcome::Rejected { .. }),
        "expected Rejected for 2+ simultaneous forced triggers; got {outcome:?}"
    );
    // No horror was applied — the reject fires before any effect runs.
    assert_eq!(state.investigators[&InvestigatorId(1)].horror, 0);
    // No events were emitted on this path.
    assert!(
        events.is_empty(),
        "no events should be emitted on the 2+ reject path"
    );
}
