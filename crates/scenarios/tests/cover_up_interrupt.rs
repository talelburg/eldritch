//! C5a (#236) integration: Cover Up's before-timing clue-discovery
//! interrupt and its game-end mental-trauma forced point, against the
//! synthetic Cover-Up fixture. Own process → installs TEST_REGISTRY.

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome};
use game_core::event::{Event, TraumaKind};
use game_core::scenario::{Resolution, ScenarioId};
use game_core::state::{
    Act, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, GameState, InvestigatorId,
    LocationId, Phase,
};
use game_core::test_support::{test_investigator, test_location, GameStateBuilder};
use game_core::{Action, InputResponse, PlayerAction};
use scenarios::test_fixtures::synth_cards::{SYNTH_COVER_UP_CODE, TEST_REGISTRY};

static INSTALL: Once = Once::new();
fn install() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);

/// A Cover-Up fixture instance carrying `clues`, for the threat area.
fn cover_up(clues: u8) -> CardInPlay {
    let mut c = CardInPlay::enter_play(CardCode(SYNTH_COVER_UP_CODE.into()), CardInstanceId(1));
    c.clues = clues;
    c
}

/// Investigation-phase state: the active investigator at a revealed
/// location holding `loc_clues`, with a Cover Up holding `cover_up_clues`
/// in their threat area. Chaos bag is a single +0 token so the
/// Intellect-3-vs-shroud-2 investigate always succeeds.
fn investigate_state(loc_clues: u8, cover_up_clues: u8) -> GameState {
    let mut investigator = test_investigator(1);
    investigator.threat_area.push(cover_up(cover_up_clues));
    let mut location = test_location(10, "Study");
    location.clues = loc_clues;
    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(investigator, LOC)
        .with_location(location)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_rng_seed(1)
        .build()
}

/// Run Investigate + commit-nothing, returning the state paused at the
/// clue-discovery interrupt (or resolved, if none was offered) plus the
/// last outcome.
fn investigate_to_interrupt(state: GameState) -> (GameState, EngineOutcome) {
    let r = apply(
        state,
        Action::Player(PlayerAction::Investigate { investigator: INV }),
    );
    assert!(
        matches!(r.outcome, EngineOutcome::AwaitingInput { .. }),
        "Investigate should open the commit window, got {:?}",
        r.outcome
    );
    let r = apply(
        r.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::CommitCards { indices: vec![] },
        }),
    );
    (r.state, r.outcome)
}

#[test]
fn confirm_replaces_discovery_with_discard_from_cover_up() {
    install();
    let (state, outcome) = investigate_to_interrupt(investigate_state(2, 3));
    assert!(
        matches!(outcome, EngineOutcome::AwaitingInput { .. }),
        "expected interrupt prompt, got {outcome:?}"
    );

    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert!(matches!(r.outcome, EngineOutcome::Done), "got {:?}", r.outcome);

    assert_eq!(r.state.locations[&LOC].clues, 2, "location clues unchanged");
    assert_eq!(
        r.state.investigators[&INV].clues, 0,
        "investigator discovered nothing"
    );
    let cu = r.state.investigators[&INV]
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == SYNTH_COVER_UP_CODE)
        .expect("cover up present");
    assert_eq!(cu.clues, 2, "1 clue discarded from Cover Up");
}

#[test]
fn skip_discovers_normally() {
    install();
    let (state, outcome) = investigate_to_interrupt(investigate_state(2, 3));
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));

    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );
    assert!(matches!(r.outcome, EngineOutcome::Done));

    assert_eq!(r.state.locations[&LOC].clues, 1, "location -1");
    assert_eq!(r.state.investigators[&INV].clues, 1, "investigator +1");
    let cu = r.state.investigators[&INV]
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == SYNTH_COVER_UP_CODE)
        .unwrap();
    assert_eq!(cu.clues, 3, "Cover Up untouched on Skip");
}

#[test]
fn no_interrupt_when_cover_up_has_no_clues() {
    install();
    // Cover Up with 0 clues: the reaction has no game-state potential, so
    // it is not offered — the commit window resolves straight to Done.
    let (state, outcome) = investigate_to_interrupt(investigate_state(2, 0));
    assert!(
        matches!(outcome, EngineOutcome::Done),
        "no interrupt expected, got {outcome:?}"
    );
    assert!(state.clue_interrupt_pending.is_none());
    assert_eq!(state.locations[&LOC].clues, 1, "discovery resolved normally");
    assert_eq!(state.investigators[&INV].clues, 1);
}

/// Terminal-act state whose `AdvanceAct` latches a Won resolution, with a
/// Cover Up holding `cover_up_clues` in the investigator's threat area.
fn resolving_state(cover_up_clues: u8) -> GameState {
    let mut investigator = test_investigator(1);
    investigator.clues = 1; // enough to meet the act's clue threshold
    investigator.threat_area.push(cover_up(cover_up_clues));
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_scenario_id(ScenarioId::new("unknown"))
        .build();
    state.act_deck = vec![Act {
        code: CardCode("_test_act".into()),
        clue_threshold: 1,
        resolution: Some(Resolution::Won { id: "test".into() }),
        round_end_advance: None,
    }];
    state
}

#[test]
fn game_end_emits_trauma_when_cover_up_has_clues() {
    install();
    let r = apply(
        resolving_state(3),
        Action::Player(PlayerAction::AdvanceAct { investigator: INV }),
    );
    assert!(
        r.events
            .iter()
            .any(|e| matches!(e, Event::ScenarioResolved { .. })),
        "resolution should latch; events = {:?}",
        r.events
    );
    assert!(
        r.events.iter().any(|e| matches!(
            e,
            Event::TraumaSuffered {
                kind: TraumaKind::Mental,
                amount: 1,
                ..
            }
        )),
        "expected mental trauma at game end; events = {:?}",
        r.events
    );
}

#[test]
fn game_end_emits_no_trauma_when_cover_up_empty() {
    install();
    let r = apply(
        resolving_state(0),
        Action::Player(PlayerAction::AdvanceAct { investigator: INV }),
    );
    assert!(r
        .events
        .iter()
        .any(|e| matches!(e, Event::ScenarioResolved { .. })));
    assert!(
        !r.events
            .iter()
            .any(|e| matches!(e, Event::TraumaSuffered { .. })),
        "no trauma when Cover Up is empty; events = {:?}",
        r.events
    );
}
