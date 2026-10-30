//! [`Quantity::ControlledCards`] end to end: the cards matching a filter that
//! the controller has in their play area and their threat area, counted as an
//! effect resolves. Daisy Walker 01002's `[elder_sign]` is the motivating
//! reader: *"+0. If you succeed, draw 1 card for each \[\[Tome\]\] you
//! control."*
//!
//! Synthetic cards only (ADR 0016): a probe investigator whose elder sign
//! draws one card per `Probe`-trait card they control, and probe assets with
//! and without the trait. Its own integration-test binary, so its
//! `MockRegistry` gets a process to itself. Every assertion is on events or
//! state crossing the `apply` boundary.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use card_dsl::dsl::{self, CardFilter, IntExpr, InvestigatorTarget, Quantity, TokenOutcome};
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, SkillKind,
};
use game_core::test_support::{
    self, seat, test_investigator_card, MockRegistry, Printed, Seat, TestSession,
};
use game_core::{assert_event, assert_no_event};

const TRAIT: &str = "Probe";
/// Elder sign: `+0`, and on success draw 1 card per `Probe` card controlled.
/// The investigator card carries the trait itself, so counting it would show.
const COUNTER: &str = "_cc_counter";
/// An asset with the `Probe` trait.
const MATCHING: &str = "_cc_matching";
/// An asset without it.
const OTHER: &str = "_cc_other";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

fn asset(code: &str, traits: &[&str]) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: code.to_owned(),
        traits: traits.iter().map(|t| (*t).to_owned()).collect(),
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: None,
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            slots: vec![],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 1,
            uses: None,
            play_only_during_turn: false,
        },
    }
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(CardMetadata {
                traits: vec![TRAIT.to_owned()],
                ..test_investigator_card(COUNTER)
            })
            .with_abilities(COUNTER, || {
                vec![dsl::elder_sign_with(TokenOutcome {
                    on_success: Some(dsl::draw_cards(
                        InvestigatorTarget::You,
                        IntExpr::Count(Quantity::ControlledCards(Box::new(
                            CardFilter::with_trait(TRAIT),
                        ))),
                    )),
                    ..TokenOutcome::default()
                })]
            })
            .with_card(asset(MATCHING, &[TRAIT]))
            .with_card(asset(OTHER, &["Item"]))
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// The counter at `HERE` with a four-card deck, drawing only `[elder_sign]`.
fn counter() -> Seat<Printed> {
    seat(1)
        .as_card(COUNTER)
        .at(HERE)
        .deck(["90001", "90002", "90003", "90004"])
}

/// `investigator` passes a willpower test (3 + 0 against 3) on the elder sign.
fn pass_on_the_elder_sign(investigator: Seat<Printed>) -> TestSession {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(investigator)
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::ElderSign]))
        .open_turn(INV)
        .build();
    let session = TestSession::new(state, registries())
        .test_skill(INV, SkillKind::Willpower, 3)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    session
}

/// One matching card in play and two in the threat area count; the
/// non-matching card in play does not.
#[test]
fn counts_matching_cards_in_the_play_area_and_the_threat_area() {
    let session = pass_on_the_elder_sign(
        counter()
            .in_play([MATCHING, OTHER])
            .threat_area([MATCHING, MATCHING]),
    );
    assert_event!(
        session.events(),
        Event::CardsDrawn {
            investigator: INV,
            count: 3,
        }
    );
    assert_eq!(session.state().investigators[&INV].hand.len(), 3);
}

/// The investigator card carries the trait but is not counted: with nothing
/// else in play the count is 0, and a 0 draw draws nothing.
#[test]
fn the_investigator_card_is_not_counted() {
    let session = pass_on_the_elder_sign(counter());
    assert_no_event!(session.events(), Event::CardsDrawn { .. });
    assert!(session.state().investigators[&INV].hand.is_empty());
}
