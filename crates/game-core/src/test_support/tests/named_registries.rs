//! A test names its registries once, on the builder or the session, and the
//! fixtures resolve against those values. Each probe here is known only to the
//! named registry, not to [`TEST_REGISTRY`](crate::test_support::TEST_REGISTRY),
//! the one most of this binary's tests name.

use std::sync::OnceLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Skills};

use crate::card_registry::CardRegistry;
use crate::engine::enumerate::TurnAction;
use crate::engine::Registries;
use crate::event::Event;
use crate::scenario::{ScenarioEnding, ScenarioId, ScenarioModule, ScenarioRegistry};
use crate::state::{Act, CardCode, GameState, GameStateBuilder, InvestigatorId, Owner};
use crate::test_support::{seat, terminal_code, TestSession, TEST_REGISTRIES, TEST_REGISTRY};

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
        weakness: None,
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

/// The scenario id only [`STAMP_SCENARIOS`] knows.
const STAMP: &str = "_named_stamp";

/// What [`stamp_apply`] sets the investigator's resources to, so a test can
/// tell the named scenario module's hook ran.
const STAMPED: u8 = 99;

fn stamp_apply(_ending: ScenarioEnding, state: &mut GameState, _events: &mut Vec<Event>) {
    if let Some(investigator) = state.investigators.values_mut().next() {
        investigator.resources = STAMPED;
    }
}

fn unused_setup() -> GameState {
    GameStateBuilder::new().build()
}

static STAMP_MODULE: ScenarioModule = ScenarioModule {
    resolve_symbol: None,
    setup: unused_setup,
    apply_resolution: stamp_apply,
    layout: &[],
};

const STAMP_SCENARIOS: ScenarioRegistry = ScenarioRegistry {
    module_for: |id| (id.as_str() == STAMP).then_some(&STAMP_MODULE),
};

/// game-core's test registry, which serves the terminal act's reverse, paired
/// with a scenario registry only these tests know.
const STAMPING: Registries = Registries {
    cards: &TEST_REGISTRY,
    scenarios: &STAMP_SCENARIOS,
};

/// The stamp scenario at `INV`'s turn menu, `INV` holding one clue.
fn stamp_board() -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).clues(1))
        .open_turn(INV)
        .with_scenario_id(ScenarioId::new(STAMP))
}

#[test]
fn the_board_session_ends_the_scenario_through_the_named_module() {
    // Lethal damage eliminates the only investigator, which ends the scenario
    // with no resolution; finishing it runs the module's `apply_resolution`.
    let session = stamp_board()
        .with_registries(STAMPING)
        .session()
        .take_damage(INV, 8);
    assert!(session.state().ending.is_some(), "the scenario ended");
    assert_eq!(session.state().investigators[&INV].resources, STAMPED);
}

#[test]
fn a_session_on_a_hand_built_state_ends_the_scenario_through_the_named_module() {
    let mut state = stamp_board().build();
    // No builder setter for the act deck. The act is terminal (ADR 0013) and
    // affordable, so advancing it latches `Resolution(1)`.
    state.act_deck = vec![Act {
        code: terminal_code(1),
        clue_threshold: 1,
    }];
    let session =
        TestSession::new(state, STAMPING).take(&TurnAction::AdvanceAct { investigator: INV });
    assert!(session.state().ending.is_some(), "the scenario ended");
    assert_eq!(session.state().investigators[&INV].resources, STAMPED);
}

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
