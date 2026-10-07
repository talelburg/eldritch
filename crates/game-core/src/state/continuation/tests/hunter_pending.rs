use super::*;

#[test]
fn hunter_choice_move_serde_roundtrip() {
    let original = HunterChoice::Move {
        enemy: EnemyId(3),
        candidates: vec![LocationId(2), LocationId(3)],
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: HunterChoice = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
}

#[test]
fn hunter_choice_engage_serde_roundtrip() {
    let original = HunterChoice::Engage {
        enemy: EnemyId(5),
        candidates: vec![InvestigatorId(1), InvestigatorId(2)],
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: HunterChoice = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
}

#[test]
fn spawn_engage_pending_serde_roundtrip() {
    let original = SpawnEngagePending {
        enemy: EnemyId(2),
        candidates: vec![InvestigatorId(1), InvestigatorId(2)],
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: SpawnEngagePending = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
}
