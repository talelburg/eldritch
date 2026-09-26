use super::*;

#[test]
fn prey_default_is_default() {
    assert_eq!(Prey::default(), Prey::Default);
}

#[test]
fn prey_serde_roundtrip_ranked_skill() {
    let original = Prey::Ranked {
        direction: PreyDirection::Highest,
        measure: PreyMeasure::Skill(SkillKind::Combat),
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: Prey = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
}

#[test]
fn prey_serde_roundtrip_ranked_remaining_health() {
    let original = Prey::Ranked {
        direction: PreyDirection::Lowest,
        measure: PreyMeasure::RemainingHealth,
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: Prey = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
}

#[test]
fn prey_serde_roundtrip_default() {
    let original = Prey::Default;
    let json = serde_json::to_string(&original).expect("serialize");
    let back: Prey = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
}
