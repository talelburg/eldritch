use super::*;
use crate::state::GameStateBuilder;

#[test]
fn awaits_input_gates_suspensions_but_not_anchors_or_fast_windows() {
    // slice 1b: the one guard rule keys off this. Phase anchors are inert
    // (open turn / loop-driven), so typed actions run there.
    assert!(!Continuation::InvestigationPhase {
        resume: InvestigationResume::TurnBegins,
    }
    .awaits_input());
    assert!(!Continuation::MythosPhase {
        resume: MythosResume::Entry,
    }
    .awaits_input());
    // A Fast-play window (a `FastWindow` with no pending candidates) is a
    // play opportunity, not a mandatory prompt — Fast plays stay allowed.
    assert!(!Continuation::FastWindow {
        candidates: Vec::new(),
        fast_actors: FastActorScope::Any,
        kind: FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
    }
    .awaits_input());
    // Every other suspension hits the `_ => true` arm and awaits
    // ResolveInput. This includes a `Choice` (e.g. a `ChooseOne` OnPlay
    // event mid-resolution) and a `SubstitutionPrompt`, which the former
    // eight-block guard ladder did NOT explicitly gate — the unified rule
    // now correctly rejects typed actions while one is on top.
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
        Continuation::FastWindow { .. }
    ));
    // The read accessor surfaces it as the former `open_windows` view.
    assert_eq!(state.open_windows().len(), 1);
    assert!(matches!(
        state.open_windows()[0],
        Continuation::FastWindow {
            kind: FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
            ..
        }
    ));
}
