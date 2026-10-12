//! The engine exits a native changes the game through (#1010): one synthetic
//! native per exit, run through the real `apply` path.
//!
//! Each native sits on its own mock act as a forced ability at the end of the
//! enemy phase, the `native_effect.rs` idiom. The natives model the exits, not
//! printed cards (ADR 0016). An act hosts them rather than an agenda so the
//! empty-agenda-deck guard can be exercised with no agenda in play.
//!
//! Its own test binary: the mock registry installs once per process, and the
//! sibling exit tests (`spend_uses.rs`, `native_cx.rs`) install their own.

use card_dsl::dsl::{self, Ability, EventPattern, EventTiming};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{board, ApplyResult, EngineOutcome, NativeCx, TimingEvent};
use game_core::event::{Event, TraumaKind};
use game_core::state::{
    self, Act, Agenda, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, Continuation,
    DiscardPile, GameState, GameStateBuilder, InvestigatorId, LocationId, Owner, SkillKind,
    SkillSubstitution, Zone,
};
use game_core::test_support::{self, MockRegistry, TestSession};
use game_core::{assert_event, assert_no_event};

const TRAUMA_ACT: &str = "TEST-ACT-TRAUMA";
const DOOM_ACT: &str = "TEST-ACT-DOOM";
const PLACE_ACT: &str = "TEST-ACT-PLACE";
const CLUES_ACT: &str = "TEST-ACT-CLUES";
const DIG_ACT: &str = "TEST-ACT-DIG";
const SUBSTITUTE_ACT: &str = "TEST-ACT-SUBSTITUTE";

/// The card instance the `clues` native removes clues from.
const CLUE_CARD: CardInstanceId = CardInstanceId(77);

/// Where the `place` native puts investigator 1.
const DEST: LocationId = LocationId(20);

/// Forced at end of enemy phase -> the native effect tagged `tag`.
fn forced_native(tag: &'static str) -> Vec<Ability> {
    vec![dsl::forced_on_event(
        EventPattern::PhaseEnded {
            phase: dsl::Phase::Enemy,
        },
        EventTiming::After,
        dsl::native(tag),
    )]
}

fn trauma(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    cx.suffer_trauma(ctx.controller, TraumaKind::Mental, 2);
    EngineOutcome::Done
}

fn doom(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    cx.place_doom_on_agenda(3);
    EngineOutcome::Done
}

fn place(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    cx.place_investigator(ctx.controller, DEST);
    EngineOutcome::Done
}

fn clues(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    cx.remove_clues(CLUE_CARD, 3);
    EngineOutcome::Done
}

/// Take the top encounter card, if any, and discard it — one step of a
/// deck search like Rise of the Ghouls 01106's.
fn dig(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    if let Some(code) = cx.take_encounter_top() {
        cx.discard_encounter_card(code, Zone::EncounterDeck);
    }
    EngineOutcome::Done
}

fn substitute(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    cx.add_skill_substitution(SkillSubstitution {
        investigator: ctx.controller,
        use_skill: SkillKind::Intellect,
        for_skills: vec![SkillKind::Combat],
    });
    EngineOutcome::Done
}

#[ctor::ctor(unsafe)]
fn install() {
    MockRegistry::new()
        .with_abilities(TRAUMA_ACT, || forced_native("test:trauma"))
        .with_native_effect("test:trauma", trauma)
        .with_abilities(DOOM_ACT, || forced_native("test:doom"))
        .with_native_effect("test:doom", doom)
        .with_abilities(PLACE_ACT, || forced_native("test:place"))
        .with_native_effect("test:place", place)
        .with_abilities(CLUES_ACT, || forced_native("test:clues"))
        .with_native_effect("test:clues", clues)
        .with_abilities(DIG_ACT, || forced_native("test:dig"))
        .with_native_effect("test:dig", dig)
        .with_abilities(SUBSTITUTE_ACT, || forced_native("test:substitute"))
        .with_native_effect("test:substitute", substitute)
        .install();
}

/// One Active investigator in turn order (the `PhaseEnded` forced dispatch
/// binds the controller to the first one), and `act` as the current act.
fn state_with_act(act: &str) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.act_deck = vec![Act {
        code: CardCode::new(act),
        clue_threshold: 99,
    }];
    state.act_index = 0;
    state
}

fn run(state: GameState) -> ApplyResult {
    TestSession::new(state)
        .fire_at(TimingEvent::PhaseEnded {
            phase: state::Phase::Enemy,
        })
        .finish()
}

#[test]
fn suffer_trauma_announces_trauma_suffered() {
    let ApplyResult {
        outcome, events, ..
    } = run(state_with_act(TRAUMA_ACT));
    assert_eq!(outcome, EngineOutcome::Done);
    assert_event!(
        events,
        Event::TraumaSuffered {
            investigator: InvestigatorId(1),
            kind: TraumaKind::Mental,
            amount: 2,
        }
    );
}

#[test]
fn place_doom_on_agenda_places_doom_on_the_current_agenda() {
    let mut state = state_with_act(DOOM_ACT);
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("TEST-AGENDA"),
        doom_threshold: 10,
    }];
    let ApplyResult { state, outcome, .. } = run(state);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.agenda_doom, 3);
}

#[test]
fn place_doom_on_agenda_is_a_no_op_on_an_empty_agenda_deck() {
    let state = state_with_act(DOOM_ACT);
    assert!(state.agenda_deck.is_empty());
    let ApplyResult { state, outcome, .. } = run(state);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.agenda_doom, 0, "no agenda to place doom on");
}

/// `state_with_act(PLACE_ACT)` with locations 10 and `DEST` on the board and
/// investigator 1 at `at`.
fn placement_state(at: Option<LocationId>) -> GameState {
    let mut state = state_with_act(PLACE_ACT);
    for loc in [
        test_support::test_location(10, "Origin"),
        test_support::test_location(DEST.0, "Destination"),
    ] {
        state.locations.insert(loc.id, loc);
    }
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .expect("investigator 1")
        .current_location = at;
    state
}

#[test]
fn place_investigator_moves_and_announces_a_change_of_location() {
    let ApplyResult {
        state,
        outcome,
        events,
        ..
    } = run(placement_state(Some(LocationId(10))));
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&InvestigatorId(1)].current_location,
        Some(DEST)
    );
    assert_event!(
        events,
        Event::InvestigatorMoved {
            investigator: InvestigatorId(1),
            from: LocationId(10),
            to: DEST,
        }
    );
}

#[test]
fn place_investigator_already_there_announces_nothing() {
    let ApplyResult {
        state,
        outcome,
        events,
        ..
    } = run(placement_state(Some(DEST)));
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&InvestigatorId(1)].current_location,
        Some(DEST)
    );
    assert_no_event!(events, Event::InvestigatorMoved { .. });
}

#[test]
fn place_investigator_from_no_location_places_without_announcing() {
    let ApplyResult {
        state,
        outcome,
        events,
        ..
    } = run(placement_state(None));
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&InvestigatorId(1)].current_location,
        Some(DEST)
    );
    assert_no_event!(events, Event::InvestigatorMoved { .. });
}

/// `state_with_act(CLUES_ACT)` with `CLUE_CARD` holding `clues` clues,
/// attached to a location rather than in the controller's areas, since the
/// exit finds the instance anywhere on the board.
fn clue_card_state(clues: u8) -> GameState {
    let mut state = state_with_act(CLUES_ACT);
    let mut card = CardInPlay::enter_play(
        CardCode::new("TEST-CLUE-CARD"),
        CLUE_CARD,
        Owner::EncounterDeck,
    );
    card.clues = clues;
    let mut location = test_support::test_location(10, "Elsewhere");
    location.attachments.push(card);
    state.locations.insert(location.id, location);
    state
}

fn clues_on_clue_card(state: &GameState) -> u8 {
    board::find_instance(state, CLUE_CARD)
        .expect("the clue card stays on the board")
        .0
        .clues
}

#[test]
fn remove_clues_removes_that_many_clues_from_the_instance() {
    let ApplyResult { state, outcome, .. } = run(clue_card_state(5));
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(clues_on_clue_card(&state), 2);
}

#[test]
fn remove_clues_saturates_at_zero() {
    let ApplyResult { state, outcome, .. } = run(clue_card_state(2));
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(clues_on_clue_card(&state), 0);
}

fn codes(codes: &[&str]) -> Vec<CardCode> {
    codes.iter().copied().map(CardCode::new).collect()
}

#[test]
fn take_encounter_top_and_discard_encounter_card_move_the_top_card_to_the_discard() {
    let mut state = state_with_act(DIG_ACT);
    state.encounter_deck = codes(&["TOP", "NEXT"]).into();
    state.encounter_discard = codes(&["OLD"]);
    let ApplyResult {
        state,
        outcome,
        events,
        ..
    } = run(state);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(Vec::from(state.encounter_deck), codes(&["NEXT"]));
    assert_eq!(state.encounter_discard, codes(&["OLD", "TOP"]));
    assert_event!(
        events,
        Event::CardDiscarded {
            code,
            from: Zone::EncounterDeck,
            to: DiscardPile::Encounter,
        } if code.as_str() == "TOP"
    );
}

#[test]
fn take_encounter_top_on_an_empty_deck_takes_nothing_and_does_not_reshuffle() {
    let mut state = state_with_act(DIG_ACT);
    state.encounter_discard = codes(&["OLD"]);
    let ApplyResult {
        state,
        outcome,
        events,
        ..
    } = run(state);
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(state.encounter_deck.is_empty());
    assert_eq!(state.encounter_discard, codes(&["OLD"]));
    assert_no_event!(events, Event::EncounterDeckShuffled);
    assert_no_event!(events, Event::CardDiscarded { .. });
}

/// Whether a Combat test started on `state` by investigator 1 offers the
/// "use Intellect in place of Combat?" prompt.
fn combat_test_offers_substitution(mut state: GameState) -> bool {
    state.chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)]);
    let result = test_support::perform_skill_test(state, InvestigatorId(1), SkillKind::Combat, 1);
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "{:?}",
        result.outcome
    );
    matches!(
        result.state.continuations.top(),
        Some(Continuation::SubstitutionPrompt(frame)) if frame.investigator == InvestigatorId(1)
    )
}

#[test]
fn add_skill_substitution_is_offered_at_the_next_skill_test() {
    assert!(
        !combat_test_offers_substitution(state_with_act(SUBSTITUTE_ACT)),
        "control: no substitution before the native runs"
    );
    let ApplyResult { state, outcome, .. } = run(state_with_act(SUBSTITUTE_ACT));
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(combat_test_offers_substitution(state));
}
