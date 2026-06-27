//! K5b-1 (#44): the defending player distributes an enemy attack's damage
//! across themselves and eligible soakers, one point at a time (RR p.7),
//! driven through the real `apply` enemy-phase path against the corpus registry.

use game_core::engine::OptionId;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, Continuation, Enemy, InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{
    take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder,
};
use game_core::{Action, EngineOutcome, InputResponse, PlayerAction, TurnAction};

const GUARD_DOG: &str = "01021"; // Ally, 3 health / 1 sanity, retaliate reaction

#[ctor::ctor(unsafe)]
fn install_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// One engaged ready enemy at the investigator's location dealing `damage` / 0 horror.
fn engaged_attacker(id: u32, inv: InvestigatorId, loc: LocationId, damage: u8) -> Enemy {
    let mut e = test_enemy(id, format!("Attacker {id}"));
    e.max_health = 5;
    e.attack_damage = damage;
    e.attack_horror = 0;
    e.current_location = Some(loc);
    e.engaged_with = Some(inv);
    e
}

/// Investigation-phase state: one active investigator controlling `assets`, with
/// `enemy` engaged. `EndTurn` advances into the Enemy phase and runs the attack.
fn attack_state(
    assets: Vec<(&str, CardInstanceId)>,
    enemy: Enemy,
) -> (game_core::GameState, InvestigatorId) {
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(101);
    let mut inv = test_investigator(1);
    // Real investigator code so max_health()/max_sanity() reads from the
    // installed cards registry (#448 cp2a). Skids O'Toole (01003, 8/6).
    inv.investigator_card.code = CardCode::new("01003");
    inv.current_location = Some(loc_id);
    inv.cards_in_play = assets
        .into_iter()
        .map(|(code, inst)| CardInPlay::enter_play(CardCode::new(code), inst))
        .collect();
    // Non-empty deck so the post-attack Upkeep draw doesn't trigger the
    // draw-from-empty horror penalty (which, per K5a, soaks onto a sanity asset
    // and would muddy these damage-only tests).
    inv.deck = vec![CardCode::new(GUARD_DOG); 5];
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(inv)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_enemy(enemy)
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: game_core::state::InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(inv_id)
        .build();
    (state, inv_id)
}

/// True iff `outcome` is the interactive soak-distribution per-point prompt
/// (as opposed to a later framework prompt the enemy phase cascades into).
fn is_distribution_prompt(outcome: &EngineOutcome) -> bool {
    matches!(
        outcome,
        EngineOutcome::AwaitingInput { request, .. } if request.prompt.contains("to which target")
    )
}

/// The `PickSingle` `OptionId` for the distribution-prompt option whose label
/// contains `needle` ("Investigator" for self, "Asset" for a soaker).
fn pick(outcome: &EngineOutcome, needle: &str) -> OptionId {
    assert!(
        is_distribution_prompt(outcome),
        "expected a distribution prompt, got {outcome:?}"
    );
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        unreachable!()
    };
    request
        .options
        .iter()
        .find(|o| o.label.contains(needle))
        .unwrap_or_else(|| panic!("no option matching {needle:?} in {:?}", request.options))
        .id
}

fn resolve(state: game_core::GameState, id: OptionId) -> game_core::ApplyResult {
    game_core::engine::apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(id),
        }),
    )
}

fn guard_dog_damage(
    state: &game_core::GameState,
    inv: InvestigatorId,
    inst: CardInstanceId,
) -> Option<u8> {
    state.investigators[&inv]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == inst)
        .map(|c| c.accumulated_damage)
}

#[test]
fn two_damage_attack_splits_one_to_guard_dog_one_to_self() {
    let dog = CardInstanceId(1);
    let (state, inv) = attack_state(
        vec![(GUARD_DOG, dog)],
        engaged_attacker(7, InvestigatorId(1), LocationId(101), 2),
    );

    // EndTurn → enemy phase → distribution prompt (Guard Dog has capacity).
    let r1 = take_turn_action(state, &TurnAction::EndTurn);
    // First point → Guard Dog; still contested → second prompt → self.
    let r2 = resolve(r1.state, pick(&r1.outcome, "Asset"));
    let r3 = resolve(r2.state, pick(&r2.outcome, "Investigator"));

    assert_eq!(
        guard_dog_damage(&r3.state, inv, dog),
        Some(1),
        "1 damage soaked onto Guard Dog"
    );
    assert_eq!(
        r3.state.investigators[&inv].damage(),
        1,
        "1 damage taken by the investigator"
    );
    // Guard Dog took damage and survived → its retaliate window opens (not a
    // further distribution prompt).
    assert!(
        matches!(r3.outcome, EngineOutcome::AwaitingInput { .. })
            && !is_distribution_prompt(&r3.outcome),
        "Guard Dog's retaliate window opens after the soak: {:?}",
        r3.outcome
    );
}

#[test]
fn player_may_decline_to_soak_taking_all_damage() {
    let dog = CardInstanceId(1);
    let (state, inv) = attack_state(
        vec![(GUARD_DOG, dog)],
        engaged_attacker(7, InvestigatorId(1), LocationId(101), 2),
    );

    let r1 = take_turn_action(state, &TurnAction::EndTurn);
    // Both points to the investigator — decline to soak.
    let r2 = resolve(r1.state, pick(&r1.outcome, "Investigator"));
    let r3 = resolve(r2.state, pick(&r2.outcome, "Investigator"));

    assert_eq!(
        r3.state.investigators[&inv].damage(),
        2,
        "investigator took all 2 damage"
    );
    assert_eq!(
        guard_dog_damage(&r3.state, inv, dog),
        Some(0),
        "Guard Dog untouched (declined to soak)"
    );
}

#[test]
fn a_full_soaker_drops_out_of_the_next_prompt() {
    let dog = CardInstanceId(1);
    let (mut state, inv) = attack_state(
        vec![(GUARD_DOG, dog)],
        engaged_attacker(7, InvestigatorId(1), LocationId(101), 2),
    );
    // Pre-damage Guard Dog to 2 (health 3) → 1 remaining capacity.
    state.investigators.get_mut(&inv).unwrap().cards_in_play[0].accumulated_damage = 2;

    let r1 = take_turn_action(state, &TurnAction::EndTurn);
    // First point → Guard Dog (its last point of capacity).
    let r2 = resolve(r1.state, pick(&r1.outcome, "Asset"));

    // Guard Dog is now full, so the second point is auto-assigned to the
    // investigator with NO further distribution prompt — and Guard Dog, filled
    // to its printed health, is defeated and discarded at placement.
    assert!(
        !is_distribution_prompt(&r2.outcome),
        "the full soaker drops out — no second distribution prompt: {:?}",
        r2.outcome
    );
    assert_eq!(
        r2.state.investigators[&inv].damage(),
        1,
        "the overflow point went to the investigator"
    );
    assert!(
        guard_dog_damage(&r2.state, inv, dog).is_none(),
        "Guard Dog filled to capacity is defeated and discarded",
    );
}
