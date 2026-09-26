use super::*;

/// A Magnifying-Glass-shaped card: *"+1 \[intellect\] while
/// investigating."* Contributes during an Investigate test and
/// nowhere else.
#[test]
fn a_during_scope_contributes_only_to_its_own_test_kind() {
    let (state, id) = state_with_cards_in_play(&["intellect-plus-1-while-investigating"]);
    let read = |context| {
        modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Investigator(id),
            ModifiedQuantity::Skill(SkillKind::Intellect),
            context,
        )
        .total()
    };
    assert_eq!(read(ReadContext::DuringTest(SkillTestKind::Investigate)), 4);
    assert_eq!(read(ReadContext::DuringTest(SkillTestKind::Plain)), 3);
    assert_eq!(read(ReadContext::DuringTest(SkillTestKind::Fight)), 3);
    assert_eq!(read(ReadContext::OutsideTest), 3);
}

/// Holy-Rosary-shaped: unqualified `WhileInPlay` contributes to
/// every read, inside a test or out of one.
#[test]
fn an_unqualified_scope_contributes_to_every_read() {
    let (state, id) = state_with_cards_in_play(&["willpower-plus-1"]);
    for context in [
        ReadContext::OutsideTest,
        ReadContext::DuringTest(SkillTestKind::Investigate),
        ReadContext::DuringTest(SkillTestKind::Fight),
        ReadContext::DuringTest(SkillTestKind::Evade),
        ReadContext::DuringTest(SkillTestKind::Plain),
    ] {
        assert_eq!(
            modified_value(
                &state,
                Some(&mock_registry()),
                ModifierTarget::Investigator(id),
                ModifiedQuantity::Skill(SkillKind::Willpower),
                context,
            )
            .total(),
            4,
            "WhileInPlay should apply in {context:?}",
        );
    }
}

/// Scope and stat are independent filters: the right test kind does
/// not make a modifier apply to the wrong skill.
#[test]
fn a_during_scope_still_respects_the_stat() {
    let (state, id) = state_with_cards_in_play(&["intellect-plus-1-while-investigating"]);
    assert_eq!(
        modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Investigator(id),
            ModifiedQuantity::Skill(SkillKind::Willpower),
            ReadContext::DuringTest(SkillTestKind::Investigate),
        )
        .total(),
        3,
    );
}
