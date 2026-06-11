//! The Gathering (Night of the Zealot, scenario 1) — Slice 1 C1a skeleton.
//!
//! Builds the faithful **Act-1 board**: only the Study is in play (the
//! Hallway/Attic/Cellar/Parlor are set aside and enter via the Act-1
//! "Door on the Floor" transition — C1b). `setup()` builds the world;
//! the `StartScenario` roster step seats investigators at
//! [`STUDY_ID`] via `GameState.starting_location`.
//!
//! Faithful where it can be (agenda doom 3/7/10; the verified Standard
//! chaos bag; Study shroud/clues); structural stand-in where the rest of
//! Group C owns fidelity (act 01110's clue threshold is a placeholder —
//! its real "Ghoul Priest defeated" objective is C1b; symbol-token
//! effects on reference card 01104 are C2). C1a does not claim faithful
//! win/lose semantics — only structural reachability, proven by
//! `tests/the_gathering.rs`.

use game_core::event::Event;
use game_core::scenario::{Resolution, ScenarioId, ScenarioModule};
use game_core::state::{
    Act, Agenda, CardCode, ChaosBag, ChaosToken, GameState, GameStateBuilder, Location, LocationId,
    TokenModifiers,
};

/// String id used to look this module up in [`crate::REGISTRY`].
pub const ID: &str = "the-gathering";

/// `ArkhamDB` reference-card code (chaos-symbol effects; evaluated in C2).
pub const REFERENCE_CARD: &str = "01104";

/// The Study's [`LocationId`] — the scenario's starting location.
pub const STUDY_ID: LocationId = LocationId(1);

/// The verified Standard-difficulty Night of the Zealot chaos bag (16
/// tokens). Source: `data/campaign-guides/SOURCE.md` (campaign guide
/// p.1, "Assemble the campaign chaos bag", Standard).
fn standard_chaos_bag() -> ChaosBag {
    use ChaosToken::{AutoFail, Cultist, ElderSign, Numeric, Skull, Tablet};
    ChaosBag::new([
        Numeric(1),
        Numeric(0),
        Numeric(0),
        Numeric(-1),
        Numeric(-1),
        Numeric(-1),
        Numeric(-2),
        Numeric(-2),
        Numeric(-3),
        Numeric(-4),
        Skull,
        Skull,
        Cultist,
        Tablet,
        AutoFail,
        ElderSign,
    ])
}

/// Build the initial [`GameState`]: the Study in play (isolated), the
/// act/agenda decks, the Standard chaos bag, and `starting_location`.
/// No investigators — the `StartScenario` roster step seats them.
pub fn setup() -> GameState {
    // The Study (01111): shroud 2, clues 2, revealed, no connections
    // (Act 1 is "trapped in the Study"). `Location::new` gives a
    // revealed, unconnected location; the Study's connection graph is
    // C1b's Door-on-the-Floor transition.
    let study = Location::new(STUDY_ID, CardCode("01111".into()), "Study", 2, 2);

    // The Gathering's symbol effects are printed on reference card 01104
    // (board-dependent; evaluated in C2). Until then these flat NotZ
    // fallbacks stand in; they are off the C1a structural test path.
    // TokenModifiers is #[non_exhaustive], so we build via Default +
    // field mutation (same pattern used elsewhere outside game-core).
    let mut token_modifiers = TokenModifiers::default();
    token_modifiers.skull = -1;
    token_modifiers.cultist = -2;
    token_modifiers.tablet = -3;
    token_modifiers.elder_thing = -4;

    let mut state = GameStateBuilder::new()
        .with_location(study)
        .with_chaos_bag(standard_chaos_bag())
        .with_scenario_id(ScenarioId::new(ID))
        .with_token_modifiers(token_modifiers)
        .build();

    state.starting_location = Some(STUDY_ID);

    // Act deck 01108 -> 01109 -> 01110. Clue thresholds 2/3 are the real
    // printed values; 01110's is a placeholder (its real "Ghoul Priest
    // defeated" objective is C1b). The terminal act carries the Won latch.
    state.act_deck = vec![
        Act {
            code: CardCode("01108".into()),
            clue_threshold: 2,
            resolution: None,
        },
        Act {
            code: CardCode("01109".into()),
            clue_threshold: 3,
            resolution: None,
        },
        Act {
            code: CardCode("01110".into()),
            clue_threshold: 2, // placeholder; real objective is C1b
            resolution: Some(Resolution::Won { id: "R1".into() }),
        },
    ];

    // Agenda deck 01105 -> 01106 -> 01107. Doom thresholds 3/7/10 are the
    // real printed values. The terminal agenda carries the Lost latch.
    state.agenda_deck = vec![
        Agenda {
            code: CardCode("01105".into()),
            doom_threshold: 3,
            resolution: None,
        },
        Agenda {
            code: CardCode("01106".into()),
            doom_threshold: 7,
            resolution: None,
        },
        Agenda {
            code: CardCode("01107".into()),
            doom_threshold: 10,
            resolution: Some(Resolution::Lost {
                reason: "The ghouls break free".into(),
            }),
        },
    ];

    state
}

/// No-op for C1a (matches the synthetic fixture). XP / trauma / campaign
/// log application is Phase 9.
pub fn apply_resolution(
    _resolution: &Resolution,
    _state: &mut GameState,
    _events: &mut Vec<Event>,
) {
}

/// The [`ScenarioModule`] value for The Gathering.
pub const MODULE: ScenarioModule = ScenarioModule {
    reference_card: REFERENCE_CARD,
    setup,
    apply_resolution,
};

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::state::ChaosToken;

    #[test]
    fn setup_places_only_the_isolated_study() {
        let s = setup();
        assert_eq!(s.locations.len(), 1, "Act-1 board is the Study only");
        let study = s.locations.get(&STUDY_ID).expect("Study present");
        assert_eq!(study.code, CardCode("01111".into()));
        assert_eq!(study.shroud, 2);
        assert_eq!(study.clues, 2);
        assert!(study.revealed);
        assert!(study.connections.is_empty(), "Study is isolated in Act 1");
        assert_eq!(s.starting_location, Some(STUDY_ID));
        assert_eq!(s.scenario_id, Some(ScenarioId::new(ID)));
        assert!(s.investigators.is_empty(), "setup() seats no one");
    }

    #[test]
    fn setup_seeds_act_and_agenda_decks_with_terminal_latches() {
        let s = setup();
        let act_codes: Vec<_> = s.act_deck.iter().map(|a| a.code.as_str()).collect();
        assert_eq!(act_codes, ["01108", "01109", "01110"]);
        assert_eq!(s.act_deck[0].clue_threshold, 2);
        assert_eq!(s.act_deck[1].clue_threshold, 3);
        assert!(matches!(
            s.act_deck[2].resolution,
            Some(Resolution::Won { .. })
        ));

        let agenda_codes: Vec<_> = s.agenda_deck.iter().map(|a| a.code.as_str()).collect();
        assert_eq!(agenda_codes, ["01105", "01106", "01107"]);
        assert_eq!(
            s.agenda_deck
                .iter()
                .map(|a| a.doom_threshold)
                .collect::<Vec<_>>(),
            [3, 7, 10]
        );
        assert!(matches!(
            s.agenda_deck[2].resolution,
            Some(Resolution::Lost { .. })
        ));
    }

    #[test]
    fn setup_seeds_verified_standard_chaos_bag() {
        let s = setup();
        let mut tokens = s.chaos_bag.tokens.clone();
        let mut expected = vec![
            ChaosToken::Numeric(1),
            ChaosToken::Numeric(0),
            ChaosToken::Numeric(0),
            ChaosToken::Numeric(-1),
            ChaosToken::Numeric(-1),
            ChaosToken::Numeric(-1),
            ChaosToken::Numeric(-2),
            ChaosToken::Numeric(-2),
            ChaosToken::Numeric(-3),
            ChaosToken::Numeric(-4),
            ChaosToken::Skull,
            ChaosToken::Skull,
            ChaosToken::Cultist,
            ChaosToken::Tablet,
            ChaosToken::AutoFail,
            ChaosToken::ElderSign,
        ];
        tokens.sort_by_key(|t| format!("{t:?}"));
        expected.sort_by_key(|t| format!("{t:?}"));
        assert_eq!(tokens, expected, "Standard NotZ bag is 16 tokens");
    }
}
