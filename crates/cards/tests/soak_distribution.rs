//! K5b-1 (#44): the defending player distributes an enemy attack's damage
//! across themselves and eligible soakers, one point at a time (RR p.7),
//! driven through the real `apply` enemy-phase path against the corpus registry.

use cards::REGISTRY;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, Enemy, GameState, GameStateBuilder, InvestigatorId,
    LocationId, Owner,
};
use game_core::test_support::{self, TestSession};

const GUARD_DOG: &str = "01021"; // Ally, 3 health / 1 sanity, retaliate reaction

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// One ready enemy dealing `damage` / 0 horror. `attack_state` engages it.
fn ready_attacker(id: u32, damage: u8) -> Enemy {
    let mut e = test_support::test_enemy(id, format!("Attacker {id}"));
    e.max_health = 5;
    e.attack_damage = damage;
    e.attack_horror = 0;
    e
}

/// Investigation-phase state: one active investigator controlling `assets`, with
/// `enemy` engaged. `EndTurn` advances into the Enemy phase and runs the attack.
fn attack_state(assets: Vec<(&str, CardInstanceId)>, enemy: Enemy) -> (GameState, InvestigatorId) {
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(101);
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.cards_in_play = assets
        .into_iter()
        .map(|(code, inst)| {
            CardInPlay::enter_play(
                CardCode::new(code),
                inst,
                Owner::Investigator(InvestigatorId(1)),
            )
        })
        .collect();
    // Non-empty deck so the post-attack Upkeep draw doesn't trigger the
    // draw-from-empty horror penalty (which, per K5a, soaks onto a sanity asset
    // and would muddy these damage-only tests).
    inv.deck = vec![CardCode::new(GUARD_DOG); 5];
    let state = GameStateBuilder::new()
        .with_location(test_support::test_location(101, "Study"))
        .with_investigator(inv)
        .with_enemy_engaged(enemy, inv_id)
        .open_turn(inv_id)
        .build();
    (state, inv_id)
}

/// True iff the session rests at the interactive soak-distribution per-point
/// prompt, as opposed to Guard Dog's reaction window, which also offers Guard
/// Dog: the distribution must be answered, the window may be passed.
fn at_distribution_prompt(session: &TestSession) -> bool {
    !session.prompt().skippable
}

fn guard_dog_damage(state: &GameState, inv: InvestigatorId, inst: CardInstanceId) -> Option<u8> {
    state.investigators[&inv]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == inst)
        .map(|c| c.accumulated_damage)
}

#[test]
fn two_damage_attack_splits_one_to_guard_dog_one_to_self() {
    let dog = CardInstanceId(1);
    let (state, inv) = attack_state(vec![(GUARD_DOG, dog)], ready_attacker(7, 2));
    let me = state.investigators[&inv].card_anchor();

    // EndTurn → enemy phase → distribution prompt (Guard Dog has capacity).
    // First point → Guard Dog; still contested → second prompt → self.
    let session = TestSession::new(state)
        .take(&TurnAction::EndTurn)
        .pick(OptionTarget::CardInstance(dog))
        .pick(me.clone());

    // The distribution is complete but nothing is placed: Guard Dog is in the
    // assignment, so its `when` cell opens between the two rules steps (#727),
    // and that is the window here rather than a further distribution prompt.
    assert!(
        !at_distribution_prompt(&session),
        "Guard Dog's when-cell window opens once distribution drains: {:?}",
        session.prompt()
    );
    assert_eq!(
        guard_dog_damage(session.state(), inv, dog),
        Some(0),
        "assigned to Guard Dog, not yet placed"
    );
    assert_eq!(
        session.state().investigators[&inv].damage(),
        0,
        "and none of it placed on the investigator yet either"
    );

    // Firing the retaliate lets the deal reach its placement — and the whole
    // assignment lands at once (RR p.7 "simultaneously").
    let session = session.pick(OptionTarget::CardInstance(dog));
    assert_eq!(
        guard_dog_damage(session.state(), inv, dog),
        Some(1),
        "1 damage placed on Guard Dog"
    );
    assert_eq!(
        session.state().investigators[&inv].damage(),
        1,
        "1 damage placed on the investigator, in the same moment"
    );
}

#[test]
fn player_may_decline_to_soak_taking_all_damage() {
    let dog = CardInstanceId(1);
    let (state, inv) = attack_state(vec![(GUARD_DOG, dog)], ready_attacker(7, 2));
    let me = state.investigators[&inv].card_anchor();

    // Both points to the investigator — decline to soak.
    let session = TestSession::new(state)
        .take(&TurnAction::EndTurn)
        .pick(me.clone())
        .pick(me);

    assert_eq!(
        session.state().investigators[&inv].damage(),
        2,
        "investigator took all 2 damage"
    );
    assert_eq!(
        guard_dog_damage(session.state(), inv, dog),
        Some(0),
        "Guard Dog untouched (declined to soak)"
    );
}

#[test]
fn a_full_soaker_drops_out_of_the_next_prompt() {
    let dog = CardInstanceId(1);
    let (mut state, inv) = attack_state(vec![(GUARD_DOG, dog)], ready_attacker(7, 2));
    // Pre-damage Guard Dog to 2 (health 3) → 1 remaining capacity.
    state.investigators.get_mut(&inv).unwrap().cards_in_play[0].accumulated_damage = 2;

    // First point → Guard Dog (its last point of capacity).
    let session = TestSession::new(state)
        .take(&TurnAction::EndTurn)
        .pick(OptionTarget::CardInstance(dog));

    // Guard Dog is now full, so the second point is auto-assigned to the
    // investigator with NO further distribution prompt. What is open instead is
    // Guard Dog's `when` cell on the completed assignment (#727) — a lethal one,
    // which it is still entitled to react to (`data/arkhamdb-faq/core/01021.md`).
    assert!(
        !at_distribution_prompt(&session),
        "the full soaker drops out — no second distribution prompt: {:?}",
        session.prompt()
    );
    let session = session.pick(OptionTarget::CardInstance(dog));
    assert_eq!(
        session.state().investigators[&inv].damage(),
        1,
        "the overflow point went to the investigator"
    );
    assert!(
        guard_dog_damage(session.state(), inv, dog).is_none(),
        "Guard Dog filled to capacity is defeated and discarded",
    );
}
