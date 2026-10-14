//! A test names its registries once, on the builder or the session, and the
//! fixtures resolve against those values — not against whatever registry the
//! process has installed. Each probe here is known only to the named registry:
//! this binary installs [`TEST_REGISTRY`](crate::test_support::TEST_REGISTRY)
//! globally, which knows none of them.

use std::sync::OnceLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Skills};

use crate::card_registry::CardRegistry;
use crate::engine::Registries;
use crate::scenario::ScenarioRegistry;
use crate::state::{CardCode, GameStateBuilder, InvestigatorId, Owner, SkillKind};
use crate::test_support::seat;

const INV: InvestigatorId = InvestigatorId(1);

/// A synthetic investigator card only [`PROBES`] knows, printed with skills
/// the stock `test_investigator` does not have.
const PROBE_INV: &str = "_NAMED_PROBE_INV";

/// A synthetic asset only [`PROBES`] knows: a player card, so in a threat area
/// its bearer owns it, where an unknown code would belong to the encounter
/// deck.
const PROBE_ASSET: &str = "_NAMED_PROBE_ASSET";

const PROBE_SKILLS: Skills = Skills {
    willpower: 5,
    intellect: 1,
    combat: 2,
    agility: 4,
};

fn probe(code: &str, name: &str, kind: CardKind) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: name.to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_test".to_owned(),
        weakness: false,
        kind,
    }
}

fn probe_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    static INVESTIGATOR: OnceLock<CardMetadata> = OnceLock::new();
    static ASSET: OnceLock<CardMetadata> = OnceLock::new();
    match code.as_str() {
        PROBE_INV => Some(INVESTIGATOR.get_or_init(|| {
            probe(
                PROBE_INV,
                "Named Probe",
                CardKind::Investigator {
                    class: Class::Neutral,
                    skills: PROBE_SKILLS,
                    health: 8,
                    sanity: 8,
                },
            )
        })),
        PROBE_ASSET => Some(ASSET.get_or_init(|| {
            probe(
                PROBE_ASSET,
                "Named Probe Asset",
                CardKind::Asset {
                    class: Class::Neutral,
                    cost: Some(0),
                    xp: None,
                    slots: vec![],
                    health: None,
                    sanity: None,
                    skill_icons: SkillIcons::default(),
                    is_fast: false,
                    deck_limit: 2,
                    uses: None,
                    play_only_during_turn: false,
                },
            )
        })),
        _ => None,
    }
}

const PROBES: CardRegistry = CardRegistry {
    metadata_for: probe_metadata_for,
    ..CardRegistry::EMPTY
};

const NAMED: Registries = Registries {
    cards: &PROBES,
    scenarios: &ScenarioRegistry::EMPTY,
};

#[test]
fn as_card_reads_the_printed_investigator_from_the_named_registries() {
    let state = GameStateBuilder::new()
        .with_registries(NAMED)
        .with_seat(seat(1).as_card(PROBE_INV))
        .build();
    let investigator = &state.investigators[&INV];
    assert_eq!(investigator.name, "Named Probe");
    assert_eq!(investigator.skills, PROBE_SKILLS);
    assert_eq!(investigator.investigator_card.code.as_str(), PROBE_INV);
}

#[test]
fn a_skill_set_after_as_card_still_wins_over_the_named_printed_one() {
    let state = GameStateBuilder::new()
        .with_registries(NAMED)
        .with_seat(seat(1).as_card(PROBE_INV).skill(SkillKind::Intellect, 6))
        .build();
    assert_eq!(
        state.investigators[&INV].skills,
        Skills {
            intellect: 6,
            ..PROBE_SKILLS
        },
    );
}

#[test]
fn a_threat_area_card_is_owned_as_the_named_registries_print_it() {
    let state = GameStateBuilder::new()
        .with_registries(NAMED)
        .with_seat(seat(1).threat_area([PROBE_ASSET]))
        .build();
    assert_eq!(
        state.investigators[&INV].threat_area[0].owner,
        Owner::Investigator(INV),
    );
}
