//! The Gathering (Night of the Zealot, scenario 1): its setup, its chaos bag,
//! and the symbol tokens of its reference card, 01104.
//!
//! `setup()` builds the **Act-1 board**. Only the Study (01111) is in play,
//! and the scenario setup (`seat_and_open`) seats the investigators there, at
//! `GameState.starting_location`. The Hallway, Attic, Cellar and Parlor are
//! set aside by code (`set_aside_cards`). They enter play through act 01108's
//! Forced on-advance reverse, which also places each investigator in the
//! Hallway and removes the Study. Their connections come from [`LAYOUT`],
//! wired as each room enters. The agenda doom and act clue thresholds are read
//! from the corpus, and the chaos bag is the verified Standard bag.
//!
//! `resolve_symbol` implements 01104's Easy / Standard face. Its doc carries
//! the Hard / Expert face, which waits on difficulty selection.
//!
//! **The two terminal cards carry no resolution point here.** A resolution
//! point is a printed *effect* on a card's reverse, not a datum on the deck
//! entry (ADR 0013), so 01107's `(→R3)` and 01110's `(→R1)`/`(→R2)` live in
//! `cards::theyre_getting_out` and `cards::what_have_you_done`. Both are the
//! last card in their deck, which is the only thing that makes them terminal.
//! 01107's reverse branches on the current act, and 01110's offers its choice
//! to the lead investigator.
//!
//! TODO(#766): apply what each ending means. The trauma, the campaign log, and
//! Lita Chantler at R1 are campaign machinery, and the cards record each gap.

use card_dsl::card_data::CardKind;
use card_dsl::dsl::{self, IntExpr, InvestigatorTarget, TokenOutcome};
use cards::generated::traits::GHOUL;
use cards::impls::{
    ancient_evils, attic, cellar, crypt_chill, dissonant_voices, frozen_in_fear, grasping_hands,
    lita_chantler, obscuring_fog, parlor, rise_of_the_ghouls, rotting_remains, the_barrier,
    theyre_getting_out, trapped, what_have_you_done, whats_going_on,
};
use game_core::event::Event;
use game_core::scenario::{LocationLayout, ScenarioEnding, ScenarioId, ScenarioModule, SymbolCtx};
use game_core::state::{Act, Agenda, CardCode, ChaosBag, ChaosToken, GameState, GameStateBuilder};

/// The encounter-deck card codes for The Gathering, grouped by the six
/// encounter sets the campaign guide gathers (Night of the Zealot guide
/// p.2: "The Gathering, Rats, Ghouls, Striking Fear, Ancient Evils, and
/// Chilling Cold"). Listed by distinct code; each is pushed at its printed
/// corpus quantity. The set membership is hand-transcribed from the guide
/// because the *generated* corpus does not carry `encounter_code` — the
/// snapshot JSON does, one hop upstream; the pipeline drops it. TODO(#579):
/// ingest `encounter_code` so scenarios can derive per-set lists.
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
    grasping_hands::CODE,
    // Striking Fear
    rotting_remains::CODE,
    frozen_in_fear::CODE,
    dissonant_voices::CODE,
    // Ancient Evils
    ancient_evils::CODE,
    // Chilling Cold
    crypt_chill::CODE,
    obscuring_fog::CODE,
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

/// The Hallway (01112). It has no card module yet, so its code is named here
/// rather than taken from one.
const HALLWAY: &str = "01112";

/// The verified Standard-difficulty Night of the Zealot chaos bag (16
/// tokens). Source: `data/campaign-guides/SOURCE.md` (campaign guide
/// p.1, "Assemble the campaign chaos bag", Standard).
fn standard_chaos_bag() -> ChaosBag {
    ChaosBag::new([
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
        .filter(|e| e.current_location == Some(loc) && cards::has_trait(e.code.as_str(), GHOUL))
        .count();
    u8::try_from(n).unwrap_or(u8::MAX)
}

/// 01104 The Gathering chaos-symbol effects — **Easy/Standard face only**
/// (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
///
/// > \[skull\] -X. X is the number of \[\[Ghoul\]\] enemies at your location.
/// > \[cultist\] -1. If you fail, take 1 horror.
/// > \[tablet\] -2. If there is a \[\[Ghoul\]\] enemy at your location, take 1 damage.
///
/// Each symbol is a [`TokenOutcome`]: the cultist's horror is its `on_fail`
/// part and the tablet's damage its `immediate` part, both plain
/// [`Effect::Deal`]s on the tester. The hook runs once at reveal, so the
/// tablet's Ghoul gate and the skull's X are decided then; every modifier is
/// an `IntExpr::Lit` computed at reveal. The Gathering's Standard bag has no
/// Elder Thing token.
///
/// TODO(#862): the Hard/Expert face is not modelled. Its text, from the same
/// snapshot entry's `back_text`:
///
/// > \[skull\] -2. If you fail, after this skill test, search the encounter deck and discard pile for a \[\[Ghoul\]\] enemy, and draw it. Shuffle the encounter deck.
/// > \[cultist\] Reveal another token. If you fail, take 2 horror.
/// > \[tablet\] -4. If there is a \[\[Ghoul\]\] enemy at your location, take 1 damage and 1 horror.
///
/// Difficulty selection gives this fn a difficulty parameter, and the
/// Hard/Expert cultist needs a token-re-draw shape `TokenOutcome` can't
/// express.
///
/// [`Effect::Deal`]: card_dsl::dsl::Effect::Deal
fn resolve_symbol(token: ChaosToken, cx: &SymbolCtx) -> TokenOutcome {
    let ghouls = ghoul_count_at_investigator_location(cx);
    match token {
        // TODO(#1138): the live-read candidate. X is frozen at reveal here; once
        // a `Quantity` can count the Ghouls at the tester's location, the
        // modifier becomes an unevaluated count that ST.5 reads live.
        ChaosToken::Skull => {
            TokenOutcome::modifier(IntExpr::Lit(-(i8::try_from(ghouls).unwrap_or(i8::MAX))))
        }
        ChaosToken::Cultist => TokenOutcome {
            on_fail: Some(dsl::deal_horror(InvestigatorTarget::You, 1u8)),
            ..TokenOutcome::modifier(IntExpr::Lit(-1))
        },
        ChaosToken::Tablet => TokenOutcome {
            immediate: (ghouls > 0).then(|| dsl::deal_damage(InvestigatorTarget::You, 1u8)),
            ..TokenOutcome::modifier(IntExpr::Lit(-2))
        },
        _ => TokenOutcome::default(),
    }
}

/// The Gathering's location layout: the Hallway (01112) is the hub, with the
/// Attic (01113), Cellar (01114) and Parlor (01115) as spokes.
///
/// Read from the connection symbols printed on the location cards — the pinned
/// snapshot carries no connection field, so the board's topology is the
/// scenario's to own (see [`LocationLayout`]).
///
/// The Study (01111) appears in no pair: Act 1 is *"trapped in the
/// Study"*, and the act's reverse removes it from the game when the rest
/// of the house arrives.
pub const LAYOUT: LocationLayout = &[
    (HALLWAY, attic::CODE),
    (HALLWAY, cellar::CODE),
    (HALLWAY, parlor::CODE),
];

/// Build The Gathering's initial [`GameState`], with no investigators seated.
///
/// The state holds the Study in play (isolated), the four set-aside rooms
/// (Hallway/Attic/Cellar/Parlor, by code), the set-aside Ghoul Priest, the
/// act/agenda decks, the Standard chaos bag, and `starting_location`. The
/// investigators are seated via `seat_and_open` at scenario setup.
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
    // Parlor under no investigator's control.
    for code in [
        HALLWAY,
        attic::CODE,
        cellar::CODE,
        parlor::CODE,
        "01116",
        lita_chantler::CODE,
    ] {
        state.add_set_aside_card(meta(code));
    }

    // Act deck 01108 -> 01109 -> 01110. Clue thresholds read from the
    // corpus by `Act::from_metadata`. 01109 advances on its objective, *"When
    // the round ends, investigators in the hallway may, as a group, spend the
    // requisite number of clues to advance."* 01110 advances via its Forced
    // EnemyDefeated objective (01116; in cards::what_have_you_done), not a
    // clue spend — its printed clue threshold is null, which reads as 0. It
    // is terminal because it is last here; its reverse — *"The lead
    // investigator must decide (choose one):"* between R1 and R2 — is an
    // ability on the card (cards::what_have_you_done).
    // Act-2 (01109) reverse — reveals the Parlor, puts Lita Chantler (01117)
    // into play there and spawns the set-aside Ghoul Priest (01116) in the
    // Hallway — is cards::the_barrier. The Parlor's barrier is its own
    // unrevealed back (cards::parlor).
    // Agenda reverses (01105 discard/horror, 01106 dig-until-Ghoul) are
    // the agendas' own `AgendaAdvanced` forced abilities (cards::whats_going_on,
    // cards::rise_of_the_ghouls).
    state.act_deck = [trapped::CODE, the_barrier::CODE, what_have_you_done::CODE]
        .map(|code| Act::from_metadata(meta(code)).expect("Gathering act code is an act"))
        .into();

    // Agenda deck 01105 -> 01106 -> 01107. Doom thresholds read from the
    // corpus. 01107 is terminal because it is last; its reverse branches on
    // the current act: (→R3) at acts 1-2, and no resolution point at act 3
    // (cards::theyre_getting_out).
    state.agenda_deck = [
        whats_going_on::CODE,
        rise_of_the_ghouls::CODE,
        theyre_getting_out::CODE,
    ]
    .map(|code| Agenda::from_metadata(meta(code)).expect("Gathering agenda code is an agenda"))
    .into();

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

/// A no-op: no ending records anything yet. TODO(#766): apply the
/// resolutions' trauma, campaign log and Lita Chantler.
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
    use std::collections::BTreeMap;

    use card_dsl::card_data::ClueValue;

    use super::*;

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
        // and no CardInstanceId for Lita until she is put into play.
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
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
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
