use super::*;

#[test]
fn parse_prey_recognizes_highest_combat() {
    assert_eq!(
        parse_prey("<b>Prey</b> - Highest [combat].\nHunter. Retaliate."),
        PreyParse::HighestSkill("Combat"),
    );
}

#[test]
fn parse_prey_recognizes_lowest_remaining_health() {
    assert_eq!(
        parse_prey("<b>Prey</b> - Lowest remaining health."),
        PreyParse::LowestRemainingHealth,
    );
}

#[test]
fn parse_prey_none_when_no_prey_line() {
    assert_eq!(parse_prey("Hunter."), PreyParse::None);
}

/// Mob Enforcer 01101, Silver Twilight Acolyte 01102 and Stubborn Detective
/// 01103 all print *"<b>Prey</b> - Bearer only."* (#1125).
#[test]
fn parse_prey_recognizes_bearer_only() {
    assert_eq!(
        parse_prey(
            "<b>Prey</b> - Bearer only.\nHunter.\n[action] Spend 4 resources: \
             <b>Parley.</b> Discard Mob Enforcer."
        ),
        PreyParse::BearerOnly,
    );
}

#[test]
fn parse_prey_unrecognized_keeps_clause() {
    assert_eq!(
        parse_prey("<b>Prey</b> - Most clues."),
        PreyParse::Unrecognized("Most clues".to_owned()),
    );
}

#[test]
fn prey_lit_emits_expected_literals() {
    assert_eq!(prey_lit(&PreyParse::None), "Prey::Default");
    assert_eq!(
        prey_lit(&PreyParse::Unrecognized("Most clues".to_owned())),
        "Prey::Default"
    );
    assert_eq!(
        prey_lit(&PreyParse::HighestSkill("Combat")),
        "Prey::Ranked { direction: PreyDirection::Highest, measure: PreyMeasure::Skill(SkillKind::Combat) }",
    );
    assert_eq!(
        prey_lit(&PreyParse::LowestRemainingHealth),
        "Prey::Ranked { direction: PreyDirection::Lowest, measure: PreyMeasure::RemainingHealth }",
    );
    assert_eq!(prey_lit(&PreyParse::BearerOnly), "Prey::BearerOnly");
}
