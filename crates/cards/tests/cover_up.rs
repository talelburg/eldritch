//! C5c (#238) integration: Cover Up 01007 end-to-end against the real
//! `cards::REGISTRY` — the 3-clue threat-area Revelation, the before-timing
//! clue-discovery interrupt, and the game-end mental-trauma forced point.
//!
//! Own process → installs `cards::REGISTRY`. The interrupt / game-end shapes
//! were first proved by the C5a synthetic fixture; that fixture's test binary is
//! gone (#871, ADR 0016) and this file is its successor.

use cards::REGISTRY;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{EngineOutcome, OptionTarget};
use game_core::event::{Event, TraumaKind};
use game_core::scenario::ScenarioId;
use game_core::state::{
    Act, CardCode, ChaosBag, ChaosToken, Continuation, GameState, GameStateBuilder, InvestigatorId,
    LocationId, MythosPhaseFrame, MythosResume, Phase,
};
use game_core::test_support::{self, clues, seat, Seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_count, assert_event_sequence, assert_no_event};

const COVER_UP: &str = "01007";
const DEDUCTION: &str = "01039";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

#[ctor::ctor(unsafe)]
fn install() {
    // The game-end fixtures below end their act deck in a synthetic terminal
    // card, which reaches its resolution point by running an effect on its
    // reverse (ADR 0013) and so needs the registry to serve it.
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Investigator 1 at the Study, holding a Cover Up with `held` clues in their
/// threat area.
fn holding_cover_up(held: u8) -> Seat {
    seat(1).at(HERE).threat_area([(COVER_UP, clues(held))])
}

/// The anchor of the Cover Up in `holder`'s threat area: its reaction and its
/// forced ability are offered on the card.
fn cover_up_of(state: &GameState, holder: InvestigatorId) -> OptionTarget {
    state.card(holder, COVER_UP)
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
    // Mythos step 1.4: `INV` is to draw the top encounter card, Cover Up.
    let mut state = GameStateBuilder::new()
        .with_seat(seat(1).at(HERE))
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_turn_order([INV])
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([INV])
        .build();
    state.encounter_deck.push_back(CardCode::new(COVER_UP));
    let drawing = TestSession::new(state);
    drawing.assert_at_encounter_draw();

    // Drawing the card reveals it: it resolves without a prompt of its own, and
    // play runs on out of the Mythos phase to `INV`'s turn.
    let session = drawing.confirm();
    session.assert_at_turn_menu(INV);

    let state = session.state();
    let placed: Vec<_> = state.investigators[&INV]
        .threat_area
        .iter()
        .filter(|c| c.code.as_str() == COVER_UP)
        .collect();
    assert_eq!(placed.len(), 1, "exactly one Cover Up in the threat area");
    assert_eq!(placed[0].clues, 3, "enters with 3 clues");
    assert!(
        !state
            .encounter_discard
            .iter()
            .any(|c| c.as_str() == COVER_UP),
        "persistent treachery is not auto-discarded after its Revelation"
    );
}

// ---- Reaction: discard from Cover Up instead of discovering -----------

/// `investigator`'s open turn at the Study holding `location_clues`. +0 chaos
/// token so the Intellect-3-vs-shroud-2 Investigate always succeeds.
fn investigate_board(investigator: Seat, location_clues: u8) -> GameStateBuilder {
    let mut location = test_support::test_location(HERE.0, "Study");
    location.clues = location_clues;
    GameStateBuilder::new()
        .with_seat(investigator)
        .with_location(location)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_rng_seed(1)
}

/// Investigate committing nothing: the session rests at the clue-discovery
/// interrupt, or back at the turn menu if none was offered.
fn investigate_to_interrupt(board: GameStateBuilder) -> TestSession {
    board
        .session()
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[])
}

#[test]
fn playing_cover_up_discards_instead_of_discovering() {
    let offered = investigate_to_interrupt(investigate_board(holding_cover_up(3), 2));
    // Play Cover Up in the before-discover window → discard-from-self + cancel
    // the discovery (Axis D #336).
    let cover_up = cover_up_of(offered.state(), INV);
    let session = offered.pick(cover_up);
    session.assert_at_turn_menu(INV);
    let state = session.state();
    assert_eq!(state.locations[&HERE].clues, 2, "location clues unchanged");
    assert_eq!(state.investigators[&INV].clues, 0, "discovered nothing");
    assert_eq!(cover_up_clues(state), 2, "1 clue discarded from Cover Up");
}

#[test]
fn skip_discovers_normally() {
    let session = investigate_to_interrupt(investigate_board(holding_cover_up(3), 2)).skip();
    session.assert_at_turn_menu(INV);
    let state = session.state();
    assert_eq!(state.locations[&HERE].clues, 1, "location -1");
    assert_eq!(state.investigators[&INV].clues, 1, "investigator +1");
    assert_eq!(cover_up_clues(state), 3, "Cover Up untouched on Skip");
}

// ---- Cover Up + Deduction: one discovery, capped ----------------------

/// Investigation-phase board with **Deduction 01039 in hand**: active
/// investigator at a `location_clues`-clue location, Cover Up holding
/// `held_clues` in the threat area. Intellect 3 + Deduction's 1 intellect icon
/// + a +0 token vs the default shroud 2 → the Investigate always succeeds.
fn investigate_board_with_deduction(location_clues: u8, held_clues: u8) -> GameStateBuilder {
    investigate_board(
        holding_cover_up(held_clues).hand([DEDUCTION]),
        location_clues,
    )
}

/// Investigate committing Deduction, then play Cover Up at the single
/// before-discover window it should open.
fn investigate_with_deduction_and_play_cover_up(board: GameStateBuilder) -> TestSession {
    let offered = board
        .session()
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[DEDUCTION]);
    let cover_up = cover_up_of(offered.state(), INV);
    offered.pick(cover_up)
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
    let session =
        investigate_with_deduction_and_play_cover_up(investigate_board_with_deduction(1, 3));
    let state = session.state();

    assert_eq!(
        state.locations[&HERE].clues, 1,
        "the discovery was replaced, so the location keeps its clue",
    );
    assert_eq!(state.investigators[&INV].clues, 0, "discovered nothing");
    assert_no_event!(session.events(), Event::CluePlaced { .. });
    assert_eq!(
        cover_up_clues(state),
        2,
        "exactly 1 of 3 discarded — the capped count, not the requested 2",
    );
    assert!(
        state.open_windows().is_empty(),
        "exactly one before-discover window opened: {:?}",
        state.open_windows(),
    );
}

/// The same shape where the location *can* pay the full count: one discovery
/// of 2 → one window → 2 clues discarded. The single pick plus the
/// no-open-windows assertion is what pins "one discovery, not two" — the
/// discarded total alone cannot tell the shapes apart here.
#[test]
fn deduction_at_a_two_clue_location_discards_two_in_one_window() {
    let session =
        investigate_with_deduction_and_play_cover_up(investigate_board_with_deduction(2, 3));
    let state = session.state();

    assert_eq!(state.locations[&HERE].clues, 2, "location untouched");
    assert_eq!(state.investigators[&INV].clues, 0, "discovered nothing");
    assert_no_event!(session.events(), Event::CluePlaced { .. });
    assert_eq!(
        cover_up_clues(state),
        1,
        "2 of 3 discarded in a single replacement",
    );
    assert!(
        state.open_windows().is_empty(),
        "no second before-discover window: {:?}",
        state.open_windows(),
    );
}

/// The reaction's RR p.2 potential gate, end-to-end: a Cover Up holding no clues
/// has nothing to discard, so the before-discover window is never offered and the
/// Investigate discovers normally. (`cards::impls::cover_up`'s unit test pins the
/// `01007:has-clues` predicate itself; this pins what the window does with it.)
///
/// Ported from the C5a synthetic binary's `no_interrupt_when_cover_up_has_no_clues`
/// under #871 — the behaviour had no real-card successor, so it moved rather than
/// being deleted.
#[test]
fn no_reaction_offered_when_cover_up_has_no_clues() {
    let session = investigate_to_interrupt(investigate_board(holding_cover_up(0), 2));
    let state = session.state();

    assert!(
        state.open_windows().is_empty(),
        "a clueless Cover Up must not open a before-discover window: {:?}",
        state.open_windows(),
    );
    assert_eq!(state.locations[&HERE].clues, 1, "location -1");
    assert_eq!(state.investigators[&INV].clues, 1, "investigator +1");
    assert_eq!(cover_up_clues(state), 0, "nothing to discard");
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
    let session =
        investigate_with_deduction_and_play_cover_up(investigate_board_with_deduction(2, 1));
    let state = session.state();

    assert_eq!(state.locations[&HERE].clues, 2, "location untouched");
    assert_eq!(state.investigators[&INV].clues, 0, "discovered nothing");
    assert_no_event!(session.events(), Event::CluePlaced { .. });
    assert_eq!(
        cover_up_clues(state),
        0,
        "the discard is capped at the 1 clue Cover Up held",
    );
    assert!(
        state.open_windows().is_empty(),
        "no second before-discover window: {:?}",
        state.open_windows(),
    );
    // The synthetic this replaces asserted an empty stack; through a real
    // Investigate the open turn is still on it, so the equivalent claim is that
    // the discovery sequence itself drained — the coordinator walked its
    // remaining cells and popped rather than stranding a cancelled emit.
    assert!(
        !state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::EmitEvent(_) | Continuation::TimingPoint(_))),
        "no stranded discovery frames: {:?}",
        state.continuations,
    );
}

// ---- Forced: game-end mental trauma if clues remain -------------------

/// Stage the act deck as one terminal act, so `AdvanceAct` latches a Won
/// resolution. The builder stages no act deck, so it is set on the built state.
fn with_terminal_act(mut state: GameState) -> GameState {
    state.act_deck = vec![Act {
        // Terminal because it is the only act; its reverse reaches R1, which is
        // what ends the scenario and opens the GameEnd point under test.
        code: test_support::terminal_code(1),
        clue_threshold: 1,
    }];
    state
}

/// Terminal-act state whose `AdvanceAct` latches a Won resolution, with a
/// Cover Up holding `cover_up_clues` in the threat area.
fn resolving_state(cover_up_clues: u8) -> GameState {
    with_terminal_act(
        GameStateBuilder::new()
            // 1 clue meets the act's clue threshold.
            .with_seat(
                seat(1)
                    .clues(1)
                    .threat_area([(COVER_UP, clues(cover_up_clues))]),
            )
            .open_turn(INV)
            .with_scenario_id(ScenarioId::new("unknown"))
            .build(),
    )
}

/// `state` with the server-default interactive acknowledge on.
fn interactive(mut state: GameState) -> GameState {
    state.interactive_acknowledge = true;
    state
}

/// Advance the terminal act and clear the acknowledge its **reverse** raises.
///
/// A terminal act reaches its resolution point by running `reach_resolution` on
/// its reverse (ADR 0013), and a lone Forced ability in interactive mode surfaces
/// a #466 acknowledge before it runs. That acknowledge is not what these tests
/// are about — the `GameEnd` forced that follows it is — so it is answered here.
fn advance_terminal_act_interactively(state: GameState) -> TestSession {
    let paused = TestSession::new(state).take(&TurnAction::AdvanceAct { investigator: INV });
    assert_eq!(
        paused.offered_at(&OptionTarget::Act),
        1,
        "expected the terminal act's reverse to raise its forced acknowledge: {:?}",
        paused.prompt(),
    );
    assert_no_event!(paused.events(), Event::ScenarioResolved { .. });
    paused.pick(OptionTarget::Act)
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
    let session =
        TestSession::new(resolving_state(3)).take(&TurnAction::AdvanceAct { investigator: INV });
    assert_event_sequence!(
        session.events(),
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
    let session =
        TestSession::new(resolving_state(0)).take(&TurnAction::AdvanceAct { investigator: INV });
    assert_event!(session.events(), Event::ScenarioResolved { .. });
    assert_no_event!(session.events(), Event::TraumaSuffered { .. });
}

// ---- Interactive mode: the game-end forced spans the apply boundary ----

/// #566: with `interactive_acknowledge` on (the server default), the lone
/// game-end forced hit pushes an `AcknowledgeForced` frame, so the ending
/// *suspends*. The scenario must not finish resolving until the player has
/// acknowledged and the trauma has landed.
#[test]
fn interactive_game_end_trauma_surfaces_an_acknowledge_before_resolving() {
    let paused = advance_terminal_act_interactively(interactive(resolving_state(3)));

    // The game-end forced acknowledge surfaces, on the Cover Up.
    let cover_up = cover_up_of(paused.state(), INV);
    assert_eq!(
        paused.offered_at(&cover_up),
        1,
        "the game-end forced acknowledge must surface: {:?}",
        paused.prompt(),
    );
    assert_no_event!(paused.events(), Event::ScenarioResolved { .. });
    assert_no_event!(paused.events(), Event::TraumaSuffered { .. });

    let done = paused.pick(cover_up);

    assert_event!(
        done.events(),
        Event::TraumaSuffered {
            kind: TraumaKind::Mental,
            amount: 1,
            ..
        }
    );
    // ...and in that order: the acknowledge completes, the trauma lands, and
    // only then does the ending finalize. Same claim as
    // `game_end_trauma_resolves_before_the_ending_finalizes`, across the apply
    // boundary the interactive acknowledge introduces.
    assert_event_sequence!(
        done.events(),
        Event::TraumaSuffered { .. },
        Event::ScenarioResolved { .. },
    );
    assert!(
        done.state().continuations.is_empty(),
        "no stranded frames after the ending finishes: {:?}",
        done.state().continuations,
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
    let session = advance_terminal_act_interactively(interactive(resolving_state(0)));

    assert_no_event!(session.events(), Event::TraumaSuffered { .. });
    assert_event!(session.events(), Event::ScenarioResolved { .. });
    // No acknowledge surfaces: the step runs on to the game's end rather than
    // stopping at one. (`finish` would panic at an unanswered prompt.)
    let r = session.finish();
    assert_eq!(
        r.outcome,
        EngineOutcome::Done,
        "a clueless Cover Up must not initiate, so nothing is there to acknowledge",
    );
}

/// #566: two investigators each holding a clue-bearing Cover Up produce two
/// simultaneous `GameEnd` forced hits, which route to the lead-ordered run
/// (#213) instead of the single-hit acknowledge. Both traumas must land, and
/// the run must not strand frames.
#[test]
fn two_simultaneous_game_end_forceds_both_resolve() {
    const INV2: InvestigatorId = InvestigatorId(2);

    let state = with_terminal_act(interactive(
        GameStateBuilder::new()
            // 1 clue meets the act's clue threshold.
            .with_seat(seat(1).clues(1).threat_area([(COVER_UP, clues(3))]))
            .with_seat(seat(2).threat_area([(COVER_UP, clues(2))]))
            .with_turn_order([INV, INV2])
            .open_turn(INV)
            .with_scenario_id(ScenarioId::new("unknown"))
            .build(),
    ));

    let session = advance_terminal_act_interactively(state);

    // Two hits, so the emit routes to the lead-ordered forced run rather than
    // the single-hit acknowledge (Rules Reference p.17: the player orders
    // simultaneous triggers, even in solo). Asserted explicitly — the shapes
    // differ, and only this one exercises `open_forced_resolution`: the run
    // offers both Cover Ups at once, where an acknowledge offers one.
    let first = cover_up_of(session.state(), INV);
    let second = cover_up_of(session.state(), INV2);
    assert_eq!(
        (
            session.offered_at(&first),
            session.offered_at(&second),
            session.prompt().options.len(),
        ),
        (1, 1, 2),
        "expected the lead-ordered forced run over both Cover Ups, got {:?}",
        session.prompt(),
    );

    // The ordering run: each pick resolves one Cover Up's forced effect.
    let session = session.pick(first).pick(second);

    assert_event_count!(
        session.events(),
        2,
        Event::TraumaSuffered {
            kind: TraumaKind::Mental,
            amount: 1,
            ..
        }
    );
    assert_event_count!(session.events(), 1, Event::ScenarioResolved { .. });
    assert!(
        session.state().continuations.is_empty(),
        "no stranded frames after the ordering run: {:?}",
        session.state().continuations,
    );
}

// ---- Co-located investigators: anyone at Roland's location (#974) ------

/// The investigator who is *not* Roland, standing at his location.
const OTHER: InvestigatorId = InvestigatorId(2);

/// Roland (`INV`) holds Cover Up with 3 clues in his threat area; `OTHER`, seated
/// as `other`, shares his 2-clue location and has the open turn.
fn colocated_investigate_board(other: Seat) -> GameStateBuilder {
    let mut location = test_support::test_location(HERE.0, "Study");
    location.clues = 2;
    GameStateBuilder::new()
        .with_seat(holding_cover_up(3))
        .with_seat(other.at(HERE))
        .with_location(location)
        .with_turn_order([INV, OTHER])
        .open_turn(OTHER)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_rng_seed(1)
}

/// `OTHER` investigates committing nothing and plays Roland's Cover Up at the
/// before-discover window.
fn colocated_investigate_and_play_cover_up(state: GameState) -> TestSession {
    let offered = TestSession::new(state)
        .take(&TurnAction::Investigate {
            investigator: OTHER,
        })
        .commit(&[]);
    let cover_up = cover_up_of(offered.state(), INV);
    offered.pick(cover_up)
}

/// The ruling (<https://arkhamdb.com/card/01007>): *"Any investigator at the
/// same location as Roland Banks with Cover Up in his threat area may trigger
/// the [reaction] to discard clues from Cover Up, as per the FAQ [V1.0, section
/// 2.1]."* The second investigator discovers, is offered the reaction, and the
/// clue comes off Roland's Cover Up rather than the location.
#[test]
fn a_colocated_investigator_discards_from_rolands_cover_up() {
    let session =
        colocated_investigate_and_play_cover_up(colocated_investigate_board(seat(2)).build());
    let state = session.state();

    assert_eq!(state.locations[&HERE].clues, 2, "location untouched");
    assert_eq!(state.investigators[&OTHER].clues, 0, "discovered nothing");
    assert_eq!(
        cover_up_clues(state),
        2,
        "1 clue discarded from Roland's Cover Up",
    );
}

/// The Forced trauma stays Roland's whoever ends the game:
/// `glossary/You_Your.md` makes "you" on a threat-area card *"the investigator
/// who has the card in his/her threat area"*.
#[test]
fn game_end_trauma_goes_to_the_investigator_holding_cover_up() {
    let state = with_terminal_act(
        GameStateBuilder::new()
            .with_seat(holding_cover_up(2))
            // 1 clue meets the act's clue threshold.
            .with_seat(seat(2).at(HERE).clues(1))
            .with_location(test_support::test_location(HERE.0, "Study"))
            .with_turn_order([INV, OTHER])
            .open_turn(OTHER)
            .with_scenario_id(ScenarioId::new("unknown"))
            .build(),
    );

    let session = TestSession::new(state).take(&TurnAction::AdvanceAct {
        investigator: OTHER,
    });

    assert_eq!(
        traumas(&session),
        vec![(INV, TraumaKind::Mental)],
        "events = {:?}",
        session.events()
    );
}

/// Every trauma suffered in the session, by investigator and kind.
fn traumas(session: &TestSession) -> Vec<(InvestigatorId, TraumaKind)> {
    session
        .events()
        .iter()
        .filter_map(|e| match e {
            Event::TraumaSuffered {
                investigator, kind, ..
            } => Some((*investigator, *kind)),
            _ => None,
        })
        .collect()
}

/// Story 13: the co-located investigator **uses** Cover Up before the game
/// ends — their Investigate's clue comes off Roland's Cover Up — and Cover Up
/// still holds clues when it does. The trauma is still Roland's alone: using a
/// threat-area card's reaction does not make it yours, and "you" on it stays
/// *"the investigator who has the card in his/her threat area"*
/// (`glossary/You_Your.md`).
#[test]
fn game_end_trauma_stays_rolands_after_a_colocated_investigator_uses_cover_up() {
    let state = with_terminal_act(
        // 1 clue meets the act's clue threshold.
        colocated_investigate_board(seat(2).clues(1))
            .with_scenario_id(ScenarioId::new("unknown"))
            .build(),
    );

    let session = colocated_investigate_and_play_cover_up(state);
    assert_eq!(
        cover_up_clues(session.state()),
        2,
        "the co-located investigator's discovery came off Roland's Cover Up",
    );
    assert_eq!(
        session.state().investigators[&OTHER].clues,
        1,
        "and they discovered nothing",
    );

    let session = session.take(&TurnAction::AdvanceAct {
        investigator: OTHER,
    });
    assert_eq!(
        traumas(&session),
        vec![(INV, TraumaKind::Mental)],
        "events = {:?}",
        session.events(),
    );
}
