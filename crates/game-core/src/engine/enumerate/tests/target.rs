use super::*;

#[test]
fn target_maps_each_variant() {
    // A state where investigator 1 stands on a location, so Investigate's
    // implicit anchor resolves to that location.
    let mut state = open_turn_state();
    let loc = test_support::test_location(10, "Study");
    let loc_id = loc.id;
    state.locations.insert(loc_id, loc);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(loc_id);
    let inv = InvestigatorId(1);

    // The three former `Global` actions now name their own surfaces (#541).
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
            hand_index: 2
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
    let mut state = open_turn_state();
    state.continuations = crate::state::ContinuationStack::new();
    assert_eq!(TurnAction::EndTurn.target(&state), None);
}

#[test]
fn investigate_target_is_none_without_a_location() {
    let mut state = open_turn_state();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = None;
    assert_eq!(
        TurnAction::Investigate {
            investigator: InvestigatorId(1)
        }
        .target(&state),
        None
    );
}
