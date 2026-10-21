//! Drawing a persistent treachery weakness (Cover Up 01007) from the player
//! deck during play reveals it and resolves its Revelation — Cover Up enters
//! the controller's threat area with 3 clues instead of staying in hand.
//!
//! A drawn one-shot weakness (Paranoia 01097) whose bearer is defeated before
//! it resolves leaves the game with them.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId, Owner, Status};
use game_core::test_support::{self, seat, TestSession};
use game_core::{assert_event, assert_no_event};

const COVER_UP: &str = "01007";
const PARANOIA: &str = "01097";
const HOLY_ROSARY: &str = "01059"; // a non-weakness asset, for the negative case

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);

/// Solo investigator at a revealed location, at their turn menu with 3
/// actions, no enemies (so the Draw action's `AoO` loop is empty and resolves
/// synchronously), with `deck_top` as the top card of an otherwise-filler
/// deck. Takes the Draw action.
fn draw(deck_top: &str) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        // Top of deck is drawn first (draw_cards drains from the front).
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .deck([deck_top, HOLY_ROSARY, HOLY_ROSARY]),
        )
        .open_turn(INV)
        .session()
        .take(&TurnAction::Draw { investigator: INV })
}

#[test]
fn drawing_cover_up_reveals_it_into_the_threat_area() {
    let session = draw(COVER_UP);

    // Draw is not rejected: it resolves back to the turn menu.
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert!(
        !inv.hand.iter().any(|c| c.as_str() == COVER_UP),
        "Cover Up must not stay in hand — it is revealed on draw",
    );
    let placed = inv
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == COVER_UP)
        .expect("Cover Up should be in the threat area after being drawn");
    assert_eq!(
        placed.clues, 3,
        "Cover Up enters the threat area with 3 clues"
    );
    // A CardRevealed event fires for the drawn weakness.
    assert_event!(
        session.events(),
        Event::CardRevealed { code, .. } if code.as_str() == COVER_UP
    );
}

/// **A weakness drawn from your deck is yours.** Cover Up enters the threat area
/// owned by the investigator who drew it, who is its bearer
/// (`glossary/Weakness.md`: *"The bearer of a weakness is the investigator who
/// started the game with the weakness in his or her deck or play area."*) —
/// not by the encounter deck, although a threat area otherwise holds encounter
/// cards.
#[test]
fn a_drawn_cover_up_is_owned_by_its_bearer() {
    let session = draw(COVER_UP);

    let placed = session.state().investigators[&INV]
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == COVER_UP)
        .expect("Cover Up is in the threat area after being drawn");
    assert_eq!(placed.owner, Owner::Investigator(INV));
}

#[test]
fn drawing_a_non_weakness_leaves_it_in_hand() {
    let session = draw(HOLY_ROSARY);

    let inv = &session.state().investigators[&INV];
    assert!(
        inv.hand.iter().any(|c| c.as_str() == HOLY_ROSARY),
        "a normal drawn card stays in hand",
    );
    assert!(inv.threat_area.is_empty(), "nothing enters the threat area");
    // No reveal for a non-weakness draw.
    assert_no_event!(session.events(), Event::CardRevealed { .. });
}

/// **A bearer defeated mid-resolution takes their weakness with them** (ADR
/// 0002). The drawer's deck is empty, so the draw reshuffles their discard
/// — Paranoia 01097 — back in, draws it, and then deals the deck-out horror
/// first, which defeats them before Paranoia's Revelation resolves.
/// `glossary/Drawing_Cards.md`: *"that investigator shuffles his or her discard
/// pile back into his or her deck, then draws the card, and upon completion of
/// the entire draw takes one horror."*
///
/// Paranoia is in limbo, so elimination step 1 removes it from the game with
/// the rest of its bearer's cards; its Revelation never resolves, and its
/// disposal does not file it into the discard pile step 1 has already removed.
#[test]
fn a_weakness_resolving_when_its_bearer_is_defeated_is_removed_from_the_game() {
    // Sanity 8: 7 horror already, so the deck-out horror is the eighth.
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .resources(5)
                .harm(0, 7)
                .discard([PARANOIA]),
        )
        .open_turn(INV)
        .session()
        .take(&TurnAction::Draw { investigator: INV });

    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.status, Status::Defeated);
    assert_eq!(inv.removed_from_game, vec![CardCode::new(PARANOIA)]);
    assert!(inv.discard.is_empty());
    assert!(inv.hand.is_empty());
    assert_no_event!(session.events(), Event::ResourcesDiscarded { .. });
    assert_no_event!(session.events(), Event::CardDiscarded { .. });
}
