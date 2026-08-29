//! All eight phase boundaries are timing points (#697).
//!
//! `Appendix_II_Timing_and_Gameplay.md` gives the same sentence to step 1.1 and
//! to step 1.5, and to no other numbered step:
//!
//! > The beginning of a phase is an important game milestone that may be
//! > referenced in card text, either as a point at which an ability may or must
//! > resolve, or as a point at which a delayed effect resolves or a lasting
//! > effect expires.
//!
//! So each of the four phases begins and ends at a point a card can listen on,
//! and all eight route through `queue_event`. Before this, two of the four
//! phase-ends pushed a bare `Event::PhaseEnded` log entry and ran no scan, and
//! there was no phase-*start* trigger point at all.
//!
//! Driven against a mock registry rather than the corpus: the corpus consumers
//! are Wizard of the Order 01170 at the Mythos end (which needs a
//! doom-on-a-card model it does not have yet) and three Dunwich cards at the
//! Enemy start, so a real card cannot yet stand at seven of the eight. The act
//! declares one marker ability per boundary instead, and the assertion is which
//! markers fired and in what order.

use card_dsl::dsl::{forced_on_event, native, Ability, EventPattern, EventTiming};
use game_core::action::InputResponse;
use game_core::card_data::{CardKind, CardMetadata};
use game_core::card_registry::{self, CardRegistry, NativeEffectFn};
use game_core::engine::enumerate::legal_actions;
use game_core::engine::OptionId;
use game_core::event::Event;
use game_core::state::{Act, CardCode, InvestigatorId, LocationId, Phase};
use game_core::test_support::{
    metadata_for_test_inv, test_investigator, test_location, GameStateBuilder,
};
use game_core::{apply, Action, Cx, EngineOutcome, EvalContext, PlayerAction, TurnAction};

/// The act carrying one marker forced ability per phase boundary.
const ACT: &str = "TEST-BOUNDARIES";

/// A boundary marker: two `ResourcesGained` events, the first naming the
/// boundary that fired it and the second the `state.phase` it observed while
/// firing. Amounts start at 10 so neither can be confused with the upkeep step
/// 4.4 resource gain, which is 1.
///
/// The observed phase is what pins the ADR 0003 tail-position discipline: a
/// forced ability queued at a phase boundary must resolve *before* the driver
/// frame's own tail work, so a step-2.3 ability sees `Investigation` and not the
/// `Enemy` the transition would have set had it run above the queued frame.
fn mark(cx: &mut Cx, ctx: &EvalContext, amount: u8) -> EngineOutcome {
    let observed = observed(cx.state.phase);
    cx.events.push(Event::ResourcesGained {
        investigator: ctx.controller,
        amount,
    });
    cx.events.push(Event::ResourcesGained {
        investigator: ctx.controller,
        amount: observed,
    });
    EngineOutcome::Done
}

/// Encode a `Phase` as a marker amount, disjoint from the boundary ids.
fn observed(phase: Phase) -> u8 {
    match phase {
        Phase::Mythos => 51,
        Phase::Investigation => 52,
        Phase::Enemy => 53,
        Phase::Upkeep => 54,
    }
}

const START_MYTHOS: u8 = 11;
const START_INVESTIGATION: u8 = 12;
const START_ENEMY: u8 = 13;
const START_UPKEEP: u8 = 14;
const END_MYTHOS: u8 = 21;
const END_INVESTIGATION: u8 = 22;
const END_ENEMY: u8 = 23;
const END_UPKEEP: u8 = 24;

/// A no-ability treachery for the encounter deck: the step-1.4 draw needs a
/// card to turn over, and a treachery with no Revelation resolves to the
/// discard pile without touching the board.
const TREACHERY: &str = "TEST-BLANK-TREACHERY";

fn blank_treachery_metadata() -> &'static CardMetadata {
    static META: std::sync::OnceLock<CardMetadata> = std::sync::OnceLock::new();
    META.get_or_init(|| CardMetadata {
        code: TREACHERY.into(),
        name: "Blank Treachery".into(),
        text: None,
        traits: Vec::new(),
        back_name: None,
        back_text: None,
        pack_code: "_synth".into(),
        weakness: false,
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    })
}

fn mock_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    if code.as_str() == TREACHERY {
        return Some(blank_treachery_metadata());
    }
    metadata_for_test_inv(code)
}

fn started(phase: card_dsl::dsl::Phase, tag: &'static str) -> Ability {
    forced_on_event(
        EventPattern::PhaseStarted { phase },
        EventTiming::At,
        native(tag),
    )
}

fn ended(phase: card_dsl::dsl::Phase, tag: &'static str) -> Ability {
    forced_on_event(
        EventPattern::PhaseEnded { phase },
        EventTiming::At,
        native(tag),
    )
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    use card_dsl::dsl::Phase as P;
    (code.as_str() == ACT).then(|| {
        vec![
            started(P::Mythos, "mark:start-mythos"),
            started(P::Investigation, "mark:start-investigation"),
            started(P::Enemy, "mark:start-enemy"),
            started(P::Upkeep, "mark:start-upkeep"),
            ended(P::Mythos, "mark:end-mythos"),
            ended(P::Investigation, "mark:end-investigation"),
            ended(P::Enemy, "mark:end-enemy"),
            ended(P::Upkeep, "mark:end-upkeep"),
        ]
    })
}

fn mock_native_for(tag: &str) -> Option<NativeEffectFn> {
    let f: NativeEffectFn = match tag {
        "mark:start-mythos" => |cx, ctx| mark(cx, ctx, START_MYTHOS),
        "mark:start-investigation" => |cx, ctx| mark(cx, ctx, START_INVESTIGATION),
        "mark:start-enemy" => |cx, ctx| mark(cx, ctx, START_ENEMY),
        "mark:start-upkeep" => |cx, ctx| mark(cx, ctx, START_UPKEEP),
        "mark:end-mythos" => |cx, ctx| mark(cx, ctx, END_MYTHOS),
        "mark:end-investigation" => |cx, ctx| mark(cx, ctx, END_INVESTIGATION),
        "mark:end-enemy" => |cx, ctx| mark(cx, ctx, END_ENEMY),
        "mark:end-upkeep" => |cx, ctx| mark(cx, ctx, END_UPKEEP),
        _ => return None,
    };
    Some(f)
}

#[ctor::ctor(unsafe)]
fn install() {
    let _ = card_registry::install(CardRegistry {
        metadata_for: mock_metadata_for,
        abilities_for: mock_abilities_for,
        back_abilities_for: |_| None,
        native_effect_for: mock_native_for,
        native_eligibility_for: |_| None,
        native_condition_for: |_| None,
    });
}

/// The boundary markers in the order they fired, dropping the observed-phase
/// companions and every other `ResourcesGained` (the upkeep 4.4 gain of 1).
fn markers(events: &[Event]) -> Vec<u8> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::ResourcesGained { amount, .. } if (10..50).contains(amount) => Some(*amount),
            _ => None,
        })
        .collect()
}

/// Every marker with the `state.phase` it observed while firing, paired.
fn markers_with_observed_phase(events: &[Event]) -> Vec<(u8, u8)> {
    let raw: Vec<u8> = events
        .iter()
        .filter_map(|e| match e {
            Event::ResourcesGained { amount, .. } if *amount >= 10 => Some(*amount),
            _ => None,
        })
        .collect();
    raw.chunks_exact(2).map(|p| (p[0], p[1])).collect()
}

/// A single investigator mid-Investigation, one turn from the phase's end, with
/// the marker act current. Mirrors `round_ended`'s fixture: `EndTurn` from here
/// cascades Investigation → Enemy → Upkeep → Mythos through the real drive loop.
fn mid_investigation() -> game_core::state::GameState {
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.actions_remaining = 0;
    // Non-empty deck so the upkeep 4.4 draw doesn't fire a deckout penalty.
    inv.deck = vec![CardCode::new("filler-card")];

    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_location(10, "Study"))
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        .with_phase_anchor(game_core::state::Continuation::InvestigationPhase {
            resume: game_core::state::InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(InvestigatorId(1))
        .build();
    state.act_deck = vec![Act {
        code: CardCode::new(ACT),
        clue_threshold: 0,
    }];
    state.act_index = 0;
    state.encounter_deck.push_back(CardCode::new(TREACHERY));
    state
}

fn end_turn_action(state: &game_core::state::GameState) -> Action {
    let idx = legal_actions(state)
        .iter()
        .position(|a| a == &TurnAction::EndTurn)
        .expect("EndTurn must be a legal open-turn action");
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
    })
}

#[test]
fn every_phase_boundary_fires_its_forced_ability_in_round_order() {
    let state = mid_investigation();
    let end_turn = end_turn_action(&state);
    let cascade = apply(state, end_turn);

    // The cascade runs 2.3 → 3.1 → 3.4 → 4.1 → 4.6 → 1.1 and parks at the
    // step-1.4 encounter-draw prompt.
    assert!(
        matches!(cascade.outcome, EngineOutcome::AwaitingInput { .. }),
        "the cascade parks at the step-1.4 encounter draw; got {:?}",
        cascade.outcome,
    );
    assert_eq!(
        markers(&cascade.events),
        vec![
            END_INVESTIGATION,
            START_ENEMY,
            END_ENEMY,
            START_UPKEEP,
            END_UPKEEP,
            START_MYTHOS,
        ],
        "each boundary the cascade crosses fires its forced ability, in the \
         printed step order 2.3 / 3.1 / 3.4 / 4.1 / 4.6 / 1.1",
    );
}

#[test]
fn the_mythos_end_and_the_investigation_start_fire_across_the_draw_prompt() {
    // The last two of the eight sit on the far side of the step-1.4 encounter
    // draw, so this picks the cascade up where the test above parks and confirms
    // the draw, driving 1.4 → 1.5 → 2.1.
    let state = mid_investigation();
    let end_turn = end_turn_action(&state);
    let parked = apply(state, end_turn);
    assert!(
        matches!(parked.outcome, EngineOutcome::AwaitingInput { .. }),
        "parked at the step-1.4 encounter draw; got {:?}",
        parked.outcome,
    );

    let resumed = apply(
        parked.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert_eq!(
        resumed.state.phase,
        Phase::Investigation,
        "confirming the draw runs 1.5 and the Mythos → Investigation transition",
    );
    assert_eq!(
        markers(&resumed.events),
        vec![END_MYTHOS, START_INVESTIGATION],
        "steps 1.5 and 2.1 each fire their forced ability, in that order",
    );
}

#[test]
fn a_boundarys_forced_ability_resolves_before_its_drivers_tail_work() {
    // ADR 0003: the emit *queues* frames rather than resolving them, so each
    // boundary emits in tail position and the driver's own tail — the phase
    // transition, the phase's opening steps, the first player window — runs from
    // the anchor's resume beneath it. The observable is what `state.phase` the
    // forced ability saw: a step-2.3 ability that saw `Enemy` would mean the
    // transition had been pushed *above* the frame the emit had just queued,
    // which is the #569 shape this discipline exists to prevent.
    let state = mid_investigation();
    let end_turn = end_turn_action(&state);
    let cascade = apply(state, end_turn);

    assert_eq!(
        markers_with_observed_phase(&cascade.events),
        vec![
            (END_INVESTIGATION, observed(Phase::Investigation)),
            (START_ENEMY, observed(Phase::Enemy)),
            (END_ENEMY, observed(Phase::Enemy)),
            (START_UPKEEP, observed(Phase::Upkeep)),
            (END_UPKEEP, observed(Phase::Upkeep)),
            (START_MYTHOS, observed(Phase::Mythos)),
        ],
        "each boundary's forced ability resolves while its own phase is still \
         current — the transition to the next one runs after it, from the \
         anchor's resume",
    );
}
