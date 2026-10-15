//! "Until the end of the round" lasting effects expire at Upkeep 4.6 **before**
//! the round-end abilities initiate (#643, #1066).
//!
//! `glossary/Lasting_Effects.md`: *"A lasting effect expires as soon as the
//! timing point specified by its duration is reached. This means that an
//! "until the end of the phase" lasting effect expires before an "at the end of
//! the phase" ability or delayed effect may initiate."* Appendix II step 4.6:
//! *"Any active "until the end of the round" lasting effects expire at this
//! time."*
//!
//! The lasting effect is a round-scoped skill substitution (the Mind over
//! Matter 01036 shape: Intellect in place of Combat), registered by a mock
//! act's forced native at the end of the enemy phase. The reader is a mock
//! agenda's forced ability that initiates a Combat test, either at the end of
//! the upkeep phase (still inside the round) or at the end of the round. A
//! substitution that covers the test shows itself as the "use Intellect in
//! place of Combat?" decision before the commit window, and the test's margin
//! says which skill it was made with.
//!
//! Its own test binary: the mock registry is built once per process.

use std::sync::LazyLock;

use card_dsl::dsl::{self, EventPattern, EventTiming};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, InputKind, NativeCx, PromptNature, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    Act, Agenda, CardCode, ChaosBag, ChaosToken, GameState, GameStateBuilder, InvestigatorId,
    LocationId, Phase, SkillKind, SkillSubstitution,
};
use game_core::test_support::{self, seat, MockRegistry, TestSession};

/// Registers the round-scoped substitution at the end of the enemy phase.
const SUBSTITUTE_ACT: &str = "_rl_act_substitute";
/// Initiates a Combat test "at the end of the round".
const ROUND_END_AGENDA: &str = "_rl_agenda_round_end";
/// Initiates a Combat test at the end of the upkeep phase: step 4.6, but
/// still inside the round.
const UPKEEP_END_AGENDA: &str = "_rl_agenda_upkeep_end";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// Combat is the seat's default 3 and Intellect is raised to 5; against a
/// difficulty of 1 with a `0` token, the margin names the skill used.
const BY_COMBAT: i8 = 2;
const BY_INTELLECT: i8 = 4;

fn substitute(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    cx.substitute_skill_until_end_of_round(SkillSubstitution {
        investigator: ctx.controller,
        use_skill: SkillKind::Intellect,
        for_skills: vec![SkillKind::Combat],
    });
    EngineOutcome::Done
}

fn combat_test() -> dsl::Effect {
    dsl::skill_test(SkillKind::Combat, 1, None, None)
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(SUBSTITUTE_ACT, || {
                vec![dsl::forced_on_event(
                    EventPattern::PhaseEnded {
                        phase: dsl::Phase::Enemy,
                    },
                    EventTiming::After,
                    dsl::native("test:substitute"),
                )]
            })
            .with_native_effect("test:substitute", substitute)
            .with_abilities(ROUND_END_AGENDA, || {
                vec![dsl::forced_on_event(
                    EventPattern::RoundEnded,
                    EventTiming::At,
                    combat_test(),
                )]
            })
            .with_abilities(UPKEEP_END_AGENDA, || {
                vec![dsl::forced_on_event(
                    EventPattern::PhaseEnded {
                        phase: dsl::Phase::Upkeep,
                    },
                    EventTiming::At,
                    combat_test(),
                )]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// The lead on their open turn, with the substitution act current and
/// `agenda` as the current agenda. A filler deck keeps Upkeep 4.4's draw from
/// dealing empty-deck horror.
fn board(agenda: &str) -> TestSession {
    let mut state: GameState = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Intellect, 5)
                .deck(["filler-card"]),
        )
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .build();
    // No builder setter stages the act or agenda deck.
    state.act_deck = vec![Act {
        code: CardCode::new(SUBSTITUTE_ACT),
        clue_threshold: 99,
    }];
    state.act_index = 0;
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(agenda),
        doom_threshold: 10,
    }];
    state.agenda_index = 0;
    TestSession::new(state, registries())
}

/// Commit nothing to the test `session` is in and let it resolve: the margin
/// it succeeded by.
fn success_margin(session: TestSession) -> i8 {
    session
        .commit(&[])
        .events()
        .iter()
        .find_map(|event| match event {
            Event::SkillTestSucceeded { margin, .. } => Some(*margin),
            _ => None,
        })
        .expect("a difficulty-1 test against 3 or more succeeds")
}

#[test]
fn a_round_scoped_substitution_is_still_active_at_the_end_of_the_upkeep_phase() {
    // Ending the turn runs the enemy phase (whose end registers the
    // substitution) and the upkeep phase, up to its "at the end of the upkeep
    // phase" forced Combat test. The round has not ended, so the
    // substitution still covers it.
    let at_test = || board(UPKEEP_END_AGENDA).take(&TurnAction::EndTurn);
    let session = at_test();
    assert_eq!(session.state().phase, Phase::Upkeep);
    let prompt = session.prompt();
    assert_eq!(prompt.nature, PromptNature::Decision, "{prompt:?}");
    assert_eq!(prompt.options.len(), 2, "{prompt:?}");
    assert_eq!(
        success_margin(at_test().pick_nth(0)),
        BY_INTELLECT,
        "the substitution covers a test inside the round",
    );
}

#[test]
fn a_round_scoped_substitution_has_expired_when_a_round_end_ability_initiates() {
    // Same round, but the forced Combat test is "at the end of the round":
    // the substitution expired as step 4.6 reached the round end, so the
    // test goes straight to its commit window and is made with Combat.
    let session = board(ROUND_END_AGENDA).take(&TurnAction::EndTurn);
    assert_eq!(session.state().phase, Phase::Upkeep);
    let prompt = session.prompt();
    assert_eq!(prompt.kind, InputKind::PickMultiple, "{prompt:?}");
    assert_eq!(
        success_margin(session),
        BY_COMBAT,
        "the round-end ability sees the printed Combat",
    );
}
