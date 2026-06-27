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
};
use game_core::{apply, Action, EngineOutcome, InputResponse, PlayerAction, TurnAction};

const COVER_UP: &str = "01007";
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
