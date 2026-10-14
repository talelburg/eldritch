//! C5d (#239) integration: Beat Cop 01018's `[fast] Discard Beat Cop: Deal 1
//! damage to an enemy at your location` end-to-end against the real
//! `cards::REGISTRY`. The activated ability is at index 1 (index 0 is the
//! constant `+1 [combat]`). Own process → installs `cards::REGISTRY`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, DiscardPile, EnemyId, GameStateBuilder,
    InvestigatorId, LocationId, Zone,
};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_no_event};

const BEAT_COP: &str = "01018";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);

/// Board: Beat Cop in play, the investigator's turn at `HERE`, and (when
/// `enemy_present`) a 3-health enemy co-located at `HERE`.
fn board(enemy_present: bool) -> TestSession {
    let mut builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE).in_play([BEAT_COP]))
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV);
    if enemy_present {
        let mut enemy = test_support::test_enemy(ENEMY.0, "Ghoul");
        enemy.max_health = 3;
        enemy.current_location = Some(HERE);
        builder = builder.with_enemy(enemy);
    }
    builder.session()
}

/// Beat Cop's `[fast]` discard-damage ability, printed at index 1.
fn activate_ability(session: &TestSession) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(session.state().in_play(INV, BEAT_COP)),
        address: AbilityAddress::Printed(1),
    }
}

#[test]
fn discards_self_and_deals_one_damage_to_the_co_located_enemy() {
    let session = board(true);
    let activate = activate_ability(&session);
    let session = session.take(&activate);
    session.assert_at_turn_menu(INV);
    let state = session.state();
    assert_event!(session.events(), Event::EnemyDamaged { amount: 1, .. });
    assert_eq!(state.enemies[&ENEMY].damage, 1);
    // Beat Cop paid its own discard as the cost.
    assert_event!(
        session.events(),
        Event::CardDiscarded {
            from: Zone::InPlay,
            to: DiscardPile::Investigator(INV),
            ..
        }
    );
    assert!(
        state.investigators[&INV].cards_in_play.is_empty(),
        "Beat Cop discarded itself",
    );
    assert_eq!(
        state.investigators[&INV].discard,
        vec![CardCode::new(BEAT_COP)],
    );
}

#[test]
fn rejects_with_no_enemy_at_location_and_keeps_beat_cop_in_play() {
    // The ability has no valid target (no enemy), so the menu does not offer
    // it; submitted off the menu, the handler itself must reject it.
    let session = board(false);
    let activate = activate_ability(&session);
    let session = session.submit_off_menu(&activate);
    session.expect_rejected();
    // Pre-cost target check (#301) rejects before paying ⇒ Beat Cop survives.
    assert_eq!(
        session.state().investigators[&INV].cards_in_play.len(),
        1,
        "Beat Cop not discarded for a no-target activation",
    );
    assert_no_event!(session.finish().events, Event::CardDiscarded { .. });
}
