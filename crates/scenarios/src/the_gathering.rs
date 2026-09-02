//! The Gathering (Night of the Zealot, scenario 1) — Slice 1 C1a skeleton.
//!
//! Builds the faithful **Act-1 board**: only the Study is in play (the
//! Hallway/Attic/Cellar/Parlor are set aside (`set_aside_cards`, by code)
//! and enter play via Act 1's (01108) Forced on-advance reverse, which
//! also relocates investigators to the Hallway and removes the Study).
//! Their connections come from [`LAYOUT`], wired as each room enters.
//! `setup()` builds the world; the scenario setup (via `seat_and_open`)
//! seats investigators at the starting location (the Study, `01111`) via
//! `GameState.starting_location`.
//!
//! Faithful where it can be (agenda doom 3/7/10; the verified Standard
//! chaos bag; Study shroud/clues); structural stand-in where the rest of
//! Group C owns fidelity (act 01110 advances via its Forced `EnemyDefeated`
//! objective (01116; in `cards`); symbol-token effects on reference card
//! 01104 are C2). C1a does
//! not claim faithful win/lose semantics — only structural reachability,
//! proven by `tests/the_gathering.rs`.
//!
//! **The two terminal cards carry no resolution point here.** A resolution
//! point is a printed *effect* on a card's reverse, not a datum on the deck
//! entry (ADR 0013), so 01107's `(→R3)` and 01110's `(→R1)`/`(→R2)` live in
//! `cards::theyre_getting_out` and `cards::what_have_you_done`. Both are the
//! last card in their deck, which is the only thing that makes them terminal.
//! Both reverses are faithful now — 01107 branches on the current act (#809)
//! and 01110 asks the lead to choose (#775). What each ending *means* is the
//! remaining gap, recorded on the cards: the trauma, the campaign log, and
//! Lita Chantler at R1 are campaign machinery and stay with #766.

use game_core::card_data::CardKind;
use game_core::event::Event;
use game_core::scenario::{
    LocationLayout, ScenarioEnding, ScenarioId, ScenarioModule, SymbolCtx, SymbolOutcome,
    TokenEffect,
};
use game_core::state::{Act, Agenda, CardCode, ChaosBag, ChaosToken, GameState, GameStateBuilder};

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
/// corpus quantity. The set membership is hand-transcribed from the guide
/// because the *generated* corpus does not carry `encounter_code` — the
/// snapshot JSON does, one hop upstream; the pipeline drops it (ingesting
/// it so scenarios can derive per-set lists is tracked in #579).
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

/// 01104 The Gathering chaos-symbol effects — **Easy/Standard face only**
/// (verified card text): `[skull]` −X (X = Ghouls at your location);
/// `[cultist]` −1, 1 horror on failure; `[tablet]` −2, 1 damage if a Ghoul
/// is at your location. The Gathering's Standard bag has no Elder Thing
/// token. The Hard/Expert back (skull −2; cultist "reveal another token";
/// tablet −4, 1 damage AND 1 horror) is unimplemented — when difficulty
/// selection lands (`TODO(#862)`) this fn needs a difficulty parameter, and
/// the Hard/Expert cultist needs a token-re-draw shape `SymbolOutcome` can't
/// express yet.
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

/// The Gathering's location layout: the Hallway (01112) is the hub, with
/// the Attic (01113), Cellar (01114) and Parlor (01115) as spokes. Read
/// from the connection symbols printed on the location cards — the pinned
/// snapshot carries no connection field, so the board's topology is the
/// scenario's to own (see [`LocationLayout`]).
///
/// The Study (01111) appears in no pair: Act 1 is *"trapped in the
/// Study"*, and the act's reverse removes it from the game when the rest
/// of the house arrives.
pub const LAYOUT: LocationLayout = &[("01112", "01113"), ("01112", "01114"), ("01112", "01115")];

/// Build the initial [`GameState`]: the Study in play (isolated), the
/// four set-aside rooms (Hallway/Attic/Cellar/Parlor, by code), the
/// set-aside Ghoul Priest, the act/agenda decks, the Standard chaos bag,
/// and `starting_location`.
/// No investigators — they are seated via `seat_and_open` at scenario setup.
///
/// # Panics
///
/// Panics if any of The Gathering's location/act/agenda codes is missing from
/// the card corpus — a build-time invariant (the scenario and the pinned
/// snapshot ship together).
pub fn setup() -> GameState {
    let mut state = GameStateBuilder::new()
        .with_chaos_bag(standard_chaos_bag())
        .with_scenario_id(ScenarioId::new(ID))
        .build();

    // The Gathering board. Ids are minted by `add_location` (deterministic,
    // construction order), so no hand-assigned LocationId literals. The
    // scenario looks up each card's metadata in the corpus and hands it to
    // the engine; stats (shroud/clues) come from the metadata. The Study
    // starts in play (isolated — Act 1 is "trapped in the Study").
    let meta = |code: &str| cards::by_code(code).expect("Gathering card in corpus");
    let study = state.add_location(meta("01111"));
    state.starting_location = Some(study);

    // Set aside by code only, in one zone: the Hallway hub +
    // Attic/Cellar/Parlor spokes until Act 1's (01108) Forced on-advance
    // reverse puts them into play (each minting its id and wiring LAYOUT
    // as it enters), then the Ghoul Priest (01116) until Act 2's (01109)
    // reverse spawns it in the Hallway (cards::the_barrier), where its
    // per-investigator health is minted with the investigator count known,
    // then Lita Chantler (01117), whom the same reverse puts into play in the
    // Parlor under no investigator's control (#771/#772).
    for code in ["01112", "01113", "01114", "01115", "01116", "01117"] {
        state.add_set_aside_card(meta(code));
    }

    // Act deck 01108 -> 01109 -> 01110. Clue thresholds read from the
    // corpus. 01110 advances via its Forced EnemyDefeated objective
    // (01116; in cards::what_have_you_done), not a clue spend — its printed
    // clue threshold is null, which the reader maps to 0.
    // Act-2 (01109) reverse — reveals the Parlor and spawns the set-aside
    // Ghoul Priest (01116) in the Hallway — ships in cards::the_barrier (#280).
    // Lita Chantler / the Parlor barrier -> #258.
    // Agenda reverses (01105 discard/horror, 01106 dig-until-Ghoul) ship as
    // the agendas' own `AgendaAdvanced` forced abilities (cards::whats_going_on,
    // cards::rise_of_the_ghouls); #281.
    state.act_deck = vec![
        Act {
            code: CardCode("01108".into()),
            clue_threshold: act_clue_threshold("01108"),
        },
        Act {
            code: CardCode("01109".into()),
            clue_threshold: act_clue_threshold("01109"),
            // "When the round ends, investigators in the hallway may, as a
            // group, spend the requisite number of clues to advance." (C3d)
        },
        Act {
            // 01110 advances via its Forced EnemyDefeated objective (01116; in cards::what_have_you_done), not a clue spend.
            // Terminal because it is last here; its reverse — which asks the
            // lead to choose between R1 and R2 — is an ability on the card
            // (cards::what_have_you_done).
            code: CardCode("01110".into()),
            clue_threshold: act_clue_threshold("01110"),
        },
    ];

    // Agenda deck 01105 -> 01106 -> 01107. Doom thresholds read from the
    // corpus. 01107 is terminal because it is last; its reverse branches on
    // the current act: (→R3) at acts 1-2, and no resolution point at act 3
    // (cards::theyre_getting_out).
    state.agenda_deck = vec![
        Agenda {
            code: CardCode("01105".into()),
            doom_threshold: agenda_doom("01105"),
        },
        Agenda {
            code: CardCode("01106".into()),
            doom_threshold: agenda_doom("01106"),
        },
        Agenda {
            code: CardCode("01107".into()),
            doom_threshold: agenda_doom("01107"),
        },
    ];

    // Encounter deck: each gathered set's enemy/treachery cards at their
    // printed quantity, in deterministic construction order. The scenario
    // setup (via `seat_and_open`) shuffles it with the scenario-start RNG
    // (Rules Reference: the encounter deck is shuffled at setup), so this
    // seeding order isn't load-bearing for play — only for replay determinism
    // before the shuffle.
    for &code in ENCOUNTER_DECK_CODES {
        for _ in 0..encounter_quantity(code) {
            state.encounter_deck.push_back(CardCode(code.into()));
        }
    }

    state
}

/// No-op for C1a (matches the synthetic fixture). XP / trauma / campaign
/// log application is Phase 9.
pub fn apply_resolution(_ending: ScenarioEnding, _state: &mut GameState, _events: &mut Vec<Event>) {
}

/// The [`ScenarioModule`] value for The Gathering.
pub const MODULE: ScenarioModule = ScenarioModule {
    resolve_symbol: Some(resolve_symbol),
    setup,
    apply_resolution,
    layout: LAYOUT,
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
    fn setup_places_study_in_play_and_the_rest_set_aside_by_code() {
        let s = setup();
        // In play: only the Study (Act-1 board).
        assert_eq!(s.locations.len(), 1);
        let study = &s.locations[&s.starting_location.unwrap()];
        assert_eq!(study.code, CardCode("01111".into()));
        assert!(study.connections.is_empty(), "Study is isolated");
        // Set aside: the four rooms, the Ghoul Priest and Lita Chantler,
        // codes only. No LocationId is minted for a room until it enters play,
        // and no CardInstanceId for Lita until she is put into play (#771).
        assert_eq!(
            s.set_aside_cards
                .iter()
                .map(|c| c.as_str().to_owned())
                .collect::<Vec<_>>(),
            ["01112", "01113", "01114", "01115", "01116", "01117"],
        );
        assert_eq!(
            s.location_ids.peek(),
            1,
            "only the Study has minted an id at setup",
        );
        assert!(s.investigators.is_empty(), "setup() seats no one");
    }

    #[test]
    fn layout_is_the_hallway_hub_and_its_three_spokes() {
        // The printed connection symbols: Hallway (01112) to each of the
        // Attic (01113), Cellar (01114) and Parlor (01115). The Study
        // (01111) is in no pair — it is removed from the game when the
        // rest of the house arrives.
        assert_eq!(
            LAYOUT,
            [("01112", "01113"), ("01112", "01114"), ("01112", "01115")],
        );
        assert!(
            !LAYOUT.iter().any(|(a, b)| *a == "01111" || *b == "01111"),
            "the Study connects to nothing",
        );
    }

    #[test]
    fn act_three_advances_on_objective_not_clues() {
        let s = setup();
        assert_eq!(s.act_deck[2].code.as_str(), "01110");
        assert_eq!(
            s.act_deck[2].clue_threshold, 0,
            "01110 advances on Ghoul-Priest-defeat, not clues"
        );
        assert_eq!(
            s.act_deck.len(),
            3,
            "01110 is terminal because it is last, not because it carries a flag"
        );
    }

    #[test]
    fn setup_seeds_act_and_agenda_decks_ending_in_their_terminal_cards() {
        let s = setup();
        let act_codes: Vec<_> = s.act_deck.iter().map(|a| a.code.as_str()).collect();
        assert_eq!(act_codes, ["01108", "01109", "01110"]);
        assert_eq!(s.act_deck[0].clue_threshold, 2);
        assert_eq!(s.act_deck[1].clue_threshold, 3);
        // 01110 is last, which is the only thing that makes it terminal
        // (ADR 0013). Which resolution point its reverse reaches — the campaign
        // guide's Resolution 1, the house burning — is the card's own assertion,
        // in `cards::what_have_you_done`.

        let agenda_codes: Vec<_> = s.agenda_deck.iter().map(|a| a.code.as_str()).collect();
        assert_eq!(agenda_codes, ["01105", "01106", "01107"]);
        assert_eq!(
            s.agenda_deck
                .iter()
                .map(|a| a.doom_threshold)
                .collect::<Vec<_>>(),
            [3, 7, 10]
        );
        // 01107 is last, and so terminal. Its reverse reaching the campaign
        // guide's Resolution 3 ("Trapped, the horde of feral creatures ...
        // close in") is asserted on the card, in `cards::theyre_getting_out`.
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
