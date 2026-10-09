//! A card discarded from a threat area goes to its **owner's** discard pile
//! (#981). `glossary/Discard_Piles.md`: *"Any time a card is discarded, it is
//! placed faceup on top of its owner's discard pile. Encounter cards are owned
//! by the encounter deck."*
//!
//! A threat area holds both kinds: an encounter treachery is the encounter
//! deck's, and a weakness its bearer's. Haunted 01098 is the shape this pins —
//! *"[action] [action]: Discard Haunted."*, which its ruling lets a co-located
//! investigator trigger (<https://arkhamdb.com/card/01098>): *"Any investigator
//! at the same location as the investigator with Haunted in their threat area
//! may trigger the [action][action] to discard Haunted, as per the FAQ [V1.0,
//! section 2.1]."* Haunted is not implemented, so the probe here models the
//! primitive — a threat-area card with an activated self-discard — rather than
//! impersonating it (ADR 0016).
//!
//! Both ways an activation discards its own source are covered: the discard as
//! the ability's effect (`DiscardSelf`), and the discard as its cost
//! (`Cost::DiscardSelf`). Each is activated by the *other* investigator at the
//! bearer's location, so the pile that receives the card is decided by the
//! owner, not by who activated it or whose threat area held it.
//!
//! Own integration-test binary so it can install its own `MockRegistry`.

use card_dsl::card_data::{CardKind, CardMetadata};
use card_dsl::dsl::{self, Ability, Cost, InvestigatorTarget};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{ApplyResult, EngineOutcome};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, DiscardPile, GameState,
    GameStateBuilder, InvestigatorId, LocationId, Owner, Zone,
};
use game_core::test_support::{self, MockRegistry};

/// Synthetic treachery that sits in a threat area and can be discarded by an
/// activation. The fixture gives it either owner.
const PROBE: &str = "_td_probe";

/// The investigator whose threat area holds the probe.
const BEARER: InvestigatorId = InvestigatorId(1);
/// A second investigator at the bearer's location, who activates it.
const NEIGHBOUR: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(1);
const PROBE_INST: CardInstanceId = CardInstanceId(50);

/// `[action] [action]: Discard this card.` — the discard is the effect.
const DISCARD_AS_EFFECT: u8 = 0;
/// `[action] Discard this card: Gain 1 resource.` — the discard is the cost.
const DISCARD_AS_COST: u8 = 1;

fn probe_abilities() -> Vec<Ability> {
    vec![
        dsl::activated(2, vec![], dsl::discard_self()),
        dsl::activated(
            1,
            vec![Cost::DiscardSelf],
            dsl::gain_resources(InvestigatorTarget::Active, 1),
        ),
    ]
}

#[ctor::ctor(unsafe)]
fn install_probe_registry() {
    MockRegistry::new()
        .with_card(CardMetadata {
            code: PROBE.to_string(),
            name: "Probe".to_string(),
            text: None,
            traits: vec![],
            back_name: None,
            back_text: None,
            pack_code: "test".to_string(),
            weakness: false,
            kind: CardKind::Treachery {
                surge: false,
                peril: false,
                quantity: 1,
            },
        })
        .with_abilities(PROBE, probe_abilities)
        .install();
}

/// Both investigators at one location, the probe owned by `owner` in the
/// bearer's threat area, and the neighbour's turn open.
fn board(owner: Owner) -> GameState {
    let mut bearer = test_support::test_investigator(1);
    bearer.threat_area.push(CardInPlay::enter_play(
        CardCode::new(PROBE),
        PROBE_INST,
        owner,
    ));
    GameStateBuilder::new()
        .with_investigator_at(bearer, HERE)
        .with_investigator_at(test_support::test_investigator(2), HERE)
        .with_location(test_support::test_location(1, "Here"))
        .open_turn(NEIGHBOUR)
        .build()
}

/// The neighbour activates the probe's ability at `index`.
fn activate(owner: Owner, index: u8) -> ApplyResult {
    test_support::take_turn_action(
        board(owner),
        &TurnAction::ActivateAbility {
            investigator: NEIGHBOUR,
            source: AbilitySource::InPlay(PROBE_INST),
            address: AbilityAddress::Printed(index),
        },
    )
}

#[test]
fn a_bearers_card_discarded_from_their_threat_area_goes_to_their_discard() {
    for index in [DISCARD_AS_EFFECT, DISCARD_AS_COST] {
        let r = activate(Owner::Investigator(BEARER), index);
        assert!(
            !matches!(r.outcome, EngineOutcome::Rejected { .. }),
            "ability {index}: {:?}",
            r.outcome,
        );
        assert!(r.state.investigators[&BEARER].threat_area.is_empty());
        assert_eq!(
            r.state.investigators[&BEARER].discard,
            vec![CardCode::new(PROBE)],
            "ability {index}: in its owner's discard",
        );
        assert!(
            r.state.investigators[&NEIGHBOUR].discard.is_empty(),
            "ability {index}: not in the activator's discard",
        );
        assert!(
            r.state.encounter_discard.is_empty(),
            "ability {index}: not in the encounter discard",
        );
        assert_event!(
            r.events,
            Event::CardDiscarded { code, from: Zone::ThreatArea, to: DiscardPile::Investigator(BEARER) }
                if code.as_str() == PROBE
        );
    }
}

#[test]
fn an_encounter_card_discarded_from_a_threat_area_goes_to_the_encounter_discard() {
    for index in [DISCARD_AS_EFFECT, DISCARD_AS_COST] {
        let r = activate(Owner::EncounterDeck, index);
        assert!(
            !matches!(r.outcome, EngineOutcome::Rejected { .. }),
            "ability {index}: {:?}",
            r.outcome,
        );
        assert!(r.state.investigators[&BEARER].threat_area.is_empty());
        assert_eq!(
            r.state.encounter_discard,
            vec![CardCode::new(PROBE)],
            "ability {index}: in the encounter discard",
        );
        assert!(r.state.investigators[&BEARER].discard.is_empty());
        assert!(r.state.investigators[&NEIGHBOUR].discard.is_empty());
        assert_event!(
            r.events,
            Event::CardDiscarded { code, from: Zone::ThreatArea, to: DiscardPile::Encounter }
                if code.as_str() == PROBE
        );
    }
}
