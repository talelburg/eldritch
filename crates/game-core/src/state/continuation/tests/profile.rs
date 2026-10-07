use card_dsl::dsl::Effect;

use super::*;
use crate::engine::evaluator::EvalContext;
use crate::engine::TimingEvent;
use crate::test_support;

fn candidate() -> ResolutionCandidate {
    ResolutionCandidate::new(
        CardCode::new("01022"),
        InvestigatorId(1),
        AbilityAddress::Printed(0),
        CandidateSource::Hand,
    )
}

fn timing_point_window(candidates: Vec<ResolutionCandidate>) -> Continuation {
    Continuation::TimingPointWindow(TimingPointWindowFrame {
        event: TimingEvent::RoundEnded,
        bucket: EventTiming::After,
        mode: TimingMode::Reaction,
        candidates,
    })
}

fn fast_window(candidates: Vec<ResolutionCandidate>) -> Continuation {
    Continuation::FastWindow(FastWindowFrame {
        candidates,
        fast_actors: FastActorScope::Any,
        kind: FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
    })
}

fn attack_loop(stage: AttackLoopStage) -> Continuation {
    Continuation::AttackLoop(AttackLoopFrame {
        investigator: InvestigatorId(1),
        remaining_attackers: vec![EnemyId(2), EnemyId(3)],
        source: EnemyAttackSource::EnemyPhase,
        stage,
    })
}

fn deal_damage(step: DealDamageStep) -> Continuation {
    Continuation::DealDamage(DealDamageFrame {
        investigator: InvestigatorId(1),
        source: DamageSource::Effect,
        assignment: Assignment::default(),
        step,
    })
}

fn investigator_turn(ending: bool) -> Continuation {
    Continuation::InvestigatorTurn {
        investigator: InvestigatorId(1),
        ending,
    }
}

// --- The eight corrected answers (#925 / #927) -----------------------------
//
// Before the profile, `awaits_input` ended in a `!is_phase_anchor()` catch-all
// that answered `true` for six framework-internal frames input routing rejects,
// and `false` for two frames routing accepts. Each is pinned here so the
// correction is explicit rather than a side effect.

#[test]
fn an_encounter_card_disposal_does_not_await_input() {
    let f = Continuation::EncounterCard(EncounterCardFrame {
        card: CardCode::new("01163"),
        disposition: EncounterDisposition::Discard,
    });
    assert!(!f.awaits_input());
}

#[test]
fn a_hand_play_disposal_does_not_await_input() {
    let f = Continuation::PlayFromHand(PlayFromHandFrame {
        investigator: InvestigatorId(1),
        card: Some(CardCode::new("01022")),
    });
    assert!(!f.awaits_input());
}

#[test]
fn the_entered_location_half_of_a_move_does_not_await_input() {
    let f = Continuation::MoveEnter {
        investigator: InvestigatorId(1),
        destination: LocationId(2),
    };
    assert!(!f.awaits_input());
}

#[test]
fn a_mythos_surge_chain_does_not_await_input() {
    let f = Continuation::PlayerDraw(PlayerDrawFrame {
        investigator: InvestigatorId(1),
        chain_count: 0,
        surge_pending: false,
    });
    assert!(!f.awaits_input());
}

#[test]
fn the_emit_event_coordinator_does_not_await_input() {
    let f = Continuation::EmitEvent(EmitEventFrame {
        event: TimingEvent::RoundEnded,
        step: EmitStep::When,
    });
    assert!(!f.awaits_input());
}

#[test]
fn the_timing_point_coordinator_does_not_await_input() {
    let f = Continuation::TimingPoint(TimingPointFrame {
        event: TimingEvent::RoundEnded,
        bucket: EventTiming::When,
        sub: TimingSub::Forced,
    });
    assert!(!f.awaits_input());
}

#[test]
fn an_attack_loop_at_its_order_pick_awaits_input() {
    assert!(attack_loop(AttackLoopStage::PickOrder).awaits_input());
}

/// D1(a) on #927: a framework Fast window is a prompt whether or not it holds
/// candidates — `Skip` closes it through `resolve_input` (#476).
#[test]
fn an_empty_fast_window_awaits_input() {
    assert!(fast_window(Vec::new()).awaits_input());
}

// --- Value-level splits ----------------------------------------------------
//
// Frames whose profile depends on their payload, not just their variant.

const DRIVEN: FrameActivity = FrameActivity::Driven;
const PROMPT: FrameActivity = FrameActivity::Prompt;
const INERT: FrameActivity = FrameActivity::Inert;
const CANCEL: ScenarioEndDisposition = ScenarioEndDisposition::Cancel;
const COMPLETE: ScenarioEndDisposition = ScenarioEndDisposition::Complete;

fn profile(activity: FrameActivity, scenario_end: ScenarioEndDisposition) -> FrameProfile {
    FrameProfile {
        activity,
        scenario_end,
    }
}

/// An event window with candidates is the prompt; an empty one is closed by the
/// `drive` loop on sight (its candidates are exhausted only by firing).
#[test]
fn a_timing_point_window_is_a_prompt_only_while_it_has_candidates() {
    assert_eq!(
        timing_point_window(vec![candidate()]).profile().activity,
        PROMPT
    );
    assert_eq!(timing_point_window(Vec::new()).profile().activity, DRIVEN);
}

/// D1(a) on #927: the candidates split is the event window's alone.
#[test]
fn a_fast_window_is_a_prompt_with_or_without_candidates() {
    assert_eq!(fast_window(vec![candidate()]).profile().activity, PROMPT);
    assert_eq!(fast_window(Vec::new()).profile().activity, PROMPT);
}

#[test]
fn the_open_turn_is_a_prompt_and_its_ending_tail_is_driven() {
    assert_eq!(investigator_turn(false).profile().activity, PROMPT);
    assert_eq!(investigator_turn(true).profile().activity, DRIVEN);
}

#[test]
fn a_deal_of_damage_is_a_prompt_only_while_distributing() {
    let distribute = DealDamageStep::Distribute {
        remaining_damage: 1,
        remaining_horror: 0,
    };
    assert_eq!(deal_damage(distribute).profile().activity, PROMPT);
    for step in [
        DealDamageStep::Announce,
        DealDamageStep::Place,
        DealDamageStep::Finish,
    ] {
        assert_eq!(
            deal_damage(step.clone()).profile().activity,
            DRIVEN,
            "{step:?}"
        );
    }
}

#[test]
fn an_attack_loop_is_a_prompt_at_its_order_pick_and_driven_while_attacking() {
    assert_eq!(
        attack_loop(AttackLoopStage::PickOrder).profile().activity,
        PROMPT
    );
    assert_eq!(
        attack_loop(AttackLoopStage::Attacking).profile().activity,
        DRIVEN
    );
}

/// The ending emits `GameEnd` when driven, then rests for the apply boundary
/// to finalize — neither the loop nor input routing advances it there.
#[test]
fn the_ending_frame_is_driven_until_it_rests_inert_at_finalize() {
    let ending = |step| Continuation::ScenarioEnd(ScenarioEndFrame { step });
    assert_eq!(
        ending(ScenarioEndStep::EmitGameEnd).profile().activity,
        DRIVEN
    );
    assert_eq!(ending(ScenarioEndStep::Finalize).profile().activity, INERT);
}

/// ADR 0004: the reaction window is an opportunity, its forced-run twin is
/// mandatory resolution — the disposition's one value-level split.
#[test]
fn a_reaction_window_cancels_and_its_forced_run_twin_completes() {
    let window = |mode| {
        Continuation::TimingPointWindow(TimingPointWindowFrame {
            event: TimingEvent::GameEnd,
            bucket: EventTiming::After,
            mode,
            candidates: vec![candidate()],
        })
    };
    assert_eq!(window(TimingMode::Reaction).profile().scenario_end, CANCEL);
    assert_eq!(window(TimingMode::Forced).profile().scenario_end, COMPLETE);
}

// --- Every variant ---------------------------------------------------------

/// One row per variant (and per value-level split), so each profile is pinned
/// here as well as decided by the exhaustive match in `profile`.
///
/// Shared with the stack's wire-format test (`stack.rs`), whose fixture is
/// these frames serialised in the pre-#928 format.
// One flat table reads better than splitting it by role across functions.
#[allow(clippy::too_many_lines)]
pub(super) fn every_variant_rows() -> Vec<(Continuation, FrameProfile)> {
    let ctx = EvalContext::for_controller(InvestigatorId(1));
    vec![
        // Windows.
        (timing_point_window(Vec::new()), profile(DRIVEN, CANCEL)),
        (
            timing_point_window(vec![candidate()]),
            profile(PROMPT, CANCEL),
        ),
        (
            Continuation::TimingPointWindow(TimingPointWindowFrame {
                event: TimingEvent::RoundEnded,
                bucket: EventTiming::When,
                mode: TimingMode::Forced,
                candidates: vec![candidate(), candidate()],
            }),
            profile(PROMPT, COMPLETE),
        ),
        (fast_window(Vec::new()), profile(PROMPT, CANCEL)),
        (fast_window(vec![candidate()]), profile(PROMPT, CANCEL)),
        // Mandatory resolution that surfaces its own prompt.
        (
            Continuation::AdvanceReverse(AdvanceReverseFrame {
                deck: AdvanceDeck::Agenda,
                from: 0,
                leaving_code: CardCode::new("01105"),
                step: AdvanceStep::AwaitAck,
                trigger: AdvanceTrigger::Forced,
            }),
            profile(PROMPT, COMPLETE),
        ),
        (
            Continuation::AcknowledgeForced(AcknowledgeForcedFrame {
                candidate: candidate(),
            }),
            profile(PROMPT, COMPLETE),
        ),
        (
            Continuation::SkillTest(test_support::test_skill_test(
                SkillTestId(0),
                InvestigatorId(1),
                SkillKind::Willpower,
                SkillTestKind::Plain,
                3,
            )),
            profile(PROMPT, COMPLETE),
        ),
        (
            Continuation::SubstitutionPrompt(SubstitutionPromptFrame {
                investigator: InvestigatorId(1),
            }),
            profile(PROMPT, COMPLETE),
        ),
        (
            Continuation::SlotDiscard(SlotDiscardFrame {
                investigator: InvestigatorId(1),
                card: None,
                entry: AssetEntry::PlayedFromHand,
            }),
            profile(PROMPT, COMPLETE),
        ),
        (
            Continuation::Effect(EffectFrame::Leaf {
                effect: Box::new(Effect::Seq(vec![])),
                ctx,
            }),
            profile(PROMPT, COMPLETE),
        ),
        (
            Continuation::Effect(EffectFrame::Seq {
                effects: vec![],
                next: 0,
                ctx,
            }),
            profile(PROMPT, COMPLETE),
        ),
        (
            deal_damage(DealDamageStep::Distribute {
                remaining_damage: 0,
                remaining_horror: 1,
            }),
            profile(PROMPT, COMPLETE),
        ),
        // Framework prompts the ended scenario cancels.
        (
            Continuation::HunterMove(HunterChoice::Move {
                enemy: EnemyId(3),
                candidates: vec![LocationId(2), LocationId(3)],
            }),
            profile(PROMPT, CANCEL),
        ),
        (
            Continuation::SpawnEngage(SpawnEngagePending {
                enemy: EnemyId(2),
                candidates: vec![InvestigatorId(1), InvestigatorId(2)],
            }),
            profile(PROMPT, CANCEL),
        ),
        (
            Continuation::HandSizeDiscard(HandSizeDiscard {
                remaining: vec![InvestigatorId(1)],
            }),
            profile(PROMPT, CANCEL),
        ),
        (
            Continuation::Mulligan(MulliganFrame {
                remaining: vec![InvestigatorId(1)],
            }),
            profile(PROMPT, CANCEL),
        ),
        (
            Continuation::EncounterDraw(EncounterDrawFrame {
                remaining: vec![InvestigatorId(1)],
            }),
            profile(PROMPT, CANCEL),
        ),
        (investigator_turn(false), profile(PROMPT, CANCEL)),
        (
            attack_loop(AttackLoopStage::PickOrder),
            profile(PROMPT, CANCEL),
        ),
        // Framework sequence the loop drives.
        (investigator_turn(true), profile(DRIVEN, CANCEL)),
        (
            attack_loop(AttackLoopStage::Attacking),
            profile(DRIVEN, CANCEL),
        ),
        (
            Continuation::PlayerDraw(PlayerDrawFrame {
                investigator: InvestigatorId(1),
                chain_count: 1,
                surge_pending: true,
            }),
            profile(DRIVEN, CANCEL),
        ),
        // Internal sequencing the loop drives, which completes.
        (
            Continuation::EmitEvent(EmitEventFrame {
                event: TimingEvent::RoundEnded,
                step: EmitStep::After,
            }),
            profile(DRIVEN, COMPLETE),
        ),
        (
            Continuation::TimingPoint(TimingPointFrame {
                event: TimingEvent::RoundEnded,
                bucket: EventTiming::After,
                sub: TimingSub::Reaction,
            }),
            profile(DRIVEN, COMPLETE),
        ),
        (
            Continuation::EncounterCard(EncounterCardFrame {
                card: CardCode::new("01116"),
                disposition: EncounterDisposition::Spawn {
                    investigator: InvestigatorId(1),
                },
            }),
            profile(DRIVEN, COMPLETE),
        ),
        (
            Continuation::PlayFromHand(PlayFromHandFrame {
                investigator: InvestigatorId(1),
                card: None,
            }),
            profile(DRIVEN, COMPLETE),
        ),
        (
            Continuation::MoveEnter {
                investigator: InvestigatorId(1),
                destination: LocationId(2),
            },
            profile(DRIVEN, COMPLETE),
        ),
        (
            Continuation::ActionResolution {
                investigator: InvestigatorId(1),
                resume: ActionResume::Resource,
            },
            profile(DRIVEN, COMPLETE),
        ),
        (
            deal_damage(DealDamageStep::Place),
            profile(DRIVEN, COMPLETE),
        ),
        (
            Continuation::Elimination(EliminationFrame {
                investigator: InvestigatorId(1),
                step: EliminationStep::FireWeaknessGameEnd,
            }),
            profile(DRIVEN, COMPLETE),
        ),
        (
            Continuation::Elimination(EliminationFrame {
                investigator: InvestigatorId(1),
                step: EliminationStep::RunSteps,
            }),
            profile(DRIVEN, COMPLETE),
        ),
        (
            Continuation::ScenarioEnd(ScenarioEndFrame {
                step: ScenarioEndStep::EmitGameEnd,
            }),
            profile(DRIVEN, COMPLETE),
        ),
        (
            Continuation::ScenarioEnd(ScenarioEndFrame {
                step: ScenarioEndStep::Finalize,
            }),
            profile(INERT, COMPLETE),
        ),
        // Phase anchors: woken only when a child frame pops.
        (
            Continuation::MythosPhase {
                resume: MythosResume::Entry,
            },
            profile(INERT, CANCEL),
        ),
        (
            Continuation::InvestigationPhase {
                resume: InvestigationResume::TurnBegins,
            },
            profile(INERT, CANCEL),
        ),
        (
            Continuation::EnemyPhase {
                resume: EnemyResume::BeforeInvestigatorAttacked,
                attacking: None,
            },
            profile(INERT, CANCEL),
        ),
        (
            Continuation::UpkeepPhase {
                resume: UpkeepResume::Begins,
            },
            profile(INERT, CANCEL),
        ),
    ]
}

#[test]
fn every_frame_has_the_profile_its_role_requires() {
    for (frame, expected) in every_variant_rows() {
        assert_eq!(frame.profile(), expected, "{frame:?}");
    }
}
