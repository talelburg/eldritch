use super::*;

/// A tag the walker should report as an [`Effect::Native`].
fn effect_ref(tag: &str) -> NativeRef<'_> {
    NativeRef {
        kind: NativeKind::Effect,
        tag,
    }
}

/// A tag the walker should report as a [`Condition::Native`].
fn condition_ref(tag: &str) -> NativeRef<'_> {
    NativeRef {
        kind: NativeKind::Condition,
        tag,
    }
}

#[test]
fn an_effect_native_inside_a_seq_is_reported_as_an_effect() {
    let ability = on_play(seq([
        draw_cards(InvestigatorTarget::You, 1),
        native("x:in-seq"),
    ]));
    assert_eq!(ability.native_refs(), [effect_ref("x:in-seq")]);
}

#[test]
fn an_ability_with_no_natives_references_nothing() {
    let ability = activated_as(
        fight(
            IntExpr::cond(control_status("01117", ControlStatus::ByAPlayer), 1, 0),
            1u8,
        ),
        1,
        vec![],
        if_(
            Condition::SkillTestKind(SkillTestKind::Fight),
            discard_self(),
        ),
    );
    assert_eq!(ability.native_refs(), []);
}

#[test]
fn a_granted_abilitys_eligibility_is_reported_as_eligibility() {
    let granted = reaction_on_event(EventPattern::RoundEnded, EventTiming::When, Effect::Cancel)
        .with_eligibility("x:granted-gate");
    let ability = constant(grant(GrantTarget::SelfCard, None, vec![granted]));
    assert_eq!(
        ability.native_refs(),
        [NativeRef {
            kind: NativeKind::Eligibility,
            tag: "x:granted-gate",
        }]
    );
}

#[test]
fn a_native_condition_inside_an_int_expr_cond_is_reported_as_a_condition() {
    let ability = on_play(deal_damage(
        InvestigatorTarget::You,
        IntExpr::cond(native_condition("x:in-cond"), 2, 1),
    ));
    assert_eq!(ability.native_refs(), [condition_ref("x:in-cond")]);
}

#[test]
fn a_native_condition_inside_an_if_is_reported_as_a_condition() {
    let ability = on_play(if_(native_condition("x:in-if"), discard_self()));
    assert_eq!(ability.native_refs(), [condition_ref("x:in-if")]);
}

#[test]
fn an_effect_native_inside_a_choose_one_branch_is_reported_as_an_effect() {
    let ability = on_play(choose_one([
        ("Draw", draw_cards(InvestigatorTarget::You, 1)),
        ("Native", native("x:in-branch")),
    ]));
    assert_eq!(ability.native_refs(), [effect_ref("x:in-branch")]);
}

#[test]
fn an_effect_native_inside_a_for_each_is_reported_as_an_effect() {
    let ability = on_play(for_each(
        InvestigatorTargetSet::All,
        native("x:in-for-each"),
    ));
    assert_eq!(ability.native_refs(), [effect_ref("x:in-for-each")]);
}

#[test]
fn a_native_condition_carried_by_an_action_designator_is_reported_as_a_condition() {
    let ability = activated_as(
        fight(
            0u8,
            IntExpr::cond(native_condition("x:in-designator"), 1, 0),
        ),
        1,
        vec![],
        seq([]),
    );
    assert_eq!(ability.native_refs(), [condition_ref("x:in-designator")]);
}
