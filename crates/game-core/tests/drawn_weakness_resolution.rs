//! A treachery weakness drawn from its bearer's deck resolves as an encounter
//! card (#1121), against a mock registry. `glossary/Weakness.md`:
//!
//! > When an investigator draws a weakness with an encounter cardtype *(for
//! > example, an enemy or a treachery weakness)*, resolve that card as if it
//! > were just drawn from the encounter deck.
//!
//! Every corpus one-shot treachery weakness (Amnesia 01096, Paranoia 01097,
//! Abandoned and Alone 01015) is still unimplemented, so two synthetic one-shot
//! weakness treacheries model the primitive (ADR 0016). Transitional: #1126
//! implements those cards, and their own tests in `crates/cards/tests` then
//! cover the path end to end. Its own test binary: the mock registry is built
//! once per process.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, WeaknessKind};
use card_dsl::dsl::{self, InvestigatorTarget};
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    CardCode, DiscardPile, GameStateBuilder, InvestigatorId, LocationId, Status, Zone,
};
use game_core::test_support::{self, seat, MockRegistry, Seat, TestSession};
use game_core::{assert_event_sequence, assert_no_event};

/// A one-shot weakness treachery whose Revelation gains its drawer 1 resource.
const ONE_SHOT_A: &str = "TEST-DWR-ONE-SHOT-A";
/// A second, gaining 2, so the two Revelations' order shows.
const ONE_SHOT_B: &str = "TEST-DWR-ONE-SHOT-B";
/// A 0-cost event: *"Draw 2 cards."*
const DRAW_TWO: &str = "TEST-DWR-DRAW-TWO";
/// Deck filler the registry doesn't know: not a weakness.
const FILLER: &str = "TEST-DWR-FILLER";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

fn metadata(code: &str, weakness: Option<WeaknessKind>, kind: CardKind) -> CardMetadata {
    CardMetadata {
        code: code.to_string(),
        name: code.to_string(),
        text: None,
        traits: vec![],
        back_name: None,
        back_text: None,
        pack_code: "test".to_string(),
        weakness,
        kind,
    }
}

fn weakness_treachery(code: &str) -> CardMetadata {
    metadata(
        code,
        Some(WeaknessKind::Basic),
        CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    )
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(weakness_treachery(ONE_SHOT_A))
            .with_card(weakness_treachery(ONE_SHOT_B))
            .with_card(metadata(
                DRAW_TWO,
                None,
                CardKind::Event {
                    class: Class::Neutral,
                    cost: Some(0),
                    xp: None,
                    skill_icons: SkillIcons::default(),
                    is_fast: false,
                    deck_limit: 2,
                    play_only_during_turn: false,
                },
            ))
            .with_abilities(ONE_SHOT_A, || {
                vec![dsl::revelation(dsl::gain_resources(
                    InvestigatorTarget::You,
                    1,
                ))]
            })
            .with_abilities(ONE_SHOT_B, || {
                vec![dsl::revelation(dsl::gain_resources(
                    InvestigatorTarget::You,
                    2,
                ))]
            })
            .with_abilities(DRAW_TWO, || {
                vec![dsl::on_play(dsl::draw_cards(InvestigatorTarget::You, 2_u8))]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// `investigator` at `HERE`, on their open turn.
fn board(investigator: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(investigator.at(HERE).actions(3).resources(0))
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .session()
}

fn draw(session: TestSession) -> TestSession {
    session.take(&TurnAction::Draw { investigator: INV })
}

/// **A resolved one-shot weakness goes to its bearer's discard pile**, with an
/// event naming the deck it was drawn from: it was in limbo while it resolved.
/// `glossary/Discard_Piles.md`: *"Any time a card is discarded, it is placed
/// faceup on top of its owner's discard pile."*
#[test]
fn a_drawn_one_shot_weakness_resolves_then_goes_to_its_bearers_discard() {
    let session = draw(board(seat(1).deck([ONE_SHOT_A, FILLER])));

    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert!(inv.hand.is_empty(), "it does not stay in hand");
    assert_eq!(inv.resources, 1, "its Revelation resolved");
    assert_eq!(inv.discard, vec![CardCode::new(ONE_SHOT_A)]);
    assert!(session.state().encounter_discard.is_empty());
    assert_event_sequence!(
        session.events(),
        Event::CardRevealed { code, .. } if code.as_str() == ONE_SHOT_A,
        Event::ResourcesGained { amount: 1, .. },
        Event::CardDiscarded {
            code,
            from: Zone::Deck,
            to: DiscardPile::Investigator(to),
        } if code.as_str() == ONE_SHOT_A && *to == INV,
    );
}

/// **Two weaknesses drawn together resolve in draw order** (#1121, Q2): both
/// are revealed once the draw completes, then the first-drawn card resolves and
/// is discarded before the second begins. The FAQ lets the player choose the
/// order; this is the kept narrowing, tracked by #1130.
/// `data/official-faq/Frequently_Asked_Questions.md`: *"once all of the cards
/// have been drawn, you must resolve all Revelation abilities on those cards
/// (in an order of your choosing)."*
#[test]
fn two_drawn_weaknesses_resolve_in_draw_order() {
    let session = board(seat(1).hand([DRAW_TWO]).deck([ONE_SHOT_A, ONE_SHOT_B])).take(
        &TurnAction::PlayCard {
            investigator: INV,
            source: PlaySource::Hand(0),
        },
    );

    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.resources, 3, "both Revelations resolved");
    assert_event_sequence!(
        session.events(),
        Event::CardRevealed { code, .. } if code.as_str() == ONE_SHOT_A,
        Event::CardRevealed { code, .. } if code.as_str() == ONE_SHOT_B,
        Event::ResourcesGained { amount: 1, .. },
        Event::CardDiscarded { code, .. } if code.as_str() == ONE_SHOT_A,
        Event::ResourcesGained { amount: 2, .. },
        Event::CardDiscarded { code, .. } if code.as_str() == ONE_SHOT_B,
    );
}

/// **A bearer defeated mid-resolution takes their weakness with them** (#1121,
/// ADR 0002). The drawer's deck is empty, so the draw reshuffles their discard
/// — the weakness — back in, draws it, and then deals the deck-out horror
/// first, which defeats them before the weakness's Revelation resolves.
/// `glossary/Drawing_Cards.md`: *"that investigator shuffles his or her discard
/// pile back into his or her deck, then draws the card, and upon completion of
/// the entire draw takes one horror."*
///
/// The weakness is in limbo, so elimination step 1 removes it from the game
/// with the rest of its bearer's cards; its Revelation never resolves, and its
/// disposal does not file it into the discard pile step 1 has already removed.
#[test]
fn a_weakness_resolving_when_its_bearer_is_defeated_is_removed_from_the_game() {
    // Sanity 8: 7 horror already, so the deck-out horror is the eighth.
    let session = draw(board(seat(1).harm(0, 7).discard([ONE_SHOT_A])));

    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.status, Status::Defeated);
    assert_eq!(inv.removed_from_game, vec![CardCode::new(ONE_SHOT_A)]);
    assert!(inv.discard.is_empty());
    assert!(inv.hand.is_empty());
    assert_no_event!(session.events(), Event::ResourcesGained { .. });
    assert_no_event!(session.events(), Event::CardDiscarded { .. });
}
