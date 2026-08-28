//! Cover Up's before-timing clue-discovery replacement + its game-end
//! mental-trauma forced point, against the synthetic Cover-Up fixture. Own
//! process → installs `TEST_REGISTRY`.
//!
//! Originally the C5a (#236) bespoke `clue_interrupt` seam; migrated in Axis D
//! (#336) onto a general reaction window, and since #703 that window is the
//! `when` cell of the one `DiscoverClues` triggering condition. The reaction is
//! *played* via `PickSingle` (a replacement reaction: discard-from-self then
//! `Effect::Cancel` the discovery), or declined via `Skip`.

use game_core::engine::{apply, EngineOutcome, OptionId};
use game_core::event::{Event, TraumaKind};
use game_core::scenario::{ResolutionId, ScenarioId};
use game_core::state::{
    AbilitySource, Act, CandidateSource, CardCode, CardInPlay, CardInstanceId, ChaosBag,
    ChaosToken, Continuation, EmitStep, GameState, InvestigatorId, LocationId, Phase,
    ResolutionCandidate, TimingSub,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, test_investigator, test_location, GameStateBuilder,
};
use game_core::{Action, InputResponse, PlayerAction, TurnAction};
use scenarios::test_fixtures::synth_cards::{SYNTH_COVER_UP_CODE, TEST_REGISTRY};

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(TEST_REGISTRY);
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
    let r = dispatch_turn_action_unchecked(state, &TurnAction::Investigate { investigator: INV });
    assert!(
        matches!(r.outcome, EngineOutcome::AwaitingInput { .. }),
        "Investigate should open the commit window, got {:?}",
        r.outcome
    );
    let r = apply(
        r.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    (r.state, r.outcome)
}

#[test]
fn playing_cover_up_replaces_discovery_with_discard() {
    let (state, outcome) = investigate_to_interrupt(investigate_state(2, 3));
    assert!(
        matches!(outcome, EngineOutcome::AwaitingInput { .. }),
        "expected the before-discover window, got {outcome:?}"
    );

    // Play Cover Up (the single offered candidate) → discard-from-self + cancel
    // the discovery.
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    assert!(
        matches!(r.outcome, EngineOutcome::Done),
        "got {:?}",
        r.outcome
    );

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
    // Cover Up with 0 clues: the reaction has no game-state potential, so
    // it is not offered — the commit window resolves straight to Done.
    let (state, outcome) = investigate_to_interrupt(investigate_state(2, 0));
    assert!(
        matches!(outcome, EngineOutcome::Done),
        "no window expected, got {outcome:?}"
    );
    assert!(
        state.open_windows().is_empty(),
        "no before-discover window opens when Cover Up holds no clues"
    );
    assert_eq!(
        state.locations[&LOC].clues, 1,
        "discovery resolved normally"
    );
    assert_eq!(state.investigators[&INV].clues, 1);
}

/// State paused at the `when` cell of a `count`-clue discovery, with a Cover Up
/// holding `cover_up_clues` in the threat area. Built by pushing the frames
/// directly, so `count` can be set independently of any discovery source,
/// exercising the `DiscoverClues.count` → `clue_discovery_count` →
/// discard-from-self threading and the **Cover Up discard cap** —
/// `min(count, card.clues)`, how many of the replaced clues Cover Up can
/// actually absorb.
///
/// Distinct from the **location cap** (`min(count, location.clues)`, #471),
/// which `discover_clue` applies *before* emitting the timing point: by the
/// time a window carries a `count`, that count is already what the location
/// could pay. `loc_clues = 5` in both cases below keeps these synthetic frames
/// reachable states under that cap. (The real-Investigate flows — `count == 1`
/// plain, and `count == 2` with Deduction 01039 committed — are covered above
/// and in `cards/tests/cover_up.rs`.)
///
/// All three frames are pushed, not just the window: since #703 the discovery
/// itself is the **coordinator's** resolve step, so a bare window would test a
/// state the engine never builds — and would silently never discover anything,
/// which is exactly what the cancel assertions expect to see. Mirrors what
/// `queue_event(DiscoverClues)` leaves on the stack once the coordinator has
/// opened the `when` cell: the `EmitEvent` cursor still on `When`, the cell's
/// `TimingPoint` past its forced sub, and the window on top.
fn paused_at_the_when_cell(count: u8, loc_clues: u8, cover_up_clues: u8) -> GameState {
    let mut investigator = test_investigator(1);
    investigator.threat_area.push(cover_up(cover_up_clues));
    let mut location = test_location(10, "Study");
    location.clues = loc_clues;
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(investigator, LOC)
        .with_location(location)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .build();
    let event = game_core::engine::TimingEvent::DiscoverClues {
        investigator: INV,
        location: LOC,
        count,
    };
    state.continuations.push(Continuation::EmitEvent {
        event: event.clone(),
        step: EmitStep::When,
    });
    state.continuations.push(Continuation::TimingPoint {
        event: event.clone(),
        bucket: game_core::dsl::EventTiming::When,
        sub: TimingSub::Done,
    });
    // The candidate is built via `ResolutionCandidate::new` (the struct is
    // `#[non_exhaustive]`): Cover Up's first (reaction) ability.
    state.continuations.push(Continuation::TimingPointWindow {
        event,
        bucket: game_core::dsl::EventTiming::When,
        mode: game_core::state::TimingMode::Reaction,
        candidates: vec![ResolutionCandidate::new(
            CardCode(SYNTH_COVER_UP_CODE.into()),
            INV,
            0,
            CandidateSource::Ability(AbilitySource::InPlay(CardInstanceId(1))),
        )],
    });
    state
}

#[test]
fn playing_cover_up_discards_the_full_replaced_count() {
    // count=2, Cover Up holds 3 → discover nothing, discard exactly 2.
    let r = apply(
        paused_at_the_when_cell(2, 5, 3),
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    assert!(
        matches!(r.outcome, EngineOutcome::Done),
        "got {:?}",
        r.outcome
    );
    assert_eq!(r.state.locations[&LOC].clues, 5, "location untouched");
    assert_eq!(r.state.investigators[&INV].clues, 0, "discovered nothing");
    let cu = r.state.investigators[&INV]
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == SYNTH_COVER_UP_CODE)
        .unwrap();
    assert_eq!(cu.clues, 1, "2 of 3 clues discarded from Cover Up");
    assert!(
        r.state.continuations.is_empty(),
        "the coordinator walked its remaining cells and popped: {:?}",
        r.state.continuations
    );
}

#[test]
fn playing_cover_up_caps_discard_at_held_clue_count() {
    // count=3 but Cover Up only holds 1 → discard is capped at 1 (no
    // underflow), and the discovery is still fully replaced.
    let r = apply(
        paused_at_the_when_cell(3, 5, 1),
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    assert!(
        matches!(r.outcome, EngineOutcome::Done),
        "got {:?}",
        r.outcome
    );
    assert_eq!(r.state.investigators[&INV].clues, 0, "discovered nothing");
    let cu = r.state.investigators[&INV]
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == SYNTH_COVER_UP_CODE)
        .unwrap();
    assert_eq!(cu.clues, 0, "discard capped at the 1 clue Cover Up held");
    assert!(
        r.state.continuations.is_empty(),
        "the coordinator walked its remaining cells and popped: {:?}",
        r.state.continuations
    );
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
        resolution: Some(ResolutionId::new(1)),
    }];
    state
}

#[test]
fn game_end_emits_trauma_when_cover_up_has_clues() {
    let r = dispatch_turn_action_unchecked(
        resolving_state(3),
        &TurnAction::AdvanceAct { investigator: INV },
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
    let r = dispatch_turn_action_unchecked(
        resolving_state(0),
        &TurnAction::AdvanceAct { investigator: INV },
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
