use super::*;

#[test]
fn modify_with_while_in_play_scope_under_non_constant_trigger_rejects() {
    // WhileInPlay belongs under Trigger::Constant; reaching the
    // evaluator with this combination means the card author
    // wired the ability wrong. Reject loudly.
    let mut state = GameStateBuilder::new().build();
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::modify(Stat::Willpower, 1, ModifierScope::WhileInPlay),
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
}

/// One investigator, with the test identified by `test_id` in flight —
/// what a `ThisSkillTest` modifier needs in order to have an identity to
/// be stamped with.
fn state_during_test(test_id: SkillTestId) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    state
        .continuations
        .push(Continuation::SkillTest(test_support::test_skill_test(
            test_id,
            InvestigatorId(1),
            SkillKind::Intellect,
            SkillTestKind::Plain,
            2,
        )));
    state
}

#[test]
fn modify_with_this_skill_test_scope_records_a_row_stamped_with_the_test() {
    let id = InvestigatorId(1);
    let test_id = SkillTestId(4);
    let mut state = state_during_test(test_id);
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::modify(Stat::Intellect, 1, ModifierScope::ThisSkillTest),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(events.is_empty(), "recording doesn't emit an event");
    assert_eq!(state.recorded_modifiers.len(), 1);
    let m = &state.recorded_modifiers[0];
    assert_eq!(m.investigator, id);
    assert_eq!(
        m.kind,
        RecordedModifierKind::Delta {
            stat: Stat::Intellect,
            delta: IntExpr::Lit(1),
        },
        "the row stores an expression, not a resolved integer",
    );
    assert_eq!(m.lifetime, Lifetime::SkillTest(test_id));
    assert_eq!(m.source, None, "no source on a bare for_controller ctx");
}

/// The scope says "for **this** skill test", so with no test in flight
/// there is no identity to stamp — the modifier is refused rather than
/// banked onto whatever test comes next (#676).
#[test]
fn modify_with_this_skill_test_scope_rejects_outside_a_test() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::modify(Stat::Intellect, 1, ModifierScope::ThisSkillTest),
        ctx(1),
    );
    assert!(
        matches!(&outcome, EngineOutcome::Rejected { reason }
            if reason.contains("no skill test in flight")),
        "expected a rejection naming the missing test, got {outcome:?}",
    );
    assert!(state.recorded_modifiers.is_empty(), "nothing recorded");
    assert!(events.is_empty());
}

#[test]
fn modify_records_source_when_ctx_has_one() {
    let id = InvestigatorId(1);
    let src = CardInstanceId(42);
    let mut state = state_during_test(SkillTestId(0));
    let mut events = Vec::new();
    let ctx_with_src = EvalContext::for_controller_with_source(id, AbilitySource::InPlay(src));
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::modify(Stat::Combat, 2, ModifierScope::ThisSkillTest),
        ctx_with_src,
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.recorded_modifiers[0].source, Some(src));
}

#[test]
fn modify_with_this_turn_scope_rejects_with_todo() {
    let mut state = GameStateBuilder::new().build();
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::modify(Stat::Willpower, 1, ModifierScope::ThisTurn),
        ctx(1),
    );
    match outcome {
        EngineOutcome::Rejected { reason } => {
            assert!(
                reason.contains("ThisTurn"),
                "reason should mention ThisTurn: {reason:?}",
            );
        }
        _ => panic!("expected Rejected"),
    }
}
