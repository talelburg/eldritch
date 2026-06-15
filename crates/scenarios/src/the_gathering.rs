//! The Gathering (Night of the Zealot, scenario 1) — Slice 1 C1a skeleton.
//!
//! Builds the faithful **Act-1 board**: only the Study is in play (the
//! Hallway/Attic/Cellar/Parlor are set aside (`set_aside_locations`) and
//! enter play via Act 1's (01108) Forced on-advance reverse, which also
//! relocates investigators to the Hallway and removes the Study).
//! `setup()` builds the world; the `StartScenario` roster step seats
//! investigators at the starting location (the Study, `01111`) via
//! `GameState.starting_location`.
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
use game_core::scenario::{
    Resolution, ScenarioId, ScenarioModule, SymbolCtx, SymbolOutcome, TokenEffect,
};
use game_core::state::{
    Act, Agenda, CardCode, ChaosBag, ChaosToken, GameState, GameStateBuilder, RoundEndAdvance,
};

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

/// The encounter-deck card codes for The Gathering, grouped by the six
/// encounter sets the campaign guide gathers (Night of the Zealot guide
/// p.2: "The Gathering, Rats, Ghouls, Striking Fear, Ancient Evils, and
/// Chilling Cold"). Listed by distinct code; each is pushed at its printed
/// corpus quantity. The set membership comes from the guide because the
/// corpus does not carry `encounter_code`.
///
/// **Set-aside cards are absent by construction:** the Ghoul Priest
/// (`01116`) and Lita Chantler (`01117`) are set aside, and the scenario's
/// structural cards (reference `01104`, acts, agendas, locations) are not
/// encounter cards — none appear here.
const ENCOUNTER_DECK_CODES: &[&str] = &[
    // The Gathering (own set) — encounter enemies only (Ghoul Priest +
    // Lita are set aside).
    "01118", // Flesh-Eater
    "01119", // Icy Ghoul
    // Rats
    "01159", // Swarm of Rats
    // Ghouls
    "01160", // Ghoul Minion
    "01161", // Ravenous Ghoul
    "01162", // Grasping Hands
    // Striking Fear
    "01163", // Rotting Remains
    "01164", // Frozen in Fear
    "01165", // Dissonant Voices
    // Ancient Evils
    "01166", // Ancient Evils
    // Chilling Cold
    "01167", // Crypt Chill
    "01168", // Obscuring Fog
];

/// Read an encounter card's printed quantity (how many copies the
/// encounter deck holds) from the corpus. Encounter cards are enemies or
/// treacheries; anything else here is a coding error in
/// [`ENCOUNTER_DECK_CODES`].
fn encounter_quantity(code: &str) -> u8 {
    match cards::by_code(code).expect("encounter card in corpus").kind {
        CardKind::Enemy { quantity, .. } | CardKind::Treachery { quantity, .. } => quantity,
        ref k => panic!("{code} is not an encounter card (enemy/treachery): {k:?}"),
    }
}

/// String id used to look this module up in [`crate::REGISTRY`].
pub const ID: &str = "the-gathering";

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

/// Number of Ghoul-trait enemies at the testing investigator's location.
fn ghoul_count_at_investigator_location(cx: &SymbolCtx) -> u8 {
    let Some(loc) = cx.investigator_location() else {
        return 0;
    };
    let n = cx
        .state()
        .enemies
        .values()
        .filter(|e| e.current_location == Some(loc) && e.traits.iter().any(|t| t == "Ghoul"))
        .count();
    u8::try_from(n).unwrap_or(u8::MAX)
}

/// 01104 The Gathering chaos-symbol effects (verified card text):
/// `[skull]` −X (X = Ghouls at your location); `[cultist]` −1, 1 horror
/// on failure; `[tablet]` −2, 1 damage if a Ghoul is at your location.
/// The Gathering's Standard bag has no Elder Thing token.
fn resolve_symbol(token: ChaosToken, cx: &SymbolCtx) -> SymbolOutcome {
    let ghouls = ghoul_count_at_investigator_location(cx);
    match token {
        ChaosToken::Skull => SymbolOutcome {
            modifier: -(i8::try_from(ghouls).unwrap_or(i8::MAX)),
            ..SymbolOutcome::default()
        },
        ChaosToken::Cultist => SymbolOutcome {
            modifier: -1,
            on_fail: vec![TokenEffect::Horror(1)],
            ..SymbolOutcome::default()
        },
        ChaosToken::Tablet => SymbolOutcome {
            modifier: -2,
            immediate: if ghouls > 0 {
                vec![TokenEffect::Damage(1)]
            } else {
                vec![]
            },
            ..SymbolOutcome::default()
        },
        _ => SymbolOutcome::default(),
    }
}

/// Build the initial [`GameState`]: the Study in play (isolated), the
/// four set-aside locations (Hallway/Attic/Cellar/Parlor, pre-connected),
/// the act/agenda decks, the Standard chaos bag, and `starting_location`.
/// No investigators — the `StartScenario` roster step seats them.
pub fn setup() -> GameState {
    let mut state = GameStateBuilder::new()
        .with_chaos_bag(standard_chaos_bag())
        .with_scenario_id(ScenarioId::new(ID))
        .build();

    // The Gathering board. Ids are minted by `add_location` /
    // `add_set_aside_location` (deterministic, construction order), so no
    // hand-assigned LocationId literals. The scenario looks up each
    // location's metadata in the corpus and hands it to the engine; stats
    // (shroud/clues) come from the metadata. The Study starts in play
    // (isolated — Act 1 is "trapped in the Study"); the Hallway hub +
    // Attic/Cellar/Parlor spokes are set aside until Act 1's (01108)
    // Forced on-advance reverse brings them into play.
    let meta = |code: &str| cards::by_code(code).expect("Gathering location in corpus");
    let study = state.add_location(meta("01111"));
    let hallway = state.add_set_aside_location(meta("01112"));
    let attic = state.add_set_aside_location(meta("01113"));
    let cellar = state.add_set_aside_location(meta("01114"));
    let parlor = state.add_set_aside_location(meta("01115"));
    state.connect(hallway, attic);
    state.connect(hallway, cellar);
    state.connect(hallway, parlor);
    state.starting_location = Some(study);

    // The Ghoul Priest (01116) starts set aside; Act 2's (01109) reverse
    // spawns it in the Hallway when the act advances (cards::act_01109).
    // Recorded by code only — its per-investigator health is minted from
    // the corpus at spawn, when the investigator count is known.
    state.add_set_aside_enemy(meta("01116"));

    // Act deck 01108 -> 01109 -> 01110. Clue thresholds read from the
    // corpus. 01110 advances via its Forced EnemyDefeated objective
    // (01116; in cards::act_01110), not a clue spend — its printed clue
    // threshold is null, which the reader maps to 0.
    // Act-2 (01109) reverse — reveals the Parlor and spawns the set-aside
    // Ghoul Priest (01116) in the Hallway — ships in cards::act_01109 (#280).
    // Lita Chantler / the Parlor barrier -> #258.
    // TODO(#281): agenda reverses (01105 discard/horror, 01106 dig-until-Ghoul)
    // + an `AgendaAdvanced` forced point — `advance_agenda` fires no reverse today.
    // TODO(#phase-9): act-3 (01110) reverse is the lead's R1/R2 resolution choice.
    state.act_deck = vec![
        Act {
            code: CardCode("01108".into()),
            clue_threshold: act_clue_threshold("01108"),
            resolution: None,
            round_end_advance: None,
        },
        Act {
            code: CardCode("01109".into()),
            clue_threshold: act_clue_threshold("01109"),
            resolution: None,
            // "When the round ends, investigators in the hallway may, as a
            // group, spend the requisite number of clues to advance." (C3d)
            round_end_advance: Some(RoundEndAdvance {
                contributor_location: CardCode("01112".into()), // the Hallway
            }),
        },
        Act {
            // 01110 advances via its Forced EnemyDefeated objective (01116; in cards::act_01110), not a clue spend.
            code: CardCode("01110".into()),
            clue_threshold: act_clue_threshold("01110"),
            resolution: Some(Resolution::Won { id: "R1".into() }),
            round_end_advance: None,
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

    // Encounter deck: each gathered set's enemy/treachery cards at their
    // printed quantity, in deterministic construction order. `StartScenario`
    // shuffles it with the scenario-start RNG (Rules Reference: the
    // encounter deck is shuffled at setup), so this seeding order isn't
    // load-bearing for play — only for replay determinism before the shuffle.
    for &code in ENCOUNTER_DECK_CODES {
        for _ in 0..encounter_quantity(code) {
            state.encounter_deck.push_back(CardCode(code.into()));
        }
    }

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
    resolve_symbol: Some(resolve_symbol),
    setup,
    apply_resolution,
};

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::card_data::ClueValue;
    use game_core::state::ChaosToken;

    #[test]
    fn setup_reads_card_stats_from_corpus() {
        // The hardcoded literals are gone — these values now come from
        // cards::by_code. Pinning them guards both the corpus data and
        // the reader helpers.
        let s = setup();
        let study = &s.locations[&s.starting_location.unwrap()];
        assert_eq!(study.shroud, 2, "Study 01111 shroud");
        assert_eq!(study.clues, 0, "Study enters unrevealed with no clues");
        assert!(!study.revealed, "Study enters unrevealed");
        assert_eq!(
            study.printed_clues,
            ClueValue::PerInvestigator(2),
            "Study 01111 printed_clues"
        );
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
        let study = &s.locations[&s.starting_location.unwrap()];
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
        assert_eq!(
            s.locations[&s.starting_location.unwrap()].code.as_str(),
            "01111"
        );
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

    /// The encounter deck is the six gathered sets' enemy/treachery cards
    /// at their printed quantities, minus the set-aside Ghoul Priest and
    /// Lita. Sets (campaign guide p.2): The Gathering, Rats, Ghouls,
    /// Striking Fear, Ancient Evils, Chilling Cold.
    #[test]
    fn setup_assembles_encounter_deck_from_the_six_sets() {
        let state = setup();
        let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for code in &state.encounter_deck {
            *counts.entry(code.as_str()).or_default() += 1;
        }
        let expected = [
            ("01118", 1usize),
            ("01119", 1), // The Gathering enemies (Flesh-Eater, Icy Ghoul)
            ("01159", 3), // Rats
            ("01160", 3),
            ("01161", 1),
            ("01162", 3), // Ghouls
            ("01163", 3),
            ("01164", 2),
            ("01165", 2), // Striking Fear
            ("01166", 3), // Ancient Evils
            ("01167", 2),
            ("01168", 2), // Chilling Cold
        ];
        let mut total = 0;
        for (code, qty) in expected {
            assert_eq!(
                counts.get(code).copied().unwrap_or(0),
                qty,
                "count of {code}"
            );
            total += qty;
        }
        assert_eq!(
            state.encounter_deck.len(),
            total,
            "no extra encounter cards"
        );
        // Set-aside cards are NOT shuffled into the encounter deck.
        assert!(
            !state.encounter_deck.contains(&CardCode("01116".into())),
            "Ghoul Priest (01116) is set aside",
        );
        assert!(
            !state.encounter_deck.contains(&CardCode("01117".into())),
            "Lita Chantler (01117) is set aside",
        );
    }
}
