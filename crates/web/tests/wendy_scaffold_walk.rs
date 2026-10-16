//! Wendy Adams 01005, seated with the picker's scaffold deck, plays The
//! Gathering to a resolution (#1113) — the acceptance line of #780, *"Wendy is
//! selectable in the picker and playable through The Gathering with a scaffold
//! deck."* The deck the walk seats is [`WENDY_DEFAULT_DECK`] itself, which is
//! why the walk lives in the crate that owns it.
//!
//! The walk mirrors "Skids"'s in `skids_scaffold_walk.rs`, against the
//! production registries (the real corpus and the real scenario modules): act 1
//! advances on spent clues, act 2 through its round-end clue-spend window,
//! whose reverse spawns the real Ghoul Priest 01116, and the Priest's defeat
//! advances the terminal act to its printed choice. Wendy's combat is 1, so on
//! the walk's `Numeric(0)` bag a basic Fight against the Priest's fight 4
//! fails. She lands the kill as Skids does, with Backstab 01051:
//!
//! > <b>Fight.</b> This attack uses [agility] instead of [combat]. This attack
//! > deals +2 damage.
//!
//! — but played from her discard pile under Wendy's Amulet 01014, *"You may
//! play the topmost event in your discard pile as if it were in your hand."*
//! Backstab reaches the discard pile through her own reaction's cost:
//!
//! > \[reaction\] When you reveal a chaos token, choose and discard 1 card from
//! > your hand: Cancel that chaos token and return it to the bag. Reveal a new
//! > chaos token. (Limit once per test/ability.)
//!
//! and, played with the Amulet in play, leaves for the bottom of her deck
//! under the Amulet's *"<b>Forced</b> - After you play an event or discard an
//! event from play: Place it on the bottom of your deck instead of in your
//! discard pile."*
//!
//! Host-only: the browser tests in this directory are wasm32-only, and this one
//! needs no DOM.
#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;

use game_core::action::RosterEntry;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::{self, OptionTarget, Registries};
use game_core::event::Event;
use game_core::scenario::{ResolutionId, ScenarioEnding};
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, DiscardPile, GameState, InvestigatorId, Phase, SkillKind, Zone,
};
use game_core::test_support::{SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_sequence};
use scenarios::the_gathering;
use web::scaffold_decks::WENDY_DEFAULT_DECK;

/// What the server passes to every `apply`: the real corpus and the real
/// scenario modules, with no synthetic test cards composed in.
const REGISTRIES: Registries = Registries {
    cards: &cards::REGISTRY,
    scenarios: &scenarios::REGISTRY,
};

const WENDY: &str = "01005";
const INV: InvestigatorId = InvestigatorId(1);
/// The Ghoul Priest, set aside until act 2 (01109)'s reverse spawns it.
const GHOUL_PRIEST: &str = "01116";
const WENDYS_AMULET: &str = "01014";
const BACKSTAB: &str = "01051";

/// Every player card #780 shipped, as printed in
/// `data/arkhamdb-snapshot/pack/core/core.json`: Wendy's Amulet and the ten
/// Survivor level-0 cards. Abandoned and Alone 01015 shipped later, in
/// #781.
const SLICE_CARDS: [&str; 11] = [
    "01014", // Wendy's Amulet (signature)
    "01072", // Leather Coat
    "01073", // Scavenging
    "01074", // Baseball Bat
    "01075", // Rabbit's Foot
    "01076", // Stray Cat
    "01077", // Dig Deep
    "01078", // Cunning Distraction
    "01079", // "Look what I found!"
    "01080", // Lucky!
    "01081", // Survival Instinct
];

/// How many copies of each code the scaffold deck holds.
fn copies() -> BTreeMap<&'static str, usize> {
    let mut copies = BTreeMap::new();
    for code in WENDY_DEFAULT_DECK {
        *copies.entry(*code).or_default() += 1;
    }
    copies
}

#[test]
fn the_scaffold_deck_holds_every_card_the_wendy_slice_shipped() {
    let copies = copies();
    let missing: Vec<_> = SLICE_CARDS
        .iter()
        .filter(|code| !copies.contains_key(*code))
        .collect();
    assert!(missing.is_empty(), "missing from the deck: {missing:?}");
}

/// Wendy's deckbuilding requirements name one copy of each signature:
/// *"Wendy's Amulet, Abandoned and Alone, 1 random basic weakness."* The scaffold
/// leaves Abandoned and Alone out; #782's legal starter deck adds it.
#[test]
fn the_scaffold_deck_holds_the_amulet_once_and_no_abandoned_and_alone() {
    let copies = copies();
    assert_eq!(copies.get(WENDYS_AMULET), Some(&1), "Wendy's Amulet");
    assert_eq!(copies.get("01015"), None, "Abandoned and Alone");
}

#[test]
fn every_card_in_the_scaffold_deck_is_playable() {
    let unplayable: Vec<_> = WENDY_DEFAULT_DECK
        .iter()
        .filter(|code| !cards::is_playable(code))
        .collect();
    assert!(unplayable.is_empty(), "not playable: {unplayable:?}");
}

/// The Gathering set up and solo Wendy seated from the corpus with the scaffold
/// deck, past the mulligan and at her first turn menu.
///
/// Two stand-ins, as in the other walks: a single-token `Numeric(0)` chaos bag
/// (production serves the random Standard bag), and an encounter deck of one
/// Ancient Evils 01166, whose Revelation only places 1 doom, so round 2's
/// Mythos draw is deterministic and cannot advance the agenda. The player deck
/// is the real one, and the fixed `rng` seed `setup()` builds with makes its
/// shuffle deterministic.
fn seated_wendy() -> GameState {
    let mut state = the_gathering::setup();
    state.chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)]);
    state.encounter_deck.clear();
    state.encounter_deck.push_back(CardCode::new("01166"));

    let roster = vec![RosterEntry {
        investigator: CardCode::new(WENDY),
        deck: WENDY_DEFAULT_DECK
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

/// Move one copy of `code` from wherever the shuffle left it, deck or hand, to
/// the front of Wendy's hand, so the walk need not depend on what was dealt.
fn to_front_of_hand(state: &mut GameState, code: &str) {
    let code = CardCode::new(code);
    let wendy = state.investigators.get_mut(&INV).expect("Wendy seated");
    if let Some(i) = wendy.hand.iter().position(|c| *c == code) {
        wendy.hand.remove(i);
    } else {
        let i = wendy
            .deck
            .iter()
            .position(|c| *c == code)
            .expect("the scaffold deck holds the card");
        wendy.deck.remove(i);
    }
    wendy.hand.insert(0, code);
}

/// Her reaction, offered on her investigator card.
fn wendys_card(session: &TestSession) -> OptionTarget {
    OptionTarget::CardInstance(
        session.state().investigators[&INV]
            .investigator_card
            .instance_id,
    )
}

/// Round 1, from the seeded state to her turn menu after the investigation:
/// act 1 advanced, the Amulet in play, and Backstab on top of her discard
/// pile.
fn round_one(state: GameState) -> TestSession {
    // --- Act 1: spend clues to advance; the reverse builds the board and moves
    // Wendy to the Hallway.
    let session =
        TestSession::new(state, REGISTRIES).take(&TurnAction::AdvanceAct { investigator: INV });
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().act_index, 1, "act 1 advanced to act 2");

    // --- Round 1, with no enemy yet: the Amulet into play, from her 5
    // starting resources.
    let session = session.take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    });
    session.assert_at_turn_menu(INV);

    // An investigation, whose reveal offers her reaction: she takes it and
    // pays its cost with Backstab, now at the front of her hand. The `0`
    // revealed in the cancelled one's place investigates the Hallway's shroud
    // 1 with intellect 3.
    let at_reveal = session
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[]);
    let reaction = wendys_card(&at_reveal);
    assert_eq!(
        at_reveal.offered_at(&reaction),
        1,
        "her reveal offers the reaction, got {:?}",
        at_reveal.prompt(),
    );
    let investigated = at_reveal.pick(reaction).pick(OptionTarget::HandCard {
        investigator: INV,
        hand_index: 0,
    });
    investigated.assert_at_turn_menu(INV);
    assert_event_sequence!(
        investigated.events(),
        Event::ChaosTokenRevealed { .. },
        Event::CardDiscarded { code, from: Zone::Hand, to: DiscardPile::Investigator(INV) }
            if *code == CardCode::new(BACKSTAB),
        Event::ChaosTokenRevealed { .. },
        Event::SkillTestSucceeded { .. },
    );
    assert_eq!(
        investigated.state().investigators[&INV].discard.last(),
        Some(&CardCode::new(BACKSTAB)),
        "Backstab tops her discard pile",
    );
    investigated
}

#[test]
fn wendy_with_her_scaffold_deck_plays_the_gathering_to_a_resolution() {
    let mut state = seated_wendy();
    // Seed: clues for both act thresholds (act 1 = 2, act 2 = 3), as Roland's
    // walk does — discovering them is covered elsewhere. And seed the Amulet
    // and Backstab at the front of her hand, in that order.
    state
        .investigators
        .get_mut(&INV)
        .expect("Wendy seated")
        .clues = 5;
    to_front_of_hand(&mut state, BACKSTAB);
    to_front_of_hand(&mut state, WENDYS_AMULET);

    let investigated = round_one(state);
    let amulet = investigated.state().in_play(INV, WENDYS_AMULET);

    // --- Act 2: ending the round opens its round-end clue-spend window; the
    // spend advances it and its reverse spawns the Ghoul Priest in the Hallway.
    let round_end = investigated.take(&TurnAction::EndTurn);
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
    // her turn.
    let investigation = after_spend.confirm();
    investigation.assert_at_turn_menu(INV);
    assert_eq!(investigation.state().round, 2);

    // --- Seed the spawned Priest's health and engagement, as Skids's walk
    // does: three damage from defeat, which Backstab's 1 + 2 lands exactly.
    let investigation = investigation.edit_state(|state| {
        let priest = state
            .enemies
            .values_mut()
            .find(|e| e.code.as_str() == GHOUL_PRIEST)
            .expect("act 2's reverse spawned the Ghoul Priest");
        priest.damage = priest.max_health - 3;
        priest.engaged_with = Some(INV);
    });

    // --- Backstab from the top of her discard pile, offered on the Amulet:
    // agility 4 + Numeric(0) against the Priest's fight 4 succeeds, and its 3
    // damage defeats the Priest. The reveal offers her reaction again, a new
    // test's: she passes it.
    let at_reveal = investigation
        .take(&TurnAction::PlayCard {
            investigator: INV,
            source: PlaySource::TopOfDiscardPile { permission: amulet },
        })
        .commit(&[]);
    assert_eq!(
        at_reveal.offered_at(&wendys_card(&at_reveal)),
        1,
        "her reveal offers the reaction, got {:?}",
        at_reveal.prompt(),
    );
    let defeated = at_reveal.skip();
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
    let resolved = defeated.pick_nth(0);

    // Backstab finishes resolving after the choice its kill opened, and the
    // Amulet's Forced sends it to the bottom of her deck.
    assert_event!(
        resolved.events(),
        Event::CardPlacedOnBottomOfDeck { investigator, code }
            if *investigator == INV && *code == CardCode::new(BACKSTAB)
    );
    let wendy = &resolved.state().investigators[&INV];
    assert_eq!(wendy.deck.last(), Some(&CardCode::new(BACKSTAB)));
    assert!(!wendy.discard.contains(&CardCode::new(BACKSTAB)));
    assert!(wendy.cards_in_play.iter().any(|c| c.instance_id == amulet));

    let result = resolved.finish();
    assert_eq!(
        result.state.ending,
        Some(ScenarioEnding::Resolution(ResolutionId::new(1))),
        "act 01110's reverse reaches (→R1)",
    );
}
