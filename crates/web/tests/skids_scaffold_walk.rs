//! "Skids" O'Toole 01003, seated with the picker's scaffold deck, plays The
//! Gathering to a resolution (#1079) — the acceptance line of #778, *"Skids is
//! selectable in the picker and playable through The Gathering."* The deck the
//! walk seats is [`SKIDS_DEFAULT_DECK`] itself, which is why the walk lives in
//! the crate that owns it.
//!
//! The walk mirrors solo Roland's in
//! `crates/scenarios/tests/the_gathering_resolutions.rs` and Agnes's in
//! `agnes_scaffold_walk.rs`, against the production registries (the real
//! corpus and the real scenario modules): act 1 advances on spent clues, act 2
//! through its round-end clue-spend window, whose reverse spawns the real Ghoul
//! Priest 01116, and the Priest's defeat advances the terminal act to its
//! printed choice. Roland lands the killing blow with a basic Fight at combat 4
//! against the Priest's fight 4; Skids's combat is 3, so on the walk's
//! `Numeric(0)` bag a basic Fight fails. He lands it with Backstab 01051
//! instead:
//!
//! > <b>Fight.</b> This attack uses [agility] instead of [combat]. This attack
//! > deals +2 damage.
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
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameState, InvestigatorId, Phase, SkillKind,
};
use game_core::test_support::TestSession;
use scenarios::the_gathering;
use web::scaffold_decks::SKIDS_DEFAULT_DECK;

/// What the server passes to every `apply`: the real corpus and the real
/// scenario modules, with no synthetic test cards composed in.
const REGISTRIES: Registries = Registries {
    cards: &cards::REGISTRY,
    scenarios: &scenarios::REGISTRY,
};

const SKIDS: &str = "01003";
const INV: InvestigatorId = InvestigatorId(1);
/// The Ghoul Priest, set aside until act 2 (01109)'s reverse spawns it.
const GHOUL_PRIEST: &str = "01116";
const BACKSTAB: &str = "01051";

/// Every player card #778 shipped, as printed in
/// `data/arkhamdb-snapshot/pack/core/core.json`: Skids's two signatures and the
/// ten Rogue level-0 cards.
const SLICE_CARDS: [&str; 12] = [
    "01010", // On the Lam (signature)
    "01011", // Hospital Debts (signature weakness)
    "01044", // Switchblade
    "01045", // Burglary
    "01046", // Pickpocketing
    "01047", // .41 Derringer
    "01048", // Leo De Luca
    "01049", // Hard Knocks
    "01050", // Elusive
    "01051", // Backstab
    "01052", // Sneak Attack
    "01053", // Opportunist
];

/// How many copies of each code the scaffold deck holds.
fn copies() -> BTreeMap<&'static str, usize> {
    let mut copies = BTreeMap::new();
    for code in SKIDS_DEFAULT_DECK {
        *copies.entry(*code).or_default() += 1;
    }
    copies
}

#[test]
fn the_scaffold_deck_holds_every_card_the_skids_slice_shipped() {
    let copies = copies();
    let missing: Vec<_> = SLICE_CARDS
        .iter()
        .filter(|code| !copies.contains_key(*code))
        .collect();
    assert!(missing.is_empty(), "missing from the deck: {missing:?}");
}

/// Skids's deckbuilding requirements name one copy of each signature:
/// *"On the Lam, Hospital Debts, 1 random basic weakness."*
#[test]
fn the_scaffold_deck_holds_one_copy_of_each_signature() {
    let copies = copies();
    assert_eq!(copies.get("01010"), Some(&1), "On the Lam");
    assert_eq!(copies.get("01011"), Some(&1), "Hospital Debts");
}

#[test]
fn every_card_in_the_scaffold_deck_is_playable() {
    let unplayable: Vec<_> = SKIDS_DEFAULT_DECK
        .iter()
        .filter(|code| !cards::is_playable(code))
        .collect();
    assert!(unplayable.is_empty(), "not playable: {unplayable:?}");
}

/// The Gathering set up and solo Skids seated from the corpus with the scaffold
/// deck, past the mulligan and at his first turn menu.
///
/// Two stand-ins, as in Roland's and Agnes's walks: a single-token `Numeric(0)`
/// chaos bag (production serves the random Standard bag), and an encounter deck
/// of one Ancient Evils 01166, whose Revelation only places 1 doom, so round
/// 2's Mythos draw is deterministic and cannot advance the agenda. The player
/// deck is the real one, and the fixed `rng` seed `setup()` builds with makes
/// its shuffle deterministic: the walk below scripts the windows that shuffle's
/// hand opens.
fn seated_skids() -> GameState {
    let mut state = the_gathering::setup();
    state.chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)]);
    state.encounter_deck.clear();
    state.encounter_deck.push_back(CardCode::new("01166"));

    let roster = vec![RosterEntry {
        investigator: CardCode::new(SKIDS),
        deck: SKIDS_DEFAULT_DECK
            .iter()
            .map(|c| CardCode::new(*c))
            .collect(),
    }];
    let started = engine::seat_and_open(state, &roster, REGISTRIES);
    let session = TestSession::new(started.state, REGISTRIES).choose_from_hand(&[]);
    let session = pass_start_of_turn_window(session);
    session.assert_at_turn_menu(INV);
    session.finish().state
}

/// His turn opens on the start-of-turn fast window (Appendix II 2.2), where his
/// own *"[fast] During your turn, spend 2 resources: You may take an additional
/// action this turn."* is on offer; the walk passes it.
fn pass_start_of_turn_window(session: TestSession) -> TestSession {
    let skids = session.state().investigators[&INV].card_anchor();
    assert_eq!(
        session.offered_at(&skids),
        1,
        "his turn opens offering his ability, got {:?}",
        session.prompt(),
    );
    session.skip()
}

#[test]
fn skids_with_his_scaffold_deck_plays_the_gathering_to_a_resolution() {
    let mut state = seated_skids();
    // Seed: clues for both act thresholds (act 1 = 2, act 2 = 3), as Roland's
    // walk does — discovering them is covered elsewhere.
    state
        .investigators
        .get_mut(&INV)
        .expect("Skids seated")
        .clues = 5;

    // --- Act 1: spend clues to advance; the reverse builds the board and moves
    // Skids to the Hallway.
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

    // Round 2's Mythos draws the seeded Ancient Evils, which resolves before
    // his turn, and his turn opens on the same fast window as round 1's.
    let investigation = pass_start_of_turn_window(after_spend.confirm());
    investigation.assert_at_turn_menu(INV);
    assert_eq!(investigation.state().round, 2);

    // --- Seed the spawned Priest's health and engagement, as Roland's walk
    // does, but three damage from defeat: Backstab's 1 + 2 lands it exactly,
    // where a basic hit's 1 would not. And seed Backstab at the front of his
    // hand, since the deck's shuffle has not dealt it by now; his 5 starting
    // resources plus the upkeep's 1 pay its cost of 3.
    let investigation = investigation.edit_state(|state| {
        let priest = state
            .enemies
            .values_mut()
            .find(|e| e.code.as_str() == GHOUL_PRIEST)
            .expect("act 2's reverse spawned the Ghoul Priest");
        priest.damage = priest.max_health - 3;
        priest.engaged_with = Some(INV);
        state
            .investigators
            .get_mut(&INV)
            .expect("Skids seated")
            .hand
            .insert(0, CardCode::new(BACKSTAB));
    });

    // --- Backstab: agility 4 + Numeric(0) against the Priest's fight 4
    // succeeds, and its 3 damage defeats the Priest. His `[fast]` ability keeps
    // the test's player windows open, as in his own card tests: pass them.
    let defeated = investigation
        .take(&TurnAction::PlayCard {
            investigator: INV,
            hand_index: 0,
        })
        .skip()
        .commit(&[])
        .skip();
    assert_event!(
        defeated.events(),
        Event::SkillTestStarted {
            skill: SkillKind::Agility,
            difficulty: 4,
            ..
        }
    );
    assert_event!(defeated.events(), Event::EnemyDamaged { amount: 3, .. });
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
