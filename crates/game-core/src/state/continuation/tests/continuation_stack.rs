use card_dsl::dsl::Effect;

use super::*;
use crate::engine::evaluator::EvalContext;
use crate::engine::TimingEvent;
use crate::state::{Continuation, EffectFrame, EmitStep, GameStateBuilder, InvestigatorId};

#[test]
fn awaits_input_gates_suspensions_but_not_anchors() {
    // Phase anchors are inert: they wake only when a child frame pops.
    assert!(!Continuation::InvestigationPhase {
        resume: InvestigationResume::TurnBegins,
    }
    .awaits_input());
    assert!(!Continuation::MythosPhase {
        resume: MythosResume::Entry,
    }
    .awaits_input());
    // A framework Fast window is a prompt even with no pending candidates:
    // `ResolveInput::Skip` closes it (#476; D1(a) on #927).
    assert!(Continuation::FastWindow(FastWindowFrame {
        candidates: Vec::new(),
        fast_actors: FastActorScope::Any,
        kind: FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
    })
    .awaits_input());
    // Suspensions awaiting `ResolveInput`.
    assert!(Continuation::SubstitutionPrompt {
        investigator: InvestigatorId(1),
    }
    .awaits_input());
    assert!(Continuation::Mulligan { remaining: vec![] }.awaits_input());
    assert!(Continuation::EncounterDraw { remaining: vec![] }.awaits_input());
}

#[test]
fn investigator_turn_frame_classification() {
    let frame = Continuation::InvestigatorTurn {
        investigator: InvestigatorId(1),
        ending: false,
    };
    // The open turn is not a framework anchor...
    assert!(!frame.is_phase_anchor());
    // ...and it DOES await input: the open turn surfaces its legal-action
    // enumeration as an `AwaitingInput` menu, resolved by
    // `ResolveInput(PickSingle(OptionId))` (2b, #447).
    assert!(frame.awaits_input());
    // The transient `ending: true` rotation sentinel is not a prompt.
    assert!(!Continuation::InvestigatorTurn {
        investigator: InvestigatorId(1),
        ending: true,
    }
    .awaits_input());
    // It carries no window candidates (the menu is re-enumerated, not stored).
    assert!(frame.pending_candidates().is_none());
}

#[test]
fn investigator_turn_frame_round_trips_both_ending_states() {
    // The frame is replay state (the `ending` flag absorbed the former
    // `pending_end_turn`), so both flag values must serialize round-trip.
    for ending in [false, true] {
        let frame = Continuation::InvestigatorTurn {
            investigator: InvestigatorId(1),
            ending,
        };
        let json = serde_json::to_string(&frame).unwrap();
        let back: Continuation = serde_json::from_str(&json).unwrap();
        assert_eq!(frame, back);
    }
}

#[test]
fn phase_anchor_variants_round_trip_and_are_not_resolution_windows() {
    let anchors = [
        Continuation::MythosPhase {
            resume: MythosResume::AfterDraws,
        },
        Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        },
        Continuation::EnemyPhase {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: Some(InvestigatorId(3)),
        },
        Continuation::UpkeepPhase {
            resume: UpkeepResume::Begins,
        },
    ];
    for a in anchors {
        // Anchors are framework frames, never reaction windows.
        assert!(a.pending_candidates().is_none());
        // Serializable like every other frame.
        let json = serde_json::to_string(&a).unwrap();
        let back: Continuation = serde_json::from_str(&json).unwrap();
        assert_eq!(a, back);
    }
}

#[test]
fn omitting_any_required_field_is_rejected() {
    // The non-`Option` formerly-`#[serde(default)]` fields are now required
    // on the wire (#453): a payload missing one fails loudly rather than
    // silently defaulting (e.g. an absent `continuations` would drop every
    // open window).
    let s = GameStateBuilder::new().build();
    let full = serde_json::to_value(&s).expect("serialize");
    serde_json::from_value::<GameState>(full.clone()).expect("full object deserializes");
    for field in [
        "continuations",
        "pending_cancellation",
        "skill_substitutions",
        // #676's two: a defaulted `skill_test_ids` would re-mint ids a
        // live row already names, and a defaulted `recorded_modifiers`
        // would silently drop every test-scoped buff in flight.
        "skill_test_ids",
        "recorded_modifiers",
    ] {
        let mut v = full.clone();
        v.as_object_mut()
            .expect("state serializes to a JSON object")
            .remove(field)
            .unwrap_or_else(|| panic!("`{field}` should be present in the serialized form"));
        assert!(
            serde_json::from_value::<GameState>(v).is_err(),
            "omitting `{field}` must be rejected, not defaulted"
        );
    }
}

#[test]
fn open_window_lives_on_the_continuation_stack_as_a_fast_window() {
    // A framework window is a `Continuation::FastWindow` frame on the one
    // stack (#433 A-ii) — there is no separate `open_windows` Vec.
    let state = GameStateBuilder::new()
        .with_open_window(
            FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
            FastActorScope::Any,
        )
        .build();
    assert_eq!(state.continuations.len(), 1);
    assert!(matches!(
        state.continuations[0],
        Continuation::FastWindow(_)
    ));
    // The read accessor surfaces it as the former `open_windows` view.
    assert_eq!(state.open_windows().len(), 1);
    assert!(matches!(
        state.open_windows()[0],
        Continuation::FastWindow(FastWindowFrame {
            kind: FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
            ..
        })
    ));
}

#[test]
fn action_resolution_frame_never_awaits_input_and_is_not_a_phase_anchor() {
    let f = Continuation::ActionResolution {
        investigator: InvestigatorId(1),
        resume: ActionResume::Resource,
    };
    assert!(
        !f.awaits_input(),
        "a mid-action frame is internal, never a prompt"
    );
    assert!(
        !f.is_phase_anchor(),
        "a mid-action frame is not a phase anchor"
    );
}

#[test]
fn current_hand_size_discard_reads_the_frame() {
    // No frame → None.
    assert_eq!(
        GameStateBuilder::new().build().current_hand_size_discard(),
        None
    );
    // Top HandSizeDiscard frame → its first remaining investigator.
    let mut state = GameStateBuilder::new().build();
    state
        .continuations
        .push(Continuation::HandSizeDiscard(HandSizeDiscard {
            remaining: vec![InvestigatorId(2), InvestigatorId(3)],
        }));
    assert_eq!(state.current_hand_size_discard(), Some(InvestigatorId(2)));
}

#[test]
fn effect_frame_variant_roundtrips_serde() {
    let frame = Continuation::Effect(EffectFrame::Seq {
        effects: vec![Effect::Seq(vec![])],
        next: 0,
        ctx: EvalContext::for_controller(InvestigatorId(1)),
    });
    let json = serde_json::to_string(&frame).expect("serialize");
    let back: Continuation = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(frame, back);
}

#[test]
fn emit_step_walks_the_sequence_with_the_resolve_step_between_when_and_at() {
    let mut step = EmitStep::When;
    let mut walk = vec![step];
    while let Some(next) = step.next() {
        step = next;
        walk.push(step);
    }
    assert_eq!(
        walk,
        vec![
            EmitStep::When,
            EmitStep::ResolveCondition,
            EmitStep::At,
            EmitStep::After
        ],
        "the condition resolves between the `when` and `at` cells (RR Nested Sequences)"
    );
    assert!(
        EmitStep::ResolveCondition.cell().is_none(),
        "the resolve step scans no cell — it resolves the condition"
    );
}

#[test]
fn emit_event_frame_roundtrips_serde() {
    let frame = Continuation::EmitEvent(EmitEventFrame {
        event: TimingEvent::RoundEnded,
        step: EmitStep::ResolveCondition,
    });
    let json = serde_json::to_string(&frame).expect("serialize");
    let back: Continuation = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(frame, back);
}

#[test]
fn hand_size_discard_serde_roundtrip() {
    let original = HandSizeDiscard {
        remaining: vec![InvestigatorId(1), InvestigatorId(2)],
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: HandSizeDiscard = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
}

/// The one variant whose bucket splits on a field rather than on the
/// variant: a reaction window is an *opportunity* the ended scenario
/// cancels, while the forced run at the same timing point is *mandatory
/// resolution* that completes (ADR 0004). They are the two halves of one
/// `queue_event`, so a `matches!` on the variant alone would get one wrong.
#[test]
fn a_reaction_window_is_cancelled_but_its_forced_run_twin_completes() {
    let window = |mode| {
        Continuation::TimingPointWindow(TimingPointWindowFrame {
            event: TimingEvent::GameEnd,
            bucket: EventTiming::After,
            mode,
            candidates: Vec::new(),
        })
    };
    assert!(
        window(TimingMode::Reaction).cancelled_by_scenario_end(),
        "a reaction window must not open once the scenario has ended"
    );
    assert!(
        !window(TimingMode::Forced).cancelled_by_scenario_end(),
        "the forced ordering run carries mandatory abilities and completes"
    );
}

#[test]
fn the_ending_frame_is_inert_and_survives_its_own_cancellation_pass() {
    let f = Continuation::ScenarioEnd {
        step: ScenarioEndStep::EmitGameEnd,
    };
    assert!(
        !f.cancelled_by_scenario_end(),
        "the ending frame must not cancel itself"
    );
    assert!(
        !f.awaits_input(),
        "the acknowledge above the ending is the prompt, not the ending"
    );
    assert!(!f.is_phase_anchor());
    assert!(
        !f.is_queued_ability(),
        "the ending frame legitimately sits beneath a phase anchor until the \
         anchor is cancelled, so the #569 backstop must not flag it"
    );
}
