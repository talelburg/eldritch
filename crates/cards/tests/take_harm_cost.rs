//! Harm paid as a cost: [`Cost::TakeHarm`] on an activated ability (#1029).
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
//! The registry is `cards::REGISTRY` with two probes composed in. No printed
//! card declares the cost yet (Forbidden Knowledge 01058 does in #777), so
//! [`FAST_PROBE`] models the primitive — *"\[fast\] Exhaust and take 1 horror:
//! gain 1 resource"* — and impersonates no printed card (ADR 0016).
//! [`ACTION_PROBE`] is the same cost on an `[action]` ability, which the engine
//! refuses.

use card_dsl::dsl::{self, Ability, Cost, InvestigatorTarget};
use cards::REGISTRY;
use game_core::card_registry::CardRegistry;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, ActionResolutionFrame, ActionResume, CardCode, Continuation,
    DamageSource, DealDamageFrame, GameState, GameStateBuilder, InvestigatorId, LocationId, Status,
};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};

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
        _ => (REGISTRY.abilities_for)(code),
    }
}

#[ctor::ctor(unsafe)]
fn install() {
    test_support::install_registry_with_test_cards(CardRegistry {
        abilities_for,
        ..REGISTRY
    });
}

fn activate(state: &GameState, probe: &str) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(state.in_play(INV, probe)),
        address: AbilityAddress::Printed(0),
    }
}

fn resources_gained(session: &TestSession) -> Option<usize> {
    session.events().iter().position(|e| {
        matches!(
            e,
            Event::ResourcesGained {
                investigator: INV,
                ..
            }
        )
    })
}

/// The harm cost stops at the soak prompt with the effect parked beneath it:
/// a `DealDamage { Effect }` frame on top of the actor-`Active` gate that holds
/// the ability's effect. The resource is gained only once the horror is placed.
#[test]
fn a_harm_cost_offers_the_soak_and_the_effect_waits_for_the_horror() {
    let state = GameStateBuilder::new()
        .with_location(test_support::test_location(HERE.0, "Study"))
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
    let session = TestSession::new(state).take(&action);

    // Paused at the soak prompt: the other costs are paid, the ability is
    // announced, and nothing of the horror or the effect has landed yet.
    assert_eq!(session.offered_at(&OptionTarget::CardInstance(rosary)), 1);
    assert!(session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .any(|c| c.code.as_str() == FAST_PROBE && c.exhausted));
    assert!(session
        .events()
        .iter()
        .any(|e| matches!(e, Event::AbilityActivated { .. })));
    assert_eq!(session.state().investigators[&INV].resources, 0);
    assert_eq!(resources_gained(&session), None);

    let mut frames = session.state().continuations.iter().rev();
    assert!(
        matches!(
            frames.next(),
            Some(Continuation::DealDamage(DealDamageFrame {
                investigator: INV,
                source: DamageSource::Effect,
                ..
            }))
        ),
        "the harm cost's deal frame is on top: {:?}",
        session.state().continuations
    );
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
        "a fast ability takes no action"
    );

    let horror = session
        .events()
        .iter()
        .position(|e| {
            matches!(
                e,
                Event::HorrorTaken {
                    investigator: INV,
                    amount: 1
                }
            )
        })
        .expect("the cost's horror is placed");
    let gained = resources_gained(&session).expect("the resource is gained");
    assert!(
        horror < gained,
        "the horror is placed before the effect resolves: {:?}",
        session.events()
    );
}

/// A lethal harm cost eliminates the payer, and the effect does not resolve.
/// Two seats, so the scenario does not end with the payer.
#[test]
fn a_lethal_harm_cost_eliminates_the_payer_and_the_effect_does_not_resolve() {
    let state = GameStateBuilder::new()
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .harm(0, 7)
                .resources(0)
                .in_play([FAST_PROBE]),
        )
        .with_seat(seat(2).at(HERE))
        .with_turn_order([INV, INV2])
        .open_turn(INV)
        .build();
    let action = activate(&state, FAST_PROBE);
    let session = TestSession::new(state).take(&action);

    assert_ne!(session.state().investigators[&INV].status, Status::Active);
    assert_eq!(resources_gained(&session), None, "{:?}", session.events());
    assert_eq!(session.state().investigators[&INV].resources, 0);
}

/// On an `[action]` ability the harm would have to resolve before the attacks
/// of opportunity, which the take-an-action step does not do, so the cost is
/// refused at validation rather than mis-ordered.
#[test]
fn a_harm_cost_on_an_action_ability_is_refused_by_name() {
    let state = GameStateBuilder::new()
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat(1).at(HERE).actions(3).in_play([ACTION_PROBE]))
        .open_turn(INV)
        .build();
    let action = activate(&state, ACTION_PROBE);

    assert!(
        !enumerate::legal_actions(&state).contains(&action),
        "a refused activation must not be offered"
    );
    let reason = TestSession::new(state).assert_rejects(|s| s.submit_off_menu(&action));
    assert!(
        reason.contains("TakeHarm") && reason.contains("[action]"),
        "the reason should name the cost and the ability shape it refuses, got: {reason}",
    );
}
