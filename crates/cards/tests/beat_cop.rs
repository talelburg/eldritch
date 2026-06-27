//! C5d (#239) integration: Beat Cop 01018's `[fast] Discard Beat Cop: Deal 1
//! damage to an enemy at your location` end-to-end against the real
//! `cards::REGISTRY`. The activated ability is at index 1 (index 0 is the
//! constant `+1 [combat]`). Own process → installs `cards::REGISTRY`.

use game_core::engine::EngineOutcome;
use game_core::engine::TurnAction;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, EnemyId, InvestigatorId, LocationId, Phase, Zone,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, take_turn_action, test_enemy, test_investigator, test_location,
    GameStateBuilder,
};
use game_core::{assert_event, assert_no_event};

const BEAT_COP: &str = "01018";
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);
const COP_INST: CardInstanceId = CardInstanceId(0);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Board: Beat Cop in play, the active investigator at `LOC`, and (when
/// `enemy_present`) a 3-health enemy co-located at `LOC`.
fn board(enemy_present: bool) -> game_core::GameState {
    let mut inv = test_investigator(1);
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(BEAT_COP), COP_INST));

    let mut builder = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(test_location(10, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV);
    if enemy_present {
        let mut enemy = test_enemy(100, "Ghoul");
        enemy.max_health = 3;
        enemy.current_location = Some(LOC);
        builder = builder.with_enemy(enemy);
    }
    builder.build()
}

const ACTIVATE_ABILITY: TurnAction = TurnAction::ActivateAbility {
    investigator: INV,
    instance_id: COP_INST,
    ability_index: 1, // index 1 = the [fast] discard-damage ability
};

#[test]
fn discards_self_and_deals_one_damage_to_the_co_located_enemy() {
    let r = take_turn_action(board(true), &ACTIVATE_ABILITY);
    assert_event!(r.events, Event::EnemyDamaged { amount: 1, .. });
    assert_eq!(r.state.enemies[&ENEMY].damage, 1);
    // Beat Cop paid its own discard as the cost.
    assert_event!(
        r.events,
        Event::CardDiscarded {
            from: Zone::InPlay,
            ..
        }
    );
    assert!(
        r.state.investigators[&INV].cards_in_play.is_empty(),
        "Beat Cop discarded itself",
    );
    assert_eq!(
        r.state.investigators[&INV].discard,
        vec![CardCode::new(BEAT_COP)],
    );
}

#[test]
fn rejects_with_no_enemy_at_location_and_keeps_beat_cop_in_play() {
    // The ability has no valid target (no enemy) — bypass the legal-actions
    // gate since the action won't be offered.
    let r = dispatch_turn_action_unchecked(board(false), &ACTIVATE_ABILITY);
    assert!(matches!(r.outcome, EngineOutcome::Rejected { .. }));
    // Pre-cost target check (#301) rejects before paying ⇒ Beat Cop survives.
    assert_no_event!(r.events, Event::CardDiscarded { .. });
    assert_eq!(
        r.state.investigators[&INV].cards_in_play.len(),
        1,
        "Beat Cop not discarded for a no-target activation",
    );
}
