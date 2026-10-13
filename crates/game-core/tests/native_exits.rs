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
use game_core::engine::enumerate::TurnAction;
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{board, EngineOutcome, NativeCx, OptionTarget, PromptNature, TimingEvent};
use game_core::event::{Event, TraumaKind};
use game_core::state::{
    self, Act, Agenda, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, DiscardPile,
    GameState, GameStateBuilder, InvestigatorId, LocationId, Owner, SkillKind, SkillSubstitution,
    Zone,
};
use game_core::test_support::{self, seat, MockRegistry, Seat, TestSession};
use game_core::{assert_event, assert_no_event};

const TRAUMA_ACT: &str = "_nx_act_trauma";
const DOOM_ACT: &str = "_nx_act_doom";
const PLACE_ACT: &str = "_nx_act_place";
const CLUES_ACT: &str = "_nx_act_clues";
const DIG_ACT: &str = "_nx_act_dig";
const SUBSTITUTE_ACT: &str = "_nx_act_substitute";

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

const INV: InvestigatorId = InvestigatorId(1);
/// Where investigator 1 starts on the placement board.
const HERE: LocationId = LocationId(10);

/// `seated` as the lead on their open turn (the `PhaseEnded` forced dispatch
/// binds the controller to the first investigator in turn order). A filler
/// deck keeps Upkeep 4.4's draw from dealing empty-deck horror once the enemy
/// phase has ended.
fn lead(seated: Seat) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_seat(seated.deck(["filler-card"]))
        .open_turn(INV)
}

/// `builder` built, with `act` as the current act.
fn with_act(builder: GameStateBuilder, act: &str) -> GameState {
    let mut state = builder.build();
    // No builder setter stages the act deck.
    state.act_deck = vec![Act {
        code: CardCode::new(act),
        clue_threshold: 99,
    }];
    state.act_index = 0;
    state
}

/// The plain lead, and `act` as the current act.
fn state_with_act(act: &str) -> GameState {
    with_act(lead(seat(1)), act)
}

/// End the lead's turn, and with it the Investigation phase, so play runs into
/// the Enemy phase and its end, where the act's forced native fires. Play then
/// goes on through Upkeep into the next round's Mythos phase, and rests at its
/// step-1.4 encounter draw.
fn run(state: GameState) -> TestSession {
    let session = TestSession::new(state).take(&TurnAction::EndTurn);
    assert_event!(
        session.events(),
        Event::PhaseEnded {
            phase: state::Phase::Enemy
        }
    );
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::EncounterDeck),
        "play went on to the next round's encounter draw"
    );
    session
}

#[test]
fn suffer_trauma_announces_trauma_suffered() {
    let session = run(state_with_act(TRAUMA_ACT));
    assert_event!(
        session.events(),
        Event::TraumaSuffered {
            investigator: INV,
            kind: TraumaKind::Mental,
            amount: 2,
        }
    );
}

#[test]
fn place_doom_on_agenda_places_doom_on_the_current_agenda() {
    let mut state = state_with_act(DOOM_ACT);
    // No builder setter stages the agenda deck.
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("TEST-AGENDA"),
        doom_threshold: 10,
    }];
    let session = run(state);
    assert_eq!(
        session.state().agenda_doom,
        3 + 1,
        "the native's 3, then the next round's Mythos 1.2 doom"
    );
}

#[test]
fn place_doom_on_agenda_is_a_no_op_on_an_empty_agenda_deck() {
    let state = state_with_act(DOOM_ACT);
    assert!(state.agenda_deck.is_empty());
    let session = run(state);
    assert_eq!(session.state().agenda_doom, 0, "no agenda to place doom on");
}

/// `PLACE_ACT` current, with `HERE` and `DEST` on the board and investigator
/// 1 at `at`.
fn placement_state(at: Option<LocationId>) -> GameState {
    let seated = match at {
        Some(location) => seat(1).at(location),
        None => seat(1),
    };
    with_act(
        lead(seated)
            .with_location(test_support::test_location(HERE.0, "Origin"))
            .with_location(test_support::test_location(DEST.0, "Destination")),
        PLACE_ACT,
    )
}

#[test]
fn place_investigator_moves_and_announces_a_change_of_location() {
    let session = run(placement_state(Some(HERE)));
    assert_eq!(
        session.state().investigators[&INV].current_location,
        Some(DEST)
    );
    assert_event!(
        session.events(),
        Event::InvestigatorMoved {
            investigator: INV,
            from: HERE,
            to: DEST,
        }
    );
}

#[test]
fn place_investigator_already_there_announces_nothing() {
    let session = run(placement_state(Some(DEST)));
    assert_eq!(
        session.state().investigators[&INV].current_location,
        Some(DEST)
    );
    assert_no_event!(session.events(), Event::InvestigatorMoved { .. });
}

#[test]
fn place_investigator_from_no_location_places_without_announcing() {
    let session = run(placement_state(None));
    assert_eq!(
        session.state().investigators[&INV].current_location,
        Some(DEST)
    );
    assert_no_event!(session.events(), Event::InvestigatorMoved { .. });
}

/// `CLUES_ACT` current, with `CLUE_CARD` holding `clues` clues, attached to a
/// location rather than in the controller's areas, since the exit finds the
/// instance anywhere on the board.
fn clue_card_state(clues: u8) -> GameState {
    let mut card = CardInPlay::enter_play(
        CardCode::new("TEST-CLUE-CARD"),
        CLUE_CARD,
        Owner::EncounterDeck,
    );
    card.clues = clues;
    let mut location = test_support::test_location(HERE.0, "Elsewhere");
    location.attachments.push(card);
    with_act(lead(seat(1)).with_location(location), CLUES_ACT)
}

fn clues_on_clue_card(state: &GameState) -> u8 {
    board::find_instance(state, CLUE_CARD)
        .expect("the clue card stays on the board")
        .0
        .clues
}

#[test]
fn remove_clues_removes_that_many_clues_from_the_instance() {
    let session = run(clue_card_state(5));
    assert_eq!(clues_on_clue_card(session.state()), 2);
}

#[test]
fn remove_clues_saturates_at_zero() {
    let session = run(clue_card_state(2));
    assert_eq!(clues_on_clue_card(session.state()), 0);
}

#[test]
fn take_encounter_top_and_discard_encounter_card_move_the_top_card_to_the_discard() {
    let mut state = state_with_act(DIG_ACT);
    // No builder setter stages the encounter deck or discard.
    state.encounter_deck = ["TOP", "NEXT"].map(CardCode::new).into();
    state.encounter_discard = vec![CardCode::new("OLD")];
    let session = run(state);
    // The next round's encounter draw has not been taken yet.
    assert_eq!(
        Vec::from(session.state().encounter_deck.clone()),
        [CardCode::new("NEXT")]
    );
    assert_eq!(
        session.state().encounter_discard,
        ["OLD", "TOP"].map(CardCode::new)
    );
    assert_event!(
        session.events(),
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
    // No builder setter stages the encounter discard.
    state.encounter_discard = vec![CardCode::new("OLD")];
    let session = run(state);
    assert!(session.state().encounter_deck.is_empty());
    assert_eq!(session.state().encounter_discard, [CardCode::new("OLD")]);
    assert_no_event!(session.events(), Event::EncounterDeckShuffled);
    assert_no_event!(session.events(), Event::CardDiscarded { .. });
}

/// Whether a Combat test investigator 1 starts from the turn menu `session`
/// rests at offers the "use Intellect in place of Combat?" decision rather
/// than going straight to the commit window.
fn combat_test_offers_substitution(session: TestSession) -> bool {
    session.assert_at_turn_menu(INV);
    let started = session.test_skill(INV, SkillKind::Combat, 1);
    let prompt = started.prompt();
    prompt.nature == PromptNature::Decision
        && prompt
            .options
            .iter()
            .map(|o| o.label.as_str())
            .eq(["Use Intellect", "Keep Combat"])
}

/// The substitution is round-scoped, so the real enemy phase end, which runs
/// on into the round end, would clear it before any later test could see it.
/// The `PhaseEnded { Enemy }` timing point is fired on its own over the lead's
/// open turn instead, and the session comes back to that turn's menu.
#[test]
fn add_skill_substitution_is_offered_at_the_next_skill_test() {
    let board = || {
        let mut state = state_with_act(SUBSTITUTE_ACT);
        state.chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)]);
        TestSession::new(state)
    };
    assert!(
        !combat_test_offers_substitution(board()),
        "control: no substitution before the native runs"
    );
    let fired = board().fire_at(TimingEvent::PhaseEnded {
        phase: state::Phase::Enemy,
    });
    assert!(combat_test_offers_substitution(fired));
}
