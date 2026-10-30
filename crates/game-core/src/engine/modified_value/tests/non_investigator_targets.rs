use super::*;

#[test]
fn a_locations_shroud_folds_in_its_attachments() {
    // Stock shroud 2.
    let loc = location(3).attachments([CardInPlay::enter_play(
        CardCode::new("shroud-plus-2"),
        CardInstanceId(0),
        Owner::EncounterDeck,
    )]);
    let state = GameStateBuilder::new().with_location(loc).build();
    let shroud = modified_value(
        &state,
        &mock_registry(),
        ModifierTarget::Location(LocationId(3)),
        ModifiedQuantity::Shroud,
        ReadContext::OutsideTest,
    );
    assert_eq!(shroud.base, 2);
    assert_eq!(shroud.total(), 4);
}

#[test]
fn a_locations_shroud_with_no_attachments_is_the_printed_value() {
    let state = GameStateBuilder::new().with_location(location(3)).build();
    assert_eq!(
        modified_value(
            &state,
            &mock_registry(),
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
    let fogged = location(3).attachments([CardInPlay::enter_play(
        CardCode::new("shroud-plus-2"),
        CardInstanceId(0),
        Owner::EncounterDeck,
    )]);
    let state = GameStateBuilder::new()
        .with_location(fogged)
        .with_location(location(4))
        .build();
    let shroud = |id| {
        modified_value(
            &state,
            &mock_registry(),
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
    let state = GameStateBuilder::new().with_enemy(enemy(7)).build();
    let read = |quantity| {
        modified_value(
            &state,
            &mock_registry(),
            ModifierTarget::Enemy(EnemyId(7)),
            quantity,
            ReadContext::OutsideTest,
        )
        .total()
    };
    assert_eq!(read(ModifiedQuantity::Fight), 2);
    assert_eq!(read(ModifiedQuantity::Evade), 2);
}
