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
//! here rather than pulled from a shared fixture, and the registry named is
//! this file's rather than the `scenarios` crate's. Building it locally also
//! makes it structural that nothing under `src/` can start a toy scenario.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata};
use card_dsl::dsl::{self, InvestigatorTarget};
use game_core::action::RosterEntry;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{EngineOutcome, Registries};
use game_core::event::Event;
use game_core::scenario::{
    ResolutionId, ScenarioEnding, ScenarioId, ScenarioModule, ScenarioRegistry,
};
use game_core::state::{
    Act, Agenda, CardCode, ChaosBag, ChaosToken, GameState, GameStateBuilder, InvestigatorId,
    LocationId, Phase,
};
use game_core::test_support::{self, MockRegistry, TestSession, TEST_INV};

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
        weakness: None,
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    }
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        // `build` composes `abilities_for_terminal`, which serves the reverses of
        // the terminal act/agenda cards `setup` seeds — without them a terminal
        // advance would reach no ending (ADR 0013).
        cards: MockRegistry::new()
            .with_card(treachery_metadata())
            .with_abilities(TREACHERY, || {
                vec![dsl::revelation(dsl::gain_resources(
                    InvestigatorTarget::You,
                    1,
                ))]
            })
            .build(),
        scenarios: &ScenarioRegistry { module_for },
    });
    *REGISTRIES
}

// ------------------------------------------------------------------
// The locally-built scenario module
// ------------------------------------------------------------------

/// The initial state: one revealed location to seat onto, a chaos bag, one
/// encounter card, and two-card act and agenda decks whose **last** card is the
/// synthetic terminal card (ADR 0013) — act → R1, agenda → R2.
fn setup() -> GameState {
    let mut location = test_support::test_location(10, "Resolution Location");
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
            code: test_support::terminal_code(2),
            doom_threshold: 2,
        },
    ];
    state.act_deck = vec![
        Act {
            code: CardCode::new(format!("{PREFIX}act_1")),
            clue_threshold: 2,
        },
        Act {
            code: test_support::terminal_code(1),
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

fn roster() -> Vec<RosterEntry> {
    vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck: vec![],
        random_basic_weaknesses: 0,
    }]
}

const INV: InvestigatorId = InvestigatorId(1);

/// The scenario seated and opened through `seat_and_open`, with the mulligan
/// window it opens closed keeping the whole hand: Investigation, round 1, at
/// `INV`'s turn menu. Neither test asserts on the events the close emits.
fn opened() -> TestSession {
    let session = TestSession::new(
        game_core::engine::seat_and_open(setup(), &roster(), registries()).state,
        registries(),
    )
    .choose_from_hand(&[]);
    assert_eq!(session.state().phase, Phase::Investigation);
    session
}

#[test]
fn scenario_resolves_won_via_act_advance() {
    // Seed enough clues to advance both acts (2 + 2), then spend twice. The
    // clues have no step that grants them, so edit the opened state and settle
    // it again.
    let mut state = opened().state().clone();
    state.investigators.get_mut(&INV).unwrap().clues = 4;
    let session = TestSession::new(state, registries())
        .take(&TurnAction::AdvanceAct { investigator: INV }) // act 0 -> 1
        .take(&TurnAction::AdvanceAct { investigator: INV }); // act 1 -> Won

    assert_event!(
        session.events(),
        Event::ScenarioResolved { ending: ScenarioEnding::Resolution(id) }
            if *id == ResolutionId::new(1)
    );
    assert!(session.state().ending.is_some());
}

#[test]
fn scenario_resolves_lost_via_doom() {
    // Each round: EndTurn cascades into Mythos, which adds doom (and may
    // advance the agenda) before pausing at step 1.4 for the encounter
    // draw; confirming the draw then resolves it and completes Mythos
    // back to Investigation. The EndTurn that enters the round whose Mythos
    // crosses the terminal agenda's threshold latches Lost at step 1.3 —
    // *before* the 1.4 draw pause, which is exactly the case #566 names: the
    // scenario has ended, so step 1.4 never happens and the draw prompt is
    // cancelled rather than surfaced.
    //
    // Break-on-resolution rather than a fixed count: tolerates cadence
    // drift and only draws when a Mythos draw is actually pending.
    let mut session = opened();
    for _ in 0..12 {
        session = session.take(&TurnAction::EndTurn);
        if session.state().ending.is_some() {
            assert!(
                session.state().current_encounter_drawer().is_none(),
                "no encounter draw is pending once the scenario has ended",
            );
            // A stack invariant is the subject here, so this reads the stack.
            assert!(
                session.state().continuations.is_empty(),
                "no stranded frames after the ending: {:?}",
                session.state().continuations,
            );
            break;
        }
        if session.state().current_encounter_drawer().is_some() {
            session = session.confirm();
            if session.state().ending.is_some() {
                break;
            }
        }
    }

    // Agenda 0 advanced once, then the terminal agenda latched Lost via doom.
    assert_event!(session.events(), Event::AgendaAdvanced { from } if *from == 0);
    assert_event!(
        session.events(),
        Event::ScenarioResolved {
            ending: ScenarioEnding::Resolution(_)
        }
    );
    assert!(matches!(
        session.state().ending,
        Some(ScenarioEnding::Resolution(_))
    ));
    assert_eq!(
        session.finish().outcome,
        EngineOutcome::Done,
        "the Mythos 1.4 draw prompt must not be surfaced for an ended scenario",
    );
}
