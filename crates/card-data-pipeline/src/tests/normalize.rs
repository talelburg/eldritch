use super::*;

#[test]
fn normalize_happy_path_populates_all_fields() {
    // Synthetic fixture; values are not meant to match any real
    // ArkhamDB card. Exercises the field-by-field normalize path
    // without coupling to snapshot data.
    let mut raw = raw_card("TEST01");
    raw.name = Some("Test Card".to_owned());
    raw.text = Some("Test ability text.".to_owned());
    raw.traits = Some("Alpha. Beta.".to_owned());
    raw.slot = None;
    raw.cost = Some(0);
    raw.xp = Some(0);
    raw.faction_code = Some("seeker".to_owned());
    raw.type_code = Some("skill".to_owned());
    raw.deck_limit = Some(2);
    raw.quantity = Some(2);
    raw.skill_intellect = Some(1);

    let n = normalize(raw).expect("happy-path RawCard normalizes");
    assert_eq!(n.code, "TEST01");
    assert_eq!(n.name, "Test Card");
    assert_eq!(n.class, "Seeker");
    assert_eq!(n.card_type, "Skill");
    assert_eq!(n.cost, Some(0));
    assert_eq!(n.xp, Some(0));
    assert_eq!(n.text.as_deref(), Some("Test ability text."));
    assert_eq!(n.traits, vec!["Alpha", "Beta"]);
    assert!(n.slots.is_empty());
    assert_eq!(n.skill_intellect, 1);
    assert_eq!(n.skill_willpower, 0);
    assert_eq!(n.deck_limit, 2);
    assert_eq!(n.quantity, 2);
}

#[test]
fn normalize_defaults_optional_skills_to_zero() {
    let raw = raw_card("01001");
    // All skill_* fields are None on the fixture; normalize should
    // unwrap_or(0) without complaint. Also pins the asymmetric
    // deck_limit / quantity defaults — deck_limit defaults to 0
    // (no copies allowed), quantity defaults to 1 (one copy in the
    // physical product), and a future swap of those would be a
    // real bug.
    let n = normalize(raw).expect("fixture normalizes");
    assert_eq!(n.skill_willpower, 0);
    assert_eq!(n.skill_intellect, 0);
    assert_eq!(n.skill_combat, 0);
    assert_eq!(n.skill_agility, 0);
    assert_eq!(n.skill_wild, 0);
    assert_eq!(n.deck_limit, 0);
    assert_eq!(n.quantity, 1);
}

#[test]
fn normalize_errors_on_missing_name() {
    let mut raw = raw_card("01001");
    raw.name = None;
    let err = normalize(raw).unwrap_err();
    assert!(err.contains("name"), "{err}");
}

#[test]
fn normalize_propagates_unknown_faction_error() {
    let mut raw = raw_card("01001");
    raw.faction_code = Some("wibble".to_owned());
    let err = normalize(raw).unwrap_err();
    assert!(err.contains("faction_code"), "{err}");
    assert!(err.contains("wibble"), "{err}");
}

#[test]
fn normalize_propagates_unknown_type_error() {
    let mut raw = raw_card("01001");
    raw.type_code = Some("wibble".to_owned());
    let err = normalize(raw).unwrap_err();
    assert!(err.contains("type_code"), "{err}");
    assert!(err.contains("wibble"), "{err}");
}

#[test]
fn normalize_errors_on_cost_overflow() {
    // i8 max is 127; upstream cost is read as i32, so 200 is a
    // valid input that doesn't fit. The branch returns an
    // explanatory error; pin the message shape so the diagnostic
    // doesn't quietly become "doesn't fit" with no context.
    let mut raw = raw_card("01001");
    raw.cost = Some(200);
    let err = normalize(raw).unwrap_err();
    assert!(err.contains("cost"), "{err}");
    assert!(err.contains("200"), "{err}");
    assert!(err.contains("01001"), "{err}");
}

#[test]
fn normalize_silently_drops_xp_overflow() {
    // xp is u8 in the normalized shape; values that don't fit
    // (which the snapshot never produces today) are silently
    // dropped to None via `and_then(|n| u8::try_from(n).ok())`.
    // Pin the silent-drop behavior so a future "error instead"
    // change is a conscious decision.
    let mut raw = raw_card("01001");
    raw.xp = Some(300);
    let n = normalize(raw).expect("xp overflow does not error");
    assert_eq!(n.xp, None);
}

#[test]
fn fast_prefix_detected_at_start_of_text() {
    let raw = RawCard {
        code: "01030".into(),
        name: Some("Magnifying Glass".into()),
        text: Some("Fast.\nYou get +1 [intellect] while investigating.".into()),
        traits: None,
        slot: Some("Hand".into()),
        cost: Some(1),
        xp: Some(0),
        health: None,
        sanity: None,
        deck_limit: Some(2),
        quantity: Some(1),
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        faction_code: Some("seeker".into()),
        type_code: Some("asset".into()),
        skill_willpower: None,
        skill_intellect: Some(1),
        skill_combat: None,
        skill_agility: None,
        skill_wild: None,
        shroud: None,
        clues: None,
        clues_fixed: None,
        victory: None,
        doom: None,
        enemy_fight: None,
        enemy_evade: None,
        enemy_damage: None,
        enemy_horror: None,
        health_per_investigator: None,
        subtype_code: None,
    };
    let norm = normalize(raw).expect("normalize");
    assert!(
        norm.is_fast,
        "card text begins with \"Fast.\", expected is_fast=true"
    );
}

#[test]
fn fast_marker_inside_text_is_not_a_fast_card() {
    let raw = RawCard {
        code: "01034".into(),
        name: Some("Hyperawareness".into()),
        text: Some("[fast] Spend 1 resource: You get +1 [intellect] for this skill test.".into()),
        traits: None,
        slot: None,
        cost: Some(2),
        xp: Some(0),
        health: None,
        sanity: None,
        deck_limit: Some(2),
        quantity: Some(1),
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        faction_code: Some("seeker".into()),
        type_code: Some("asset".into()),
        skill_willpower: None,
        skill_intellect: Some(1),
        skill_combat: None,
        skill_agility: Some(1),
        skill_wild: None,
        shroud: None,
        clues: None,
        clues_fixed: None,
        victory: None,
        doom: None,
        enemy_fight: None,
        enemy_evade: None,
        enemy_damage: None,
        enemy_horror: None,
        health_per_investigator: None,
        subtype_code: None,
    };
    let norm = normalize(raw).expect("normalize");
    assert!(
        !norm.is_fast,
        "card text does NOT begin with \"Fast.\"; [fast] inside text is unrelated"
    );
}

#[test]
fn normalize_weakness_subtype_sets_weakness_true() {
    let mut raw = raw_card("01007");
    raw.subtype_code = Some("weakness".to_owned());
    let n = normalize(raw).expect("normalize");
    assert!(n.weakness, "subtype_code=weakness should set weakness=true");
}

#[test]
fn normalize_basicweakness_subtype_sets_weakness_true() {
    let mut raw = raw_card("01097");
    raw.subtype_code = Some("basicweakness".to_owned());
    let n = normalize(raw).expect("normalize");
    assert!(
        n.weakness,
        "subtype_code=basicweakness should set weakness=true"
    );
}

#[test]
fn normalize_carries_back_name_and_text() {
    // A raw agenda with a reverse side keeps back_name/back_text through normalize.
    let mut raw = raw_card("01105");
    raw.faction_code = Some("mythos".to_owned());
    raw.type_code = Some("agenda".to_owned());
    raw.back_name = Some("A Lapse in Time".to_owned());
    raw.back_text = Some("The lead investigator must decide…".to_owned());
    let n = normalize(raw).expect("agenda normalizes");
    assert_eq!(n.back_name.as_deref(), Some("A Lapse in Time"));
    assert_eq!(
        n.back_text.as_deref(),
        Some("The lead investigator must decide…")
    );
}

#[test]
fn normalize_no_subtype_sets_weakness_false() {
    let raw = raw_card("01059");
    // subtype_code is None on the default fixture
    let n = normalize(raw).expect("normalize");
    assert!(
        !n.weakness,
        "absent subtype_code should leave weakness=false"
    );
}

#[test]
fn normalize_other_subtype_sets_weakness_false() {
    let mut raw = raw_card("01088");
    raw.subtype_code = Some("other".to_owned());
    let n = normalize(raw).expect("normalize");
    assert!(
        !n.weakness,
        "unrecognized subtype_code should leave weakness=false"
    );
}
