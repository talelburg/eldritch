//! The gate seam: one test per cell of the check × kind table, on synthetic
//! fixtures that each model one primitive (ADR 0016) — an ability with a usage
//! limit, an ability with an eligibility tag, an ability whose effect provably
//! cannot change the game state, a card in hand with a resource cost, a
//! constant "cannot play events". None of them stands in for a printed card.

use std::sync::OnceLock;

use card_dsl::card_data::{CardKind, CardMetadata, CardType, Class, SkillIcons};
use card_dsl::dsl::{
    self, Ability, ActionDesignator, Cost, Effect, EventPattern, EventTiming, InvestigatorTarget,
    LocationTarget, Restriction, UsageLimit, UsagePeriod,
};

use super::*;
use crate::card_registry::EligibilityFn;
use crate::engine::evaluator::EvalContext;
use crate::state::{
    AbilityAddress, AbilitySource, CandidateSource, CardCode, CardInPlay, CardInstanceId,
    GameStateBuilder, InvestigatorId, LocationId,
};
use crate::test_support;

/// The card carrying every ability primitive under test, one per printed index.
const PRIMITIVES: &str = "_gate_primitives";
/// A card in hand with a printed resource cost and one Fast-event-shaped
/// `[reaction]` ability.
const COSTED_EVENT: &str = "_gate_costed_event";
/// A constant "cannot play events".
const EVENT_BAN: &str = "_gate_event_ban";

const OPEN_TAG: &str = "_gate:open";
const SHUT_TAG: &str = "_gate:shut";
const UNREGISTERED_TAG: &str = "_gate:unregistered";

const CONTROLLER: InvestigatorId = InvestigatorId(1);
const LOCATION: LocationId = LocationId(10);
const INSTANCE: CardInstanceId = CardInstanceId(1);
/// The event's printed play cost, and so the resources it needs.
const EVENT_COST: i8 = 3;

/// An effect that can always change the game state.
fn live() -> Effect {
    dsl::gain_resources(InvestigatorTarget::You, 1)
}

/// An effect that provably cannot: discovering a clue at a location with none.
fn inert() -> Effect {
    dsl::discover_clue(LocationTarget::YourLocation, 1)
}

fn reaction(effect: Effect) -> Ability {
    dsl::reaction_on_event(EventPattern::RoundEnded, EventTiming::When, effect)
}

fn forced(effect: Effect) -> Ability {
    dsl::forced_on_event(EventPattern::RoundEnded, EventTiming::When, effect)
}

fn exhaust_cost(mut ability: Ability) -> Ability {
    ability.costs = vec![Cost::Exhaust];
    ability
}

fn once_per_round() -> UsageLimit {
    UsageLimit {
        count: 1,
        period: UsagePeriod::Round,
    }
}

// Printed indices on `PRIMITIVES`.
const REACTION_LIVE: u8 = 0;
const REACTION_INERT: u8 = 1;
const REACTION_OPEN_TAG: u8 = 2;
const REACTION_SHUT_TAG: u8 = 3;
const REACTION_UNREGISTERED_TAG: u8 = 4;
const REACTION_LIMITED: u8 = 5;
const REACTION_EXHAUST_COST: u8 = 6;
const FORCED_LIVE: u8 = 7;
const FORCED_INERT: u8 = 8;
const FORCED_SHUT_TAG: u8 = 9;
const FORCED_LIMITED: u8 = 10;
const FORCED_EXHAUST_COST: u8 = 11;
const ACTIVATED_LIVE: u8 = 12;
const ACTIVATED_INERT: u8 = 13;
const ACTIVATED_SHUT_TAG: u8 = 14;
const ACTIVATED_LIMITED: u8 = 15;
const ACTIVATED_EXHAUST_COST: u8 = 16;
const ACTIVATED_DESIGNATED_INERT: u8 = 17;
const ACTIVATED_PARLEY_INERT: u8 = 18;
/// One past the last printed index: an address nothing resolves to.
const NOT_PRINTED: u8 = 19;

fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        PRIMITIVES => Some(vec![
            reaction(live()),
            reaction(inert()),
            reaction(live()).with_eligibility(OPEN_TAG),
            reaction(live()).with_eligibility(SHUT_TAG),
            reaction(live()).with_eligibility(UNREGISTERED_TAG),
            reaction(live()).with_usage_limit(once_per_round()),
            exhaust_cost(reaction(live())),
            forced(live()),
            forced(inert()),
            forced(live()).with_eligibility(SHUT_TAG),
            forced(live()).with_usage_limit(once_per_round()),
            exhaust_cost(forced(live())),
            dsl::activated(0, vec![], live()),
            dsl::activated(0, vec![], inert()),
            dsl::activated(0, vec![], live()).with_eligibility(SHUT_TAG),
            dsl::activated(0, vec![], live()).with_usage_limit(once_per_round()),
            dsl::activated(0, vec![Cost::Exhaust], live()),
            dsl::activated_as(ActionDesignator::Evade, 1, vec![], inert()),
            dsl::activated_as(ActionDesignator::Parley, 1, vec![], inert()),
        ]),
        COSTED_EVENT => Some(vec![reaction(live())]),
        EVENT_BAN => Some(vec![dsl::constant(dsl::restrict(Restriction::CannotPlay(
            CardType::Event,
        )))]),
        _ => None,
    }
}

fn costed_event_metadata() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(|| CardMetadata {
        code: COSTED_EVENT.to_owned(),
        name: "Gate Costed Event".to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_test".to_owned(),
        weakness: false,
        kind: CardKind::Event {
            class: Class::Neutral,
            cost: Some(EVENT_COST),
            xp: Some(0),
            skill_icons: SkillIcons::default(),
            is_fast: true,
            deck_limit: 2,
            play_only_during_turn: false,
        },
    })
}

fn metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    (code.as_str() == COSTED_EVENT).then(costed_event_metadata)
}

fn always(_: &GameState, _: &EvalContext) -> bool {
    true
}

fn never(_: &GameState, _: &EvalContext) -> bool {
    false
}

fn eligibility_for(tag: &str) -> Option<EligibilityFn> {
    match tag {
        OPEN_TAG => Some(always),
        SHUT_TAG => Some(never),
        _ => None,
    }
}

fn registry() -> CardRegistry {
    CardRegistry {
        metadata_for,
        abilities_for,
        native_eligibility_for: eligibility_for,
        ..CardRegistry::EMPTY
    }
}

/// The controller at a clueless location, with `PRIMITIVES` in play and
/// `COSTED_EVENT` in hand.
fn state() -> GameState {
    let mut inv = test_support::test_investigator(CONTROLLER.0);
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(PRIMITIVES), INSTANCE));
    inv.hand.push(CardCode::new(COSTED_EVENT));
    GameStateBuilder::new()
        .with_investigator_at(inv, LOCATION)
        .with_location(test_support::test_location(LOCATION.0, "Clueless"))
        .with_turn_order([CONTROLLER])
        .with_round(1)
        .build()
}

fn in_play(index: u8) -> ResolutionCandidate {
    ResolutionCandidate::new(
        CardCode::new(PRIMITIVES),
        CONTROLLER,
        AbilityAddress::Printed(index),
        CandidateSource::Ability(AbilitySource::InPlay(INSTANCE)),
    )
}

fn from_hand() -> ResolutionCandidate {
    ResolutionCandidate::new(
        CardCode::new(COSTED_EVENT),
        CONTROLLER,
        AbilityAddress::Printed(0),
        CandidateSource::Hand,
    )
}

fn gate(
    state: &GameState,
    candidate: &ResolutionCandidate,
    kind: InitiationKind,
) -> Result<(), Refusal> {
    check_with(state, &registry(), candidate, kind)
}

// ---- potential to change the game state ---------------------------------

#[test]
fn forced_ability_that_cannot_change_state_does_not_initiate() {
    let state = state();
    assert_eq!(
        gate(&state, &in_play(FORCED_INERT), InitiationKind::Forced),
        Err(Refusal::NoStateChange)
    );
    assert_eq!(
        gate(&state, &in_play(FORCED_LIVE), InitiationKind::Forced),
        Ok(())
    );
}

#[test]
fn reaction_that_cannot_change_state_is_refused() {
    let state = state();
    assert_eq!(
        gate(&state, &in_play(REACTION_INERT), InitiationKind::Reaction),
        Err(Refusal::NoStateChange)
    );
    assert_eq!(
        gate(&state, &in_play(REACTION_LIVE), InitiationKind::Reaction),
        Ok(())
    );
}

#[test]
fn activated_ability_that_cannot_change_state_is_refused() {
    let state = state();
    assert_eq!(
        gate(&state, &in_play(ACTIVATED_INERT), InitiationKind::Activated),
        Err(Refusal::NoStateChange)
    );
    assert_eq!(
        gate(&state, &in_play(ACTIVATED_LIVE), InitiationKind::Activated),
        Ok(())
    );
}

/// A designated ability's substance is the action it performs, not its residual
/// effect (#805), so an inert residual beside a bold word is not a no-op — the
/// activation path asks the action's own question (`designator::can_perform`).
/// **Parley** performs nothing, so its residual is all there is to ask about.
#[test]
fn a_designated_activated_ability_is_judged_by_its_action_not_its_residual() {
    let state = state();
    assert_eq!(
        gate(
            &state,
            &in_play(ACTIVATED_DESIGNATED_INERT),
            InitiationKind::Activated
        ),
        Ok(())
    );
    assert_eq!(
        gate(
            &state,
            &in_play(ACTIVATED_PARLEY_INERT),
            InitiationKind::Activated
        ),
        Err(Refusal::NoStateChange)
    );
}

// ---- side in effect -------------------------------------------------------

#[test]
fn an_address_nothing_resolves_to_is_refused_on_every_kind() {
    let state = state();
    for kind in [
        InitiationKind::Forced,
        InitiationKind::Reaction,
        InitiationKind::Activated,
    ] {
        assert_eq!(
            gate(&state, &in_play(NOT_PRINTED), kind),
            Err(Refusal::SideNotInEffect),
            "{kind:?}"
        );
    }
}

// ---- eligibility tag ------------------------------------------------------

#[test]
fn forced_ability_whose_eligibility_is_false_does_not_initiate() {
    let state = state();
    assert_eq!(
        gate(&state, &in_play(FORCED_SHUT_TAG), InitiationKind::Forced),
        Err(Refusal::NotEligible)
    );
}

#[test]
fn reaction_is_gated_by_its_eligibility_predicate() {
    let state = state();
    assert_eq!(
        gate(
            &state,
            &in_play(REACTION_OPEN_TAG),
            InitiationKind::Reaction
        ),
        Ok(())
    );
    assert_eq!(
        gate(
            &state,
            &in_play(REACTION_SHUT_TAG),
            InitiationKind::Reaction
        ),
        Err(Refusal::NotEligible)
    );
}

/// A tag with no registered predicate is suppressed, so a half-installed host
/// never surfaces a gated ability it can't evaluate.
#[test]
fn reaction_whose_eligibility_tag_is_unregistered_is_refused() {
    let state = state();
    assert_eq!(
        gate(
            &state,
            &in_play(REACTION_UNREGISTERED_TAG),
            InitiationKind::Reaction
        ),
        Err(Refusal::NotEligible),
    );
}

#[test]
fn activated_ability_whose_eligibility_is_false_is_refused() {
    let state = state();
    assert_eq!(
        gate(
            &state,
            &in_play(ACTIVATED_SHUT_TAG),
            InitiationKind::Activated
        ),
        Err(Refusal::NotEligible)
    );
}

// ---- usage limit, and recording a use -------------------------------------

/// Record one initiation of the limited ability at `index`, as the firing path
/// does at Appendix I step 3.
fn record(state: &mut GameState, index: u8) {
    record_initiation(state, &in_play(index), Some(once_per_round()));
}

#[test]
fn reaction_at_its_limit_is_refused_until_the_next_round() {
    let mut state = state();
    assert_eq!(
        gate(&state, &in_play(REACTION_LIMITED), InitiationKind::Reaction),
        Ok(())
    );
    record(&mut state, REACTION_LIMITED);
    assert_eq!(
        gate(&state, &in_play(REACTION_LIMITED), InitiationKind::Reaction),
        Err(Refusal::UsageLimitReached),
    );
    state.round += 1;
    assert_eq!(
        gate(&state, &in_play(REACTION_LIMITED), InitiationKind::Reaction),
        Ok(())
    );
}

#[test]
fn activated_ability_at_its_limit_is_refused() {
    let mut state = state();
    record(&mut state, ACTIVATED_LIMITED);
    assert_eq!(
        gate(
            &state,
            &in_play(ACTIVATED_LIMITED),
            InitiationKind::Activated
        ),
        Err(Refusal::UsageLimitReached),
    );
}

/// The gate gives a forced ability only the change-state and eligibility
/// halves (`glossary/Ability.md`), so a recorded use does not hold it back.
#[test]
fn forced_ability_is_not_held_back_by_its_usage_limit() {
    let mut state = state();
    record(&mut state, FORCED_LIMITED);
    assert_eq!(
        gate(&state, &in_play(FORCED_LIMITED), InitiationKind::Forced),
        Ok(())
    );
}

/// Recording one ability's use leaves its siblings on the same card uncounted.
#[test]
fn a_recorded_use_counts_against_that_ability_alone() {
    let mut state = state();
    record(&mut state, ACTIVATED_LIMITED);
    assert_eq!(
        gate(&state, &in_play(REACTION_LIMITED), InitiationKind::Reaction),
        Ok(())
    );
}

/// An ability with no printed limit has nothing to count.
#[test]
fn recording_an_unlimited_ability_counts_nothing() {
    let mut state = state();
    record_initiation(&mut state, &in_play(REACTION_LIMITED), None);
    assert_eq!(
        gate(&state, &in_play(REACTION_LIMITED), InitiationKind::Reaction),
        Ok(())
    );
}

// ---- cost can be paid -----------------------------------------------------

fn exhaust_primitives(state: &mut GameState) {
    state
        .investigators
        .get_mut(&CONTROLLER)
        .expect("seated")
        .cards_in_play[0]
        .exhausted = true;
}

fn set_resources(state: &mut GameState, resources: u8) {
    state
        .investigators
        .get_mut(&CONTROLLER)
        .expect("seated")
        .resources = resources;
}

#[test]
fn reaction_whose_cost_cannot_be_paid_is_refused() {
    let mut state = state();
    assert_eq!(
        gate(
            &state,
            &in_play(REACTION_EXHAUST_COST),
            InitiationKind::Reaction
        ),
        Ok(())
    );
    exhaust_primitives(&mut state);
    assert!(matches!(
        gate(
            &state,
            &in_play(REACTION_EXHAUST_COST),
            InitiationKind::Reaction
        ),
        Err(Refusal::CostUnpayable(_)),
    ));
}

/// A forced ability is not paid for, so the gate skips the cost half for it.
#[test]
fn forced_ability_skips_the_cost_check() {
    let mut state = state();
    exhaust_primitives(&mut state);
    assert_eq!(
        gate(
            &state,
            &in_play(FORCED_EXHAUST_COST),
            InitiationKind::Forced
        ),
        Ok(())
    );
}

#[test]
fn activated_ability_whose_cost_cannot_be_paid_is_refused() {
    let mut state = state();
    exhaust_primitives(&mut state);
    assert!(matches!(
        gate(
            &state,
            &in_play(ACTIVATED_EXHAUST_COST),
            InitiationKind::Activated
        ),
        Err(Refusal::CostUnpayable(_)),
    ));
}

#[test]
fn play_needs_the_cards_resource_cost() {
    let mut state = state();
    set_resources(&mut state, u8::try_from(EVENT_COST - 1).expect("positive"));
    assert!(matches!(
        gate(&state, &from_hand(), InitiationKind::Play),
        Err(Refusal::CostUnpayable(_)),
    ));
    set_resources(&mut state, u8::try_from(EVENT_COST).expect("positive"));
    assert_eq!(gate(&state, &from_hand(), InitiationKind::Play), Ok(()));
}

// ---- play-ban -------------------------------------------------------------

fn ban_events(state: &mut GameState) {
    state
        .investigators
        .get_mut(&CONTROLLER)
        .expect("seated")
        .threat_area
        .push(CardInPlay::enter_play(
            CardCode::new(EVENT_BAN),
            CardInstanceId(2),
        ));
}

#[test]
fn play_is_refused_under_a_ban_on_its_card_type() {
    let mut state = state();
    ban_events(&mut state);
    assert_eq!(
        gate(&state, &from_hand(), InitiationKind::Play),
        Err(Refusal::PlayBanned {
            investigator: CONTROLLER,
            card_type: CardType::Event,
        }),
    );
}

/// A ban on *playing* events says nothing about an in-play card's reaction.
#[test]
fn reaction_is_not_held_back_by_a_play_ban() {
    let mut state = state();
    ban_events(&mut state);
    assert_eq!(
        gate(&state, &in_play(REACTION_LIVE), InitiationKind::Reaction),
        Ok(())
    );
}

// ---- investigator status --------------------------------------------------

fn eliminate_controller(state: &mut GameState) {
    state
        .investigators
        .get_mut(&CONTROLLER)
        .expect("seated")
        .status = Status::Defeated;
}

#[test]
fn an_eliminated_investigator_cannot_activate_or_play() {
    let mut state = state();
    eliminate_controller(&mut state);
    let refused = Err(Refusal::NotActive {
        investigator: CONTROLLER,
        status: Some(Status::Defeated),
    });
    assert_eq!(
        gate(&state, &in_play(ACTIVATED_LIVE), InitiationKind::Activated),
        refused
    );
    assert_eq!(gate(&state, &from_hand(), InitiationKind::Play), refused);
}

/// The forced arms decide status themselves: a game-end forced ability still
/// reaches an eliminated investigator (the elimination game-end collector).
#[test]
fn forced_ability_is_not_held_back_by_status() {
    let mut state = state();
    eliminate_controller(&mut state);
    assert_eq!(
        gate(&state, &in_play(FORCED_LIVE), InitiationKind::Forced),
        Ok(())
    );
}

/// Not yet enforced on reactions — `TODO(#959)` flips this cell to a refusal.
#[test]
fn reaction_is_not_yet_held_back_by_status() {
    let mut state = state();
    eliminate_controller(&mut state);
    assert_eq!(
        gate(&state, &in_play(REACTION_LIVE), InitiationKind::Reaction),
        Ok(())
    );
}

// ---- rendering at a handler boundary --------------------------------------

/// The play-ban keeps the wording the play validator has always rejected with.
#[test]
fn a_play_ban_renders_the_play_validators_wording() {
    let reason: Cow<'static, str> = Refusal::PlayBanned {
        investigator: CONTROLLER,
        card_type: CardType::Event,
    }
    .into();
    assert_eq!(
        reason,
        "PlayCard: InvestigatorId(1) cannot play a Event (a constant restriction forbids it)",
    );
}
