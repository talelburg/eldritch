use super::*;
use crate::test_support;

#[test]
fn open_window_serde_roundtrip() {
    // A framework window is a `FastWindow` frame on the stack (#433); the
    // whole `Continuation` serializes for replay.
    let window = Continuation::FastWindow(FastWindowFrame {
        candidates: Vec::new(),
        fast_actors: FastActorScope::Any,
        kind: FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
    });
    let json = serde_json::to_string(&window).expect("serialize");
    let back: Continuation = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, window);
}

/// The parked source rides the wire as the whole descriptor, not its
/// instance projection — an act's `on_fail` would otherwise come back
/// un-anchored on the far side of the chaos draw (#834). Same
/// break-without-migration posture as #707 / #709 / #735.
#[test]
fn an_in_flight_tests_parked_board_source_round_trips_through_serde() {
    let test = InFlightSkillTest {
        source: Some(AbilitySource::Act),
        ..test_support::test_skill_test(
            SkillTestId(0),
            InvestigatorId(1),
            SkillKind::Willpower,
            SkillTestKind::Plain,
            3,
        )
    };
    let json = serde_json::to_string(&test).expect("serialize");
    let back: InFlightSkillTest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.source, Some(AbilitySource::Act));
    assert_eq!(back, test);
}

#[test]
fn hand_candidate_serde_round_trips() {
    // A Fast event playable from hand (Axis C) rides ResolutionCandidate
    // with a `Hand` source — distinct from a board card's `None`/`Board`.
    let candidate = ResolutionCandidate {
        code: CardCode::new("01022"),
        controller: InvestigatorId(1),
        address: AbilityAddress::Printed(0),
        source: CandidateSource::Hand,
    };
    let json = serde_json::to_string(&candidate).expect("serialize");
    let back: ResolutionCandidate = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, candidate);
}
