//! Heirloom of Hyperborea 01012 end-to-end against the real card registry.
//! Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > Agnes Baker deck only.
//! > [reaction] After you play a [[Spell]] card: Draw 1 card.
//!
//! Rulings (<https://arkhamdb.com/card/01012>):
//!
//! > Yes, the Heirloom of Hyperborea can trigger off of any Spell card its
//! > controller plays. This trigger occurs after the Spell card has resolved in
//! > full.
//!
//! > For Heirloom of Hyperborea, it triggers after you play a ***Spell*** card.
//! > If the ***Spell*** card has a single effect like Ward of Protection and
//! > then gets discarded, you can trigger Heirloom once it's in the discard
//! > pile. If you instead play a ***Spell*** like Scrying, you could activate
//! > Heirloom once Scrying enters play.
//!
//! So the window is the after cell of the `CardPlayed` condition, announced
//! once play step 4 has completed: a Spell event already in its owner's discard
//! pile (Dark Memory 01013), a Spell asset already in play (Scrying 01061).
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::OptionTarget;
use game_core::state::{Agenda, CardCode, EnemyId, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, seat, Seat, SeatLookup as _, TestSession};

const HEIRLOOM: &str = "01012";
/// Event. `Spell.`
const DARK_MEMORY: &str = "01013";
/// Asset. `Spell.`
const SCRYING: &str = "01061";
/// Event. `Supply.`, not a Spell.
const EMERGENCY_CACHE: &str = "01088";
/// Fast event. `Tactic.`, not a Spell, played from a reaction window.
const DODGE: &str = "01023";
/// Deck filler, the card Heirloom draws.
const GUTS: &str = "01089";

const INV: InvestigatorId = InvestigatorId(1);
const INV2: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
const ATTACKER: EnemyId = EnemyId(7);

/// Investigator 1 at `HERE` with Heirloom in play, `hand` in hand, 5
/// resources, and a one-card deck for Heirloom to draw from.
fn agnes(hand: &[&str]) -> Seat {
    seat(1)
        .at(HERE)
        .in_play([HEIRLOOM])
        .hand(hand.iter().copied())
        .resources(5)
        .deck([GUTS])
}

/// `investigator`'s open turn at `HERE`, solo, with The Gathering's first
/// agenda current (Dark Memory places its doom there).
fn board(investigator: Seat) -> TestSession {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(investigator)
        .open_turn(INV)
        .build();
    // The builder stages no agenda.
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01105"),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;
    TestSession::new(state, TEST_REGISTRIES)
}

const PLAY_FIRST: TurnAction = TurnAction::PlayCard {
    investigator: INV,
    source: PlaySource::Hand(0),
};

fn heirloom(session: &TestSession, holder: InvestigatorId) -> OptionTarget {
    session.state().card(holder, HEIRLOOM)
}

#[test]
fn playing_a_spell_event_offers_the_draw_once_it_is_in_the_discard_pile() {
    let s = board(agnes(&[DARK_MEMORY])).take(&PLAY_FIRST);

    // The window is open, and Dark Memory has resolved in full: its doom is on
    // the agenda and the card is in the discard pile.
    assert_eq!(s.offered_at(&heirloom(&s, INV)), 1, "Heirloom is offered");
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.discard, vec![CardCode::new(DARK_MEMORY)]);
    assert_eq!(s.state().agenda_doom, 1);

    let target = heirloom(&s, INV);
    let s = s.pick(target);
    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].hand,
        vec![CardCode::new(GUTS)],
        "Heirloom drew 1 card",
    );
}

#[test]
fn playing_a_spell_asset_offers_the_draw_once_it_is_in_play() {
    let s = board(agnes(&[SCRYING])).take(&PLAY_FIRST);

    assert_eq!(s.offered_at(&heirloom(&s, INV)), 1, "Heirloom is offered");
    let in_play = &s.state().investigators[&INV].cards_in_play;
    assert!(
        in_play.iter().any(|c| c.code == CardCode::new(SCRYING)),
        "Scrying is already in play when the window opens",
    );

    let target = heirloom(&s, INV);
    let s = s.pick(target);
    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].hand,
        vec![CardCode::new(GUTS)],
        "Heirloom drew 1 card",
    );
}

#[test]
fn playing_a_card_that_is_not_a_spell_offers_nothing() {
    let s = board(agnes(&[EMERGENCY_CACHE])).take(&PLAY_FIRST);

    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].discard,
        vec![CardCode::new(EMERGENCY_CACHE)],
        "the play itself completed",
    );
    assert!(
        s.state().investigators[&INV].hand.is_empty(),
        "nothing drawn"
    );
}

#[test]
fn another_investigators_spell_offers_nothing() {
    // "After **you** play": investigator 2's Heirloom does not hear
    // investigator 1's Scrying.
    let s = TestSession::new(
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_location(test_support::test_location(HERE.0, "Study"))
            .with_seat(seat(1).at(HERE).hand([SCRYING]).resources(5))
            .with_seat(seat(2).at(HERE).in_play([HEIRLOOM]).deck([GUTS]))
            .with_turn_order([INV, INV2])
            .open_turn(INV)
            .build(),
        TEST_REGISTRIES,
    )
    .take(&PLAY_FIRST);

    s.assert_at_turn_menu(INV);
    assert!(
        s.state().investigators[&INV2].hand.is_empty(),
        "investigator 2 drew nothing",
    );
}

/// The reaction-window play path announces the completed play too, so a Fast
/// event played from a window reaches the `CardPlayed` scan. Dodge is no Spell,
/// so Heirloom stays silent, and Dodge's cancel still holds: the `CardPlayed`
/// sequence nested inside the attack's `when` cell does not consume it. The
/// positive case, a Spell played from a window, is Ward of Protection's.
#[test]
fn a_non_spell_played_from_a_reaction_window_offers_nothing() {
    let mut attacker = test_support::test_enemy(ATTACKER.0, "Attacker");
    attacker.attack_damage = 2;
    attacker.attack_horror = 1;
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(agnes(&[DODGE]))
        .with_enemy_engaged(attacker, INV)
        .open_turn(INV)
        .build();
    let dodge = OptionTarget::HandCardByCode {
        investigator: INV,
        code: CardCode::new(DODGE),
    };

    let s = TestSession::new(state, TEST_REGISTRIES)
        .take(&TurnAction::EndTurn)
        .pick(dodge);

    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.damage(), 0, "the attack is still cancelled");
    assert_eq!(inv.horror(), 0, "the attack is still cancelled");
    assert!(inv.discard.contains(&CardCode::new(DODGE)));
    // The round ran on into the next Mythos without stopping for Heirloom.
    s.assert_at_encounter_draw();
}
