use super::*;
use crate::test_support::TEST_REGISTRIES;

#[test]
fn target_maps_each_variant() {
    // A state where investigator 1 stands on a location, so Investigate's
    // implicit anchor resolves to that location.
    let loc_id = LocationId(10);
    let state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(loc_id.0, "Study"))
        .with_seat(seat(1).at(loc_id))
        .open_turn(InvestigatorId(1))
        .build();
    let inv = InvestigatorId(1);

    // Each of these three actions names its own surface.
    assert_eq!(
        TurnAction::EndTurn.target(&state),
        Some(OptionTarget::TurnControl(inv)),
        "EndTurn anchors to the active investigator's turn control"
    );
    assert_eq!(
        TurnAction::Resource { investigator: inv }.target(&state),
        Some(OptionTarget::ResourcePool(inv))
    );
    assert_eq!(
        TurnAction::Draw { investigator: inv }.target(&state),
        Some(OptionTarget::PlayerDeck(inv))
    );
    assert_eq!(
        TurnAction::Move {
            investigator: inv,
            destination: LocationId(11)
        }
        .target(&state),
        Some(OptionTarget::Location(LocationId(11)))
    );
    assert_eq!(
        TurnAction::Investigate { investigator: inv }.target(&state),
        Some(OptionTarget::Location(loc_id))
    );
    assert_eq!(
        TurnAction::Fight {
            investigator: inv,
            enemy: EnemyId(7)
        }
        .target(&state),
        Some(OptionTarget::Enemy(EnemyId(7)))
    );
    assert_eq!(
        TurnAction::Evade {
            investigator: inv,
            enemy: EnemyId(7)
        }
        .target(&state),
        Some(OptionTarget::Enemy(EnemyId(7)))
    );
    assert_eq!(
        TurnAction::Engage {
            investigator: inv,
            enemy: EnemyId(7)
        }
        .target(&state),
        Some(OptionTarget::Enemy(EnemyId(7)))
    );
    assert_eq!(
        TurnAction::PlayCard {
            investigator: inv,
            source: PlaySource::Hand(2)
        }
        .target(&state),
        Some(OptionTarget::HandCard {
            investigator: inv,
            hand_index: 2
        })
    );
    assert_eq!(
        TurnAction::ActivateAbility {
            investigator: inv,
            source: AbilitySource::InPlay(CardInstanceId(5)),
            address: AbilityAddress::Printed(0),
        }
        .target(&state),
        Some(OptionTarget::CardInstance(CardInstanceId(5)))
    );
    assert_eq!(
        TurnAction::AdvanceAct { investigator: inv }.target(&state),
        Some(OptionTarget::Act)
    );
}

#[test]
fn end_turn_target_is_none_without_an_open_turn() {
    // `EndTurn` carries no investigator field, so its anchor comes from the
    // turn frame; off-turn there is nothing to anchor to.
    // The Investigation phase with investigator 1 active, but no turn open.
    let state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1))
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .build();
    assert_eq!(TurnAction::EndTurn.target(&state), None);
}

#[test]
fn investigate_target_is_none_without_a_location() {
    // A seat stands at no location unless placed with `at`.
    let state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1))
        .open_turn(InvestigatorId(1))
        .build();
    assert_eq!(
        TurnAction::Investigate {
            investigator: InvestigatorId(1)
        }
        .target(&state),
        None
    );
}
