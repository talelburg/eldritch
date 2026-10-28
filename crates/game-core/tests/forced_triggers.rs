//! End-to-end `queue_forced_triggers` flow with a mock `CardRegistry`
//! covering a single `EventPattern::EnteredLocation` forced ability.
//!
//! Lives at `crates/game-core/tests/` (a separate integration-test
//! binary, hence its own process and its own `MockRegistry`)
//! so building a mock registry here doesn't collide with game-core's
//! in-crate tests.
//! Mirrors `activate_ability.rs` / `on_skill_test_resolution.rs`.
//!
//! No real card carries `EventPattern::EnteredLocation` yet — the
//! first consumer will land when a scenario-structure card with a
//! location-entry forced ability is implemented. Until then, mock
//! cards are the only way to exercise the full path.

use std::sync::LazyLock;

use card_dsl::dsl::{
    self, Ability, EventPattern, EventTiming, InvestigatorTarget, SkillTestKind, TestOutcome,
    TestedLocationScope,
};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, OptionTarget, Registries, TimingEvent};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    self, Act, Agenda, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, Continuation,
    Enemy, EnemyId, GameState, GameStateBuilder, InvestigatorId, Location, LocationId,
    MythosPhaseFrame, MythosResume, Owner,
};
use game_core::test_support::{self, seat, MockRegistry, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

/// Mock location code: one `EventPattern::EnteredLocation` forced ability
/// that deals 1 horror to the entering investigator.
const HORROR_ATTIC: &str = "test-attic";

/// Mock agenda code: one `EventPattern::PhaseEnded { phase: Enemy }` forced
/// ability that deals 1 horror to the controller (the lead-investigator proxy).
const DOOM_AGENDA: &str = "test-agenda";

/// Mock act code: one `EventPattern::PhaseEnded { phase: Enemy }` forced
/// ability that deals 1 horror to the controller (the lead-investigator proxy).
const DOOM_ACT: &str = "test-act";

/// Mock location code: TWO `EventPattern::EnteredLocation` forced abilities,
/// both dealing 1 horror to the entering investigator. Used to test that a
/// single timing point with 2+ simultaneous forced triggers rejects loudly
/// instead of silently choosing an order.
const DOUBLE_FORCED: &str = "test-double-forced";

/// Mock threat-area card: one `EventPattern::EndOfTurn` forced ability
/// dealing 1 horror to the controller. The Frozen-in-Fear shape, minus the
/// skill test (kept non-suspending so the ability fires without a prompt).
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
/// doubled, so leaving opens a lead-ordered run that *suspends* mid-move.
const DOUBLE_LEFT_LOCATION: &str = "test-double-left-location";

/// Mock location attachment: one `EventPattern::LeftLocation` forced ability in
/// the **`when`** cell, dealing 1 horror to the leaving investigator. The
/// Barricade-01038 shape as the card actually prints it — *"Forced - When an
/// investigator leaves attached location"* — declarable because
/// `LeftLocation` is coordinator-owned.
const WHEN_LEFT_LOCATION: &str = "test-when-left-location";

/// The same, doubled: two `when`-cell `LeftLocation` forced abilities, so
/// leaving opens a lead-ordered run that suspends *before the departure has
/// landed*. The suspension is what makes the mid-sequence board state
/// observable.
const DOUBLE_WHEN_LEFT_LOCATION: &str = "test-double-when-left-location";

/// Mock threat-area card: one `EventPattern::SkillTestResolved { Success,
/// Some(Investigate) }` forced ability dealing 1 horror to the controller. The
/// Obscuring Fog shape, minus the location attachment.
const AFTER_INVESTIGATE_CARD: &str = "test-after-investigate";

/// Mock locations exercising the forced scan's **eligibility gate**: the
/// `EnteredLocation` horror ability of [`HORROR_ATTIC`], carrying an
/// `Ability::eligibility` native condition that respectively passes, fails,
/// and has no registered predicate at all. RR p.2: an ability with no potential
/// to change the game state does not initiate — so the last two must fire
/// nothing, the unresolvable one because a host that cannot evaluate its own
/// gate must not guess in the ability's favour.
const GATED_ELIGIBLE: &str = "test-gated-eligible";
const GATED_INELIGIBLE: &str = "test-gated-ineligible";
const GATED_UNKNOWN_TAG: &str = "test-gated-unknown-tag";

/// Tags the mock registry resolves as native conditions. `UNKNOWN_TAG` is
/// deliberately absent from it.
const ALWAYS_TAG: &str = "test:always";
const NEVER_TAG: &str = "test:never";
const UNKNOWN_TAG: &str = "test:unregistered";

/// One forced ability on `pattern` in `cell`, dealing `horror` to the
/// investigator it fires for — the single shape every mock card here prints.
fn forced_horror(pattern: EventPattern, cell: EventTiming, horror: u8) -> Ability {
    dsl::forced_on_event(
        pattern,
        cell,
        dsl::deal_horror(InvestigatorTarget::You, horror),
    )
}

/// [`HORROR_ATTIC`]'s on-enter ability, gated on the native condition `tag`.
fn gated_on_enter(tag: &'static str) -> Vec<Ability> {
    vec![
        forced_horror(EventPattern::EnteredLocation, EventTiming::After, 1)
            .with_eligibility(dsl::native_condition(tag)),
    ]
}

/// A `LeftLocation` forced ability in `cell` — a card printing two of these
/// opens a lead-ordered run that suspends mid-move.
fn left_location(cell: EventTiming) -> Ability {
    forced_horror(EventPattern::LeftLocation, cell, 1)
}

/// The end-of-enemy-phase horror both [`DOOM_AGENDA`] and [`DOOM_ACT`] print.
fn enemy_phase_end_horror() -> Vec<Ability> {
    vec![forced_horror(
        EventPattern::PhaseEnded {
            phase: dsl::Phase::Enemy,
        },
        EventTiming::After,
        1,
    )]
}

fn always(_: &GameState, _: &EvalContext) -> bool {
    true
}

fn never(_: &GameState, _: &EvalContext) -> bool {
    false
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        // `TEST_INV` rides `build`'s composed `metadata_for_test_inv`, so capacity
        // reads (`max_health()` / `max_sanity()`) work under this registry.
        cards: MockRegistry::new()
            .with_abilities(HORROR_ATTIC, || {
                vec![forced_horror(
                    EventPattern::EnteredLocation,
                    EventTiming::After,
                    1,
                )]
            })
            .with_abilities(DOOM_AGENDA, enemy_phase_end_horror)
            .with_abilities(DOOM_ACT, enemy_phase_end_horror)
            // Two distinct forced `EnteredLocation` abilities at the same timing
            // point — exercises ordered multi-resolution (both fire in order).
            .with_abilities(DOUBLE_FORCED, || {
                vec![
                    forced_horror(EventPattern::EnteredLocation, EventTiming::After, 1),
                    forced_horror(EventPattern::EnteredLocation, EventTiming::After, 1),
                ]
            })
            .with_abilities(UPKEEP_END_ACT, || {
                vec![forced_horror(
                    EventPattern::PhaseEnded {
                        phase: dsl::Phase::Upkeep,
                    },
                    EventTiming::After,
                    1,
                )]
            })
            .with_abilities(ROUND_END_AGENDA, || {
                vec![forced_horror(EventPattern::RoundEnded, EventTiming::At, 2)]
            })
            .with_abilities(DOUBLE_LEFT_LOCATION, || {
                vec![
                    left_location(EventTiming::After),
                    left_location(EventTiming::After),
                ]
            })
            .with_abilities(WHEN_LEFT_LOCATION, || {
                vec![left_location(EventTiming::When)]
            })
            .with_abilities(DOUBLE_WHEN_LEFT_LOCATION, || {
                vec![
                    left_location(EventTiming::When),
                    left_location(EventTiming::When),
                ]
            })
            .with_abilities(END_OF_TURN_CARD, || {
                vec![forced_horror(
                    EventPattern::EndOfTurn,
                    EventTiming::After,
                    1,
                )]
            })
            .with_abilities(GATED_ELIGIBLE, || gated_on_enter(ALWAYS_TAG))
            .with_abilities(GATED_INELIGIBLE, || gated_on_enter(NEVER_TAG))
            .with_abilities(GATED_UNKNOWN_TAG, || gated_on_enter(UNKNOWN_TAG))
            .with_abilities(AFTER_INVESTIGATE_CARD, || {
                vec![forced_horror(
                    EventPattern::SkillTestResolved {
                        outcome: TestOutcome::Success,
                        kind: Some(SkillTestKind::Investigate),
                        by_controller: true,
                        tested_location: TestedLocationScope::Any,
                        margin: None,
                    },
                    EventTiming::After,
                    1,
                )]
            })
            .with_native_condition(ALWAYS_TAG, always)
            .with_native_condition(NEVER_TAG, never)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ATTIC: LocationId = LocationId(11);
const LURKER: EnemyId = EnemyId(1);

/// A location at `id` printed as `code`.
fn location_as(id: LocationId, name: &str, code: &str) -> Location {
    let mut location = test_support::test_location(id.0, name);
    location.code = CardCode(code.into());
    location
}

/// The Hallway at `HERE`, a plain location connected to the Attic.
fn hallway() -> Location {
    let mut hallway = test_support::test_location(HERE.0, "Hallway");
    hallway.connections = vec![ATTIC];
    hallway
}

/// The instance id of the attachment [`hallway_with`] places. A seat cannot
/// place a location attachment, so the builder does not mint this one; it is
/// fixed here, and seated cards are minted past it.
const HALLWAY_ATTACHMENT_ID: CardInstanceId = CardInstanceId(7);

/// The [`hallway`] with `attachment` attached as a location attachment.
fn hallway_with(attachment: &str) -> Location {
    let mut hallway = hallway();
    hallway.attachments.push(CardInPlay::enter_play(
        CardCode(attachment.into()),
        HALLWAY_ATTACHMENT_ID,
        Owner::EncounterDeck,
    ));
    hallway
}

/// The anchor of the attachment [`hallway_with`] places: the abilities printed
/// on it are offered on it.
const HALLWAY_ATTACHMENT: OptionTarget = OptionTarget::CardInstance(HALLWAY_ATTACHMENT_ID);

/// `INV`'s open turn in `hallway` (connected to the Attic), with the Attic as
/// `attic`.
fn move_board(hallway: Location, attic: Location) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE).actions(3))
        .with_location(hallway)
        .with_location(attic)
        .open_turn(INV)
}

/// The Move from the Hallway to the Attic.
const MOVE_TO_ATTIC: TurnAction = TurnAction::Move {
    investigator: INV,
    destination: ATTIC,
};

/// `INV` moves from the plain Hallway into the Attic printed as `code`: the
/// real Move whose arrival fires the Attic's `EnteredLocation` forced
/// abilities.
fn move_into_attic_as(code: &str) -> TestSession {
    move_board(hallway(), location_as(ATTIC, "Attic", code))
        .session()
        .take(&MOVE_TO_ATTIC)
}

fn horror(session: &TestSession) -> u8 {
    session.state().investigators[&INV].horror()
}

/// The Attic's forced on-enter horror resolves as part of the Move itself:
/// the step that takes the Move comes back to the turn menu with the horror
/// dealt and no prompt in between.
#[test]
fn forced_on_enter_resolves_immediately() {
    let session = move_into_attic_as(HORROR_ATTIC);

    session.assert_at_turn_menu(INV);
    assert_eq!(
        session.state().investigators[&INV].current_location,
        Some(ATTIC)
    );
    assert_eq!(horror(&session), 1);
    assert_event!(
        session.events(),
        Event::InvestigatorMoved {
            investigator: INV,
            to: ATTIC,
            ..
        }
    );
    assert_event!(
        session.events(),
        Event::HorrorPlacedOnInvestigator { investigator, amount: 1 } if *investigator == INV
    );
}

#[test]
fn forced_on_enter_no_op_when_location_has_no_abilities() {
    // "plain-loc" is not HORROR_ATTIC — mock registry returns None.
    let session = move_into_attic_as("plain-loc");

    session.assert_at_turn_menu(INV);
    assert_eq!(
        session.state().investigators[&INV].current_location,
        Some(ATTIC),
        "the investigator entered the location",
    );
    assert_eq!(horror(&session), 0);
    assert_no_event!(session.events(), Event::HorrorPlacedOnInvestigator { .. });
}

// ── PhaseEnded tests ──────────────────────────────────────────────────────────

/// Set the current act and agenda to `act` and `agenda` (either may be a code
/// the mock registry does not know, or absent). The builder stages neither
/// deck, so they are set on the built state.
fn with_act_and_agenda(mut state: GameState, act: Option<&str>, agenda: Option<&str>) -> GameState {
    if let Some(act) = act {
        state.act_deck = vec![Act {
            code: CardCode(act.into()),
            clue_threshold: 3,
        }];
        state.act_index = 0;
    }
    if let Some(agenda) = agenda {
        state.agenda_deck = vec![Agenda {
            code: CardCode(agenda.into()),
            doom_threshold: 3,
        }];
        state.agenda_index = 0;
    }
    state
}

/// One seated investigator, the lead, with a card to draw at Upkeep 4.4 so the
/// empty-deck horror penalty doesn't muddy the horror assertions of a phase
/// end that runs on through the round.
fn lead_alone() -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).deck(["filler-card"]))
        .with_turn_order([INV])
}

/// End the Enemy phase for real — step 3.4, staged with
/// [`ending_enemy_phase`](GameStateBuilder::ending_enemy_phase) — with `act`
/// and `agenda` current. Settling runs the phase end, its `PhaseEnded { Enemy }`
/// forced abilities, and play on through Upkeep into the next round's Mythos.
fn end_the_enemy_phase(act: Option<&str>, agenda: Option<&str>) -> TestSession {
    let session = TestSession::new(
        with_act_and_agenda(lead_alone().ending_enemy_phase().build(), act, agenda),
        registries(),
    );
    assert_phase_ended(&session, state::Phase::Enemy);
    session
}

/// Assert `phase` really ended in the session, so a no-op assertion is about
/// that phase end rather than a drive that never reached it.
#[track_caller]
fn assert_phase_ended(session: &TestSession, phase: state::Phase) {
    assert_event!(session.events(), Event::PhaseEnded { phase: p } if *p == phase);
}

/// End the Mythos phase for real, under the Enemy-keyed doom agenda: its
/// anchor staged after step 1.4's draws, so settling runs step 1.5's
/// `PhaseEnded { Mythos }` and play goes on into the Investigation phase.
fn end_the_mythos_phase() -> TestSession {
    let session = TestSession::new(
        with_act_and_agenda(
            lead_alone()
                .with_phase(state::Phase::Mythos)
                .with_phase_anchor(MythosPhaseFrame {
                    resume: MythosResume::AfterDraws,
                })
                .build(),
            None,
            Some(DOOM_AGENDA),
        ),
        registries(),
    );
    assert_phase_ended(&session, state::Phase::Mythos);
    session
}

/// End the Upkeep phase for real, under the Enemy-keyed doom agenda, staged
/// with [`ending_upkeep_phase`](GameStateBuilder::ending_upkeep_phase).
fn end_the_upkeep_phase() -> TestSession {
    let session = TestSession::new(
        with_act_and_agenda(
            lead_alone().ending_upkeep_phase().build(),
            None,
            Some(DOOM_AGENDA),
        ),
        registries(),
    );
    assert_phase_ended(&session, state::Phase::Upkeep);
    session
}

/// Fire `PhaseEnded { Investigation }` on its own over `INV`'s open turn,
/// under the Enemy-keyed doom agenda. Not the real phase end: ending the
/// Investigation phase for real runs straight on into the Enemy phase's end,
/// where the agenda's ability *does* fire, so the Investigation phase end
/// cannot be told apart from it in one drive.
fn fire_the_investigation_phase_end() -> TestSession {
    TestSession::new(
        with_act_and_agenda(lead_alone().open_turn(INV).build(), None, Some(DOOM_AGENDA)),
        registries(),
    )
    .fire_at(TimingEvent::PhaseEnded {
        phase: state::Phase::Investigation,
    })
}

#[test]
fn forced_on_enemy_phase_end_fires_agenda_ability() {
    let session = end_the_enemy_phase(None, Some(DOOM_AGENDA));

    assert_eq!(
        horror(&session),
        1,
        "the lead-investigator proxy should have taken 1 horror from agenda forced ability"
    );
    assert_event!(
        session.events(),
        Event::HorrorPlacedOnInvestigator { investigator, amount: 1 } if *investigator == INV
    );
}

#[test]
fn forced_on_phase_end_wrong_phase_fires_nothing() {
    // The agenda ability is keyed to Enemy; ending the Mythos phase is a no-op.
    let session = end_the_mythos_phase();

    assert_eq!(horror(&session), 0, "no horror for a non-matching phase");
    assert_no_event!(session.events(), Event::HorrorPlacedOnInvestigator { .. });
}

/// The three non-Enemy phases do not fire an Enemy-keyed forced ability —
/// exercises the `dsl_phase` mapping's negative side.
#[test]
fn dsl_phase_mapping_non_enemy_phases_produce_no_hits() {
    let ended = [
        (state::Phase::Mythos, end_the_mythos_phase()),
        (
            state::Phase::Investigation,
            fire_the_investigation_phase_end(),
        ),
        (state::Phase::Upkeep, end_the_upkeep_phase()),
    ];
    for (phase, session) in ended {
        assert_eq!(
            horror(&session),
            0,
            "no horror for phase {phase:?} (agenda keyed to Enemy only)"
        );
        assert_no_event!(session.events(), Event::HorrorPlacedOnInvestigator { .. });
    }
}

#[test]
fn forced_on_phase_end_no_op_when_agenda_has_no_abilities() {
    // "plain-agenda" → None from mock registry.
    let session = end_the_enemy_phase(None, Some("plain-agenda"));

    assert_eq!(horror(&session), 0);
    assert_no_event!(session.events(), Event::HorrorPlacedOnInvestigator { .. });
}

#[test]
fn forced_on_phase_end_no_op_when_no_act_or_agenda() {
    // Empty decks — common fixture shape for tests not modeling scenarios.
    // state.agenda_deck / act_deck are empty by default from GameStateBuilder.
    let session = end_the_enemy_phase(None, None);

    assert_eq!(horror(&session), 0);
    assert_no_event!(session.events(), Event::HorrorPlacedOnInvestigator { .. });
}

#[test]
fn forced_on_phase_end_no_op_when_no_lead_investigator() {
    // No turn_order → no lead-investigator proxy → early return. No real phase end runs
    // without one — every round needs one — so the timing point is
    // fired on its own, over a staged turn.
    let mut state =
        with_act_and_agenda(lead_alone().open_turn(INV).build(), None, Some(DOOM_AGENDA));
    state.turn_order.clear();

    let session = TestSession::new(state, registries()).fire_at(TimingEvent::PhaseEnded {
        phase: state::Phase::Enemy,
    });

    session.assert_at_turn_menu(INV);
    assert!(
        session.finish().events.is_empty(),
        "no events without a lead-investigator proxy"
    );
}

#[test]
fn forced_on_phase_end_fires_act_ability() {
    // Set current act to DOOM_ACT, no matching agenda (plain code → None).
    let session = end_the_enemy_phase(Some(DOOM_ACT), Some("plain-agenda"));

    assert_eq!(
        horror(&session),
        1,
        "the lead-investigator proxy should have taken 1 horror from act forced ability"
    );
    assert_event!(
        session.events(),
        Event::HorrorPlacedOnInvestigator { investigator, amount: 1 } if *investigator == INV
    );
}

// ── EndOfTurn tests ───────────────────────────────────────────────────────────

/// A lone `INV` at the Study holding `threat_area` ends their turn: the
/// `EndTurn` whose end of turn fires the threat area's `EndOfTurn` forced
/// abilities.
fn end_the_turn(threat_area: &[&str]) -> TestSession {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(1)
                .at(HERE)
                // A non-empty deck so Upkeep 4.4 draw_one_with_deckout doesn't
                // fire its "draw from empty deck" horror penalty and muddy the
                // horror assertion.
                .deck(["filler-card"])
                .threat_area(threat_area.iter().copied()),
        )
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .session()
        .take(&TurnAction::EndTurn)
}

#[test]
fn end_of_turn_forced_resolves_threat_area_ability() {
    // EndTurn for a lone investigator with an EndOfTurn threat-area card fires
    // its forced effect as part of ending the turn.
    let session = end_the_turn(&[END_OF_TURN_CARD]);

    assert_eq!(horror(&session), 1);
    assert_event!(
        session.events(),
        Event::HorrorPlacedOnInvestigator { investigator, amount: 1 } if *investigator == INV
    );
}

#[test]
fn end_of_turn_forced_no_op_without_threat_area_card() {
    let session = end_the_turn(&[]);

    assert_event!(session.events(), Event::TurnEnded { investigator } if *investigator == INV);
    assert_eq!(horror(&session), 0);
    assert_no_event!(session.events(), Event::HorrorPlacedOnInvestigator { .. });
}

// ── AfterLocationInvestigated tests ───────────────────────────────────────────

/// `INV` investigates the Study successfully, holding `threat_area` in their
/// threat area, and commits nothing: shroud 0, the test investigator's
/// intellect 3 and a `Numeric(0)` token always succeed.
fn investigate_successfully(threat_area: &[&str]) -> TestSession {
    let mut study = test_support::test_location(HERE.0, "Study");
    study.shroud = 0;
    study.clues = 1;
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(1)
                .threat_area(threat_area.iter().copied()),
        )
        .with_location(study)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .session()
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[])
}

#[test]
fn after_investigate_forced_resolves_threat_area_ability() {
    // A successful Investigate fires the threat-area AfterLocationInvestigated
    // forced effect, and the turn goes on.
    let session = investigate_successfully(&[AFTER_INVESTIGATE_CARD]);

    session.assert_at_turn_menu(INV);
    assert_eq!(horror(&session), 1);
    assert_event!(
        session.events(),
        Event::HorrorPlacedOnInvestigator { investigator, amount: 1 } if *investigator == INV
    );
}

#[test]
fn after_investigate_forced_no_op_without_threat_area_card() {
    let session = investigate_successfully(&[]);

    session.assert_at_turn_menu(INV);
    assert_eq!(horror(&session), 0);
    assert_no_event!(session.events(), Event::HorrorPlacedOnInvestigator { .. });
}

/// `INV`'s turn in the Hallway, next to the Double-Forced Room (two forced
/// on-enter abilities printed on the one location).
fn double_forced_board() -> GameStateBuilder {
    move_board(
        hallway(),
        location_as(ATTIC, "Double-Forced Room", DOUBLE_FORCED),
    )
}

#[test]
fn two_simultaneous_forced_triggers_present_a_choice() {
    // 2+ forced abilities at the same timing point let the lead choose the
    // order — dispatch suspends with
    // `AwaitingInput` instead of auto-resolving both in a fixed order. Driven
    // through `apply` (Move into a location with two forced on-enter abilities,
    // a terminal emit site) so the suspension round-trips.
    let session = double_forced_board().session().take(&MOVE_TO_ATTIC);

    assert_eq!(
        session.offered_at(&OptionTarget::Location(ATTIC)),
        2,
        "2+ simultaneous forced triggers must present the lead a choice; got {:?}",
        session.prompt(),
    );
    assert_eq!(
        horror(&session),
        0,
        "no forced effect resolves until the lead orders them",
    );
}

#[test]
fn two_simultaneous_forced_triggers_resolved_in_lead_chosen_order() {
    // Resume the choice: pick each forced trigger in turn; both resolve, the
    // move completes (back to the turn menu), total 2 horror.
    let paused = double_forced_board().session().take(&MOVE_TO_ATTIC);

    // Pick the first forced trigger. Both are printed on the room, so the
    // anchor names the room and the position names the ability.
    let after_first = paused.pick_nth_at(OptionTarget::Location(ATTIC), 0);
    // One forced resolved; the second is still pending, so the move isn't
    // done yet.
    assert_eq!(horror(&after_first), 1);
    assert_eq!(
        after_first.offered_at(&OptionTarget::Location(ATTIC)),
        1,
        "the remaining forced trigger is still to resolve",
    );

    // Pick the remaining forced trigger.
    let done = after_first.pick(OptionTarget::Location(ATTIC));
    done.assert_at_turn_menu(INV);
    assert_eq!(horror(&done), 2);
}

// ── An emit queues, so every site with post-emit work rides a frame ──────────

/// Both mock board cards carry a `PhaseEnded { Enemy }` forced ability, so step
/// 3.4 has two simultaneous hits and the lead orders them — a suspension
/// that outlives `engine::apply()`.
///
/// Regression for #569: the Enemy anchor is re-parked beneath the ordering
/// run and the transition runs on its re-exposure, so the run closes onto the
/// anchor rather than onto an empty continuation stack, which would leave
/// `phase` stuck at `Enemy` with every later input rejected.
#[test]
fn two_forced_at_enemy_phase_end_resolve_and_the_phase_still_transitions() {
    let state = board_with_two_phase_end_forced();

    // EndTurn cascades Investigation → Enemy → step 3.4 (no enemies, so the
    // attack loop drains and the final window auto-skips into `enemy_phase_end`).
    let paused = TestSession::new(state, registries()).take(&TurnAction::EndTurn);

    assert_eq!(
        (
            paused.offered_at(&OptionTarget::Act),
            paused.offered_at(&OptionTarget::Agenda),
            paused.prompt().options.len(),
        ),
        (1, 1, 2),
        "2+ forced at the enemy phase end must present the lead a choice: {:?}",
        paused.prompt(),
    );
    assert_eq!(
        horror(&paused),
        0,
        "no forced effect resolves until the lead orders them",
    );
    // The load-bearing bit: the phase's own frame is still there to resume onto.
    assert!(
        paused
            .state()
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::EnemyPhase(_))),
        "the Enemy anchor must survive beneath the ordering run; stack = {:?}",
        paused.state().continuations,
    );

    // Order them: each pick resolves one, and the second closes the run.
    let first = paused.pick(OptionTarget::Act);
    assert_eq!(horror(&first), 1);
    let second = first.pick(OptionTarget::Agenda);

    assert_eq!(
        horror(&second),
        2,
        "both forced abilities resolve once ordered",
    );
    assert_ne!(
        second.state().phase,
        state::Phase::Enemy,
        "the Enemy → Upkeep transition must run once the forced run closes, \
         not stall the phase; stack = {:?}",
        second.state().continuations,
    );
    assert!(
        !second.state().continuations.is_empty(),
        "the game must never be left with an empty continuation stack",
    );
}

/// A ready enemy waiting in the Attic.
fn lurker_in_the_attic() -> Enemy {
    let mut enemy = test_support::test_enemy(LURKER.0, "Lurker");
    enemy.current_location = Some(ATTIC);
    enemy
}

/// A `LeftLocation` forced that suspends (two attachment abilities → an ordering
/// run) must not cost the move its entered-location half.
///
/// Regression for #569: `engage_ready_enemies_on_enter` and the
/// `EnteredLocation` emit ride the `MoveEnter` frame parked beneath the emit,
/// so the suspension does not skip them, and leaving resolves before
/// entering.
#[test]
fn suspending_left_location_forced_still_engages_and_fires_entered_location() {
    // Two `LeftLocation` forced abilities on the left location's attachment.
    // The destination has its own on-enter forced (1 horror) and a ready enemy.
    let paused = move_board(
        hallway_with(DOUBLE_LEFT_LOCATION),
        location_as(ATTIC, "Attic", HORROR_ATTIC),
    )
    .with_enemy(lurker_in_the_attic())
    .session()
    .take(&MOVE_TO_ATTIC);
    assert_eq!(
        paused.offered_at(&HALLWAY_ATTACHMENT),
        2,
        "the two LeftLocation forced abilities must open an ordering run",
    );
    assert_eq!(
        horror(&paused),
        0,
        "leaving resolves nothing until the lead orders the two abilities",
    );

    // Each ability is interchangeable with its sibling; the first, then the
    // lone one left.
    let done = paused
        .pick_nth_at(HALLWAY_ATTACHMENT, 0)
        .pick(HALLWAY_ATTACHMENT)
        .finish();

    assert_eq!(
        done.state.investigators[&INV].horror(),
        3,
        "2 horror from leaving, then 1 from the entered location's forced — the \
         enter half is not skipped by the suspension",
    );
    assert_eq!(
        done.state.enemies[&LURKER].engaged_with,
        Some(INV),
        "the destination's ready enemy engages on entry, after the move resumes",
    );
    // Ordering: leaving fully resolves before entering fires. This last step
    // holds the tail of the sequence — the second LeftLocation ability, then the
    // engage, then the entered location's forced.
    let horror_positions: Vec<usize> = done
        .events
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e, Event::HorrorPlacedOnInvestigator { .. }))
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
        "this step resolves the second LeftLocation ability and the entered \
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
/// and the entered-location half still resolves after it.
///
/// `glossary/When.md`: *"the moment immediately after the specified timing point
/// or triggering condition initiates, but before its impact upon the game state
/// resolves"* — and the departure's impact is `InvestigatorMoved`. Barricade
/// 01038's *"Forced - When an investigator leaves attached location: Discard
/// Barricade."* is the real card this shape stands in for; its own end-to-end
/// coverage is `cards/tests/barricade.rs`.
#[test]
fn a_when_cell_left_location_forced_resolves_before_the_departure_lands() {
    // The destination has its own on-enter forced (1 horror) and a ready enemy,
    // so the whole tail of the move is visible in one event log.
    let r = move_board(
        hallway_with(WHEN_LEFT_LOCATION),
        location_as(ATTIC, "Attic", HORROR_ATTIC),
    )
    .with_enemy(lurker_in_the_attic())
    .session()
    .take(&MOVE_TO_ATTIC)
    .finish();
    assert!(
        !matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "the `when` cell is walked on a coordinator-owned condition: {:?}",
        r.outcome,
    );
    // The whole sequence, in order: the interrupt, then the departure's own
    // impact, then the arrival's engage, then the entered location's forced.
    assert_event_sequence!(
        r.events,
        Event::HorrorPlacedOnInvestigator { .. },
        Event::InvestigatorMoved { .. },
        Event::EnemyEngaged { .. },
        Event::HorrorPlacedOnInvestigator { .. },
    );
    assert_eq!(
        r.state.investigators[&INV].horror(),
        2,
        "1 horror from leaving, 1 from the entered location's forced",
    );
    assert_eq!(
        r.state.investigators[&INV].current_location,
        Some(ATTIC),
        "the departure still lands",
    );
}

/// The destination reveal is the **arrival's** business, not the departure's:
/// it rides the `MoveEnter` frame parked beneath the emit, so it lands after
/// the whole `LeftLocation` sequence and before the entered location's own
/// forced ability.
///
/// In `move::perform` it would reveal the destination before the
/// investigator had even left the location they were on — Rules Reference
/// p.14 reveals a location when an investigator *enters* it.
#[test]
fn the_destination_reveal_belongs_to_the_arrival_not_the_departure() {
    let mut attic = location_as(ATTIC, "Attic", HORROR_ATTIC);
    attic.revealed = false;

    let session = move_board(hallway_with(WHEN_LEFT_LOCATION), attic)
        .session()
        .take(&MOVE_TO_ATTIC);
    assert_event_sequence!(
        session.events(),
        Event::HorrorPlacedOnInvestigator { .. },
        Event::InvestigatorMoved { .. },
        Event::LocationRevealed { .. },
        Event::HorrorPlacedOnInvestigator { .. },
    );
    assert!(
        session.state().locations[&ATTIC].revealed,
        "the destination is still revealed by the move",
    );
}

/// The mid-sequence board, observed directly: with two `when`-cell
/// `LeftLocation` abilities the lead has to order them, and the move suspends
/// with the departure **not yet resolved** — the investigator still standing on
/// the location they are leaving, the enemy engaged with them still beside them.
///
/// This is the state-level counterpart to the event-order assertion above, and
/// the one an event log cannot make: it reads the board while the ability
/// runs, before either has moved. It also pins that the engaged enemy still
/// rides along (`glossary/Enemy_Engagement.md`: such an enemy *"remains engaged
/// and moves to the new location simultaneously with the investigator"*) when the
/// departure finally lands.
#[test]
fn a_suspended_when_cell_sees_the_investigator_still_at_the_location_they_are_leaving() {
    let mut attic = test_support::test_location(ATTIC.0, "Attic");
    attic.connections = vec![HERE];

    // Engaged, and exhausted so the departure is not followed by a re-engage at
    // the destination muddying the reading.
    let mut enemy = test_support::test_enemy(LURKER.0, "Lurker");
    enemy.exhausted = true;

    let paused = move_board(hallway_with(DOUBLE_WHEN_LEFT_LOCATION), attic)
        .with_enemy_engaged(enemy, INV)
        .session()
        .take(&MOVE_TO_ATTIC);
    assert_eq!(
        paused.offered_at(&HALLWAY_ATTACHMENT),
        2,
        "the two `when`-cell abilities must open an ordering run",
    );
    assert_eq!(
        paused.state().investigators[&INV].current_location,
        Some(HERE),
        "the `when` cell interrupts the departure: it has not landed yet",
    );
    assert_eq!(
        paused.state().enemies[&LURKER].current_location,
        Some(HERE),
        "nor has the engaged enemy been dragged along yet",
    );
    assert_no_event!(paused.events(), Event::InvestigatorMoved { .. });

    let done = paused
        .pick_nth_at(HALLWAY_ATTACHMENT, 0)
        .pick(HALLWAY_ATTACHMENT);
    // The move completes with no further prompt.
    done.assert_at_turn_menu(INV);
    assert_eq!(
        done.state().investigators[&INV].current_location,
        Some(ATTIC),
        "the departure lands once the `when` cell is done",
    );
    let enemy = &done.state().enemies[&LURKER];
    assert_eq!(
        enemy.current_location,
        Some(ATTIC),
        "the engaged enemy rides along at the resolve step",
    );
    assert_eq!(enemy.engaged_with, Some(INV), "still engaged");
}

/// Step 4.6's `PhaseEnded { Upkeep }` forced ability resolves before the round
/// end that follows it.
///
/// Regression for #569: the round-end emit lives in the anchor's
/// `AfterPhaseEndForced` resume arm, which the loop reaches only once the
/// phase-end ability has resolved. Emitting `RoundEnded` on the phase end's
/// `Done` would push the round-end coordinator *above* the ability it had
/// just queued, so the round end would resolve first.
#[test]
fn upkeep_phase_end_forced_resolves_before_the_round_end() {
    let state = with_act_and_agenda(
        GameStateBuilder::new()
            .with_registries(registries())
            .with_seat(seat(1).at(HERE))
            .with_location(test_support::test_location(HERE.0, "Study"))
            .with_turn_order([INV])
            .ending_upkeep_phase()
            .build(),
        Some(UPKEEP_END_ACT),
        Some(ROUND_END_AGENDA),
    );

    let session = TestSession::new(state, registries());
    let events = session.events();

    let phase_end = events
        .iter()
        .position(|e| matches!(e, Event::HorrorPlacedOnInvestigator { amount: 1, .. }))
        .expect("the PhaseEnded(Upkeep) forced ability must resolve");
    let round_end = events
        .iter()
        .position(|e| matches!(e, Event::HorrorPlacedOnInvestigator { amount: 2, .. }))
        .expect("the RoundEnded forced ability must resolve");
    assert!(
        phase_end < round_end,
        "step 4.6's phase-end ability resolves before the round end it precedes; \
         events = {events:?}",
    );
}

/// Investigation-phase board with one investigator out of actions and both mock
/// board cards keyed to `PhaseEnded { Enemy }` — the two-hit step-3.4 fixture.
fn board_with_two_phase_end_forced() -> GameState {
    with_act_and_agenda(
        GameStateBuilder::new()
            .with_registries(registries())
            .with_seat(
                seat(1)
                    .at(HERE)
                    .actions(0)
                    // A card to draw at Upkeep 4.4, so the empty-deck horror
                    // penalty doesn't muddy the horror assertions downstream of
                    // the transition.
                    .deck(["filler-card"]),
            )
            .with_location(test_support::test_location(HERE.0, "Study"))
            .open_turn(INV)
            .build(),
        Some(DOOM_ACT),
        Some(DOOM_AGENDA),
    )
}

// 2+ simultaneous forced abilities resolve through the lead-ordered run,
// covered by `two_simultaneous_forced_triggers_present_a_choice` +
// `two_simultaneous_forced_triggers_resolved_in_lead_chosen_order` through `apply`.

// The forced scan asks the same eligibility question the reaction-offer
// scan does, so a forced ability's `eligibility` condition gates *initiation*. Rules
// Reference, "Ability" -> "Forced Abilities" (p.2):
//
//   "If a forced ability does not have the potential to change the game state,
//    the ability does not initiate."
//
// Three tags, one per outcome: a passing predicate fires, a failing one does
// not, and one with no registered predicate is suppressed rather than resolved
// — the reaction side's posture, so a half-installed host never fires a gate it
// cannot evaluate. That last is a content fault, so a debug build panics on it
// rather than letting it pass the tests silently.

/// Enter `code`'s location and return the entering investigator's horror, so a
/// gated `EnteredLocation` forced is measured by whether its 1 horror landed.
fn horror_after_entering(code: &str) -> u8 {
    let session = move_into_attic_as(code);
    session.assert_at_turn_menu(INV);
    horror(&session)
}

#[test]
fn a_forced_with_a_satisfied_eligibility_tag_still_fires() {
    assert_eq!(horror_after_entering(GATED_ELIGIBLE), 1);
}

/// Regression for #786: a forced ability whose eligibility condition fails
/// does not initiate.
#[test]
fn a_forced_with_an_unmet_eligibility_tag_does_not_initiate() {
    assert_eq!(
        horror_after_entering(GATED_INELIGIBLE),
        0,
        "a false eligibility condition keeps the forced out of the candidate list",
    );
}

#[test]
#[cfg_attr(
    debug_assertions,
    should_panic(expected = "Native condition \"test:unregistered\": no predicate registered")
)]
fn a_forced_whose_eligibility_tag_has_no_predicate_panics_in_debug_and_is_suppressed_in_release() {
    assert_eq!(
        horror_after_entering(GATED_UNKNOWN_TAG),
        0,
        "an unresolvable gate suppresses rather than guessing in the ability's favour",
    );
}
