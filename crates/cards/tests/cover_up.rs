//! C5c (#238) integration: Cover Up 01007 end-to-end against the real
//! `cards::REGISTRY` — the 3-clue threat-area Revelation, the before-timing
//! clue-discovery interrupt, and the game-end mental-trauma forced point.
//!
//! Own process → installs `cards::REGISTRY`. The interrupt / game-end shapes
//! were first proved by the C5a synthetic fixture; that fixture's test binary is
//! gone (#871, ADR 0016) and this file is its successor.

use game_core::action::EngineRecord;
use game_core::event::{Event, TraumaKind};
use game_core::scenario::ScenarioId;
use game_core::state::{
    Act, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, Continuation, GameState,
    InvestigatorId, LocationId, Phase, TimingMode,
};
use game_core::test_support::{
    drive, take_turn_action, terminal_code, test_investigator, test_location, GameStateBuilder,
    ScriptedResolver, TestSession,
};
use game_core::{
    apply, assert_event_sequence, assert_no_event, Action, EngineOutcome, InputResponse,
    PlayerAction, TurnAction,
};

const COVER_UP: &str = "01007";
const DEDUCTION: &str = "01039";
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);

#[ctor::ctor(unsafe)]
fn install() {
    // The real registry plus `test_support`'s synthetic terminal card: the
    // game-end fixtures below end their act deck in one, because a terminal card
    // reaches its resolution point by running an effect on its reverse (ADR
    // 0013) and so needs the registry to serve it.
    game_core::test_support::install_registry_with_terminal_cards(cards::REGISTRY);
}

/// A Cover-Up instance carrying `clues`, pre-placed in the threat area.
fn cover_up(clues: u8) -> CardInPlay {
    let mut c = CardInPlay::enter_play(CardCode::new(COVER_UP), CardInstanceId(1));
    c.clues = clues;
    c
}

/// Clues remaining on the Cover Up in `INV`'s threat area.
fn cover_up_clues(state: &GameState) -> u8 {
    state.investigators[&INV]
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == COVER_UP)
        .expect("Cover Up is in the threat area")
        .clues
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
    assert_eq!(
        cover_up_clues(&r.state),
        2,
        "1 clue discarded from Cover Up"
    );
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
    assert_eq!(cover_up_clues(&r.state), 3, "Cover Up untouched on Skip");
}

// ---- Cover Up + Deduction: one discovery, capped ----------------------

/// Investigation-phase state with **Deduction 01039 in hand**: active
/// investigator at a `location_clues`-clue location, Cover Up holding
/// `held_clues` in the threat area. Intellect 3 + Deduction's 1 intellect icon
/// + a +0 token vs the default shroud 2 → the Investigate always succeeds.
fn investigate_state_with_deduction(location_clues: u8, held_clues: u8) -> GameState {
    let mut investigator = test_investigator(1);
    investigator.threat_area.push(cover_up(held_clues));
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
    let r = investigate_with_deduction_and_play_cover_up(investigate_state_with_deduction(1, 3));

    assert_eq!(
        r.state.locations[&LOC].clues, 1,
        "the discovery was replaced, so the location keeps its clue",
    );
    assert_eq!(r.state.investigators[&INV].clues, 0, "discovered nothing");
    assert_no_event!(r.events, Event::CluePlaced { .. });
    assert_eq!(
        cover_up_clues(&r.state),
        2,
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
    let r = investigate_with_deduction_and_play_cover_up(investigate_state_with_deduction(2, 3));

    assert_eq!(r.state.locations[&LOC].clues, 2, "location untouched");
    assert_eq!(r.state.investigators[&INV].clues, 0, "discovered nothing");
    assert_no_event!(r.events, Event::CluePlaced { .. });
    assert_eq!(
        cover_up_clues(&r.state),
        1,
        "2 of 3 discarded in a single replacement",
    );
    assert!(
        r.state.open_windows().is_empty(),
        "no second before-discover window: {:?}",
        r.state.open_windows(),
    );
}

/// The reaction's RR p.2 potential gate, end-to-end: a Cover Up holding no clues
/// has nothing to discard, so the before-discover window is never offered and the
/// Investigate discovers normally. (`cards::impls::cover_up`'s unit test pins the
/// `01007:has_clues` predicate itself; this pins what the window does with it.)
///
/// Ported from the C5a synthetic binary's `no_interrupt_when_cover_up_has_no_clues`
/// under #871 — the behaviour had no real-card successor, so it moved rather than
/// being deleted.
#[test]
fn no_reaction_offered_when_cover_up_has_no_clues() {
    let (state, _) = investigate_to_interrupt(investigate_state(0));

    assert!(
        !state.open_windows().iter().any(|w| matches!(
            w,
            Continuation::TimingPointWindow {
                mode: TimingMode::Reaction,
                ..
            }
        )),
        "a clueless Cover Up must not open a before-discover window: {:?}",
        state.open_windows(),
    );
    assert_eq!(state.locations[&LOC].clues, 1, "location -1");
    assert_eq!(state.investigators[&INV].clues, 1, "investigator +1");
    assert_eq!(cover_up_clues(&state), 0, "nothing to discard");
}

/// The **held-clue cap** — `min(count, card.clues)` — distinct from the location
/// cap the two Deduction tests above pin. Cover Up holds 1, the location holds 2,
/// and Deduction makes the discovery 2: the discard is capped at the 1 clue Cover
/// Up actually has (no underflow), and the discovery is still *fully* replaced,
/// because the reaction cancels the triggering condition outright rather than
/// paying for it clue by clue (`glossary/Instead.md`).
///
/// Ported from the C5a synthetic binary's
/// `playing_cover_up_caps_discard_at_held_clue_count` under #871, which reached
/// the same shape by pushing frames by hand with `count = 3`. Real cards reach it
/// at `count = 2`.
#[test]
fn deduction_discard_is_capped_at_the_clues_cover_up_holds() {
    let r = investigate_with_deduction_and_play_cover_up(investigate_state_with_deduction(2, 1));

    assert_eq!(r.state.locations[&LOC].clues, 2, "location untouched");
    assert_eq!(r.state.investigators[&INV].clues, 0, "discovered nothing");
    assert_no_event!(r.events, Event::CluePlaced { .. });
    assert_eq!(
        cover_up_clues(&r.state),
        0,
        "the discard is capped at the 1 clue Cover Up held",
    );
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
        // Terminal because it is the only act; its reverse reaches R1, which is
        // what ends the scenario and opens the GameEnd point under test.
        code: terminal_code(1),
        clue_threshold: 1,
    }];
    state
}

/// Advance the terminal act and clear the acknowledge its **reverse** raises,
/// returning the merged result.
///
/// A terminal act reaches its resolution point by running `reach_resolution` on
/// its reverse (ADR 0013), and a lone Forced ability in interactive mode surfaces
/// a #466 acknowledge before it runs. That acknowledge is not what these tests
/// are about — the `GameEnd` forced that follows it is — so it is drained here,
/// with the events from both applies merged so the ordering assertions still read
/// one sequence.
fn advance_terminal_act_interactively(state: GameState) -> game_core::engine::ApplyResult {
    let paused = take_turn_action(state, &TurnAction::AdvanceAct { investigator: INV });
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "expected the terminal act's reverse to raise its forced acknowledge, got {:?}",
        paused.outcome,
    );
    assert!(
        !paused
            .events
            .iter()
            .any(|e| matches!(e, Event::ScenarioResolved { .. })),
        "the ending is latched by the reverse, so nothing resolves before it runs; events = {:?}",
        paused.events,
    );
    let mut events = paused.events;
    let mut done = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    events.append(&mut done.events);
    done.events = events;
    done
}

/// The trauma resolves in the `when` cell of the `GameEnd` condition (#720), and
/// the ending finalizes only once the whole sequence has drained — so the order
/// is trauma *then* `ScenarioResolved`, never the reverse.
///
/// That ordering is what the bare-milestone claim rests on: the game's ending is
/// a bare milestone precisely because the victory-display scan and
/// `apply_resolution` are not the condition's impact but the `ScenarioEnd`
/// frame's own tail, run at the apply boundary after a tail-position emit. If
/// the finalize ever moved ahead of the queued abilities, the resolve step would
/// stop being a no-op and the `when` cell would stop being safe to walk — so
/// this is asserted as a sequence rather than as two independent presences.
#[test]
fn game_end_trauma_resolves_before_the_ending_finalizes() {
    let r = take_turn_action(
        resolving_state(3),
        &TurnAction::AdvanceAct { investigator: INV },
    );
    assert_event_sequence!(
        r.events,
        Event::TraumaSuffered {
            kind: TraumaKind::Mental,
            amount: 1,
            ..
        },
        Event::ScenarioResolved { .. },
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

    let paused = advance_terminal_act_interactively(state);

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
    // ...and in that order: the acknowledge completes, the trauma lands, and
    // only then does the ending finalize. Same claim as
    // `game_end_trauma_resolves_before_the_ending_finalizes`, across the apply
    // boundary the interactive acknowledge introduces.
    assert_event_sequence!(
        done.events,
        Event::TraumaSuffered { .. },
        Event::ScenarioResolved { .. },
    );
    assert!(
        done.state.continuations.is_empty(),
        "no stranded frames after the ending finishes: {:?}",
        done.state.continuations,
    );
}

/// #786: the mirror of the test above at zero clues. "if there are any clues on
/// Cover Up" is an *initiation* condition — RR p.2, "Forced Abilities": *"If a
/// forced ability does not have the potential to change the game state, the
/// ability does not initiate."* So the forced scan collects nothing, the #466
/// acknowledge never surfaces, and the scenario resolves in one uninterrupted
/// batch. Before the fix this raised a `"Forced — Cover Up"` prompt whose only
/// option resolved to no events at all.
#[test]
fn interactive_game_end_with_a_clueless_cover_up_neither_prompts_nor_resolves_it() {
    let mut state = resolving_state(0);
    state.interactive_acknowledge = true;

    let r = advance_terminal_act_interactively(state);

    assert!(
        !matches!(r.outcome, EngineOutcome::AwaitingInput { .. }),
        "a clueless Cover Up must not initiate, so nothing is there to acknowledge; got {:?}",
        r.outcome,
    );
    assert!(
        !r.state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::AcknowledgeForced { .. })),
        "no acknowledge frame is pushed; stack = {:?}",
        r.state.continuations,
    );
    assert!(
        !r.events
            .iter()
            .any(|e| matches!(e, Event::TraumaSuffered { .. })),
        "no trauma; events = {:?}",
        r.events,
    );
    assert!(
        r.events
            .iter()
            .any(|e| matches!(e, Event::ScenarioResolved { .. })),
        "the scenario resolves in the same batch; events = {:?}",
        r.events,
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
        // Terminal because it is the only act; its reverse reaches R1, which is
        // what ends the scenario and opens the GameEnd point under test.
        code: terminal_code(1),
        clue_threshold: 1,
    }];

    let mut result = advance_terminal_act_interactively(state);
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
