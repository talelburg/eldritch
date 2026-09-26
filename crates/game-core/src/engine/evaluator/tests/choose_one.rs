use super::*;

#[test]
fn choose_one_single_branch_auto_resolves() {
    // 1 legal option ⇒ auto-bind, no input round-trip.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let before = state.investigators[&id].resources;
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::choose_one([(
            "Gain 2 resources",
            dsl::gain_resources(InvestigatorTarget::You, 2),
        )]),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&id].resources, before + 2);
    assert!(state.continuations.is_empty(), "no choice frame for auto");
}

#[test]
fn choose_one_offers_only_its_live_branches() {
    // #664: the dead branch (discover at a 0-clue location) is filtered out,
    // leaving one live branch — which auto-resolves rather than prompting.
    let id = InvestigatorId(1);
    let mut state = state_with_clues_at_location(0);
    let before = state.investigators[&id].resources;
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::choose_one([
            (
                "Discover 1 clue",
                dsl::discover_clue(LocationTarget::YourLocation, 1),
            ),
            (
                "Gain 2 resources",
                dsl::gain_resources(InvestigatorTarget::You, 2),
            ),
        ]),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&id].resources,
        before + 2,
        "the sole live branch resolved",
    );
    assert!(
        state.continuations.is_empty(),
        "no prompt for one live branch"
    );
}

#[test]
fn choose_one_with_every_branch_dead_skips() {
    // #664 / #639: filtered-to-empty is a skip, not a reject — rejecting
    // would unwind whatever already resolved above it (a skill test's draw).
    let mut state = state_with_clues_at_location(0);
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::choose_one([
            (
                "Discover 1 clue",
                dsl::discover_clue(LocationTarget::YourLocation, 1),
            ),
            (
                "Discover 1 clue again",
                dsl::discover_clue(LocationTarget::YourLocation, 1),
            ),
        ]),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(state.continuations.is_empty(), "nothing pushed by a skip");
    assert!(events.is_empty(), "a skipped choice changes nothing");
}

#[test]
fn choose_one_with_no_branches_still_rejects() {
    // A branchless ChooseOne is a malformed effect, not a board state — the
    // #664 skip is for a list the *filter* emptied.
    let mut state = state_with_clues_at_location(0);
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &Effect::ChooseOne(vec![]),
        ctx(1),
    );
    assert!(
        matches!(outcome, EngineOutcome::Rejected { .. }),
        "expected Rejected, got {outcome:?}",
    );
}

#[test]
fn choose_one_two_branches_suspends_with_a_choice_frame() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let before = state.investigators[&id].resources;
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::choose_one([
            (
                "Gain 1 resource",
                dsl::gain_resources(InvestigatorTarget::You, 1),
            ),
            (
                "Gain 3 resources",
                dsl::gain_resources(InvestigatorTarget::You, 3),
            ),
        ]),
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    // Suspended before any mutation; the ChooseOne Leaf is the prompt.
    assert_eq!(state.investigators[&id].resources, before);
    assert_eq!(offered_count(&outcome), 2);
    assert_suspended_leaf(&state);
}

#[test]
fn choose_one_resumes_the_pick() {
    // Resuming with pick = branch 1 runs the +3 branch.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let before = state.investigators[&id].resources;
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::choose_one([
            (
                "Gain 1 resource",
                dsl::gain_resources(InvestigatorTarget::You, 1),
            ),
            (
                "Gain 3 resources",
                dsl::gain_resources(InvestigatorTarget::You, 3),
            ),
        ]),
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    let outcome = resume_pick(&mut state, &mut events, 1);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&id].resources, before + 3);
}

#[test]
fn choice_after_earlier_seq_step_no_longer_rejects() {
    // Seq[ GainResources(+1), ChooseOne[ +1, +3 ] ] — a choice *after* a
    // mutating Seq step. The old single-pass replay model rejected this
    // (#346); the frame model suspends on the choice (the +1 already
    // applied) and resumes without double-applying the first step (#422).
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let before = state.investigators[&id].resources;
    let effect = Effect::Seq(vec![
        dsl::gain_resources(InvestigatorTarget::You, 1),
        dsl::choose_one([
            (
                "Gain 1 resource",
                dsl::gain_resources(InvestigatorTarget::You, 1),
            ),
            (
                "Gain 3 resources",
                dsl::gain_resources(InvestigatorTarget::You, 3),
            ),
        ]),
    ]);
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &effect,
        ctx(1),
    );
    assert!(
        matches!(outcome, EngineOutcome::AwaitingInput { .. }),
        "a choice after an earlier Seq step suspends, not rejects: {outcome:?}",
    );
    assert_eq!(
        state.investigators[&id].resources,
        before + 1,
        "the earlier Seq step applied exactly once before the suspend",
    );
    let outcome = resume_pick(&mut state, &mut events, 1);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&id].resources,
        before + 1 + 3,
        "resume runs the chosen branch with no double-apply of the first step",
    );
}

#[test]
fn choose_one_then_chosen_target_resumes_both_picks() {
    // Two suspensions in one effect (the First Aid shape): a ChooseOne
    // branch pick, then the chosen branch's `*::Chosen` target pick — the
    // case the old single-pass replay model rejected (#346). The parent
    // ChooseOne pop leaves the branch's grounding to suspend independently.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_investigator(test_support::test_investigator(2))
        .build();
    let before1 = state.investigators[&InvestigatorId(1)].resources;
    let before2 = state.investigators[&InvestigatorId(2)].resources;
    let effect = dsl::choose_one([
        (
            "Gain 1 resource",
            dsl::gain_resources(InvestigatorTarget::chosen_anywhere(), 1),
        ),
        (
            "Gain 9 resources",
            dsl::gain_resources(InvestigatorTarget::chosen_anywhere(), 9),
        ),
    ]);
    let mut events = Vec::new();

    // Suspend on the branch choice.
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &effect,
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    // Pick branch 1 (+9) → suspends again on its chosen target.
    let outcome = resume_pick(&mut state, &mut events, 1);
    assert!(
        matches!(outcome, EngineOutcome::AwaitingInput { .. }),
        "second suspend on the target choice: {outcome:?}",
    );
    // Pick target 1 (investigator 2) → completes.
    let outcome = resume_pick(&mut state, &mut events, 1);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&InvestigatorId(2)].resources,
        before2 + 9
    );
    assert_eq!(state.investigators[&InvestigatorId(1)].resources, before1);
}

#[test]
fn two_choices_resume_one_round_trip_at_a_time() {
    // The real client flow: branch choice suspends, resume picks it and
    // suspends *again* on the target choice (a fresh suspended Leaf), resume
    // completes. Drives `resume_effect_choice` (via `resume_pick`) — the same
    // path `apply(ResolveInput)` routes to.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_investigator(test_support::test_investigator(2))
        .build();
    let before2 = state.investigators[&InvestigatorId(2)].resources;
    let effect = dsl::choose_one([
        (
            "Gain 1 resource",
            dsl::gain_resources(InvestigatorTarget::chosen_anywhere(), 1),
        ),
        (
            "Gain 9 resources",
            dsl::gain_resources(InvestigatorTarget::chosen_anywhere(), 9),
        ),
    ]);
    let mut events = Vec::new();

    // First suspend: the branch choice.
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &effect,
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));

    // Resume the branch pick (the +9 branch) → suspends again on the
    // target choice (a new suspended Leaf, no replay payload).
    let outcome = resume_pick(&mut state, &mut events, 1);
    assert!(
        matches!(outcome, EngineOutcome::AwaitingInput { .. }),
        "second suspend on the target choice: {outcome:?}",
    );
    assert_suspended_leaf(&state);

    // Resume the target pick (investigator 2) → completes.
    let outcome = resume_pick(&mut state, &mut events, 1);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&InvestigatorId(2)].resources,
        before2 + 9
    );
    assert!(state.continuations.is_empty());
}
