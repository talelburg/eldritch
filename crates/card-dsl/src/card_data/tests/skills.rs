use super::*;

#[test]
fn skills_value_indexes_each_kind() {
    let s = Skills {
        willpower: 3,
        intellect: 2,
        combat: 4,
        agility: 1,
    };
    assert_eq!(s.value(SkillKind::Willpower), 3);
    assert_eq!(s.value(SkillKind::Intellect), 2);
    assert_eq!(s.value(SkillKind::Combat), 4);
    assert_eq!(s.value(SkillKind::Agility), 1);
}
