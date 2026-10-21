//! Regression for #482: on the act path, an *interactive* act on-advance
//! reverse (a synthetic Forced `ChooseOne`) resolves cleanly through the
//! `AdvanceReverse` frame — it does not strand, mirroring the agenda path.
//!
//! No such card exists in the corpus, so we build a mock registry that gives
//! act code `_iact` a `ChooseOne` reverse.

use card_dsl::dsl::{self, EventPattern, EventTiming, InvestigatorTarget};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::InputKind;
use game_core::engine::Registries;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{Act, CardCode, GameStateBuilder, InvestigatorId};
use game_core::test_support::{seat, MockRegistry, TestSession};
use std::sync::LazyLock;

const IACT: &str = "_iact";

/// The binary's one mock, built once on first use.
fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(IACT, || {
                vec![dsl::forced_on_event(
                    EventPattern::ActAdvanced,
                    EventTiming::After,
                    // Two always-legal branches ⇒ the choice suspends.
                    dsl::choose_one(vec![
                        (
                            "Take 1 horror",
                            dsl::deal_horror(InvestigatorTarget::You, 1u8),
                        ),
                        (
                            "Take 2 horror",
                            dsl::deal_horror(InvestigatorTarget::You, 2u8),
                        ),
                    ]),
                )]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

#[test]
fn interactive_act_reverse_resolves_cleanly() {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_registries(registries())
        // 1 clue funds the AdvanceAct clue-spend (threshold 1 below).
        .with_seat(seat(1).clues(1))
        .open_turn(inv)
        .build();
    // Two acts: the leaving one (_iact) carries the interactive reverse; a
    // successor so the advance is non-terminal. _iact has a positive clue
    // threshold (the AdvanceAct action requires one) and the
    // investigator is funded for it above.
    state.act_deck = vec![
        Act {
            code: CardCode(IACT.into()),
            clue_threshold: 1,
        },
        Act {
            code: CardCode("_iact_2".into()),
            clue_threshold: 3,
        },
    ];
    state.act_index = 0;

    let session =
        TestSession::new(state, registries()).take(&TurnAction::AdvanceAct { investigator: inv });

    // The act's interactive reverse is the live prompt (it did not strand); the
    // act cursor has NOT bumped yet (Finalize runs after the choice).
    let request = session.prompt();
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert_eq!(
        session.state().act_index,
        0,
        "cursor bumps only after the reverse resolves"
    );
}
