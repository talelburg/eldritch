//! The gate seam: one test per cell of the check × kind table, on synthetic
//! fixtures that each model one primitive (ADR 0016) — an ability with a usage
//! limit, an ability with an eligibility tag, an ability whose effect provably
//! cannot change the game state, a card in hand with a resource cost, a
//! constant "cannot play events". None of them stands in for a printed card.

use std::sync::OnceLock;

use card_dsl::card_data::{CardKind, Class, SkillIcons};
use card_dsl::dsl::{
    self, Cost, Effect, EventPattern, EventTiming, InvestigatorTarget, LocationTarget, Restriction,
    UsagePeriod,
};

use super::*;
use crate::card_registry::EligibilityFn;
use crate::state::{AbilityAddress, CardInPlay, CardInstanceId, GameStateBuilder, LocationId};
use crate::test_support;

/// The card carrying every ability primitive under test, one per printed index.
const PRIMITIVES: &str = "_gate_primitives";
/// A card in hand with a printed resource cost and one Fast-event-shaped
/// `[reaction]` ability.
const COSTED_EVENT: &str = "_gate_costed_event";
/// A constant "cannot play events".
const EVENT_BAN: &str = "_gate_event_ban";
/// Events played from hand, each resolving one `OnPlay` effect: one that can
/// change the game state (and costs `EVENT_COST`), and two free ones — one that
/// provably cannot, and one gated by an eligibility predicate that is false.
const PLAYED_LIVE: &str = "_gate_played_live";
const PLAYED_INERT: &str = "_gate_played_inert";
const PLAYED_SHUT_TAG: &str = "_gate_played_shut_tag";

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
        PLAYED_LIVE => Some(vec![dsl::on_play(live())]),
        PLAYED_INERT => Some(vec![dsl::on_play(inert())]),
        PLAYED_SHUT_TAG => Some(vec![dsl::on_play(live()).with_eligibility(SHUT_TAG)]),
        EVENT_BAN => Some(vec![dsl::constant(dsl::restrict(Restriction::CannotPlay(
            CardType::Event,
        )))]),
        _ => None,
    }
}

/// Event metadata for `code` at printed `cost`, built once into `cell`.
fn event_metadata(
    cell: &'static OnceLock<CardMetadata>,
    code: &str,
    cost: i8,
) -> &'static CardMetadata {
    cell.get_or_init(|| CardMetadata {
        code: code.to_owned(),
        name: code.to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_test".to_owned(),
        weakness: false,
        kind: CardKind::Event {
            class: Class::Neutral,
            cost: Some(cost),
            xp: Some(0),
            skill_icons: SkillIcons::default(),
            is_fast: true,
            deck_limit: 2,
            play_only_during_turn: false,
        },
    })
}

fn metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    static COSTED: OnceLock<CardMetadata> = OnceLock::new();
    static LIVE: OnceLock<CardMetadata> = OnceLock::new();
    static INERT: OnceLock<CardMetadata> = OnceLock::new();
    static SHUT: OnceLock<CardMetadata> = OnceLock::new();
    match code.as_str() {
        COSTED_EVENT => Some(event_metadata(&COSTED, COSTED_EVENT, EVENT_COST)),
        PLAYED_LIVE => Some(event_metadata(&LIVE, PLAYED_LIVE, EVENT_COST)),
        PLAYED_INERT => Some(event_metadata(&INERT, PLAYED_INERT, 0)),
        PLAYED_SHUT_TAG => Some(event_metadata(&SHUT, PLAYED_SHUT_TAG, 0)),
        _ => None,
    }
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

/// The gate asked as a Play of `code` from hand, with no ability address — the
/// play validator's question.
fn play(state: &GameState, code: &str) -> Result<(), Refusal> {
    check_play_with(state, &registry(), CONTROLLER, &CardCode::new(code))
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

/// A Fast event offered from hand is asked about the `[reaction]` ability it
/// plays at; its effect must be able to change the game state too.
#[test]
fn a_fast_event_whose_reaction_effect_cannot_change_state_is_not_played() {
    let state = state();
    let inert_reaction = ResolutionCandidate::new(
        CardCode::new(PRIMITIVES),
        CONTROLLER,
        AbilityAddress::Printed(REACTION_INERT),
        CandidateSource::Hand,
    );
    assert_eq!(
        gate(&state, &inert_reaction, InitiationKind::Play),
        Err(Refusal::NoStateChange)
    );
}

/// A card played from hand names no ability; its `OnPlay` effect is what must
/// be able to change the game state.
#[test]
fn an_event_whose_play_effect_cannot_change_state_is_not_played() {
    let state = state();
    assert_eq!(play(&state, PLAYED_INERT), Err(Refusal::NoStateChange));
    assert_eq!(play(&state, PLAYED_LIVE), Ok(()));
}

/// A card played from hand is gated by its `OnPlay` effect's eligibility tag.
#[test]
fn an_event_whose_play_effect_is_ineligible_is_not_played() {
    assert_eq!(play(&state(), PLAYED_SHUT_TAG), Err(Refusal::NotEligible));
}

// ---- side in effect -------------------------------------------------------

/// A code the registry does not know resolves to nothing to play.
#[test]
fn a_card_the_registry_does_not_know_is_not_played() {
    assert_eq!(
        play(&state(), "_gate_unknown"),
        Err(Refusal::SideNotInEffect)
    );
}

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

/// A limit counts every initiation, forced ones included
/// (`glossary/Limits_and_Maximums.md`), so a forced ability at its limit is
/// refused like any other.
#[test]
fn forced_ability_at_its_limit_is_refused() {
    let mut state = state();
    record(&mut state, FORCED_LIMITED);
    assert_eq!(
        gate(&state, &in_play(FORCED_LIMITED), InitiationKind::Forced),
        Err(Refusal::UsageLimitReached),
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

/// The play validator's form of the same cell: a card played from hand with no
/// ability address still needs its resource cost.
#[test]
fn play_from_the_turn_menu_needs_the_cards_resource_cost() {
    let mut state = state();
    set_resources(&mut state, u8::try_from(EVENT_COST - 1).expect("positive"));
    assert!(matches!(
        play(&state, PLAYED_LIVE),
        Err(Refusal::CostUnpayable(_))
    ));
    set_resources(&mut state, u8::try_from(EVENT_COST).expect("positive"));
    assert_eq!(play(&state, PLAYED_LIVE), Ok(()));
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
    assert_eq!(
        play(&state, PLAYED_LIVE),
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
    assert_eq!(play(&state, PLAYED_LIVE), refused);
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

#[test]
fn an_eliminated_investigator_cannot_react() {
    let mut state = state();
    eliminate_controller(&mut state);
    assert_eq!(
        gate(&state, &in_play(REACTION_LIVE), InitiationKind::Reaction),
        Err(Refusal::NotActive {
            investigator: CONTROLLER,
            status: Some(Status::Defeated),
        })
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

#[test]
fn payable_play_cost_reads_a_printed_number() {
    let code = CardCode::new("01018");
    assert_eq!(payable_play_cost(Some(0), &code), Ok(0));
    assert_eq!(payable_play_cost(Some(4), &code), Ok(4));
}

/// A `"–"` cost is not an unmodelled case: per the official FAQ, *"Cards
/// with a cost of '–' have no cost that can be paid, and therefore cannot
/// be played."* (`data/official-faq/Frequently_Asked_Questions.md`.) The
/// Necronomicon 01009 and every Dunwich permanent print it.
#[test]
fn payable_play_cost_rejects_a_dash_cost_permanently() {
    let err =
        payable_play_cost(None, &CardCode::new("01009")).expect_err("a \"–\" cost cannot be paid");
    assert!(
        err.contains("cannot be played"),
        "the reject should state the rule, not a deferral: {err}"
    );
    assert!(
        !err.contains("#501"),
        "a \"–\" cost is final behaviour, not deferred work: {err}"
    );
}

/// X ingests as `Some(-2)`, so without this arm `u8::try_from(-2)
/// .unwrap_or(0)` would make an X-cost card **free**. Jenny's Twin .45s
/// 02010 is in the compiled corpus.
#[test]
fn payable_play_cost_rejects_an_x_cost_as_unmodelled() {
    let err = payable_play_cost(Some(-2), &CardCode::new("02010"))
        .expect_err("an X cost is not yet modeled");
    assert!(
        err.contains("X cost") && err.contains("TODO(#577)"),
        "the reject should name X and the deferral: {err}"
    );
}
