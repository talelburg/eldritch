//! The round position: where the round stands, as the round band lights
//! it. See **Round position** in `GLOSSARY.md`.
//!
//! One game is walked from the setup mulligan through a full round and into the
//! next, asserting the position at every pause. `Appendix_II_Timing_and_Gameplay.md`:
//! *"The "Phase Sequence timing chart" depicts the phases and steps of a game
//! round."* Its red boxes are *"player windows"*; each of the six parks here
//! because a free ability is in play to offer.
//!
//! Synthetic probes only (ADR 0016), each modelling one primitive: a free
//! ability a player window offers, an act whose forced abilities listen on the
//! eight phase boundaries, and a blank treachery for the step-1.4 draw.

use std::cell::RefCell;
use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata};
use card_dsl::dsl::{self, Ability, EventPattern, EventTiming, InvestigatorTarget};
use game_core::action::{Action, InputResponse, PlayerAction, RosterEntry};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{self, EngineOutcome, NativeCx, Registries};
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    CardCode, CardInPlay, GameState, GameStateBuilder, InvestigatorId, LocationId, Owner,
    PhaseStep, RoundPosition,
};
use game_core::test_support::{location, objective_act, MockRegistry, TestSession, TEST_INV};

/// `[free]: Gain 1 resource.` — a 0-action ability, so every player window has
/// something to offer and parks rather than closing unseen.
const FREE: &str = "_rp_free";
/// The act carrying one forced ability per phase boundary, each recording the
/// round position it resolved at.
const ACT: &str = "_rp_boundaries";
/// A treachery with no Revelation, for the step-1.4 draw to turn over.
const TREACHERY: &str = "_rp_treachery";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

thread_local! {
    /// Each boundary marker that resolved on this test's thread, with the
    /// round position it saw.
    static OBSERVED: RefCell<Vec<(&'static str, Option<RoundPosition>)>> =
        const { RefCell::new(Vec::new()) };
}

fn observe(cx: &NativeCx, boundary: &'static str) -> EngineOutcome {
    let position = cx.state().round_position;
    OBSERVED.with(|o| o.borrow_mut().push((boundary, position)));
    EngineOutcome::Done
}

/// Drain the markers recorded so far.
fn observed() -> Vec<(&'static str, Option<RoundPosition>)> {
    OBSERVED.with(|o| std::mem::take(&mut *o.borrow_mut()))
}

fn boundary_markers() -> Vec<Ability> {
    let at = |pattern, tag| dsl::forced_on_event(pattern, EventTiming::At, dsl::native(tag));
    let started = |phase, tag| at(EventPattern::PhaseStarted { phase }, tag);
    let ended = |phase, tag| at(EventPattern::PhaseEnded { phase }, tag);
    vec![
        started(dsl::Phase::Mythos, "rp:start-mythos"),
        started(dsl::Phase::Investigation, "rp:start-investigation"),
        started(dsl::Phase::Enemy, "rp:start-enemy"),
        started(dsl::Phase::Upkeep, "rp:start-upkeep"),
        ended(dsl::Phase::Mythos, "rp:end-mythos"),
        ended(dsl::Phase::Investigation, "rp:end-investigation"),
        ended(dsl::Phase::Enemy, "rp:end-enemy"),
        ended(dsl::Phase::Upkeep, "rp:end-upkeep"),
    ]
}

fn treachery() -> CardMetadata {
    CardMetadata {
        code: TREACHERY.into(),
        name: "Blank Treachery".into(),
        text: None,
        traits: Vec::new(),
        back_name: None,
        back_text: None,
        pack_code: "_synth".into(),
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
        cards: MockRegistry::new()
            .with_card(treachery())
            .with_abilities(FREE, || -> Vec<Ability> {
                vec![dsl::activated(
                    0,
                    vec![],
                    dsl::gain_resources(InvestigatorTarget::You, 1),
                )]
            })
            .with_abilities(ACT, boundary_markers)
            .with_native_effect("rp:start-mythos", |cx, _: &EvalContext| {
                observe(cx, "start-mythos")
            })
            .with_native_effect("rp:start-investigation", |cx, _| {
                observe(cx, "start-investigation")
            })
            .with_native_effect("rp:start-enemy", |cx, _| observe(cx, "start-enemy"))
            .with_native_effect("rp:start-upkeep", |cx, _| observe(cx, "start-upkeep"))
            .with_native_effect("rp:end-mythos", |cx, _| observe(cx, "end-mythos"))
            .with_native_effect("rp:end-investigation", |cx, _| {
                observe(cx, "end-investigation")
            })
            .with_native_effect("rp:end-enemy", |cx, _| observe(cx, "end-enemy"))
            .with_native_effect("rp:end-upkeep", |cx, _| observe(cx, "end-upkeep"))
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// The scenario as setup leaves it, before seating: one revealed location, the
/// marker act current, and one treachery in the encounter deck.
fn setup() -> GameState {
    let mut state = GameStateBuilder::new()
        .with_location(location(HERE))
        .with_encounter_deck([TREACHERY])
        .with_act_deck([objective_act(ACT)])
        .build();
    state.starting_location = Some(HERE);
    state
}

/// One investigator with 8 filler cards: 5 for the opening hand, the rest for
/// the step-4.4 draw.
fn roster() -> Vec<RosterEntry> {
    vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck: (0..8)
            .map(|i| CardCode::new(format!("_rp_filler_{i}")))
            .collect(),
        random_basic_weaknesses: 0,
    }]
}

/// Seat the roster through `seat_and_open`, put the free-ability probe into
/// play, and settle: the game rests at the setup mulligan.
///
/// Seated through `seat_and_open` rather than staged with `with_seat`, because
/// the subject starts at setup: the position must still be unset at the
/// mulligan that `start_scenario` opens, and the replay test re-drives from
/// these creation inputs. The probe is pushed by hand since a roster seats
/// decks, not cards in play.
fn the_states_at_the_mulligan() -> GameState {
    let mut state = engine::seat_and_open(setup(), &roster(), 0, registries()).state;
    let id = state.card_instance_ids.mint();
    state
        .investigators
        .get_mut(&INV)
        .expect("seated")
        .cards_in_play
        .push(CardInPlay::enter_play(
            CardCode::new(FREE),
            id,
            Owner::Investigator(INV),
        ));
    state
}

fn at_the_mulligan() -> TestSession {
    observed();
    TestSession::new(the_states_at_the_mulligan(), registries())
}

/// Keep the opening hand: the round's first Investigation phase begins.
fn after_the_mulligan() -> TestSession {
    at_the_mulligan().choose_from_hand(&[])
}

/// On through the windows after 2.1 and 2.2 to the first turn menu.
fn at_the_first_turn() -> TestSession {
    after_the_mulligan().skip().skip()
}

/// End the round's only turn: the Enemy phase begins.
fn after_the_turn() -> TestSession {
    at_the_first_turn().take(&TurnAction::EndTurn)
}

fn position(session: &TestSession) -> Option<RoundPosition> {
    session.state().round_position
}

#[test]
fn the_mulligan_is_before_the_round() {
    let session = at_the_mulligan();
    assert_eq!(session.state().current_mulligan(), Some(INV));
    assert_eq!(position(&session), None);
}

#[test]
fn the_first_investigation_phase_begins_at_2_1_and_its_window() {
    let session = after_the_mulligan();
    assert_eq!(
        observed(),
        [(
            "start-investigation",
            Some(RoundPosition::InvestigationBegins)
        )],
        "step 2.1's forced abilities resolve at 2.1",
    );
    session.assert_at_player_window(PhaseStep::InvestigationBegins);
    assert_eq!(
        position(&session),
        Some(RoundPosition::Window(PhaseStep::InvestigationBegins))
    );
}

#[test]
fn the_window_after_2_2_then_the_turn_menu_at_2_2_1() {
    let session = after_the_mulligan().skip();
    session.assert_at_player_window(PhaseStep::TurnPlayerWindow);
    assert_eq!(
        position(&session),
        Some(RoundPosition::Window(PhaseStep::TurnPlayerWindow))
    );

    let session = session.skip();
    session.assert_at_turn_menu(INV);
    assert_eq!(position(&session), Some(RoundPosition::TakeAction));
}

#[test]
fn an_action_returns_to_the_turns_window_then_to_2_2_1() {
    // Appendix II 2.2.1: "If an action was taken, return to previous player
    // window."
    let session = at_the_first_turn().take(&TurnAction::Resource { investigator: INV });
    session.assert_at_player_window(PhaseStep::TurnPlayerWindow);
    assert_eq!(
        position(&session),
        Some(RoundPosition::Window(PhaseStep::TurnPlayerWindow))
    );

    let session = session.skip();
    session.assert_at_turn_menu(INV);
    assert_eq!(position(&session), Some(RoundPosition::TakeAction));
}

#[test]
fn the_enemy_phase_windows_before_and_after_the_attacks() {
    let session = after_the_turn();
    session.assert_at_player_window(PhaseStep::BeforeInvestigatorAttacked);
    assert_eq!(
        position(&session),
        Some(RoundPosition::Window(PhaseStep::BeforeInvestigatorAttacked))
    );

    let session = session.skip();
    session.assert_at_player_window(PhaseStep::AfterAllInvestigatorsAttacked);
    assert_eq!(
        position(&session),
        Some(RoundPosition::Window(
            PhaseStep::AfterAllInvestigatorsAttacked
        ))
    );
}

#[test]
fn the_window_after_4_1() {
    let session = after_the_turn().skip().skip();
    session.assert_at_player_window(PhaseStep::UpkeepBegins);
    assert_eq!(
        position(&session),
        Some(RoundPosition::Window(PhaseStep::UpkeepBegins))
    );
}

#[test]
fn the_encounter_draw_is_1_4_and_the_window_after_it() {
    let session = after_the_turn().skip().skip().skip();
    session.assert_at_encounter_draw();
    assert_eq!(position(&session), Some(RoundPosition::EncounterDraws));

    let session = session.confirm();
    session.assert_at_player_window(PhaseStep::MythosAfterDraws);
    assert_eq!(
        position(&session),
        Some(RoundPosition::Window(PhaseStep::MythosAfterDraws))
    );
}

#[test]
fn each_phase_boundarys_forced_abilities_resolve_at_its_own_step() {
    use RoundPosition as P;

    // A prompt raised by a phase's start or end forced abilities sits at that
    // step: 2.3, 3.1, 3.4, 4.1, 4.6, 1.1, then 1.5 and 2.1 of round 2.
    let session = at_the_first_turn();
    observed();
    let session = session.take(&TurnAction::EndTurn).skip().skip().skip();
    let round_one = observed();
    let session = session.confirm();
    observed();
    let _ = session.skip();
    let round_two = observed();

    assert_eq!(
        round_one,
        [
            ("end-investigation", Some(P::InvestigationEnds)),
            ("start-enemy", Some(P::EnemyBegins)),
            ("end-enemy", Some(P::EnemyEnds)),
            ("start-upkeep", Some(P::UpkeepBegins)),
            ("end-upkeep", Some(P::UpkeepEnds)),
            ("start-mythos", Some(P::MythosBegins)),
        ],
    );
    assert_eq!(
        round_two,
        [
            ("end-mythos", Some(P::MythosEnds)),
            ("start-investigation", Some(P::InvestigationBegins)),
        ],
    );
}

#[test]
fn a_replayed_log_arrives_at_the_same_position() {
    // The server stores only the creation inputs and the action log, so the
    // position has to come back from a replay. Keep the hand, then pass the
    // windows after 2.1 and after 2.2.
    let answer = |response| {
        Action::Player(PlayerAction::ResolveInput {
            investigator: INV,
            response,
        })
    };
    let log = [
        answer(InputResponse::PickMultiple { selected: vec![] }),
        answer(InputResponse::Skip),
        answer(InputResponse::Skip),
    ];
    let mut state = the_states_at_the_mulligan();
    for action in log {
        let result = engine::apply(state, action, registries());
        assert!(
            matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
            "{:?}",
            result.outcome
        );
        state = result.state;
    }
    assert_eq!(
        state.round_position,
        at_the_first_turn().state().round_position
    );
    assert_eq!(state.round_position, Some(RoundPosition::TakeAction));
}
