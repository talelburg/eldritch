//! Every card the weakness pool can deal resolves when its bearer draws it
//! (#1128): a seeded game under the production [`cards::REGISTRIES`] seats
//! Roland with one random basic weakness, and the Draw action brings it into
//! play. Seeds are walked until each of the pool's eight titles has been dealt.
//!
//! `glossary/Weakness.md`: *"When an investigator draws a weakness with an
//! encounter cardtype (for example, an enemy or a treachery weakness), resolve
//! that card as if it were just drawn from the encounter deck."*
//!
//! The pool's eight titles, printed text from
//! `data/arkhamdb-snapshot/pack/core/core.json`:
//!
//! - Amnesia 01096: *"**Revelation** - Choose and discard all but 1 card from
//!   your hand."*
//! - Paranoia 01097: *"**Revelation** - Discard all your resources."*
//! - Haunted 01098, Psychosis 01099, Hypochondria 01100: *"**Revelation** - Add
//!   \[the card\] to your threat area."*
//! - Mob Enforcer 01101, Silver Twilight Acolyte 01102, Stubborn Detective
//!   01103: enemies with *"**Prey** - Bearer only. Hunter."* and no Spawn
//!   instruction, so `glossary/Spawn.md`: *"If the enemy does not have a "Spawn
//!   –" instruction, the investigator drawing the enemy spawns it engaged with
//!   him/her, unless it is aloof."*
//!
//! Each card's own file pins what its effect does; this one pins that the
//! pool deals nothing the draw path leaves stranded in hand.

use std::collections::BTreeSet;

use cards::{decks, REGISTRIES};
use game_core::action::RosterEntry;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{self, EngineOutcome, OptionTarget};
use game_core::state::{CardCode, GameState, GameStateBuilder, InvestigatorId, Owner};
use game_core::test_support::TestSession;

const ROLAND: &str = "01001";
/// The Study 01111, the starting location.
const STUDY: &str = "01111";
/// Four skills to fill out the deck: Guts 01089 and Unexpected Courage 01093,
/// two each. With the dealt weakness that is five cards, so the opening hand
/// draws every one of them, sets the weakness aside (setup step 8), and the
/// mulligan's close shuffles it back as the deck's only card.
const DECK: [&str; 4] = ["01089", "01089", "01093", "01093"];

const AMNESIA: &str = "01096";
const PARANOIA: &str = "01097";
const THREAT_AREA_WEAKNESSES: [&str; 3] = ["01098", "01099", "01100"];
const ENEMY_WEAKNESSES: [&str; 3] = ["01101", "01102", "01103"];

const BEARER: InvestigatorId = InvestigatorId(1);

/// Seeds to walk before giving up on seeing every title dealt.
const SEEDS: u64 = 500;

fn board(seed: u64) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_registries(REGISTRIES)
        .with_rng_seed(seed)
        .build();
    let study = state.add_location(cards::by_code(STUDY).expect("Study 01111 in corpus"));
    state.starting_location = Some(study);
    state
}

/// Roland seated under `seed` with one random basic weakness, the mulligan
/// kept, at his turn menu with the dealt weakness alone in his deck. Returns
/// the session and the dealt code.
fn seated(seed: u64) -> (TestSession, CardCode) {
    let roster = [RosterEntry {
        investigator: CardCode::new(ROLAND),
        deck: DECK.iter().map(|c| CardCode::new(*c)).collect(),
        random_basic_weaknesses: 1,
    }];
    let opened = engine::seat_and_open(board(seed), &roster, REGISTRIES);
    assert!(
        matches!(opened.outcome, EngineOutcome::AwaitingInput { .. }),
        "seed {seed}: seating opens the mulligan prompt, got {:?}",
        opened.outcome
    );
    let session = TestSession::new(opened.state, REGISTRIES).choose_from_hand(&[]);
    session.assert_at_turn_menu(BEARER);
    let deck = &session.state().investigators[&BEARER].deck;
    assert_eq!(deck.len(), 1, "seed {seed}: the weakness alone is left");
    let dealt = deck[0].clone();
    assert!(
        decks::weakness_pool().contains(&dealt),
        "seed {seed}: {dealt:?} is a pool card"
    );
    (session, dealt)
}

/// Draw the dealt weakness and answer the one prompt a pool card raises here:
/// Amnesia's choice of the hand card to keep.
fn draw(session: TestSession, dealt: &CardCode) -> TestSession {
    let session = session.take(&TurnAction::Draw {
        investigator: BEARER,
    });
    if dealt.as_str() == AMNESIA {
        session.pick(OptionTarget::HandCard {
            investigator: BEARER,
            hand_index: 0,
        })
    } else {
        session
    }
}

/// `dealt` has resolved: it left the hand for where its card text puts it.
fn assert_resolved(seed: u64, session: &TestSession, dealt: &CardCode) {
    session.assert_at_turn_menu(BEARER);
    let state = session.state();
    let inv = &state.investigators[&BEARER];
    let code = dealt.as_str();
    assert!(
        !inv.hand.contains(dealt),
        "seed {seed}: {code} left the hand"
    );
    if code == AMNESIA {
        // Kept one of the four skills and discarded the other three, then
        // Amnesia itself.
        assert_eq!(inv.hand.len(), 1, "seed {seed}: Amnesia keeps one card");
        assert_eq!(
            inv.discard.len(),
            4,
            "seed {seed}: three skills and Amnesia"
        );
        assert_eq!(inv.discard.last(), Some(dealt), "seed {seed}");
    } else if code == PARANOIA {
        assert_eq!(inv.discard, vec![dealt.clone()], "seed {seed}");
    } else if THREAT_AREA_WEAKNESSES.contains(&code) {
        let placed = inv
            .threat_area
            .iter()
            .find(|c| &c.code == dealt)
            .unwrap_or_else(|| panic!("seed {seed}: {code} is in the threat area"));
        assert_eq!(placed.owner, Owner::Investigator(BEARER));
    } else if ENEMY_WEAKNESSES.contains(&code) {
        let enemy = state
            .enemies
            .values()
            .find(|e| &e.code == dealt)
            .unwrap_or_else(|| panic!("seed {seed}: {code} is in play"));
        assert_eq!(enemy.engaged_with, Some(BEARER), "seed {seed}: {code}");
        assert_eq!(
            enemy.owner,
            Owner::Investigator(BEARER),
            "seed {seed}: {code}"
        );
    } else {
        panic!("seed {seed}: {code} is not one of the pool's eight titles");
    }
}

#[test]
fn every_pool_card_resolves_when_its_bearer_draws_it() {
    let titles: BTreeSet<&str> = decks::weakness_pool()
        .iter()
        .map(CardCode::as_str)
        .collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();

    for seed in 0..SEEDS {
        let (session, dealt) = seated(seed);
        let session = draw(session, &dealt);
        assert_resolved(seed, &session, &dealt);
        let title = titles
            .get(dealt.as_str())
            .expect("the dealt code is a pool title");
        seen.insert(title);
        if seen == titles {
            return;
        }
    }
    let missing: Vec<&&str> = titles.difference(&seen).collect();
    panic!("{SEEDS} seeds never dealt {missing:?}");
}
