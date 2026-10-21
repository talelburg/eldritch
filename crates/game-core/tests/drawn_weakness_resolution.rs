//! Weaknesses drawn together from their bearer's deck resolve as encounter
//! cards in draw order, against a mock registry. `glossary/Weakness.md`:
//!
//! > When an investigator draws a weakness with an encounter cardtype *(for
//! > example, an enemy or a treachery weakness)*, resolve that card as if it
//! > were just drawn from the encounter deck.
//!
//! The one-shot weakness treacheries themselves (Amnesia 01096, Paranoia 01097,
//! Abandoned and Alone 01015) are tested from their printed cards in
//! `crates/cards/tests`. What stays here is the ordering of several
//! weaknesses drawn by one draw, which no core card can stage: none draws more
//! than one card at once. So two synthetic one-shot weakness treacheries and a
//! synthetic "draw 2" event model the primitive (ADR 0016). Its own test
//! binary: the mock registry is built once per process.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, WeaknessKind};
use card_dsl::dsl::{self, InvestigatorTarget};
use game_core::assert_event_sequence;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, seat, MockRegistry, Seat, TestSession};

/// A one-shot weakness treachery whose Revelation gains its drawer 1 resource.
const ONE_SHOT_A: &str = "TEST-DWR-ONE-SHOT-A";
/// A second, gaining 2, so the two Revelations' order shows.
const ONE_SHOT_B: &str = "TEST-DWR-ONE-SHOT-B";
/// A 0-cost event: *"Draw 2 cards."*
const DRAW_TWO: &str = "TEST-DWR-DRAW-TWO";

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

/// **Two weaknesses drawn together resolve in draw order**: both are revealed
/// once the draw completes, then the first-drawn card resolves and is
/// discarded before the second begins. The FAQ lets the player choose the
/// order; this is the kept narrowing. TODO(#1130): let the player choose.
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
