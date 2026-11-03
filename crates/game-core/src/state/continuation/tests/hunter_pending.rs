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
fn engagement_tie_serde_roundtrip() {
    let original = EngagementTieFrame::new(vec![
        TiedEngagement::new(EnemyId(2), vec![InvestigatorId(1), InvestigatorId(2)]),
        TiedEngagement::new(EnemyId(5), vec![InvestigatorId(2), InvestigatorId(3)]),
    ]);
    let json = serde_json::to_string(&original).expect("serialize");
    let back: EngagementTieFrame = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
}
