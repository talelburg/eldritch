//! A hand-discard cost — *"choose and discard 1 card from your hand"* — on a
//! triggered ability and on a `[fast]` activated one.
//!
//! The cost is payable only with a card in hand that is not a weakness. Dark
//! Memory 01013's ruling: *"You cannot choose to discard a Weakness card from
//! your hand, neither for exceeding the max number of cards in hand, nor for
//! card effects that make you 'choose and discard'."*
//! (<https://arkhamdb.com/card/01013>). It is paid through a choice of hand
//! card before the effect resolves, and stays paid when the effect is cancelled
//! (`glossary/Cancel.md`: *"Any time the effects of an ability are canceled,
//! the ability (apart from its effects) is still regarded as initiated, and any
//! costs have still been paid."*).
//!
//! Synthetic probes only (ADR 0016). Each `_hd_*` code models one primitive —
//! a reaction or an activated ability carrying the cost, a cancelling
//! interrupt, a weakness and two plain hand cards — and none stands in for a
//! printed card.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, WeaknessKind};
use card_dsl::dsl::{
    self, Cost, Effect, EventPattern, EventTiming, InvestigatorTarget, LocationTarget,
};
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx, OptionTarget, Registries};
use game_core::event::{Event, TraumaKind};
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, DiscardPile, GameStateBuilder, InvestigatorId,
    LocationId, Zone,
};
use game_core::test_support::{self, seat, MockRegistry, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

/// `[reaction] When you would discover clues at your location, choose and
/// discard 1 card from your hand: mark that many.` The mark names the bound
/// discovery count, so it shows the binding survived the cost's choice.
const REACTION: &str = "_hd_reaction";
/// `[fast] Discover 2 clues at your location.` Drives a real discovery for
/// [`REACTION`] to hear.
const DISCOVER_TWO: &str = "_hd_discover_two";
/// `[fast] Choose and discard 1 card from your hand: Gain 1 resource.`
const FAST_GAIN: &str = "_hd_fast_gain";
/// `[fast] Choose and discard 1 card from your hand: Discover 1 clue at your
/// location.` An effect [`CANCEL_DISCOVERY`] can cancel.
const FAST_DISCOVERY: &str = "_hd_fast_discovery";
/// `[action] Choose and discard 1 card from your hand: Gain 1 resource.`
const ACTION_GAIN: &str = "_hd_action_gain";
/// `[reaction] When you would discover clues: Cancel that discovery.`
const CANCEL_DISCOVERY: &str = "_hd_cancel_discovery";
/// A weakness, which the cost may not choose.
const WEAKNESS: &str = "_hd_weakness";
/// Plain hand cards the registry knows nothing about: not weaknesses.
const CARD_A: &str = "_hd_card_a";
const CARD_B: &str = "_hd_card_b";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// Marks the bound clue-discovery count as one `TraumaSuffered`, an exit that
/// only announces; 0 when nothing was bound.
fn mark_count(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let amount = ctx.clue_discovery_count().unwrap_or(0);
    cx.suffer_trauma(ctx.controller, TraumaKind::Physical, amount);
    EngineOutcome::Done
}

fn weakness_metadata() -> CardMetadata {
    CardMetadata {
        code: WEAKNESS.to_owned(),
        name: "Mock weakness".to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: Some(WeaknessKind::Other),
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
            .with_abilities(REACTION, || {
                vec![dsl::reaction_on_event(
                    EventPattern::DiscoverClues,
                    EventTiming::When,
                    dsl::native("_hd:mark_count"),
                )
                .with_costs(vec![Cost::DiscardCardFromHand])]
            })
            .with_native_effect("_hd:mark_count", mark_count)
            .with_abilities(DISCOVER_TWO, || {
                vec![dsl::activated(
                    0,
                    vec![],
                    dsl::discover_clue(LocationTarget::YourLocation, 2),
                )]
            })
            .with_abilities(FAST_GAIN, || {
                vec![dsl::activated(
                    0,
                    vec![Cost::DiscardCardFromHand],
                    dsl::gain_resources(InvestigatorTarget::You, 1),
                )]
            })
            .with_abilities(FAST_DISCOVERY, || {
                vec![dsl::activated(
                    0,
                    vec![Cost::DiscardCardFromHand],
                    dsl::discover_clue(LocationTarget::YourLocation, 1),
                )]
            })
            .with_abilities(ACTION_GAIN, || {
                vec![dsl::activated(
                    1,
                    vec![Cost::DiscardCardFromHand],
                    dsl::gain_resources(InvestigatorTarget::You, 1),
                )]
            })
            .with_abilities(CANCEL_DISCOVERY, || {
                vec![dsl::reaction_on_event(
                    EventPattern::DiscoverClues,
                    EventTiming::When,
                    Effect::Cancel,
                )]
            })
            .with_card(weakness_metadata())
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// `INV`'s open turn at `HERE` (3 clues), with `in_play` in play and `hand` in
/// hand.
fn session(in_play: &[&str], hand: &[&str]) -> TestSession {
    let mut location = test_support::test_location(HERE.0, "Study");
    location.clues = 3;
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(INV.0)
                .at(HERE)
                .in_play(in_play.iter().copied())
                .hand(hand.iter().copied()),
        )
        .with_location(location)
        .open_turn(INV)
        .session()
}

fn activate(session: &TestSession, code: &str) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(session.state().in_play(INV, code)),
        address: AbilityAddress::Printed(0),
    }
}

fn hand_card(hand_index: u8) -> OptionTarget {
    OptionTarget::HandCard {
        investigator: INV,
        hand_index,
    }
}

fn hand(session: &TestSession) -> Vec<CardCode> {
    session.state().investigators[&INV].hand.clone()
}

// ---- triggered abilities ---------------------------------------------------

#[test]
fn paying_the_cost_discards_the_chosen_card_then_resolves_the_effect_with_its_binding() {
    let session = session(&[REACTION, DISCOVER_TWO], &[CARD_A, WEAKNESS, CARD_B]);
    let action = activate(&session, DISCOVER_TWO);
    let reaction = session.state().card(INV, REACTION);

    let at_cost = session.take(&action).pick(reaction);
    // The choice offers the non-weakness cards only.
    assert_eq!(at_cost.offered_at(&hand_card(0)), 1);
    assert_eq!(
        at_cost.offered_at(&hand_card(1)),
        0,
        "a weakness can't be chosen"
    );
    assert_eq!(at_cost.offered_at(&hand_card(2)), 1);
    assert_no_event!(at_cost.events(), Event::TraumaSuffered { .. });

    let paid = at_cost.pick(hand_card(2));
    assert_event_sequence!(
        paid.events(),
        Event::CardDiscarded {
            code,
            from: Zone::Hand,
            to: DiscardPile::Investigator(INV),
        } if code.as_str() == CARD_B,
        // The mark is the bound "that many": 2 would-be discovered clues.
        Event::TraumaSuffered {
            investigator: INV,
            kind: TraumaKind::Physical,
            amount: 2,
        },
    );
    assert_eq!(
        hand(&paid),
        [CardCode::new(CARD_A), CardCode::new(WEAKNESS)]
    );
    assert_eq!(
        paid.state().investigators[&INV].discard,
        [CardCode::new(CARD_B)]
    );
    paid.assert_at_turn_menu(INV);
}

/// The discovery `DISCOVER_TWO` drives, taken with `hand` in hand: the reaction
/// is not offered, so the discovery resolves straight back to the turn menu.
fn reaction_is_not_offered_holding(hand: &[&str]) {
    let session = session(&[REACTION, DISCOVER_TWO], hand);
    let action = activate(&session, DISCOVER_TWO);

    let after = session.take(&action);

    after.assert_at_turn_menu(INV);
    assert_eq!(
        after.state().investigators[&INV].clues,
        2,
        "the discovery resolved"
    );
    assert_no_event!(after.events(), Event::TraumaSuffered { .. });
    assert_no_event!(after.events(), Event::CardDiscarded { .. });
}

#[test]
fn the_reaction_is_not_offered_when_the_hand_holds_only_weaknesses() {
    reaction_is_not_offered_holding(&[WEAKNESS]);
}

#[test]
fn the_reaction_is_not_offered_with_an_empty_hand() {
    reaction_is_not_offered_holding(&[]);
}

/// The resolve convention every choice among board entities follows: a lone
/// candidate is taken with no prompt.
#[test]
fn a_lone_non_weakness_card_is_discarded_with_no_prompt() {
    let session = session(&[REACTION, DISCOVER_TWO], &[WEAKNESS, CARD_A]);
    let action = activate(&session, DISCOVER_TWO);
    let reaction = session.state().card(INV, REACTION);

    let paid = session.take(&action).pick(reaction);

    assert_event_sequence!(
        paid.events(),
        Event::CardDiscarded { code, from: Zone::Hand, .. } if code.as_str() == CARD_A,
        Event::TraumaSuffered { amount: 2, .. },
    );
    assert_eq!(hand(&paid), [CardCode::new(WEAKNESS)]);
    paid.assert_at_turn_menu(INV);
}

// ---- activated abilities ---------------------------------------------------

#[test]
fn a_fast_ability_with_the_cost_discards_the_chosen_card_then_resolves() {
    let session = session(&[FAST_GAIN], &[CARD_A, CARD_B]);
    let action = activate(&session, FAST_GAIN);
    let resources = session.state().investigators[&INV].resources;

    let at_cost = session.take(&action);
    assert_eq!(at_cost.offered_at(&hand_card(0)), 1);
    assert_eq!(at_cost.offered_at(&hand_card(1)), 1);
    assert_no_event!(at_cost.events(), Event::ResourcesGained { .. });

    let paid = at_cost.pick(hand_card(0));
    assert_event_sequence!(
        paid.events(),
        Event::AbilityActivated { .. },
        Event::CardDiscarded { code, from: Zone::Hand, .. } if code.as_str() == CARD_A,
        Event::ResourcesGained { investigator: INV, amount: 1 },
    );
    assert_eq!(hand(&paid), [CardCode::new(CARD_B)]);
    assert_eq!(paid.state().investigators[&INV].resources, resources + 1);
    paid.assert_at_turn_menu(INV);
}

#[test]
fn a_fast_ability_with_the_cost_is_not_offered_without_a_non_weakness_card() {
    for hand in [&[WEAKNESS][..], &[]] {
        let session = session(&[FAST_GAIN], hand);
        let action = activate(&session, FAST_GAIN);
        assert!(
            !enumerate::legal_actions(session.state(), registries().cards).contains(&action),
            "not offered holding {hand:?}",
        );
        let reason = session.assert_rejects(|s| s.submit_off_menu(&action));
        assert!(reason.contains("not a weakness"), "{reason}");
    }
}

/// `glossary/Cancel.md`: *"Any time the effects of an ability are canceled,
/// the ability (apart from its effects) is still regarded as initiated, and any
/// costs have still been paid."* The discovery the ability makes is cancelled
/// by an interrupt; the card discarded for it stays discarded.
#[test]
fn the_discard_stays_made_when_the_effect_is_cancelled() {
    let session = session(&[FAST_DISCOVERY, CANCEL_DISCOVERY], &[CARD_A, CARD_B]);
    let action = activate(&session, FAST_DISCOVERY);
    let interrupt = session.state().card(INV, CANCEL_DISCOVERY);

    let cancelled = session.take(&action).pick(hand_card(1)).pick(interrupt);

    assert_eq!(
        cancelled.state().investigators[&INV].clues,
        0,
        "the discovery was cancelled"
    );
    assert_eq!(cancelled.state().locations[&HERE].clues, 3);
    assert_event!(
        cancelled.events(),
        Event::CardDiscarded { code, from: Zone::Hand, .. } if code.as_str() == CARD_B
    );
    assert_eq!(hand(&cancelled), [CardCode::new(CARD_A)]);
    assert_eq!(
        cancelled.state().investigators[&INV].discard,
        [CardCode::new(CARD_B)]
    );
}

/// An `[action]` ability's costs are paid inside the take-an-action step, which
/// has no place for the choice before the attacks of opportunity.
/// TODO(#1031): pay an `[action]` ability's costs before its attacks of
/// opportunity.
#[test]
fn an_action_ability_with_the_cost_is_refused_with_a_todo() {
    let session = session(&[ACTION_GAIN], &[CARD_A]);
    let action = activate(&session, ACTION_GAIN);
    assert!(!enumerate::legal_actions(session.state(), registries().cards).contains(&action));

    let reason = session.assert_rejects(|s| s.submit_off_menu(&action));

    assert!(reason.contains("TODO(#1031)"), "{reason}");
}
