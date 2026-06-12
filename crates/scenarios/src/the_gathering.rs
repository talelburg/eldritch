//! The Gathering (Night of the Zealot, scenario 1) — Slice 1 C1a skeleton.
//!
//! Builds the faithful **Act-1 board**: only the Study is in play (the
//! Hallway/Attic/Cellar/Parlor are set aside (`set_aside_locations`) and
//! enter play via Act 1's (01108) Forced on-advance reverse, which also
//! relocates investigators to the Hallway and removes the Study).
//! `setup()` builds the world; the `StartScenario` roster step seats
//! investigators at [`STUDY_ID`] via `GameState.starting_location`.
//!
//! Faithful where it can be (agenda doom 3/7/10; the verified Standard
//! chaos bag; Study shroud/clues); structural stand-in where the rest of
//! Group C owns fidelity (act 01110 advances via its Forced `EnemyDefeated`
//! objective (01116; in `cards`) — its R1/R2 resolution choice is Phase-9
//! — TODO; symbol-token effects on reference card 01104 are C2). C1a does
//! not claim faithful win/lose semantics — only structural reachability,
//! proven by `tests/the_gathering.rs`.

use game_core::card_data::CardKind;
use game_core::event::Event;
use game_core::scenario::{Resolution, ScenarioId, ScenarioModule};
use game_core::state::{
    Act, Agenda, CardCode, ChaosBag, ChaosToken, GameState, GameStateBuilder, Location, LocationId,
    TokenModifiers,
};

/// Read a location's printed `(shroud, clues)` from the generated corpus.
/// The code is a build-time invariant of the corpus, so a miss is a bug.
fn location_stats(code: &str) -> (u8, u8) {
    match cards::by_code(code).expect("location code in corpus").kind {
        CardKind::Location { shroud, clues, .. } => (shroud, clues),
        ref k => panic!("{code} is not a Location ({k:?})"),
    }
}

/// Read an agenda's printed doom threshold from the corpus.
fn agenda_doom(code: &str) -> u8 {
    match cards::by_code(code).expect("agenda code in corpus").kind {
        CardKind::Agenda { doom_threshold } => doom_threshold,
        ref k => panic!("{code} is not an Agenda ({k:?})"),
    }
}

/// Read an act's printed clue threshold from the corpus. Acts that
/// advance on a non-clue objective (01110) carry `null` clues -> 0.
fn act_clue_threshold(code: &str) -> u8 {
    match cards::by_code(code).expect("act code in corpus").kind {
        CardKind::Act { clue_threshold, .. } => clue_threshold.unwrap_or(0),
        ref k => panic!("{code} is not an Act ({k:?})"),
    }
}

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

/// The Hallway's [`LocationId`] — the hub of the Act-2 board.
const HALLWAY_ID: LocationId = LocationId(2);
/// The Attic's [`LocationId`].
const ATTIC_ID: LocationId = LocationId(3);
/// The Cellar's [`LocationId`].
const CELLAR_ID: LocationId = LocationId(4);
/// The Parlor's [`LocationId`].
const PARLOR_ID: LocationId = LocationId(5);

/// Build the initial [`GameState`]: the Study in play (isolated), the
/// four set-aside locations (Hallway/Attic/Cellar/Parlor, pre-connected),
/// the act/agenda decks, the Standard chaos bag, and `starting_location`.
/// No investigators — the `StartScenario` roster step seats them.
pub fn setup() -> GameState {
    // The Study (01111): shroud/clues read from the corpus. `Location::new`
    // gives a revealed, unconnected location (Act 1 is "trapped in the Study").
    let (study_shroud, study_clues) = location_stats("01111");
    let study = Location::new(
        STUDY_ID,
        CardCode("01111".into()),
        "Study",
        study_shroud,
        study_clues,
    );

    // LocationIds 2–5: the four set-aside locations. Connections are wired
    // here (scenario map knowledge — the corpus carries none) so they enter
    // play already connected when Act 1's (01108) Forced on-advance reverse
    // fires. The Hallway is the hub; Attic/Cellar/Parlor are the spokes.
    // TODO(#260): replace the hand-assigned LocationIds + manual connection
    // wiring with a location-construction/id-allocation helper.
    let make = |id: LocationId, code: &str, name: &str| {
        let (shroud, clues) = location_stats(code);
        Location::new(id, CardCode(code.into()), name, shroud, clues)
    };
    let mut hallway = make(HALLWAY_ID, "01112", "Hallway");
    hallway.connections = vec![ATTIC_ID, CELLAR_ID, PARLOR_ID];
    let mut attic = make(ATTIC_ID, "01113", "Attic");
    attic.connections = vec![HALLWAY_ID];
    let mut cellar = make(CELLAR_ID, "01114", "Cellar");
    cellar.connections = vec![HALLWAY_ID];
    let mut parlor = make(PARLOR_ID, "01115", "Parlor");
    parlor.connections = vec![HALLWAY_ID];

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
    state.set_aside_locations = vec![hallway, attic, cellar, parlor];

    // Act deck 01108 -> 01109 -> 01110. Clue thresholds read from the
    // corpus. 01110 advances via its Forced EnemyDefeated objective
    // (01116; in cards::act_01110), not a clue spend — its printed clue
    // threshold is null, which the reader maps to 0.
    // TODO(#231): the Ghoul Priest (01116) spawns at Act-2 (01109) advance — C3b.
    // TODO(#phase-9): the reverse is the lead investigator's R1/R2 resolution choice (campaign log).
    state.act_deck = vec![
        Act {
            code: CardCode("01108".into()),
            clue_threshold: act_clue_threshold("01108"),
            resolution: None,
        },
        Act {
            code: CardCode("01109".into()),
            clue_threshold: act_clue_threshold("01109"),
            resolution: None,
        },
        Act {
            // 01110 advances via its Forced EnemyDefeated objective (01116; in cards::act_01110), not a clue spend.
            code: CardCode("01110".into()),
            clue_threshold: act_clue_threshold("01110"),
            resolution: Some(Resolution::Won { id: "R1".into() }),
        },
    ];

    // Agenda deck 01105 -> 01106 -> 01107. Doom thresholds read from the
    // corpus. The terminal agenda carries the Lost latch.
    state.agenda_deck = vec![
        Agenda {
            code: CardCode("01105".into()),
            doom_threshold: agenda_doom("01105"),
            resolution: None,
        },
        Agenda {
            code: CardCode("01106".into()),
            doom_threshold: agenda_doom("01106"),
            resolution: None,
        },
        Agenda {
            code: CardCode("01107".into()),
            doom_threshold: agenda_doom("01107"),
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
    fn setup_reads_card_stats_from_corpus() {
        // The hardcoded literals are gone — these values now come from
        // cards::by_code. Pinning them guards both the corpus data and
        // the reader helpers.
        let s = setup();
        let study = s.locations.get(&STUDY_ID).unwrap();
        assert_eq!((study.shroud, study.clues), (2, 2), "Study 01111 stats");
        assert_eq!(
            s.agenda_deck
                .iter()
                .map(|a| a.doom_threshold)
                .collect::<Vec<_>>(),
            [3, 7, 10],
            "agenda doom thresholds from corpus",
        );
        assert_eq!(s.act_deck[0].clue_threshold, 2, "act 01108 from corpus");
        assert_eq!(s.act_deck[1].clue_threshold, 3, "act 01109 from corpus");
    }

    #[test]
    fn setup_places_study_in_play_and_four_set_aside() {
        let s = setup();
        // In play: only the Study (Act-1 board).
        assert_eq!(s.locations.len(), 1);
        let study = s.locations.get(&STUDY_ID).expect("Study present");
        assert_eq!(study.code, CardCode("01111".into()));
        assert!(study.connections.is_empty(), "Study is isolated");
        // Set aside: Hallway, Attic, Cellar, Parlor, each pre-connected.
        let codes: Vec<_> = s
            .set_aside_locations
            .iter()
            .map(|l| l.code.as_str().to_owned())
            .collect();
        assert_eq!(codes, ["01112", "01113", "01114", "01115"]);
        let hallway = s
            .set_aside_locations
            .iter()
            .find(|l| l.code.as_str() == "01112")
            .unwrap();
        let mut hall_conns: Vec<_> = hallway.connections.clone();
        hall_conns.sort();
        let mut others: Vec<_> = s
            .set_aside_locations
            .iter()
            .filter(|l| l.code.as_str() != "01112")
            .map(|l| l.id)
            .collect();
        others.sort();
        assert_eq!(
            hall_conns, others,
            "Hallway connects to Attic/Cellar/Parlor"
        );
        for l in s
            .set_aside_locations
            .iter()
            .filter(|l| l.code.as_str() != "01112")
        {
            assert_eq!(
                l.connections,
                vec![hallway.id],
                "spokes connect back to the Hallway"
            );
        }
        assert_eq!(s.starting_location, Some(STUDY_ID));
        assert!(s.investigators.is_empty(), "setup() seats no one");
    }

    #[test]
    fn act_three_advances_on_objective_not_clues() {
        let s = setup();
        assert_eq!(s.act_deck[2].code.as_str(), "01110");
        assert_eq!(
            s.act_deck[2].clue_threshold, 0,
            "01110 advances on Ghoul-Priest-defeat, not clues"
        );
        assert!(matches!(
            s.act_deck[2].resolution,
            Some(Resolution::Won { .. })
        ));
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
