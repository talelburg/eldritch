use super::*;
use crate::engine::enumerate::TurnAction;
use crate::state::{EnemyId, LocationId};
use crate::test_support::TestSession;
use crate::test_support::TEST_REGISTRY;

const HERE: LocationId = LocationId(10);
const GHOUL: EnemyId = EnemyId(7);

/// `inv`'s open turn at [`HERE`], engaged by [`GHOUL`], with a round-scoped
/// Intellect-for-Combat/Agility substitution active (Mind over Matter 01036's
/// shape). Taking Fight or Evade starts a test the substitution applies to.
fn substitution_turn(inv: InvestigatorId) -> TestSession {
    let mut state = GameStateBuilder::new()
        .with_registries(crate::engine::TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(test_support::seat(1).at(HERE))
        .with_enemy_engaged(test_support::test_enemy(GHOUL.0, "Ghoul"), inv)
        .open_turn(inv)
        .build();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
    state.skill_substitutions.push(SkillSubstitution {
        investigator: inv,
        use_skill: SkillKind::Intellect,
        for_skills: vec![SkillKind::Combat, SkillKind::Agility],
    });
    TestSession::new(state, crate::engine::TEST_REGISTRIES)
}

fn substitution_state(inv: InvestigatorId) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_registries(crate::engine::TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_active_investigator(inv)
        .build();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
    state.skill_substitutions.push(SkillSubstitution {
        investigator: inv,
        use_skill: SkillKind::Intellect,
        for_skills: vec![SkillKind::Combat, SkillKind::Agility],
    });
    state
}

#[test]
fn combat_test_with_substitution_prompts_then_becomes_intellect_on_yes() {
    let inv = InvestigatorId(1);
    let mut state = substitution_state(inv);
    let mut events = Vec::new();
    let out = {
        let mut cx = Cx::for_test(&mut state, &mut events);
        // A weapon's "+2 [combat] for this attack".
        start_skill_test(
            &mut cx,
            inv,
            SkillKind::Combat,
            SkillTestKind::Fight,
            DifficultyBasis::Fixed(3),
            SkillTestFollowUp::None,
            None,
            None,
            None,
            Some(InitiatorModifier {
                target: ModifierTarget::Investigator(inv),
                stat: Stat::Combat,
                delta: IntExpr::Lit(2),
            }),
        )
    };
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }), "prompt");
    assert!(
        matches!(state.continuations.top(), Some(Continuation::SubstitutionPrompt(SubstitutionPromptFrame { investigator })) if *investigator == inv)
    );

    let out = {
        let mut cx = Cx::for_test(&mut state, &mut events);
        // The Decision's first option, "yes". The weapon's initiator modifier
        // below is one no session step can supply, so this test drives the
        // entry point and the resume handler directly.
        resume_substitution_choice(&mut cx, &InputResponse::PickSingle(OptionId(0)))
    };
    let out = {
        let mut cx = Cx::for_test(&mut state, &mut events);
        dispatch::drive(&mut cx, out)
    };
    assert!(
        matches!(out, EngineOutcome::AwaitingInput { .. }),
        "commit window"
    );
    let t = state.current_skill_test().unwrap();
    assert_eq!(t.skill, SkillKind::Intellect, "now an intellect test");
    assert_eq!(t.kind, SkillTestKind::Fight, "still a Fight (damage)");
    // The weapon's bonus falls away with no second mechanism: it is a row
    // over `Stat::Combat`, and the test now reads Intellect (FAQ: "ignore
    // any bonuses to Combat or Agility").
    let read = |skill| {
        modified_value::modified_value(
            &state,
            &TEST_REGISTRY,
            ModifierTarget::Investigator(inv),
            ModifiedQuantity::Skill(skill),
            ReadContext::DuringTest(SkillTestKind::Fight),
        )
        .total()
    };
    assert_eq!(
        read(SkillKind::Intellect),
        3,
        "weapon combat bonus does not reach the intellect the test now uses",
    );
    assert_eq!(
        read(SkillKind::Combat),
        5,
        "the row itself is still recorded — the stat mismatch is what drops it",
    );
    assert!(!matches!(
        state.continuations.top(),
        Some(Continuation::SubstitutionPrompt(_))
    ));
}

#[test]
fn substitution_prompt_keeps_the_test_on_its_frame() {
    // The substitution prompt suspends *before* the commit window — the one
    // place test data exists pre-commit. Pin that it lives on a SkillTest
    // frame (not a removed Option field) during that window (#348).
    let inv = InvestigatorId(1);
    let mut state = substitution_state(inv);
    let mut events = Vec::new();
    let out = {
        let mut cx = Cx::for_test(&mut state, &mut events);
        start_skill_test(
            &mut cx,
            inv,
            SkillKind::Combat,
            SkillTestKind::Fight,
            DifficultyBasis::Fixed(3),
            SkillTestFollowUp::None,
            None,
            None,
            None,
            None,
        )
    };
    assert!(
        matches!(out, EngineOutcome::AwaitingInput { .. }),
        "substitution prompt should suspend",
    );
    assert!(
        matches!(state.continuations.top(), Some(Continuation::SubstitutionPrompt(SubstitutionPromptFrame { investigator })) if *investigator == inv)
    );
    assert!(
        state.current_skill_test().is_some(),
        "the in-flight test must live on a SkillTest frame during the \
         substitution prompt, not in a removed Option field",
    );
    assert!(
        state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::SkillTest(_))),
        "a SkillTest frame is on the stack before the commit window",
    );
}

#[test]
fn substitution_choice_no_keeps_the_printed_skill() {
    let inv = InvestigatorId(1);
    let session = substitution_turn(inv).take(&TurnAction::Evade {
        investigator: inv,
        enemy: GHOUL,
    });
    // The substitution prompt is a Decision: "yes" then "no". Decline.
    let session = session.pick_nth(1);
    assert!(
        session.state().current_skill_test().is_some()
            && session.prompt().kind == InputKind::PickMultiple,
        "commit window"
    );
    assert_eq!(
        session.state().current_skill_test().unwrap().skill,
        SkillKind::Agility,
        "declined — keeps the printed skill",
    );
}

/// The substitution resume parks the test for the loop rather than driving to
/// the commit window itself: choosing the substitution pops the
/// `SubstitutionPrompt`, rewrites the skill, and returns `Done` with the
/// `SkillTest` on top; the `drive` loop then opens the commit window. (Slice C,
/// #431 — substitution-resume re-entry retired.)
#[test]
fn resume_substitution_choice_parks_for_the_loop() {
    let inv = InvestigatorId(1);
    let mut state = substitution_state(inv);
    let mut events = Vec::new();
    let out = {
        let mut cx = Cx::for_test(&mut state, &mut events);
        start_skill_test(
            &mut cx,
            inv,
            SkillKind::Combat,
            SkillTestKind::Fight,
            DifficultyBasis::Fixed(3),
            SkillTestFollowUp::None,
            None,
            None,
            None,
            None,
        )
    };
    assert!(
        matches!(out, EngineOutcome::AwaitingInput { .. }),
        "substitution prompt"
    );

    // Choose the substitution (the Decision's first option, "yes"): the resume
    // PARKS — it does not itself open the commit window. The resume handler's
    // own return is the subject, so it is called directly.
    let out = {
        let mut cx = Cx::for_test(&mut state, &mut events);
        resume_substitution_choice(&mut cx, &InputResponse::PickSingle(OptionId(0)))
    };
    assert_eq!(
        out,
        EngineOutcome::Done,
        "the substitution resume parks for the loop"
    );
    assert!(
        !matches!(
            state.continuations.top(),
            Some(Continuation::SubstitutionPrompt(_))
        ),
        "the SubstitutionPrompt was consumed",
    );
    assert!(
        matches!(state.continuations.top(), Some(Continuation::SkillTest(_))),
        "the SkillTest frame is parked on top for the loop to drive",
    );
    assert_eq!(
        state.current_skill_test().unwrap().skill,
        SkillKind::Intellect,
        "the substitution rewrote the skill before parking",
    );

    // The loop drives the parked test to its commit window.
    let out = {
        let mut cx = Cx::for_test(&mut state, &mut events);
        dispatch::drive(&mut cx, out)
    };
    assert!(
        matches!(out, EngineOutcome::AwaitingInput { .. }),
        "commit window"
    );
}

#[test]
fn no_active_substitution_opens_commit_window_directly() {
    let inv = InvestigatorId(1);
    let mut state = substitution_turn(inv).state().clone();
    state.skill_substitutions.clear();
    let session =
        TestSession::new(state, crate::engine::TEST_REGISTRIES).take(&TurnAction::Fight {
            investigator: inv,
            enemy: GHOUL,
        });
    // No prompt: straight to the commit window.
    assert_eq!(session.prompt().kind, InputKind::PickMultiple);
    assert_eq!(
        session.state().current_skill_test().unwrap().skill,
        SkillKind::Combat,
    );
}
