//! Harm paid as a cost: [`Cost::TakeHarm`] on an activated ability.
//!
//! The cost is paid at Appendix I step 2 and the effect resolves at steps 3–4,
//! so the harm — its soak prompt, its placement and every nested sequence it
//! opens — resolves before the effect. `glossary/Costs.md`: *"If an
//! investigator takes damage or horror as a cost and reassigns any of it to an
//! asset, the cost is still considered paid."* If the harm eliminates the
//! payer, the effect does not resolve, as Dynamite Blast 01024's ruling has it
//! for an attack of opportunity: *"If the enemies' attacks defeat you, the
//! effects won't resolve."*
//!
//! The registry is `cards::TEST_REGISTRY` with two probes composed in. The
//! printed consumer, Forbidden Knowledge 01058, is tested on its own in
//! `forbidden_knowledge.rs`; [`FAST_PROBE`] models the primitive — *"\[fast\] Exhaust and take 1 horror:
//! gain 1 resource"* — and impersonates no printed card (ADR 0016).
//! [`ACTION_PROBE`] is the same cost on an `[action]` ability, which the engine
//! refuses.

use card_dsl::dsl::{self, Ability, Cost, InvestigatorTarget};
use cards::TEST_REGISTRY;
use game_core::card_registry::CardRegistry;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::OptionTarget;
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, ActionResolutionFrame, ActionResume, CardCode, Continuation,
    GameState, GameStateBuilder, InvestigatorId, LocationId, Status,
};
use game_core::test_support::{self, location, seat, BoardLookup as _, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

/// *"\[fast\] Exhaust and take 1 horror: gain 1 resource."*
const FAST_PROBE: &str = "_take_harm_fast";
/// *"\[action\] Take 1 horror: gain 1 resource."* Refused at validation.
const ACTION_PROBE: &str = "_take_harm_action";
/// Holy Rosary 01059: sanity 2, no health. The sanity-bearing soaker.
const HOLY_ROSARY: &str = "01059";

const INV: InvestigatorId = InvestigatorId(1);
const INV2: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(101);

fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    let take_one_horror = Cost::TakeHarm {
        damage: 0,
        horror: 1,
    };
    match code.as_str() {
        FAST_PROBE => Some(vec![dsl::activated(
            0,
            vec![Cost::Exhaust, take_one_horror],
            dsl::gain_resources(InvestigatorTarget::You, 1),
        )]),
        ACTION_PROBE => Some(vec![dsl::activated(
            1,
            vec![take_one_horror],
            dsl::gain_resources(InvestigatorTarget::You, 1),
        )]),
        _ => (TEST_REGISTRY.abilities_for)(code),
    }
}

/// [`cards::TEST_REGISTRY`] with this file's probe composed in.
const CARDS: CardRegistry = CardRegistry {
    abilities_for,
    ..TEST_REGISTRY
};

const REGISTRIES: Registries = Registries {
    cards: &CARDS,
    scenarios: &ScenarioRegistry::EMPTY,
};

fn activate(state: &GameState, probe: &str) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(state.in_play(INV, probe)),
        address: AbilityAddress::Printed(0),
    }
}

/// The harm cost stops at the soak prompt with the effect parked beneath it:
/// a `DealDamage { Effect }` frame on top of the actor-`Active` gate that holds
/// the ability's effect. The resource is gained only once the horror is placed.
#[test]
fn a_harm_cost_offers_the_soak_and_the_effect_waits_for_the_horror() {
    let state = GameStateBuilder::new()
        .with_registries(REGISTRIES)
        .with_location(location(HERE))
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .resources(0)
                .in_play([FAST_PROBE, HOLY_ROSARY]),
        )
        .open_turn(INV)
        .build();
    let rosary = state.in_play(INV, HOLY_ROSARY);
    let action = activate(&state, FAST_PROBE);
    let session = TestSession::new(state, REGISTRIES).take(&action);

    // Paused at the soak prompt: the other costs are paid, the ability is
    // announced, and nothing of the horror or the effect has landed yet.
    assert_eq!(session.offered_at(&OptionTarget::CardInstance(rosary)), 1);
    assert!(session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .any(|c| c.code.as_str() == FAST_PROBE && c.exhausted));
    assert_event!(session.events(), Event::AbilityActivated { .. });
    assert_eq!(session.state().investigators[&INV].resources, 0);
    assert_no_event!(
        session.events(),
        Event::ResourcesGained {
            investigator: INV,
            ..
        }
    );

    // The harm cost's deal frame is on top.
    let mut frames = session.state().continuations.iter().rev();
    test_support::assert_effect_deal(frames.next(), INV, 0, 1);
    assert!(
        matches!(
            frames.next(),
            Some(Continuation::ActionResolution(ActionResolutionFrame {
                investigator: INV,
                resume: ActionResume::ActivateAbility { .. },
            }))
        ),
        "the gated effect is parked beneath the deal frame: {:?}",
        session.state().continuations
    );

    // Take it on the investigator: the effect resolves once it is placed.
    let me = session.state().investigators[&INV].card_anchor();
    let session = session.pick(me);
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].horror(), 1);
    assert_eq!(session.state().investigators[&INV].resources, 1);
    assert_eq!(
        session.state().investigators[&INV].actions_remaining,
        3,
        "a zero-action ability takes no action"
    );

    // The horror is placed before the effect resolves.
    assert_event_sequence!(
        session.events(),
        Event::HorrorPlacedOnInvestigator {
            investigator: INV,
            amount: 1
        },
        Event::ResourcesGained {
            investigator: INV,
            ..
        },
    );
}

/// A lethal harm cost eliminates the payer, and the effect does not resolve.
/// Two seats, so the scenario does not end with the payer.
#[test]
fn a_lethal_harm_cost_eliminates_the_payer_and_the_effect_does_not_resolve() {
    let state = GameStateBuilder::new()
        .with_registries(REGISTRIES)
        .with_location(location(HERE))
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .harm(0, 7)
                .resources(0)
                .in_play([FAST_PROBE]),
        )
        .with_seat(seat(2).at(HERE))
        .with_seating([INV, INV2])
        .open_turn(INV)
        .build();
    let probe = state.in_play(INV, FAST_PROBE);
    let action = activate(&state, FAST_PROBE);
    let session = TestSession::new(state, REGISTRIES).take(&action);

    // The cost was paid — the probe exhausted and the ability activated — and
    // the horror it cost eliminated the payer before the effect could resolve.
    assert_event!(
        session.events(),
        Event::CardExhausted { instance_id, .. } if *instance_id == probe
    );
    assert_event!(session.events(), Event::AbilityActivated { .. });
    assert_ne!(session.state().investigators[&INV].status, Status::Active);
    assert_no_event!(
        session.events(),
        Event::ResourcesGained {
            investigator: INV,
            ..
        }
    );
    assert_eq!(session.state().investigators[&INV].resources, 0);
}

/// Sending the cost's horror to an asset still pays the cost, so the effect
/// resolves. `glossary/Costs.md`: *"If an investigator takes damage or horror
/// as a cost and reassigns any of it to an asset, the cost is still considered
/// paid."*
#[test]
fn a_harm_cost_soaked_by_an_asset_is_still_paid() {
    let state = GameStateBuilder::new()
        .with_registries(REGISTRIES)
        .with_location(location(HERE))
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .resources(0)
                .in_play([FAST_PROBE, HOLY_ROSARY]),
        )
        .open_turn(INV)
        .build();
    let rosary = state.in_play(INV, HOLY_ROSARY);
    let action = activate(&state, FAST_PROBE);
    let session = TestSession::new(state, REGISTRIES)
        .take(&action)
        .pick(OptionTarget::CardInstance(rosary));

    session.assert_at_turn_menu(INV);
    let investigator = &session.state().investigators[&INV];
    assert_eq!(investigator.horror(), 0, "the horror went to the rosary");
    let rosary_card = investigator
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == rosary)
        .expect("Holy Rosary still in play");
    assert_eq!(rosary_card.accumulated_horror, 1, "soaked by the player");
    assert_eq!(investigator.resources, 1, "the cost is still paid");
    assert_event!(
        session.events(),
        Event::ResourcesGained {
            investigator: INV,
            ..
        }
    );
}

/// On an `[action]` ability the harm would have to resolve before the attacks
/// of opportunity, which the take-an-action step does not do, so the cost is
/// refused at validation rather than mis-ordered.
#[test]
fn a_harm_cost_on_an_action_ability_is_refused_by_name() {
    let state = GameStateBuilder::new()
        .with_registries(REGISTRIES)
        .with_location(location(HERE))
        .with_seat(seat(1).at(HERE).actions(3).in_play([ACTION_PROBE]))
        .open_turn(INV)
        .build();
    let action = activate(&state, ACTION_PROBE);

    assert!(
        !enumerate::legal_actions(&state, REGISTRIES.cards).contains(&action),
        "a refused activation must not be offered"
    );
    let reason = TestSession::new(state, REGISTRIES).assert_rejects(|s| s.submit_off_menu(&action));
    assert!(
        reason.contains("TakeHarm") && reason.contains("[action]"),
        "the reason should name the cost and the ability shape it refuses, got: {reason}",
    );
}
