//! A card discarded from a threat area goes to its **owner's** discard pile
//! `glossary/Discard_Piles.md`: *"Any time a card is discarded, it is
//! placed faceup on top of its owner's discard pile. Encounter cards are owned
//! by the encounter deck."*
//!
//! A threat area holds both kinds: an encounter treachery is the encounter
//! deck's, and a weakness its bearer's. Haunted 01098 is the shape this pins —
//! *"\[action\] \[action\]: Discard Haunted."*, which its ruling lets a co-located
//! investigator trigger (<https://arkhamdb.com/card/01098>): *"Any investigator
//! at the same location as the investigator with Haunted in their threat area
//! may trigger the \[action\]\[action\] to discard Haunted, as per the FAQ [V1.0,
//! section 2.1]."* Haunted itself is pinned in `haunted.rs`. The probe here
//! models the primitive — a threat-area card with an activated self-discard —
//! rather than impersonating it (ADR 0016), so it can be printed both as an
//! encounter card and as a weakness.
//!
//! Both ways an activation discards its own source are covered: the discard as
//! the ability's effect (`DiscardSelf`), and the discard as its cost
//! (`Cost::DiscardSelf`). Each is activated by the *other* investigator at the
//! bearer's location, so the pile that receives the card is decided by the
//! owner, not by who activated it or whose threat area held it.
//!
//! Own integration-test binary so it can build its own `MockRegistry`.

use card_dsl::card_data::{CardKind, CardMetadata, WeaknessKind};
use card_dsl::dsl::{self, Ability, Cost, InvestigatorTarget};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, DiscardPile, GameStateBuilder, InvestigatorId,
    LocationId, Zone,
};
use game_core::test_support::{self, seat, MockRegistry, SeatLookup as _, TestSession};
use std::sync::LazyLock;

/// Synthetic encounter treachery that sits in a threat area and can be
/// discarded by an activation: owned by the encounter deck.
const PROBE: &str = "_td_probe";
/// The same probe printed as a weakness: owned by its bearer.
const WEAKNESS_PROBE: &str = "_td_probe_weakness";

/// The investigator whose threat area holds the probe.
const BEARER: InvestigatorId = InvestigatorId(1);
/// A second investigator at the bearer's location, who activates it.
const NEIGHBOUR: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(1);

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

fn probe_card(code: &str, weakness: bool) -> CardMetadata {
    CardMetadata {
        code: code.to_string(),
        name: "Probe".to_string(),
        text: None,
        traits: vec![],
        back_name: None,
        back_text: None,
        pack_code: "test".to_string(),
        weakness: weakness.then_some(WeaknessKind::Other),
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    }
}

/// Both investigators at one location, the probe `probe` in the bearer's
/// threat area (owned as its printed card says), and the neighbour's turn
/// open; the neighbour activates its ability at `index`.
fn activate(probe: &str, index: u8) -> TestSession {
    let session = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE).threat_area([probe]))
        .with_seat(seat(2).at(HERE))
        .with_location(test_support::test_location(HERE.0, "Here"))
        .open_turn(NEIGHBOUR)
        .session();
    let source = AbilitySource::InPlay(session.state().in_play(BEARER, probe));
    session.take(&TurnAction::ActivateAbility {
        investigator: NEIGHBOUR,
        source,
        address: AbilityAddress::Printed(index),
    })
}

/// The binary's one mock, built once on first use.
fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(probe_card(PROBE, false))
            .with_card(probe_card(WEAKNESS_PROBE, true))
            .with_abilities(PROBE, probe_abilities)
            .with_abilities(WEAKNESS_PROBE, probe_abilities)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

#[test]
fn a_bearers_card_discarded_from_their_threat_area_goes_to_their_discard() {
    for index in [DISCARD_AS_EFFECT, DISCARD_AS_COST] {
        let session = activate(WEAKNESS_PROBE, index);
        // Not rejected: the activation resolved and the neighbour's turn goes on.
        session.assert_at_turn_menu(NEIGHBOUR);
        let state = session.state();
        assert!(state.investigators[&BEARER].threat_area.is_empty());
        assert_eq!(
            state.investigators[&BEARER].discard,
            vec![CardCode::new(WEAKNESS_PROBE)],
            "ability {index}: in its owner's discard",
        );
        assert!(
            state.investigators[&NEIGHBOUR].discard.is_empty(),
            "ability {index}: not in the activator's discard",
        );
        assert!(
            state.encounter_discard.is_empty(),
            "ability {index}: not in the encounter discard",
        );
        assert_event!(
            session.events(),
            Event::CardDiscarded { code, from: Zone::ThreatArea, to: DiscardPile::Investigator(BEARER) }
                if code.as_str() == WEAKNESS_PROBE
        );
    }
}

#[test]
fn an_encounter_card_discarded_from_a_threat_area_goes_to_the_encounter_discard() {
    for index in [DISCARD_AS_EFFECT, DISCARD_AS_COST] {
        let session = activate(PROBE, index);
        // Not rejected: the activation resolved and the neighbour's turn goes on.
        session.assert_at_turn_menu(NEIGHBOUR);
        let state = session.state();
        assert!(state.investigators[&BEARER].threat_area.is_empty());
        assert_eq!(
            state.encounter_discard,
            vec![CardCode::new(PROBE)],
            "ability {index}: in the encounter discard",
        );
        assert!(state.investigators[&BEARER].discard.is_empty());
        assert!(state.investigators[&NEIGHBOUR].discard.is_empty());
        assert_event!(
            session.events(),
            Event::CardDiscarded { code, from: Zone::ThreatArea, to: DiscardPile::Encounter }
                if code.as_str() == PROBE
        );
    }
}
