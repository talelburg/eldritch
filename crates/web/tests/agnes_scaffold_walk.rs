//! Agnes Baker 01004, seated with the picker's scaffold deck, plays The
//! Gathering to a resolution (#1058) — the acceptance line of #777, *"Agnes is
//! selectable in the picker and playable through The Gathering."* The deck the
//! walk seats is [`AGNES_DEFAULT_DECK`] itself, which is why the walk lives in
//! the crate that owns it.
//!
//! The walk mirrors solo Roland's in
//! `crates/scenarios/tests/the_gathering_resolutions.rs`, against the production
//! registries (the real corpus and the real scenario modules): act 1 advances on
//! spent clues, act 2 through its round-end clue-spend window, whose reverse
//! spawns the real Ghoul Priest 01116, and the Priest's defeat advances the
//! terminal act to its printed choice. Where Roland lands the killing blow with
//! a Fight, Agnes lands it with her own reaction:
//!
//! > [reaction] After 1 or more horror is placed on Agnes Baker: Deal 1 damage
//! > to an enemy at your location. (Limit once per phase.)
//!
//! Host-only: the browser tests in this directory are wasm32-only, and this one
//! needs no DOM.
#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;

use game_core::action::RosterEntry;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{self, OptionTarget, Registries};
use game_core::event::Event;
use game_core::scenario::{ResolutionId, ScenarioEnding};
use game_core::state::{CardCode, ChaosBag, ChaosToken, GameState, InvestigatorId, Phase};
use game_core::test_support::TestSession;
use scenarios::the_gathering;
use web::scaffold_decks::AGNES_DEFAULT_DECK;

/// What the server passes to every `apply`: the real corpus and the real
/// scenario modules, with no synthetic test cards composed in.
const REGISTRIES: Registries = Registries {
    cards: &cards::REGISTRY,
    scenarios: &scenarios::REGISTRY,
};

const AGNES: &str = "01004";
const INV: InvestigatorId = InvestigatorId(1);
/// The Ghoul Priest, set aside until act 2 (01109)'s reverse spawns it.
const GHOUL_PRIEST: &str = "01116";

/// Every player card #777 shipped, as printed in
/// `data/arkhamdb-snapshot/pack/core/core.json`: Agnes's two signatures, the
/// nine Mystic level-0 cards, and Holy Rosary 01059, which shipped before it.
const SLICE_CARDS: [&str; 12] = [
    "01012", // Heirloom of Hyperborea (signature)
    "01013", // Dark Memory (signature weakness)
    "01058", // Forbidden Knowledge
    "01059", // Holy Rosary
    "01060", // Shrivelling
    "01061", // Scrying
    "01062", // Arcane Studies
    "01063", // Arcane Initiate
    "01064", // Drawn to the Flame
    "01065", // Ward of Protection
    "01066", // Blinding Light
    "01067", // Fearless
];

/// How many copies of each code the scaffold deck holds.
fn copies() -> BTreeMap<&'static str, usize> {
    let mut copies = BTreeMap::new();
    for code in AGNES_DEFAULT_DECK {
        *copies.entry(*code).or_default() += 1;
    }
    copies
}

#[test]
fn the_scaffold_deck_holds_every_card_the_agnes_slice_shipped() {
    let copies = copies();
    let missing: Vec<_> = SLICE_CARDS
        .iter()
        .filter(|code| !copies.contains_key(*code))
        .collect();
    assert!(missing.is_empty(), "missing from the deck: {missing:?}");
}

/// Agnes's deckbuilding requirements name one copy of each signature:
/// *"Heirloom of Hyperborea, Dark Memory, 1 random basic weakness."*
#[test]
fn the_scaffold_deck_holds_one_copy_of_each_signature() {
    let copies = copies();
    assert_eq!(copies.get("01012"), Some(&1), "Heirloom of Hyperborea");
    assert_eq!(copies.get("01013"), Some(&1), "Dark Memory");
}

#[test]
fn every_card_in_the_scaffold_deck_is_playable() {
    let unplayable: Vec<_> = AGNES_DEFAULT_DECK
        .iter()
        .filter(|code| !cards::is_playable(code))
        .collect();
    assert!(unplayable.is_empty(), "not playable: {unplayable:?}");
}

/// The Gathering set up and solo Agnes seated from the corpus with the scaffold
/// deck, past the mulligan and at her first turn menu.
///
/// Two stand-ins, as in Roland's walk: a single-token `Numeric(0)` chaos bag
/// (production serves the random Standard bag), and an encounter deck of one
/// Ancient Evils 01166, whose Revelation only places 1 doom, so round 2's Mythos
/// draw is deterministic and cannot advance the agenda. The player deck is the
/// real one, and the fixed `rng` seed `setup()` builds with makes its shuffle
/// deterministic: the walk below scripts the windows that shuffle's hand opens.
fn seated_agnes() -> GameState {
    let mut state = the_gathering::setup();
    state.chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)]);
    state.encounter_deck.clear();
    state.encounter_deck.push_back(CardCode::new("01166"));

    let roster = vec![RosterEntry {
        investigator: CardCode::new(AGNES),
        deck: AGNES_DEFAULT_DECK
            .iter()
            .map(|c| CardCode::new(*c))
            .collect(),
        random_basic_weaknesses: 0,
    }];
    let started = engine::seat_and_open(state, &roster, REGISTRIES);
    let session = TestSession::new(started.state, REGISTRIES).choose_from_hand(&[]);
    session.assert_at_turn_menu(INV);
    session.finish().state
}

#[test]
fn agnes_with_her_scaffold_deck_plays_the_gathering_to_a_resolution() {
    let mut state = seated_agnes();
    // Seed: clues for both act thresholds (act 1 = 2, act 2 = 3), as Roland's
    // walk does — discovering them is covered elsewhere.
    state
        .investigators
        .get_mut(&INV)
        .expect("Agnes seated")
        .clues = 5;

    // --- Act 1: spend clues to advance; the reverse builds the board and moves
    // Agnes to the Hallway.
    let session =
        TestSession::new(state, REGISTRIES).take(&TurnAction::AdvanceAct { investigator: INV });
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().act_index, 1, "act 1 advanced to act 2");

    // --- Act 2: ending the round opens its round-end clue-spend window; the
    // spend advances it and its reverse spawns the Ghoul Priest in the Hallway.
    let round_end = session.take(&TurnAction::EndTurn);
    assert_eq!(round_end.state().phase, Phase::Upkeep);
    assert_eq!(
        round_end.offered_at(&OptionTarget::Act),
        1,
        "the act-2 round-end window, got {:?}",
        round_end.prompt(),
    );
    let after_spend = round_end.pick(OptionTarget::Act);
    assert_eq!(after_spend.state().act_index, 2, "act 2 advanced to act 3");

    // Round 2's Mythos draws the seeded Ancient Evils. The deck's shuffle put
    // Ward of Protection 01065 in her hand, so the draw offers it (*"Play when
    // you draw a non-weakness treachery card."*); she passes, and the treachery
    // resolves before her turn.
    let drawn = after_spend.confirm();
    let ward = OptionTarget::HandCardByCode {
        investigator: INV,
        code: CardCode::new("01065"),
    };
    assert_eq!(
        drawn.offered_at(&ward),
        1,
        "Ward of Protection is offered on the treachery draw, got {:?}",
        drawn.prompt(),
    );
    let investigation = drawn.skip();
    investigation.assert_at_turn_menu(INV);
    assert_eq!(investigation.state().round, 2);

    // --- Seed only the spawned Priest's health and engagement, as Roland's walk
    // does: one damage from defeat, engaged with Agnes in the Hallway.
    let investigation = investigation.edit_state(|state| {
        let priest = state
            .enemies
            .values_mut()
            .find(|e| e.code.as_str() == GHOUL_PRIEST)
            .expect("act 2's reverse spawned the Ghoul Priest");
        priest.damage = priest.max_health - 1;
        priest.engaged_with = Some(INV);
    });

    // --- The Enemy phase: the Priest attacks, horror is placed on Agnes, and
    // her reaction deals the 1 damage that defeats it.
    let attacked = investigation.take(&TurnAction::EndTurn);
    let agnes = attacked.state().investigators[&INV].card_anchor();
    assert_eq!(
        attacked.offered_at(&agnes),
        1,
        "the Priest's horror offers Agnes's reaction, got {:?}",
        attacked.prompt(),
    );
    let defeated = attacked.pick(agnes);
    assert_event!(defeated.events(), Event::EnemyDefeated { .. });
    assert_event!(defeated.events(), Event::ActAdvanced { from } if *from == 2);

    // Act 3's printed choice; its first bullet reaches Resolution 1.
    assert!(
        defeated.state().ending.is_none(),
        "nothing latches before the pick"
    );
    let result = defeated.pick_nth(0).finish();
    assert_eq!(
        result.state.ending,
        Some(ScenarioEnding::Resolution(ResolutionId::new(1))),
        "act 01110's reverse reaches (→R1)",
    );
}
