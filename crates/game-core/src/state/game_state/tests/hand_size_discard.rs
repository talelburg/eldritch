use super::*;

#[test]
fn hand_size_discard_serde_roundtrip() {
    let original = HandSizeDiscard {
        remaining: vec![InvestigatorId(1), InvestigatorId(2)],
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: HandSizeDiscard = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
}
