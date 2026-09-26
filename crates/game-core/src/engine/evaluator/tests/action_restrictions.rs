use super::*;

/// Mock registry that maps a small hardcoded set of codes to
/// abilities. Keeps the constant-modifier query tests isolated
/// from the global `OnceLock` and from the cards crate.
fn mock_registry(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn fake_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        "willpower-plus-1" => Some(vec![dsl::constant(dsl::modify(
            Stat::Willpower,
            1,
            ModifierScope::WhileInPlay,
        ))]),
        "intellect-plus-2" => Some(vec![dsl::constant(dsl::modify(
            Stat::Intellect,
            2,
            ModifierScope::WhileInPlay,
        ))]),
        // A standalone fake investigator card carrying a constant +2
        // willpower — used to prove the unified `controlled_card_instances()`
        // scan now sums the investigator card (not just `cards_in_play`).
        "inv-willpower-plus-2" => Some(vec![dsl::constant(dsl::modify(
            Stat::Willpower,
            2,
            ModifierScope::WhileInPlay,
        ))]),
        "intellect-plus-1-while-investigating" => Some(vec![dsl::constant(dsl::modify(
            Stat::Intellect,
            1,
            ModifierScope::WhileInPlayDuring(SkillTestKind::Investigate),
        ))]),
        "willpower-plus-1-this-test-only" => Some(vec![dsl::constant(dsl::modify(
            Stat::Willpower,
            1,
            ModifierScope::ThisSkillTest,
        ))]),
        "willpower-minus-1" => Some(vec![dsl::constant(dsl::modify(
            Stat::Willpower,
            -1,
            ModifierScope::WhileInPlay,
        ))]),
        "non-constant-willpower" => Some(vec![dsl::on_play(dsl::modify(
            Stat::Willpower,
            5,
            ModifierScope::WhileInPlay,
        ))]),
        "max-health-plus-1" => Some(vec![dsl::constant(dsl::modify(
            Stat::MaxHealth,
            1,
            ModifierScope::WhileInPlay,
        ))]),
        "shroud-plus-2" => Some(vec![dsl::constant(dsl::modify(
            Stat::Shroud,
            2,
            ModifierScope::WhileInPlay,
        ))]),
        "cannot-play-assets" => Some(vec![dsl::constant(dsl::restrict(Restriction::CannotPlay(
            CardType::Asset,
        )))]),
        "frozen-surcharge" => Some(vec![dsl::constant(dsl::restrict(
            Restriction::ExtraActionCost {
                actions: vec![ActionClass::Move, ActionClass::Fight, ActionClass::Evade],
                first_each_round: true,
            },
        ))]),
        _ => None,
    }
}

fn fake_registry() -> CardRegistry {
    CardRegistry {
        metadata_for: mock_registry,
        abilities_for: fake_abilities_for,
        ..CardRegistry::EMPTY
    }
}

#[test]
fn play_is_prohibited_matches_only_the_forbidden_type() {
    let (state, id) = state_with_cards_in_play(&["cannot-play-assets"]);
    let reg = fake_registry();
    assert!(play_is_prohibited(&state, &reg, id, CardType::Asset));
    assert!(!play_is_prohibited(&state, &reg, id, CardType::Event));
}

#[test]
fn surcharge_charges_first_matching_action_then_not_again_until_reset() {
    let (mut state, id) = state_with_cards_in_play(&["frozen-surcharge"]);
    let reg = fake_registry();

    // First move this round: +1, and the source (instance 0) to mark.
    let (extra, to_mark) = pending_action_surcharge(&state, &reg, id, ActionClass::Move);
    assert_eq!(extra, 1);
    assert_eq!(to_mark, vec![CardInstanceId(0)]);

    // Mark it spent (what the action handler does on commit).
    state
        .investigators
        .get_mut(&id)
        .unwrap()
        .action_surcharge_spent_this_round
        .insert(CardInstanceId(0));

    // Second matching action this round: no surcharge.
    let (extra, to_mark) = pending_action_surcharge(&state, &reg, id, ActionClass::Fight);
    assert_eq!(extra, 0);
    assert!(to_mark.is_empty());

    // New round reset → charges again.
    state
        .investigators
        .get_mut(&id)
        .unwrap()
        .action_surcharge_spent_this_round
        .clear();
    let (extra, _) = pending_action_surcharge(&state, &reg, id, ActionClass::Evade);
    assert_eq!(extra, 1);
}

#[test]
fn surcharge_two_sources_each_charge_the_first_action() {
    let (state, id) = state_with_cards_in_play(&["frozen-surcharge", "frozen-surcharge"]);
    let reg = fake_registry();
    let (extra, to_mark) = pending_action_surcharge(&state, &reg, id, ActionClass::Move);
    assert_eq!(
        extra, 2,
        "two Frozen in Fear each surcharge the first action"
    );
    assert_eq!(to_mark, vec![CardInstanceId(0), CardInstanceId(1)]);
}

#[test]
fn play_is_prohibited_false_with_no_restriction() {
    let (state, id) = state_with_cards_in_play(&["willpower-plus-1"]);
    let reg = fake_registry();
    assert!(!play_is_prohibited(&state, &reg, id, CardType::Asset));
}
