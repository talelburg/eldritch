use super::*;

#[test]
fn a_locations_shroud_folds_in_its_attachments() {
    let mut loc = test_support::test_location(3, "Study"); // printed shroud 2
    loc.attachments.push(CardInPlay::enter_play(
        CardCode::new("shroud-plus-2"),
        CardInstanceId(0),
    ));
    let state = GameStateBuilder::new().with_location(loc).build();
    let shroud = modified_value(
        &state,
        Some(&mock_registry()),
        ModifierTarget::Location(LocationId(3)),
        ModifiedQuantity::Shroud,
        ReadContext::OutsideTest,
    );
    assert_eq!(shroud.base, 2);
    assert_eq!(shroud.total(), 4);
}

#[test]
fn a_locations_shroud_with_no_attachments_is_the_printed_value() {
    let state = GameStateBuilder::new()
        .with_location(test_support::test_location(3, "Study"))
        .build();
    assert_eq!(
        modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Location(LocationId(3)),
            ModifiedQuantity::Shroud,
            ReadContext::OutsideTest,
        )
        .total(),
        2,
    );
}

/// An `AttachedCard` modifier reaches only the entity its source is
/// attached to — a second location in play is unaffected.
#[test]
fn an_attached_card_reaches_only_what_it_is_attached_to() {
    let mut fogged = test_support::test_location(3, "Study");
    fogged.attachments.push(CardInPlay::enter_play(
        CardCode::new("shroud-plus-2"),
        CardInstanceId(0),
    ));
    let state = GameStateBuilder::new()
        .with_location(fogged)
        .with_location(test_support::test_location(4, "Hallway"))
        .build();
    let shroud = |id| {
        modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Location(id),
            ModifiedQuantity::Shroud,
            ReadContext::OutsideTest,
        )
        .total()
    };
    assert_eq!(shroud(LocationId(3)), 4);
    assert_eq!(shroud(LocationId(4)), 2);
}

#[test]
fn an_enemys_fight_and_evade_answer_from_their_printed_values() {
    let state = GameStateBuilder::new()
        .with_enemy(test_support::test_enemy(7, "Ghoul"))
        .build();
    let read = |quantity| {
        modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Enemy(EnemyId(7)),
            quantity,
            ReadContext::OutsideTest,
        )
        .total()
    };
    assert_eq!(read(ModifiedQuantity::Fight), 2);
    assert_eq!(read(ModifiedQuantity::Evade), 2);
}
