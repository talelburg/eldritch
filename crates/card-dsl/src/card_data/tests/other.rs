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

#[test]
fn clue_value_round_trips_through_serde() {
    for cv in [ClueValue::PerInvestigator(2), ClueValue::Fixed(3)] {
        let json = serde_json::to_string(&cv).expect("serialize");
        let back: ClueValue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cv, back);
    }
}
