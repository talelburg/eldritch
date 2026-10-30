//! `NativeCx::move_horror`, the exit a native moves horror from one card to
//! another through.
//!
//! A move is neither a deal nor a take. The Necronomicon 01009's ruling
//! (<https://arkhamdb.com/card/01009>): *"Moving damage or horror is different
//! from 'dealing' or 'taking' damage or horror, so does not trigger effects"*.
//! The official FAQ bounds where it may go: *"An effect that can “move”
//! damage/horror from one card to another—such as Wrong Place, Right Time
//! ([fhvp] 115)—must move that damage/horror to a card that has a health or
//! sanity value, respectively."*
//!
//! Synthetic probes only (ADR 0016): each models the primitive, and none
//! stands in for a printed card. Its own test binary, so its `MockRegistry`
//! gets a process to itself.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use card_dsl::dsl::{
    self, EventPattern, EventTiming, HarmKind, HarmPlacedScope, InvestigatorTarget,
};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, MoveHorrorError, NativeCx, OptionTarget, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInstanceId, DiscardPile, GameState,
    GameStateBuilder, InvestigatorId, LocationId, Zone,
};
use game_core::test_support::{
    self, horror, seat, test_investigator_card, Either, MockRegistry, Seat, SeatLookup as _,
    TestSession,
};
use game_core::{assert_event, assert_no_event};

/// An investigator (8 sanity) with a reaction to horror placed on them, the
/// shape Agnes Baker 01004's prints: the harm-keyed trigger a move must not
/// reach.
const WATCHER: &str = "_mh_harm_watcher";
/// An asset with no sanity value, whose `[action]` moves 1 horror from itself
/// to its controller's investigator card.
const VESSEL: &str = "_mh_vessel";
/// An asset with sanity 2.
const WARD: &str = "_mh_ward";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

fn asset(code: &str, sanity: Option<u8>) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: code.to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: None,
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            slots: vec![],
            health: None,
            sanity,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 1,
            uses: None,
            play_only_during_turn: false,
        },
    }
}

/// Move 1 horror from the ability's source to its controller.
fn move_to_controller(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let source = ctx.source_instance().expect("an in-play source");
    let to = cx.state().investigators[&ctx.controller]
        .investigator_card
        .instance_id;
    match cx.move_horror(source, to, 1) {
        Ok(()) => EngineOutcome::Done,
        Err(err) => EngineOutcome::Rejected {
            reason: format!("{err:?}").into(),
        },
    }
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(test_investigator_card(WATCHER))
            .with_abilities(WATCHER, || {
                vec![dsl::reaction_on_event(
                    EventPattern::HarmPlaced {
                        kind: HarmKind::Horror,
                        scope: HarmPlacedScope::YourInvestigator,
                    },
                    EventTiming::After,
                    dsl::gain_resources(InvestigatorTarget::You, 1),
                )]
            })
            .with_card(asset(VESSEL, None))
            .with_abilities(VESSEL, || {
                vec![dsl::activated(1, vec![], dsl::native("test:move-horror"))]
            })
            .with_native_effect("test:move-horror", move_to_controller)
            .with_card(asset(WARD, Some(2)))
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// `seated` on their open turn, alone at `HERE`.
fn board(seated: impl Into<Seat<Either>>) -> GameState {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_seat(seated)
        .open_turn(INV)
        .build()
}

fn investigator_card(state: &GameState) -> CardInstanceId {
    state.investigators[&INV].investigator_card.instance_id
}

/// Run `move_horror(from, to, amount)` on `state` outside any ability.
fn move_horror(
    state: &mut GameState,
    from: CardInstanceId,
    to: CardInstanceId,
    amount: u8,
) -> (Result<(), MoveHorrorError>, Vec<Event>) {
    let mut result = Ok(());
    let (outcome, events) = test_support::run_native(
        state,
        registries().cards,
        |cx, _| {
            result = cx.move_horror(from, to, amount);
            EngineOutcome::Done
        },
        &EvalContext::for_controller(INV),
    );
    assert!(matches!(outcome, EngineOutcome::Done));
    (result, events)
}

/// The FAQ: horror moves only *"to a card that has a health or sanity
/// value"*. A destination without one refuses the move, and nothing changes.
#[test]
fn moving_horror_to_a_card_with_no_sanity_is_refused() {
    let mut state = board(seat(1).at(HERE).harm(0, 2).in_play([VESSEL]));
    let before = state.clone();
    let from = investigator_card(&state);
    let to = state.in_play(INV, VESSEL);

    let (result, events) = move_horror(&mut state, from, to, 1);

    assert_eq!(result, Err(MoveHorrorError::NoSanity));
    assert!(events.is_empty(), "nothing moved: {events:?}");
    assert_eq!(state, before);
}

/// The watcher's reaction is wired: horror dealt to them opens its window.
#[test]
fn horror_dealt_to_the_watcher_offers_its_reaction() {
    let state = board(seat(1).as_card(WATCHER).at(HERE));
    let watcher = OptionTarget::CardInstance(investigator_card(&state));

    let session = TestSession::new(state, registries()).take_horror(INV, 1);

    assert_eq!(session.offered_at(&watcher), 1);
}

/// The same horror moved onto the watcher opens no window: no harm event, no
/// reaction offered, and play goes straight back to the turn menu.
#[test]
fn horror_moved_to_the_watcher_offers_no_reaction() {
    let state = board(
        seat(1)
            .as_card(WATCHER)
            .at(HERE)
            .in_play([(VESSEL, horror(2))]),
    );
    let vessel = state.in_play(INV, VESSEL);
    let watcher = investigator_card(&state);

    let session = TestSession::new(state, registries()).take(&TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(vessel),
        address: AbilityAddress::Printed(0),
    });

    assert_eq!(session.rejection(), None);
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::HorrorMoved { from, to, amount: 1 } if *from == vessel && *to == watcher
    );
    assert_no_event!(session.events(), Event::HorrorPlacedOnInvestigator { .. });
    assert_eq!(session.state().investigators[&INV].horror(), 1);
}

/// `glossary/Sanity_and_Horror.md`: *"If an asset with a sanity value has
/// horror on it equal to or greater than its sanity, it is defeated and placed
/// on its owner's discard pile."* Horror moved onto it counts.
#[test]
fn an_asset_the_moved_horror_reaches_is_defeated() {
    let mut state = board(seat(1).at(HERE).harm(0, 1).in_play([(WARD, horror(1))]));
    let from = investigator_card(&state);
    let to = state.in_play(INV, WARD);

    let (result, events) = move_horror(&mut state, from, to, 1);

    assert_eq!(result, Ok(()));
    assert_event!(events, Event::HorrorMoved { amount: 1, .. });
    assert_event!(
        events,
        Event::CardDiscarded { code, from: Zone::InPlay, to: DiscardPile::Investigator(INV) }
            if code.as_str() == WARD
    );
    let inv = &state.investigators[&INV];
    assert_eq!(inv.horror(), 0);
    assert_eq!(inv.discard, vec![CardCode::new(WARD)]);
}
