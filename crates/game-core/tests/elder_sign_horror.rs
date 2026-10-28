//! An elder-sign modifier of `Count(Quantity::HorrorOnSource)`, read
//! live at ST.5 (ADR 0005).
//!
//! The motivating consumer is Agnes Baker 01004
//! (`data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > \[elder_sign\] effect: +1 for each horror on Agnes Baker.
//!
//! Synthetic investigators only (ADR 0016): each models the primitive, and
//! none stands in for the printed card. Its own integration-test binary, so
//! its `MockRegistry` gets a process to itself.

use std::sync::LazyLock;

use card_dsl::dsl::{self, Effect, HarmKind, IntExpr, InvestigatorTarget, Quantity, TokenOutcome};
use game_core::assert_event;
use game_core::engine::modified_value::{
    self, Contribution, ContributionSource, ModifiedQuantity, ReadContext,
};
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    ChaosBag, ChaosToken, GameState, GameStateBuilder, InvestigatorId, LocationId, ModifierTarget,
    SkillKind,
};
use game_core::test_support::{self, seat, test_investigator_card, MockRegistry, TestSession};

/// Elder sign: `+1 for each horror on you.`
const COUNTS_HORROR: &str = "_es_counts_horror";

/// Elder sign: `Take 2 horror. +1 for each horror on you.` The horror lands
/// at ST.4, between the reveal and the ST.5 read.
const TAKES_THEN_COUNTS: &str = "_es_takes_then_counts";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(test_investigator_card(COUNTS_HORROR))
            .with_abilities(COUNTS_HORROR, || {
                vec![dsl::elder_sign(IntExpr::Count(Quantity::HorrorOnSource))]
            })
            .with_card(test_investigator_card(TAKES_THEN_COUNTS))
            .with_abilities(TAKES_THEN_COUNTS, || {
                vec![dsl::elder_sign_with(TokenOutcome {
                    immediate: Some(Effect::Deal {
                        kind: HarmKind::Horror,
                        target: InvestigatorTarget::You,
                        amount: IntExpr::Lit(2),
                        direct: false,
                    }),
                    ..TokenOutcome::modifier(IntExpr::Count(Quantity::HorrorOnSource))
                })]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// The investigator printed as `code`, at `HERE`, with `horror` horror on
/// them, drawing only `[elder_sign]`, with play at their turn menu.
fn session(code: &str, horror: u8) -> TestSession {
    TestSession::new(board(code, horror), registries())
}

/// The board [`session`] starts from.
fn board(code: &str, horror: u8) -> GameState {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).as_card(code).at(HERE).harm(0, horror))
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::ElderSign]))
        .open_turn(INV)
        .build()
}

/// A difficulty-1 willpower test, committing nothing, run to the end.
fn willpower_test(code: &str, horror: u8) -> TestSession {
    let session = session(code, horror)
        .test_skill(INV, SkillKind::Willpower, 1)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn the_elder_sign_counts_one_per_horror_on_the_controller() {
    for (horror, margin) in [(0, 2), (2, 4), (5, 7)] {
        // Willpower 3 + `horror` against 1.
        let session = willpower_test(COUNTS_HORROR, horror);
        assert_event!(
            session.events(),
            Event::SkillTestSucceeded {
                investigator: INV,
                margin: m,
                ..
            } if *m == margin
        );
    }
}

/// ADR 0005: the row is the unevaluated expression, read at ST.5. Horror the
/// elder sign's own ST.4 part places is on the investigator by then, so it
/// counts; a value taken at the ST.3 reveal would have been 0.
#[test]
fn horror_placed_at_st4_counts_at_st5() {
    // No horror at the reveal; 2 by ST.5. Willpower 3 + 2 against 1.
    let session = willpower_test(TAKES_THEN_COUNTS, 0);
    assert_event!(
        session.events(),
        Event::HorrorTaken {
            investigator: INV,
            amount: 2,
        }
    );
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded {
            investigator: INV,
            margin: 4,
            ..
        }
    );
}

/// The ST.5 breakdown names the elder-sign row after the investigator card
/// whose ability it is, as the elder sign's ST.4 and ST.7 parts already are.
/// Read at the post-ST.6 acknowledge, while the test frame is still open.
#[test]
fn the_elder_sign_row_is_sourced_to_the_investigator_card() {
    let mut state = board(COUNTS_HORROR, 2);
    state.interactive_acknowledge = true;
    let session = TestSession::new(state, registries())
        .test_skill(INV, SkillKind::Willpower, 1)
        .commit(&[]);
    let state = session.state();
    let card = state.investigators[&INV].investigator_card.instance_id;
    let breakdown = modified_value::modified_value(
        state,
        registries().cards,
        ModifierTarget::Investigator(INV),
        ModifiedQuantity::Skill(SkillKind::Willpower),
        ReadContext::from_state(state),
    );
    assert_eq!(
        breakdown.contributions,
        vec![Contribution {
            source: ContributionSource::Recorded {
                instance: Some(card),
            },
            delta: 2,
        }]
    );
}
