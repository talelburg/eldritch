use super::*;
use crate::state::GameStateBuilder;

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
