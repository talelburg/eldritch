use super::*;

#[test]
fn clue_value_round_trips_through_serde() {
    for cv in [ClueValue::PerInvestigator(2), ClueValue::Fixed(3)] {
        let json = serde_json::to_string(&cv).expect("serialize");
        let back: ClueValue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cv, back);
    }
}
