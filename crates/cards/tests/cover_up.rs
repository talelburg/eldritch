//! C5c (#238) integration: Cover Up 01007 end-to-end against the real
//! `cards::REGISTRY` — the 3-clue threat-area Revelation, the before-timing
//! clue-discovery interrupt, and the game-end mental-trauma forced point.
//!
//! Own process → installs `cards::REGISTRY`. The interrupt / game-end
//! shapes mirror the C5a synthetic test (`scenarios::tests::cover_up_interrupt`),
//! now driven through the real card.

use game_core::action::EngineRecord;
use game_core::event::{Event, TraumaKind};
use game_core::scenario::{Resolution, ScenarioId};
use game_core::state::{
    Act, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, GameState, InvestigatorId,
    LocationId, Phase,
};
use game_core::test_support::{
    drive, take_turn_action, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
    TestSession,
};
use game_core::{
    apply, assert_no_event, Action, EngineOutcome, InputResponse, PlayerAction, TurnAction,
};

const COVER_UP: &str = "01007";
const DEDUCTION: &str = "01039";
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// A Cover-Up instance carrying `clues`, pre-placed in the threat area.
fn cover_up(clues: u8) -> CardInPlay {
    let mut c = CardInPlay::enter_play(CardCode::new(COVER_UP), CardInstanceId(1));
    c.clues = clues;
    c
}

// ---- Revelation: places into the threat area with 3 clues -------------

#[test]
fn revelation_puts_cover_up_in_threat_area_with_three_clues() {
    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_investigator(1), LOC)
        .with_location(test_location(10, "Study"))
        .with_turn_order([INV])
        .build();
    state.encounter_deck.push_back(CardCode::new(COVER_UP));

    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    let r = drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed { investigator: INV }),
        resolver,
    );
    assert_eq!(r.outcome, EngineOutcome::Done);

    let placed: Vec<_> = r.state.investigators[&INV]
        .threat_area
        .iter()
        .filter(|c| c.code.as_str() == COVER_UP)
        .collect();
    assert_eq!(placed.len(), 1, "exactly one Cover Up in the threat area");
    assert_eq!(placed[0].clues, 3, "enters with 3 clues");
    assert!(
        !r.state
            .encounter_discard
            .iter()
            .any(|c| c.as_str() == COVER_UP),
        "persistent treachery is not auto-discarded after its Revelation"
    );
}

// ---- Reaction: discard from Cover Up instead of discovering -----------

/// Investigation-phase state: active investigator at a 2-clue location,
/// Cover Up holding `cover_up_clues` in the threat area. +0 chaos token so
/// the Intellect-3-vs-shroud-2 Investigate always succeeds.
fn investigate_state(cover_up_clues: u8) -> GameState {
    let mut investigator = test_investigator(1);
    investigator.threat_area.push(cover_up(cover_up_clues));
    let mut location = test_location(10, "Study");
    location.clues = 2;
    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(investigator, LOC)
        .with_location(location)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_rng_seed(1)
        .build()
}

/// Investigate + commit-nothing, returning the state paused at the
/// clue-discovery interrupt (or resolved if none was offered).
fn investigate_to_interrupt(state: GameState) -> (GameState, EngineOutcome) {
    let r = take_turn_action(state, &TurnAction::Investigate { investigator: INV });
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    let r = apply(
        r.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    (r.state, r.outcome)
}

#[test]
fn playing_cover_up_discards_instead_of_discovering() {
    let (state, outcome) = investigate_to_interrupt(investigate_state(3));
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    // Play Cover Up (the single offered candidate) in the before-discover
    // window → discard-from-self + cancel the discovery (Axis D #336).
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(r.state.locations[&LOC].clues, 2, "location clues unchanged");
    assert_eq!(r.state.investigators[&INV].clues, 0, "discovered nothing");
    let cu = r.state.investigators[&INV]
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == COVER_UP)
        .unwrap();
    assert_eq!(cu.clues, 2, "1 clue discarded from Cover Up");
}

#[test]
fn skip_discovers_normally() {
    let (state, outcome) = investigate_to_interrupt(investigate_state(3));
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(r.state.locations[&LOC].clues, 1, "location -1");
    assert_eq!(r.state.investigators[&INV].clues, 1, "investigator +1");
    let cu = r.state.investigators[&INV]
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == COVER_UP)
        .unwrap();
    assert_eq!(cu.clues, 3, "Cover Up untouched on Skip");
}

// ---- Cover Up + Deduction: one discovery, capped ----------------------

/// Investigation-phase state with **Deduction 01039 in hand**: active
/// investigator at a `location_clues`-clue location, Cover Up holding 3 in the
/// threat area. Intellect 3 + Deduction's 1 intellect icon + a +0 token vs the
/// default shroud 2 → the Investigate always succeeds.
fn investigate_state_with_deduction(location_clues: u8) -> GameState {
    let mut investigator = test_investigator(1);
    investigator.threat_area.push(cover_up(3));
    investigator.hand = vec![CardCode::new(DEDUCTION)];
    let mut location = test_location(10, "Study");
    location.clues = location_clues;
    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(investigator, LOC)
        .with_location(location)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_rng_seed(1)
        .build()
}

/// Investigate committing Deduction, then play Cover Up at the single
/// before-discover window it should open.
fn investigate_with_deduction_and_play_cover_up(state: GameState) -> game_core::ApplyResult {
    TestSession::new(state)
        .take(&TurnAction::Investigate { investigator: INV })
        .resolve_choices(|c| {
            c.commit_cards(&[CardCode::new(DEDUCTION)]);
            c.pick_single(game_core::engine::OptionId(0));
        })
        .run()
}

/// The #471 bug, end-to-end through both real cards: Deduction used to spawn a
/// *second* discovery, so Cover Up's replacement fired twice and discarded 2
/// clues at a location that only ever had 1 to discover. Per the FAQ —
/// *"Deduction doesn't allow you to discover clues that aren't at that
/// location. If your location has 1 clue at it, you can only discover 1 clue
/// at most when you investigate it."* — the correct reading is one discovery
/// of 2, capped to 1, so Cover Up discards exactly 1.
#[test]
fn deduction_at_a_one_clue_location_discards_one_from_cover_up() {
    let r = investigate_with_deduction_and_play_cover_up(investigate_state_with_deduction(1));

    assert_eq!(
        r.state.locations[&LOC].clues, 1,
        "the discovery was replaced, so the location keeps its clue",
    );
    assert_eq!(r.state.investigators[&INV].clues, 0, "discovered nothing");
    assert_no_event!(r.events, Event::CluePlaced { .. });
    let cu = r.state.investigators[&INV]
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == COVER_UP)
        .unwrap();
    assert_eq!(
        cu.clues, 2,
        "exactly 1 of 3 discarded — the capped count, not the requested 2",
    );
    assert!(
        r.state.open_windows().is_empty(),
        "exactly one before-discover window opened: {:?}",
        r.state.open_windows(),
    );
}

/// The same shape where the location *can* pay the full count: one discovery
/// of 2 → one window → 2 clues discarded. The single script step plus the
/// no-open-windows assertion is what pins "one discovery, not two" — the
/// discarded total alone cannot tell the shapes apart here.
#[test]
fn deduction_at_a_two_clue_location_discards_two_in_one_window() {
    let r = investigate_with_deduction_and_play_cover_up(investigate_state_with_deduction(2));

    assert_eq!(r.state.locations[&LOC].clues, 2, "location untouched");
    assert_eq!(r.state.investigators[&INV].clues, 0, "discovered nothing");
    assert_no_event!(r.events, Event::CluePlaced { .. });
    let cu = r.state.investigators[&INV]
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == COVER_UP)
        .unwrap();
    assert_eq!(cu.clues, 1, "2 of 3 discarded in a single replacement");
    assert!(
        r.state.open_windows().is_empty(),
        "no second before-discover window: {:?}",
        r.state.open_windows(),
    );
}

// ---- Forced: game-end mental trauma if clues remain -------------------

/// Terminal-act state whose `AdvanceAct` latches a Won resolution, with a
/// Cover Up holding `cover_up_clues` in the threat area.
fn resolving_state(cover_up_clues: u8) -> GameState {
    let mut investigator = test_investigator(1);
    investigator.clues = 1; // meets the act's clue threshold
    investigator.threat_area.push(cover_up(cover_up_clues));
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .with_scenario_id(ScenarioId::new("unknown"))
        .build();
    state.act_deck = vec![Act {
        code: CardCode("_test_act".into()),
        clue_threshold: 1,
        resolution: Some(Resolution::Won { id: "test".into() }),
    }];
    state
}

#[test]
fn game_end_emits_mental_trauma_when_cover_up_has_clues() {
    let r = take_turn_action(
        resolving_state(3),
        &TurnAction::AdvanceAct { investigator: INV },
    );
    assert!(r
        .events
        .iter()
        .any(|e| matches!(e, Event::ScenarioResolved { .. })));
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
    let r = take_turn_action(
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

// ---- Interactive mode: the game-end forced spans the apply boundary ----

/// #566: with `interactive_acknowledge` on (the server default), the lone
/// game-end forced hit pushes an `AcknowledgeForced` frame, so the ending
/// *suspends*. The scenario must not finish resolving until the player has
/// acknowledged and the trauma has landed.
#[test]
fn interactive_game_end_trauma_surfaces_an_acknowledge_before_resolving() {
    let mut state = resolving_state(3);
    state.interactive_acknowledge = true;

    let paused = take_turn_action(state, &TurnAction::AdvanceAct { investigator: INV });

    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "the game-end forced acknowledge must surface, got {:?}",
        paused.outcome,
    );
    assert!(
        !paused
            .events
            .iter()
            .any(|e| matches!(e, Event::ScenarioResolved { .. })),
        "the scenario must not resolve before the game-end forced has run; events = {:?}",
        paused.events,
    );
    assert!(
        !paused
            .events
            .iter()
            .any(|e| matches!(e, Event::TraumaSuffered { .. })),
        "the trauma lands on the acknowledge, not before it",
    );

    let done = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );

    assert!(
        done.events.iter().any(|e| matches!(
            e,
            Event::TraumaSuffered {
                kind: TraumaKind::Mental,
                amount: 1,
                ..
            }
        )),
        "expected mental trauma once acknowledged; events = {:?}",
        done.events,
    );
    assert!(
        done.events
            .iter()
            .any(|e| matches!(e, Event::ScenarioResolved { .. })),
        "the resolution completes after the forced effect; events = {:?}",
        done.events,
    );
    assert!(
        done.state.continuations.is_empty(),
        "no stranded frames after the ending finishes: {:?}",
        done.state.continuations,
    );
}

/// #566: two investigators each holding a clue-bearing Cover Up produce two
/// simultaneous `GameEnd` forced hits, which route to the lead-ordered run
/// (#213) instead of the single-hit acknowledge. Both traumas must land, and
/// the run must not strand frames.
#[test]
fn two_simultaneous_game_end_forceds_both_resolve() {
    const INV2: InvestigatorId = InvestigatorId(2);

    let mut first = test_investigator(1);
    first.clues = 1; // meets the act's clue threshold
    first.threat_area.push(cover_up(3));
    let mut second = test_investigator(2);
    let mut second_cover_up = cover_up(2);
    second_cover_up.instance_id = CardInstanceId(2);
    second.threat_area.push(second_cover_up);

    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(first)
        .with_investigator(second)
        .with_active_investigator(INV)
        .with_turn_order([INV, INV2])
        .with_investigator_turn(INV)
        .with_scenario_id(ScenarioId::new("unknown"))
        .build();
    state.interactive_acknowledge = true;
    state.act_deck = vec![Act {
        code: CardCode("_test_act".into()),
        clue_threshold: 1,
        resolution: Some(Resolution::Won { id: "test".into() }),
    }];

    let mut result = take_turn_action(state, &TurnAction::AdvanceAct { investigator: INV });
    let mut events = std::mem::take(&mut result.events);
    let mut state = result.state;

    // Two hits, so the emit routes to the lead-ordered forced run rather than
    // the single-hit acknowledge (Rules Reference p.17: the player orders
    // simultaneous triggers, even in solo). Asserted explicitly — the shapes
    // differ, and only this one exercises `open_forced_resolution`.
    assert!(
        matches!(
            state.continuations.last(),
            Some(game_core::state::Continuation::TimingPointWindow {
                mode: game_core::state::TimingMode::Forced,
                ..
            })
        ),
        "expected the lead-ordered forced run on top, got {:?}",
        state.continuations,
    );

    // Drain the ordering run: each pick resolves one Cover Up's forced effect.
    for step in 0..8 {
        if !state
            .continuations
            .iter()
            .any(|c| !matches!(c, game_core::state::Continuation::ScenarioEnd { .. }))
        {
            break;
        }
        let r = apply(
            state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
            }),
        );
        assert!(
            !matches!(r.outcome, EngineOutcome::Rejected { .. }),
            "ordering-run step {step} rejected: {:?}",
            r.outcome,
        );
        events.extend(r.events);
        state = r.state;
    }

    let traumas = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::TraumaSuffered {
                    kind: TraumaKind::Mental,
                    amount: 1,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        traumas, 2,
        "both Cover Ups suffer trauma; events = {events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::ScenarioResolved { .. }))
            .count(),
        1,
        "the resolution fires exactly once",
    );
    assert!(
        state.continuations.is_empty(),
        "no stranded frames after the ordering run: {:?}",
        state.continuations,
    );
}
