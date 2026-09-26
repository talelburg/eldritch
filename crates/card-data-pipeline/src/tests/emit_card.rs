use super::*;

/// Build a `NormalizedCard` with all-default fields for the given
/// code/name/type — so emit tests don't repeat the full literal.
fn normalized(code: &str, name: &str, card_type: &'static str) -> NormalizedCard {
    NormalizedCard {
        code: code.into(),
        name: name.into(),
        class: "Mythos",
        card_type,
        cost: None,
        xp: None,
        text: None,
        traits: Vec::new(),
        slots: Vec::new(),
        skill_willpower: 0,
        skill_intellect: 0,
        skill_combat: 0,
        skill_agility: 0,
        skill_wild: 0,
        health: None,
        sanity: None,
        deck_limit: 0,
        quantity: 1,
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        is_fast: false,
        play_only_during_turn: false,
        uses: None,
        commit_limit: None,
        shroud: None,
        clues: None,
        clues_fixed: false,
        victory: None,
        doom: None,
        enemy_fight: None,
        enemy_evade: None,
        enemy_damage: None,
        enemy_horror: None,
        health_per_investigator: false,
        hunter: false,
        retaliate: false,
        prey: PreyParse::None,
        spawn_name: None,
        spawn_code: None,
        weakness: false,
    }
}

#[test]
fn emitted_treachery_renders_treachery_kind() {
    // A treachery emits a `CardKind::Treachery { … }` with its
    // surge/peril defaults and quantity — and carries no class.
    let card = normalized("01001", "Test", "Treachery");
    let mut buf = String::new();
    emit_card(&mut buf, &card);
    assert!(
        buf.contains("CardKind::Treachery {"),
        "emitted treachery should render a Treachery kind; got:\n{buf}",
    );
    assert!(
        !buf.contains("class:"),
        "treachery carries no class; got:\n{buf}",
    );
}

#[test]
fn emitted_location_renders_location_kind() {
    let mut c = normalized("01111", "Study", "Location");
    c.shroud = Some(2);
    c.clues = Some(2);
    let mut buf = String::new();
    emit_card(&mut buf, &c);
    assert!(
        buf.contains(
            "CardKind::Location { shroud: 2, printed_clues: ClueValue::PerInvestigator(2)"
        ),
        "got:\n{buf}",
    );
}

#[test]
fn location_clue_value_reflects_clues_fixed() {
    assert_eq!(clue_value_lit(2, false), "ClueValue::PerInvestigator(2)");
    assert_eq!(clue_value_lit(2, true), "ClueValue::Fixed(2)");
}

#[test]
fn emitted_enemy_renders_combat_stats() {
    let mut c = normalized("01116", "Ghoul Priest", "Enemy");
    c.enemy_fight = Some(4);
    c.enemy_evade = Some(4);
    c.health = Some(5);
    let mut buf = String::new();
    emit_card(&mut buf, &c);
    assert!(
        buf.contains("CardKind::Enemy { fight: 4, evade: 4,"),
        "got:\n{buf}",
    );
}
