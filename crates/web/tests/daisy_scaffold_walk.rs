//! Daisy Walker 01002, seated with the picker's scaffold deck, plays The
//! Gathering to a resolution — the acceptance line of #1089, *"Daisy is
//! selectable in the picker (`Pick::Daisy`) with a scaffold deck built from her
//! level-0 Seeker pool, and a scaffold walk test plays her through The
//! Gathering."* The deck the walk seats is [`DAISY_DEFAULT_DECK`] itself, which
//! is why the walk lives in the crate that owns it.
//!
//! The walk mirrors solo Roland's in
//! `crates/scenarios/tests/the_gathering_resolutions.rs` and Skids's in
//! `skids_scaffold_walk.rs`, against the production registries (the real
//! corpus and the real scenario modules): act 1 advances on spent clues, act 2
//! through its round-end clue-spend window, whose reverse spawns the real Ghoul
//! Priest 01116, and the Priest's defeat advances the terminal act to its
//! printed choice. Roland lands the killing blow with a basic Fight at combat 4
//! against the Priest's fight 4; Daisy's combat is 2, so on the walk's
//! `Numeric(0)` bag a basic Fight fails. Her round-1 upkeep draws Knife 01086,
//! which she plays and then discards for its second ability:
//!
//! > [action] Discard Knife: **Fight.** You get +2 [combat] for this attack.
//! > This attack deals +1 damage.
//!
//! Combat 2 + 2 meets the Priest's fight 4.
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
    AbilityAddress, AbilitySource, CardCode, ChaosBag, ChaosToken, GameState, InvestigatorId, Phase,
};
use game_core::test_support::{SeatLookup as _, TestSession};
use scenarios::the_gathering;
use web::scaffold_decks::DAISY_DEFAULT_DECK;

/// What the server passes to every `apply`: the real corpus and the real
/// scenario modules, with no synthetic test cards composed in.
const REGISTRIES: Registries = Registries {
    cards: &cards::REGISTRY,
    scenarios: &scenarios::REGISTRY,
};

const DAISY: &str = "01002";
/// Her signature asset.
const TOTE_BAG: &str = "01008";
/// Her signature weakness.
const NECRONOMICON: &str = "01009";
const INV: InvestigatorId = InvestigatorId(1);
/// The Ghoul Priest, set aside until act 2 (01109)'s reverse spawns it.
const GHOUL_PRIEST: &str = "01116";
const WORKING_A_HUNCH: &str = "01037";
const KNIFE: &str = "01086";

/// Her level-0 Seeker pool, as printed in
/// `data/arkhamdb-snapshot/pack/core/core.json`.
const SEEKER_LEVEL_0: [&str; 10] = [
    "01030", // Magnifying Glass
    "01031", // Old Book of Lore
    "01032", // Research Librarian
    "01033", // Dr. Milan Christopher
    "01034", // Hyperawareness
    "01035", // Medical Texts
    "01036", // Mind over Matter
    "01037", // Working a Hunch
    "01038", // Barricade
    "01039", // Deduction
];

/// How many copies of each code the scaffold deck holds.
fn copies() -> BTreeMap<&'static str, usize> {
    let mut copies = BTreeMap::new();
    for code in DAISY_DEFAULT_DECK {
        *copies.entry(*code).or_default() += 1;
    }
    copies
}

#[test]
fn the_scaffold_deck_holds_her_whole_level_0_seeker_pool() {
    let copies = copies();
    let missing: Vec<_> = SEEKER_LEVEL_0
        .iter()
        .filter(|code| !copies.contains_key(*code))
        .collect();
    assert!(missing.is_empty(), "missing from the deck: {missing:?}");
}

#[test]
fn the_scaffold_deck_holds_one_of_each_signature() {
    let copies = copies();
    assert_eq!(copies.get(TOTE_BAG), Some(&1));
    assert_eq!(copies.get(NECRONOMICON), Some(&1));
}

#[test]
fn every_card_in_the_scaffold_deck_is_playable() {
    let unplayable: Vec<_> = DAISY_DEFAULT_DECK
        .iter()
        .filter(|code| !cards::is_playable(code))
        .collect();
    assert!(unplayable.is_empty(), "not playable: {unplayable:?}");
}

/// The Gathering set up and solo Daisy seated from the corpus with the scaffold
/// deck, past the mulligan and at her first turn menu.
///
/// Two stand-ins, as in Roland's and Skids's walks: a single-token `Numeric(0)`
/// chaos bag (production serves the random Standard bag), and an encounter deck
/// of one Ancient Evils 01166, whose Revelation only places 1 doom, so round
/// 2's Mythos draw is deterministic and cannot advance the agenda. The player
/// deck is the real one, and the fixed `rng` seed `setup()` builds with makes
/// its shuffle deterministic: the walk below scripts the windows that shuffle's
/// hand opens.
fn seated_daisy() -> GameState {
    let mut state = the_gathering::setup();
    state.chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)]);
    state.encounter_deck.clear();
    state.encounter_deck.push_back(CardCode::new("01166"));

    let roster = vec![RosterEntry {
        investigator: CardCode::new(DAISY),
        deck: DAISY_DEFAULT_DECK
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

/// The labels of the fast window `session` rests at.
fn offered(session: &TestSession) -> Vec<&str> {
    session
        .prompt()
        .options
        .iter()
        .map(|option| option.label.as_str())
        .collect()
}

/// The deck's shuffle deals Working a Hunch 01037, a Fast event, into her
/// opening hand, so her turn opens on the start-of-turn fast window (Appendix
/// II 2.2) offering it. The walk passes it, keeping her actions for the acts.
fn pass_start_of_turn_window(session: TestSession) -> TestSession {
    assert_eq!(
        offered(&session),
        [format!("Play {WORKING_A_HUNCH}")],
        "her turn opens offering Working a Hunch",
    );
    session.skip()
}

/// Where `code` sits in her hand.
fn hand_index(session: &TestSession, code: &str) -> u8 {
    let index = session.state().investigators[&INV]
        .hand
        .iter()
        .position(|c| c.as_str() == code)
        .unwrap_or_else(|| panic!("{code} is in her hand"));
    u8::try_from(index).expect("a small hand")
}

#[test]
fn daisy_with_her_scaffold_deck_plays_the_gathering_to_a_resolution() {
    let mut state = seated_daisy();
    // Seed: clues for both act thresholds (act 1 = 2, act 2 = 3), as Roland's
    // walk does — discovering them is covered elsewhere.
    state
        .investigators
        .get_mut(&INV)
        .expect("Daisy seated")
        .clues = 5;

    // --- Act 1: spend clues to advance; the reverse builds the board and moves
    // Daisy to the Hallway.
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
    // her turn. Working a Hunch is still in her hand, but the Hallway has no
    // clue for it to discover, so no fast window opens.
    let investigation = after_spend.confirm();
    investigation.assert_at_turn_menu(INV);
    assert_eq!(investigation.state().round, 2);

    // --- Seed the spawned Priest's health and engagement, as Roland's walk
    // does: one damage from defeat, engaged with Daisy in the Hallway.
    let investigation = investigation.edit_state(|state| {
        let priest = state
            .enemies
            .values_mut()
            .find(|e| e.code.as_str() == GHOUL_PRIEST)
            .expect("act 2's reverse spawned the Ghoul Priest");
        priest.damage = priest.max_health - 1;
        priest.engaged_with = Some(INV);
    });
    // --- Her round-1 upkeep drew Knife; her resources pay its cost of 1. Its
    // discard ability fights the one enemy at her location, at combat 2 + 2 +
    // Numeric(0) against the Priest's fight 4, which succeeds, and its 1 + 1
    // damage defeats the Priest.
    let knife = hand_index(&investigation, KNIFE);
    let armed = investigation.take(&TurnAction::PlayCard {
        investigator: INV,
        hand_index: knife,
    });
    armed.assert_at_turn_menu(INV);
    let knife = armed.state().in_play(INV, KNIFE);
    let fighting = armed.take(&TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(knife),
        address: AbilityAddress::Printed(1),
    });
    assert_eq!(fighting.rejection(), None, "Knife's discard ability");
    let defeated = fighting.commit(&[]);
    assert_event!(defeated.events(), Event::SkillTestSucceeded { .. });
    assert_event!(defeated.events(), Event::EnemyDamaged { amount: 2, .. });
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
