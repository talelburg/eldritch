//! `Effect::ForEach` walks its targets one at a time, so a target's body can
//! change the board before a later target's turn comes.
//!
//! An investigator eliminated partway through the walk is skipped.
//!
//! No printed card's per-target body eliminates *another* target, so the case
//! runs on a synthetic native that models the situation, not a card (ADR 0016):
//! a forced ability on a mock act, the `native_exits.rs` idiom.
//!
//! Its own test binary: the mock registry is built once per process.

use std::sync::LazyLock;

use card_dsl::dsl::{self, EventPattern, EventTiming, InvestigatorTarget, InvestigatorTargetSet};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{self, Act, CardCode, GameStateBuilder, InvestigatorId, Status};
use game_core::test_support::{seat, MockRegistry, TestSession};

const ACT: &str = "_fe_act";
const DEFEAT_THE_SECOND: &str = "test:defeat-the-second";

const FIRST: InvestigatorId = InvestigatorId(1);
const SECOND: InvestigatorId = InvestigatorId(2);
const THIRD: InvestigatorId = InvestigatorId(3);

/// The body's second half: running for the first investigator, defeat the
/// second — who comes next in the walk.
fn defeat_the_second(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    if ctx.each_investigator() == Some(FIRST) {
        cx.defeat_investigator(SECOND);
    }
    EngineOutcome::Done
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(ACT, || {
                vec![dsl::forced_on_event(
                    EventPattern::PhaseEnded {
                        phase: dsl::Phase::Enemy,
                    },
                    EventTiming::After,
                    dsl::for_each(
                        InvestigatorTargetSet::All,
                        dsl::seq([
                            dsl::gain_resources(InvestigatorTarget::Each, 1),
                            dsl::native(DEFEAT_THE_SECOND),
                        ]),
                    ),
                )]
            })
            .with_native_effect(DEFEAT_THE_SECOND, defeat_the_second)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// Three investigators in player order 1, 2, 3; the first's body defeats the
/// second before the walk reaches them, so the second gains nothing and the
/// walk goes on to the third.
#[test]
fn an_investigator_eliminated_partway_through_the_walk_is_skipped() {
    let mut state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1))
        .with_seat(seat(2))
        .with_seat(seat(3))
        .with_seating([FIRST, SECOND, THIRD])
        .ending_enemy_phase()
        .build();
    // No builder setter stages the act deck.
    state.act_deck = vec![Act {
        code: CardCode::new(ACT),
        clue_threshold: 99,
    }];

    // Settling runs step 3.4: the Enemy phase ends, and the act's forced
    // ability walks its targets.
    let session = TestSession::new(state, registries());

    // The Upkeep phase that follows gives everyone a resource too: read only
    // what happened before it began.
    let gainers: Vec<InvestigatorId> = session
        .events()
        .iter()
        .take_while(|e| {
            !matches!(
                e,
                Event::PhaseStarted {
                    phase: state::Phase::Upkeep
                }
            )
        })
        .filter_map(|e| match e {
            Event::ResourcesGained { investigator, .. } => Some(*investigator),
            _ => None,
        })
        .collect();
    assert_eq!(gainers, vec![FIRST, THIRD]);
    assert_eq!(
        session.state().investigators[&SECOND].status,
        Status::Defeated
    );
}

/// `glossary/Priority_of_Simultaneous_Resolution.md`: *"these choices are made
/// in player order."* With the second seat the lead, the walk starts there
/// and wraps to the first seat last.
#[test]
fn investigators_walk_in_player_order_from_the_lead() {
    let mut state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1))
        .with_seat(seat(2))
        .with_seat(seat(3))
        .with_seating([FIRST, SECOND, THIRD])
        .with_lead(SECOND)
        .ending_enemy_phase()
        .build();
    state.act_deck = vec![Act {
        code: CardCode::new(ACT),
        clue_threshold: 99,
    }];

    let session = TestSession::new(state, registries());

    let gainers: Vec<InvestigatorId> = session
        .events()
        .iter()
        .take_while(|e| {
            !matches!(
                e,
                Event::PhaseStarted {
                    phase: state::Phase::Upkeep
                }
            )
        })
        .filter_map(|e| match e {
            Event::ResourcesGained { investigator, .. } => Some(*investigator),
            _ => None,
        })
        .collect();
    // The first seat's body defeats the second, but the walk has already
    // passed them.
    assert_eq!(gainers, vec![SECOND, THIRD, FIRST]);
}
