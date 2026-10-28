//! The random basic weakness draw at scenario start: a roster entry's
//! `random_basic_weaknesses` count draws that many cards from the weakness
//! pool, without replacement and shared across the roster, and shuffles them
//! into that investigator's deck.
//!
//! Runs under the production [`cards::REGISTRY`], the only registry with a
//! non-empty weakness pool. Rules: `glossary/Deckbuilding.md`, *"Each required
//! random basic weakness is added to a player's deck at the end of the
//! deckbuilding process."*

use cards::{decks, REGISTRIES};
use game_core::action::RosterEntry;
use game_core::card_registry::CardRegistry;
use game_core::engine::{self, ApplyResult, EngineOutcome, Registries};
use game_core::scenario::ScenarioRegistry;
use game_core::state::{CardCode, GameState, GameStateBuilder, InvestigatorId};

const ROLAND: &str = "01001";
const DAISY: &str = "01002";

/// A ten-card deck of implemented core cards: Magnifying Glass 01030 ×2,
/// Emergency Cache 01088 ×2, Guts 01089 ×2, Unexpected Courage 01093 ×2 and
/// Knife 01086 ×2.
const DECK: [&str; 10] = [
    "01030", "01030", "01088", "01088", "01089", "01089", "01093", "01093", "01086", "01086",
];

fn entry(investigator: &str, random_basic_weaknesses: u8) -> RosterEntry {
    RosterEntry {
        investigator: CardCode::new(investigator),
        deck: DECK.iter().map(|c| CardCode::new(*c)).collect(),
        random_basic_weaknesses,
    }
}

fn setup(seed: u64, registries: Registries) -> GameState {
    GameStateBuilder::new()
        .with_registries(registries)
        .with_rng_seed(seed)
        .build()
}

fn seat(seed: u64, roster: &[RosterEntry]) -> ApplyResult {
    let result = engine::seat_and_open(setup(seed, REGISTRIES), roster, 0, REGISTRIES);
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "seating opens the mulligan prompt, got {:?}",
        result.outcome
    );
    result
}

/// Every card investigator `id` was seated with, sorted: after seating they
/// are split across the deck, the opening hand, and the set-aside zone (a
/// weakness dealt into the opening hand waits there until the mulligan ends).
fn seated_cards(state: &GameState, id: InvestigatorId) -> Vec<String> {
    let inv = state.investigators.get(&id).expect("investigator seated");
    let mut cards: Vec<String> = inv
        .deck
        .iter()
        .chain(&inv.hand)
        .chain(&inv.setaside)
        .map(|c| c.as_str().to_string())
        .collect();
    cards.sort();
    cards
}

/// The cards investigator `id` was seated with beyond [`DECK`].
fn drawn_weaknesses(state: &GameState, id: InvestigatorId) -> Vec<String> {
    let mut extra = seated_cards(state, id);
    for code in DECK {
        let at = extra
            .iter()
            .position(|c| c == code)
            .expect("every roster deck card is seated");
        extra.remove(at);
    }
    extra
}

fn in_pool(code: &str) -> bool {
    decks::weakness_pool().iter().any(|c| c.as_str() == code)
}

fn pool_copies(code: &str) -> usize {
    decks::weakness_pool()
        .iter()
        .filter(|c| c.as_str() == code)
        .count()
}

#[test]
fn a_count_of_one_seats_one_pool_card_more_than_the_roster_deck() {
    let result = seat(7, &[entry(ROLAND, 1)]);

    assert_eq!(
        seated_cards(&result.state, InvestigatorId(1)).len(),
        DECK.len() + 1
    );
    let drawn = drawn_weaknesses(&result.state, InvestigatorId(1));
    assert_eq!(drawn.len(), 1);
    assert!(in_pool(&drawn[0]), "{} is in the weakness pool", drawn[0]);
}

#[test]
fn two_seats_drawing_two_each_never_draw_more_copies_than_the_pool_holds() {
    for seed in 0..20 {
        let result = seat(seed, &[entry(ROLAND, 2), entry(DAISY, 2)]);

        let mut drawn: Vec<String> = Vec::new();
        for id in [InvestigatorId(1), InvestigatorId(2)] {
            let mine = drawn_weaknesses(&result.state, id);
            assert_eq!(
                mine.len(),
                2,
                "seed {seed}: investigator {} draws two",
                id.0
            );
            drawn.extend(mine);
        }
        for code in &drawn {
            assert!(in_pool(code), "seed {seed}: {code} is in the weakness pool");
            let times = drawn.iter().filter(|c| *c == code).count();
            assert!(
                times <= pool_copies(code),
                "seed {seed}: {code} drawn {times} times, the pool holds {}",
                pool_copies(code)
            );
        }
    }
}

/// A count of 0 seats exactly as a registry with no weakness pool at all
/// does: the pool is never drawn from, and the game's RNG is left as the
/// setup shuffle alone leaves it.
#[test]
fn a_count_of_zero_seats_exactly_as_with_no_pool() {
    const NO_POOL: CardRegistry = CardRegistry {
        weakness_pool: || &[],
        ..cards::REGISTRY
    };
    const NO_POOL_REGISTRIES: Registries = Registries {
        cards: &NO_POOL,
        scenarios: &ScenarioRegistry::EMPTY,
    };
    let roster = [entry(ROLAND, 0), entry(DAISY, 0)];

    let with_pool = seat(3, &roster);
    let without_pool =
        engine::seat_and_open(setup(3, NO_POOL_REGISTRIES), &roster, 0, NO_POOL_REGISTRIES);

    assert_eq!(with_pool.state, without_pool.state);
    assert_eq!(with_pool.events, without_pool.events);
    assert_eq!(
        seated_cards(&with_pool.state, InvestigatorId(1)).len(),
        DECK.len()
    );
}

/// The pool holds ten cards, so a two-seat roster asking for six each is
/// refused rather than short-drawn.
#[test]
fn a_roster_asking_for_more_weaknesses_than_the_pool_holds_is_refused() {
    let state = setup(0, REGISTRIES);
    let roster = [entry(ROLAND, 6), entry(DAISY, 6)];

    let result = engine::seat_and_open(state.clone(), &roster, 0, REGISTRIES);

    let EngineOutcome::Rejected { reason } = &result.outcome else {
        panic!(
            "an over-large weakness count is refused, got {:?}",
            result.outcome
        );
    };
    assert!(
        reason.contains("random basic weaknesses, but the weakness pool holds"),
        "the reason names the pool shortfall, got {reason:?}"
    );
    assert_eq!(
        result.state, state,
        "a refused seating leaves state unchanged"
    );
    assert!(result.events.is_empty());
}
