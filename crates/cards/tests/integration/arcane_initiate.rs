//! Integration: Arcane Initiate 01063 against `cards::TEST_REGISTRIES`.
//!
//! Printed text (`data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > **Forced** - After Arcane Initiate enters play: Place 1 doom on it.
//! > \[fast\] Exhaust Arcane Initiate: Search the top 3 cards of your deck for a
//! > \[\[Spell\]\] card and draw it. Shuffle your deck.
//!
//! Ruling (<https://arkhamdb.com/card/01063>): *"When you use the ability to
//! search the top 3 cards, if there is a **Spell** among them, you must draw
//! it, even if it is Dark Memory. You cannot choose to fail to find what you're
//! searching for if it's there."*
//!
//! The doom counts: `glossary/Doom.md` — *"Doom on cards other than the agenda
//! (such as enemies, allies, locations, etc.) counts towards the amount of doom
//! in play."* It is cleared when the agenda advances:
//! `glossary/Act_Deck_and_Agenda_Deck.md` step 1 — *"If the agenda deck is
//! advancing, remove all doom from each card in play."*

use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::assert_event;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::{board, OptionId, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInstanceId, GameStateBuilder, InvestigatorId,
    LocationId,
};
use game_core::test_support::{self, doom, seat, BoardLookup as _, Seat, TestSession};

const INITIATE: &str = "01063";
/// Dark Memory — an Agnes weakness event, trait Spell.
const DARK_MEMORY: &str = "01013";
/// Shrivelling — a Spell asset.
const SHRIVELLING: &str = "01060";
/// Guts — a skill, not a Spell (filler).
const GUTS: &str = "01089";
/// Rise of the Ghouls (doom 7), then They're Getting Out! (doom 10).
const AGENDAS: [&str; 2] = ["01106", "01107"];

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// `investigator`'s turn, alone at `HERE`.
fn turn(investigator: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HERE))
        .with_seat(investigator.at(HERE))
        .open_turn(INV)
        .session()
}

/// `investigator`'s turn, alone at `HERE`, with Rise of the Ghouls current and
/// `agenda_doom` on it.
fn turn_with_agenda(investigator: Seat, agenda_doom: u8) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HERE))
        .with_seat(investigator.at(HERE))
        .open_turn(INV)
        .with_agenda_deck(AGENDAS)
        .with_agenda_doom(agenda_doom)
        .session()
}

/// `investigator` seated at the end of the Upkeep phase with Rise of the
/// Ghouls current and `agenda_doom` on it, so settling runs the next Mythos
/// phase: step 1.2 places 1 doom, step 1.3 checks the threshold.
fn ending_the_round(investigator: Seat, agenda_doom: u8) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HERE))
        .with_seat(investigator.at(HERE))
        .with_seating([INV])
        .ending_upkeep_phase()
        .with_agenda_deck(AGENDAS)
        .with_agenda_doom(agenda_doom)
        .session()
}

fn initiate(session: &TestSession) -> CardInstanceId {
    session.state().in_play(INV, INITIATE)
}

fn initiate_doom(session: &TestSession) -> Option<u8> {
    session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.code == CardCode::new(INITIATE))
        .map(|c| c.doom)
}

fn search(initiate: CardInstanceId) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(initiate),
        address: AbilityAddress::Printed(1),
    }
}

/// Take the `position`th eligible card of the search. Sent raw: a deck
/// search's options are cards in a deck, with no board anchor (ADR 0015
/// excludes them), so no session step can name one.
fn take_searched(position: u32) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(position)),
    })
}

/// *"**Forced** - After Arcane Initiate enters play: Place 1 doom on it."*
#[test]
fn entering_play_places_one_doom_on_it() {
    let s = turn(seat(1).hand([INITIATE]).resources(5)).take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    });
    s.assert_at_turn_menu(INV);
    assert_eq!(initiate_doom(&s), Some(1), "the forced placed 1 doom on it");
    assert_eq!(s.state().agenda_doom, 0, "on it, not on the agenda");
}

/// Held in hand, the Initiate is out of play, and its forced does not reference
/// a use from there, so another asset entering play leaves it inert:
/// `glossary/Ability.md` — *"Card abilities only interact with the game if the
/// card bearing the ability is in play, unless the ability (or rules for the
/// cardtype) specifically references its use from an out-of-play area."*
#[test]
fn held_in_hand_it_stays_inert_when_another_asset_enters_play() {
    let s = turn(seat(1).hand([SHRIVELLING, INITIATE]).resources(3)).take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    });
    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.hand, vec![CardCode::new(INITIATE)], "still in hand");
    assert!(
        inv.cards_in_play
            .iter()
            .any(|c| c.code == CardCode::new(SHRIVELLING)),
        "Shrivelling entered play",
    );
    assert_eq!(board::doom_in_play(s.state()), 0, "no doom placed anywhere");
}

/// The Initiate's doom counts toward the threshold: 5 on the agenda, 1 from
/// Mythos step 1.2 and 1 on the Initiate make Rise of the Ghouls' 7, so the
/// agenda advances — and step 1 of the advance clears the Initiate's doom.
#[test]
fn its_doom_counts_toward_the_threshold_and_clears_on_advance() {
    let s = ending_the_round(seat(1).in_play([(INITIATE, doom(1))]), 5);
    assert_event!(s.events(), Event::AgendaAdvanced { .. });
    assert_eq!(s.state().agenda_index, 1, "Rise of the Ghouls advanced");
    assert_eq!(s.state().agenda_doom, 0);
    assert_eq!(
        initiate_doom(&s),
        Some(0),
        "advancing the agenda removes all doom from each card in play",
    );
}

/// The same board without the Initiate's doom stays one short: the advance
/// above is the Initiate's doom, not the agenda's alone.
#[test]
fn without_doom_on_cards_the_same_board_stays_short() {
    let s = ending_the_round(seat(1).in_play([INITIATE]), 5);
    assert_eq!(s.state().agenda_index, 0, "6 doom in play, threshold 7");
    assert_eq!(s.state().agenda_doom, 6);
}

/// Once the Initiate leaves play its doom stops counting: soaking 1 damage
/// defeats it (health 1), it is discarded, and the doom in play drops by its 1.
#[test]
fn its_doom_leaves_play_with_it() {
    let s = turn_with_agenda(seat(1).in_play([(INITIATE, doom(1))]), 5);
    assert_eq!(board::doom_in_play(s.state()), 6, "agenda 5 + Initiate 1");

    let id = initiate(&s);
    let s = s.take_damage(INV, 1).pick(OptionTarget::CardInstance(id));
    assert_eq!(initiate_doom(&s), None, "the Initiate left play");
    assert!(s.state().investigators[&INV]
        .discard
        .contains(&CardCode::new(INITIATE)));
    assert_eq!(board::doom_in_play(s.state()), 5, "its doom left with it");
}

/// The fast search takes the one Spell among the top 3 — Dark Memory
/// included, per the ruling — then shuffles.
#[test]
fn the_search_must_take_dark_memory() {
    let s = turn(
        seat(1)
            .in_play([INITIATE])
            .deck([GUTS, DARK_MEMORY, GUTS, SHRIVELLING]),
    );
    let id = initiate(&s);
    let s = s.take(&search(id));
    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.hand, vec![CardCode::new(DARK_MEMORY)]);
    assert!(
        inv.cards_in_play
            .iter()
            .any(|c| c.instance_id == id && c.exhausted),
        "exhaust paid",
    );
    assert_event!(s.events(), Event::CardSearchedToHand { .. });
    assert_event!(s.events(), Event::DeckShuffled { .. });
}

/// Two Spells in the top 3: the player picks one, with no way to decline.
#[test]
fn two_spells_prompt_a_pick_that_cannot_be_declined() {
    let s = turn(
        seat(1)
            .in_play([INITIATE])
            .deck([SHRIVELLING, GUTS, DARK_MEMORY]),
    );
    let id = initiate(&s);
    let s = s.take(&search(id));
    assert_eq!(s.prompt().options.len(), 2, "both Spells offered");
    assert!(!s.prompt().skippable, "finding is obligatory");
    let s = s.apply(take_searched(0));
    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].hand,
        vec![CardCode::new(SHRIVELLING)],
    );
}
