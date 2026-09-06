//! The scenario-resolution hook, against a locally-built scenario registry.
//!
//! Two ways a scenario ends, both latched by the push model: walking the act
//! deck past its terminal card latches `GameState.ending = Resolution(R1)` and
//! emits `Event::ScenarioResolved`; doom crossing the terminal agenda's
//! threshold latches at Mythos step 1.3, *before* the 1.4 draw (#566), so the
//! draw prompt is cancelled rather than surfaced.
//!
//! Moved down from `crates/scenarios/tests/synthetic_resolution.rs` (#873).
//! What these tests need is a two-card act deck that ends in a couple of clicks
//! and a scenario module to hang `apply_resolution` off — no scenario *content*
//! at all — so under ADR 0016 the module shell and its `setup()` state are built
//! here rather than pulled from a shared fixture, and the registry installed is
//! this file's rather than the `scenarios` crate's. Building it locally also
//! makes it structural that nothing under `src/` can start a toy scenario.

use game_core::card_data::{CardKind, CardMetadata};
use game_core::dsl::{gain_resources, revelation, InvestigatorTarget};
use game_core::engine::apply;
use game_core::event::Event;
use game_core::scenario::{
    ResolutionId, ScenarioEnding, ScenarioId, ScenarioModule, ScenarioRegistry,
};
use game_core::seat_and_open;
use game_core::state::{
    Act, Agenda, CardCode, ChaosBag, ChaosToken, GameState, InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{
    take_turn_action, terminal_code, test_location, GameStateBuilder, MockRegistry, TEST_INV,
};
use game_core::{assert_event, Action, EngineOutcome, InputResponse, PlayerAction, TurnAction};

/// Per-binary code prefix (ADR 0016: probe cards are test-local).
const PREFIX: &str = "_sr_";

/// The scenario id this file's module answers to.
const SCENARIO_ID: &str = "_sr_scenario";

/// The one card the encounter deck holds. A probe, not a stand-in for a printed
/// treachery: its only job is to give the Mythos step-1.4 draw something to
/// resolve while the doom walk runs, so its Revelation is the smallest effect
/// that observably resolves.
const TREACHERY: &str = "_sr_treachery";

fn treachery_metadata() -> CardMetadata {
    CardMetadata {
        code: TREACHERY.to_owned(),
        name: "Probe Treachery".to_owned(),
        text: Some("Revelation - You gain 1 resource.".to_owned()),
        traits: Vec::new(),
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: false,
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    }
}

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::scenario_registry::install(ScenarioRegistry { module_for });
    // `install` composes `abilities_for_terminal`, which serves the reverses of
    // the terminal act/agenda cards `setup` seeds — without them a terminal
    // advance would reach no ending (ADR 0013).
    MockRegistry::new()
        .with_card(treachery_metadata())
        .with_abilities(TREACHERY, || {
            vec![revelation(gain_resources(InvestigatorTarget::You, 1))]
        })
        .install();
}

// ------------------------------------------------------------------
// The locally-built scenario module
// ------------------------------------------------------------------

/// The initial state: one revealed location to seat onto, a chaos bag, one
/// encounter card, and two-card act and agenda decks whose **last** card is the
/// synthetic terminal card (ADR 0013) — act → R1, agenda → R2.
fn setup() -> GameState {
    let mut location = test_location(10, "Resolution Location");
    location.code = CardCode::new(format!("{PREFIX}loc"));

    let mut state = GameStateBuilder::new()
        .with_location(location)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_scenario_id(ScenarioId::new(SCENARIO_ID))
        .build();
    // `seat_and_open` places roster investigators at `starting_location` and
    // reveals it; point it at the already-revealed location above.
    state.starting_location = Some(LocationId(10));
    state.encounter_deck.push_back(CardCode::new(TREACHERY));
    state.agenda_deck = vec![
        Agenda {
            code: CardCode::new(format!("{PREFIX}agenda_1")),
            doom_threshold: 2,
        },
        Agenda {
            code: terminal_code(2),
            doom_threshold: 2,
        },
    ];
    state.act_deck = vec![
        Act {
            code: CardCode::new(format!("{PREFIX}act_1")),
            clue_threshold: 2,
        },
        Act {
            code: terminal_code(1),
            clue_threshold: 2,
        },
    ];
    state
}

/// No-op: these tests assert on the ending being latched and the event emitted,
/// not on what a campaign log does with it.
fn apply_resolution(_ending: ScenarioEnding, _state: &mut GameState, _events: &mut Vec<Event>) {}

static MODULE: ScenarioModule = ScenarioModule {
    resolve_symbol: None,
    setup,
    apply_resolution,
    layout: &[],
};

fn module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    (id.as_str() == SCENARIO_ID).then_some(&MODULE)
}

fn roster() -> Vec<game_core::action::RosterEntry> {
    vec![game_core::action::RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck: vec![],
    }]
}

/// Drive a sequence of actions from an initial state, collecting all
/// events. Returns the final state and the concatenation of all event
/// vecs.
fn drive(initial_state: GameState, actions: Vec<Action>) -> (GameState, Vec<Event>) {
    let mut state = initial_state;
    let mut all_events = Vec::new();
    for action in actions {
        let result = apply(state, action);
        all_events.extend(result.events);
        state = result.state;
    }
    (state, all_events)
}

#[test]
fn scenario_resolves_won_via_act_advance() {
    let inv = InvestigatorId(1);

    // seat_and_open + close the mulligan window -> Investigation, round 1.
    let state = seat_and_open(setup(), &roster()).state;
    let (mut state, _) = drive(
        state,
        vec![Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        })],
    );
    assert_eq!(state.phase, Phase::Investigation);

    // Seed enough clues to advance both acts (2 + 2), then spend twice.
    state.investigators.get_mut(&inv).unwrap().clues = 4;
    let mut all_events = Vec::new();
    let r = take_turn_action(state, &TurnAction::AdvanceAct { investigator: inv }); // act 0 -> 1
    all_events.extend(r.events);
    let state = r.state;
    let r = take_turn_action(state, &TurnAction::AdvanceAct { investigator: inv }); // act 1 -> Won
    all_events.extend(r.events);
    let (state, events) = (r.state, all_events);

    assert_event!(
        events,
        Event::ScenarioResolved { ending: ScenarioEnding::Resolution(id) }
            if *id == ResolutionId::new(1)
    );
    assert!(state.ending.is_some());
}

#[test]
fn scenario_resolves_lost_via_doom() {
    // seat_and_open + close mulligan -> Investigation, round 1.
    let state = seat_and_open(setup(), &roster()).state;
    let (mut state, _) = drive(
        state,
        vec![Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        })],
    );

    // Each round: EndTurn cascades into Mythos, which adds doom (and may
    // advance the agenda) before pausing at step 1.4 for the encounter
    // draw; DrawEncounterCard then resolves the draw and completes Mythos
    // back to Investigation. The EndTurn that enters the round whose Mythos
    // crosses the terminal agenda's threshold latches Lost at step 1.3 —
    // *before* the 1.4 draw pause, which is exactly the case #566 names: the
    // scenario has ended, so step 1.4 never happens and the draw prompt is
    // cancelled rather than surfaced.
    //
    // Break-on-resolution rather than a fixed count: tolerates cadence
    // drift and only draws when a Mythos draw is actually pending.
    let mut doom_events = Vec::new();
    for _ in 0..12 {
        let r1 = take_turn_action(state, &TurnAction::EndTurn);
        doom_events.extend(r1.events);
        let latched = r1.state.ending.is_some();
        if latched {
            assert_eq!(
                r1.outcome,
                EngineOutcome::Done,
                "the Mythos 1.4 draw prompt must not be surfaced for an ended scenario",
            );
        }
        state = r1.state;
        if latched {
            assert!(
                state.current_encounter_drawer().is_none(),
                "no encounter draw is pending once the scenario has ended",
            );
            assert!(
                state.continuations.is_empty(),
                "no stranded frames after the ending: {:?}",
                state.continuations,
            );
            break;
        }
        if state.current_encounter_drawer().is_some() {
            let r2 = apply(
                state,
                Action::Player(PlayerAction::ResolveInput {
                    response: InputResponse::Confirm,
                }),
            );
            doom_events.extend(r2.events);
            state = r2.state;
            if state.ending.is_some() {
                break;
            }
        }
    }
    let all_events = doom_events;

    // Agenda 0 advanced once, then the terminal agenda latched Lost via doom.
    assert_event!(all_events, Event::AgendaAdvanced { from } if *from == 0);
    assert_event!(
        all_events,
        Event::ScenarioResolved {
            ending: ScenarioEnding::Resolution(_)
        }
    );
    assert!(matches!(state.ending, Some(ScenarioEnding::Resolution(_))));
}
