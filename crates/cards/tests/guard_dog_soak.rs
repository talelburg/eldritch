//! End-to-end enemy-attack damage soak + Guard Dog reaction, driven
//! through the public [`apply`] API with the real card corpus installed.
//!
//! These cover the coverage deferred from `game-core`'s unit tests, which
//! can't install the real `cards::REGISTRY` (the engine crate can't depend
//! on `cards`):
//!
//! 1. Happy path: an enemy attack assigns damage to Guard Dog; the
//!    `DamageAssigned` window opens **before anything is placed**; firing
//!    Guard Dog's reaction deals 1 damage to the attacker, and only then
//!    does the damage land on the dog.
//! 2. Asset defeat on overflow: damage reaching Guard Dog's printed
//!    health (3) defeats it (`CardDiscarded`, removed from
//!    `cards_in_play`) — *after* it has retaliated, since the assignment
//!    is announced while it is still in play.
//! 3. Instance self-binding: with a second damage-soaking asset in play
//!    that has no reaction, only Guard Dog's reaction is offered.
//! 4. Two-attacker suspend/resume: an investigator engaged by two enemies
//!    suspends on the first attack's soak window and resumes the second
//!    attacker after the reaction resolves.
//!
//! Guard Dog 01021: "[reaction] When an enemy attack deals damage to Guard
//! Dog: Deal 1 damage to the attacking enemy." Health 3, sanity 1, Ally.
//!
//! **The retaliate resolves in the `when` cell of `DamageAssigned`** (#722),
//! which is why every case here reads the dog's `accumulated_damage` as 0 at
//! the window and non-zero only after the reaction has resolved. That order is
//! the Rules Reference's — `glossary/Nested_Sequences.md` runs its worked
//! example on this card, and `glossary/Dealing_Damage_Horror.md` puts the
//! window between assigning and placing. See
//! `docs/adr/0009-damage-is-assigned-then-placed.md`.

use game_core::engine::{apply, ApplyResult, EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, Continuation, Enemy, EnemyId, InvestigatorId, LocationId,
    Phase, Status, Zone,
};
use game_core::test_support::{take_turn_action, test_enemy, test_investigator, test_location};
use game_core::{Action, InputResponse, PlayerAction, TurnAction};

/// Guard Dog (01021): Guardian Ally, health 3 / sanity 1, with the
/// damage-retaliate reaction.
const GUARD_DOG: &str = "01021";

/// Bulletproof Vest (01094): Body-slot asset, health 4 / no sanity, no
/// reaction. A damage soaker that legally co-exists with Guard Dog (Body
/// vs Ally slot) and never reacts — used for the self-binding case.
const BULLETPROOF_VEST: &str = "01094";

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// An engaged enemy at the investigator's location dealing `attack_damage`
/// damage / 0 horror, ready (not exhausted), with `max_health`.
fn engaged_attacker(
    id: u32,
    inv: InvestigatorId,
    loc: LocationId,
    attack_damage: u8,
    max_health: u8,
) -> Enemy {
    let mut e = test_enemy(id, format!("Attacker {id}"));
    e.max_health = max_health;
    e.attack_damage = attack_damage;
    e.attack_horror = 0;
    e.current_location = Some(loc);
    e.engaged_with = Some(inv);
    e
}

/// Build an Investigation-phase state with one active investigator at a
/// location, `assets` in play, and `enemies` engaged. Driving
/// `PlayerAction::EndTurn` from here advances Investigation → Enemy and
/// runs the per-investigator attack loop (the `BeforeInvestigatorAttacked`
/// Fast window auto-skips — Guard Dog has no Fast ability — so the loop
/// runs inline and suspends on the soak reaction window).
fn soak_state(
    assets: Vec<(&str, CardInstanceId)>,
    enemies: Vec<Enemy>,
) -> (game_core::GameState, InvestigatorId, LocationId) {
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
    // A deck with cards left in it. The Upkeep draw these cases cascade through
    // otherwise draws from an empty deck, whose penalty is 1 horror (#429) —
    // which Guard Dog's 1 printed sanity soaks and dies to, in the middle of a
    // test measuring what the *attack* did to it.
    inv.deck = vec![CardCode::new("01087"); 5];

    let mut builder = game_core::test_support::GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(inv)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id]);
    for enemy in enemies {
        builder = builder.with_enemy(enemy);
    }
    // Mid-Investigation invariant (slice 1a): the EndTurn cascade pops the
    // InvestigationPhase anchor at investigation_phase_end.
    builder = builder.with_phase_anchor(game_core::state::Continuation::InvestigationPhase {
        resume: game_core::state::InvestigationResume::TurnBegins,
    });
    // Open-turn invariant (slice 2a-i, #393): the InvestigatorTurn frame the
    // EndTurn cascade pops before advancing into the Enemy phase.
    builder = builder.with_investigator_turn(inv_id);
    (builder.build(), inv_id, loc_id)
}

/// Find the investigator's Guard Dog instance.
fn guard_dog_card(
    state: &game_core::GameState,
    inv: InvestigatorId,
    inst: CardInstanceId,
) -> &CardInPlay {
    state.investigators[&inv]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == inst)
        .expect("Guard Dog still in play")
}

/// From a suspended attack-order prompt (#143), the `PickSingle` `OptionId`
/// whose label matches `enemy`'s debug repr.
fn order_pick(outcome: &EngineOutcome, enemy: EnemyId) -> game_core::engine::OptionId {
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("expected an attack-order prompt, got {outcome:?}");
    };
    request
        .options
        .iter()
        .find(|o| o.label == format!("{enemy:?}"))
        .expect("attacker offered in the order pick")
        .id
}

/// Resume a suspended prompt/window by selecting option `id`.
fn resolve_pick(state: game_core::GameState, id: game_core::engine::OptionId) -> ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(id),
        }),
    )
}

/// True iff the outcome is the interactive soak-distribution per-point prompt
/// (#44/K5b), as opposed to a soak/retaliate window or a framework prompt.
fn is_distribution_prompt(outcome: &EngineOutcome) -> bool {
    matches!(
        outcome,
        EngineOutcome::AwaitingInput { request, .. } if request.prompt.contains("to which target")
    )
}

/// Resolve a soak distribution (#44/K5b) by assigning every point to the soaker
/// `inst` while it has capacity, then to the investigator once it is full —
/// reproducing the pre-K5b soak-first default. Returns the first result that is
/// no longer a distribution prompt.
fn distribute_onto(mut result: ApplyResult, inst: CardInstanceId) -> ApplyResult {
    while is_distribution_prompt(&result.outcome) {
        let EngineOutcome::AwaitingInput { request, .. } = &result.outcome else {
            unreachable!()
        };
        let needle = format!("CardInstanceId({})", inst.0);
        let id = request
            .options
            .iter()
            .find(|o| o.label.contains(&needle))
            .or_else(|| request.options.iter().find(|o| o.label == "Investigator"))
            .expect("a distribution option")
            .id;
        result = resolve_pick(result.state, id);
    }
    result
}

/// Fire the single pending trigger of an open reaction window — Guard Dog's
/// retaliate, in every case here. Since #727 the window is `DamageAssigned`'s
/// `when` cell, so this is also what lets the deal proceed to its placement.
fn fire_retaliate(state: game_core::GameState) -> ApplyResult {
    resolve_pick(state, OptionId(0))
}

// ---------------------------------------------------------------------
// Case 1 — happy path
// ---------------------------------------------------------------------

#[test]
fn enemy_attack_soaks_onto_guard_dog_then_retaliate_damages_attacker() {
    let dog = CardInstanceId(1);
    let enemy_id = EnemyId(7);
    let inv = InvestigatorId(1);
    let loc = LocationId(101);
    // Attack deals 2 damage; Guard Dog (health 3) soaks all of it, the
    // investigator takes none.
    let (mut state, inv_id, _) = soak_state(
        vec![(GUARD_DOG, dog)],
        vec![engaged_attacker(7, inv, loc, 2, 3)],
    );

    let result = take_turn_action(state, &TurnAction::EndTurn);
    // Distribute the attack: assign both points onto Guard Dog (#44/K5b).
    let result = distribute_onto(result, dog);
    state = result.state;

    // The attack-loop suspended on Guard Dog's `when`-cell window.
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "the assignment's when-cell window must suspend the attack loop: {:?}",
        result.outcome
    );
    // Nothing is placed yet: the damage is *assigned* to Guard Dog — tokens
    // "next to" it, in the Rules Reference's words — and the window between the
    // two steps is what is open.
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        0,
        "damage is assigned, not yet placed, while the when cell is open"
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "investigator took no damage"
    );
    // No damage on the attacker yet (reaction not fired).
    assert_eq!(state.enemies[&enemy_id].damage, 0);

    // Fire Guard Dog's reaction (the single pending trigger).
    let result = fire_retaliate(state);
    state = result.state;

    // The attacker took exactly 1 damage.
    assert_eq!(
        state.enemies[&enemy_id].damage, 1,
        "Guard Dog dealt 1 damage to the attacker"
    );
    // …and only then did the assignment land on Guard Dog.
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        2,
        "Guard Dog takes its assigned 2 damage once the when cell has run"
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "investigator still took none"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == enemy_id
        )),
        "EnemyDamaged {{ amount: 1 }} emitted: {:?}",
        result.events
    );
}

// ---------------------------------------------------------------------
// Case 2a — overflow defeats Guard Dog by the SAME attack: it retaliates
// anyway, because the assignment is announced while it is still in play.
//
// This is the live bug #727 fixed, not merely a retag. Placement used to
// come first and only *surviving* damaged assets were announced, so a Guard
// Dog killed by the attack that damaged it never bit back — against its own
// ruling, `data/arkhamdb-faq/core/01021.md`: "You can use Guard Dog's
// ability when you assign lethal damage/horror to it."
// ---------------------------------------------------------------------

#[test]
fn guard_dog_retaliates_on_a_lethal_assignment_then_is_defeated() {
    let dog = CardInstanceId(1);
    let enemy_id = EnemyId(7);
    let inv = InvestigatorId(1);
    let loc = LocationId(101);
    // Attack deals 3 damage = Guard Dog's printed health → the assignment is
    // lethal, and the dog is defeated once it is placed.
    let (mut state, inv_id, _) = soak_state(
        vec![(GUARD_DOG, dog)],
        vec![engaged_attacker(7, inv, loc, 3, 3)],
    );

    let result = take_turn_action(state, &TurnAction::EndTurn);
    let result = distribute_onto(result, dog);
    state = result.state;

    // The lethal assignment opened Guard Dog's `when` cell, with the dog still
    // in play and undamaged.
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "a lethal assignment still opens the when cell: {:?}",
        result.outcome
    );
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        0,
        "nothing placed yet — the dog is alive and about to bite"
    );

    let result = fire_retaliate(state);
    state = result.state;

    // It bit back, and *then* the damage landed and defeated it.
    assert_eq!(
        state.enemies.get(&enemy_id).map(|e| e.damage),
        Some(1),
        "a Guard Dog assigned lethal damage still retaliates (01021's ruling)"
    );
    assert!(
        !state.investigators[&inv_id]
            .cards_in_play
            .iter()
            .any(|c| c.instance_id == dog),
        "defeated Guard Dog removed from cards_in_play"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::CardDiscarded { code, from, .. }
                if *code == CardCode::new(GUARD_DOG) && *from == Zone::InPlay
        )),
        "Guard Dog discard emitted: {:?}",
        result.events
    );
    // The retaliate preceded the damage it reacted to, in the log as in the
    // rules: `EnemyDamaged` before the dog's `CardDiscarded`.
    let retaliate_at = result
        .events
        .iter()
        .position(|e| matches!(e, Event::EnemyDamaged { enemy, .. } if *enemy == enemy_id));
    let discard_at = result.events.iter().position(|e| {
        matches!(e, Event::CardDiscarded { code, from, .. }
            if *code == CardCode::new(GUARD_DOG) && *from == Zone::InPlay)
    });
    assert!(
        retaliate_at < discard_at,
        "the when-cell retaliate precedes the placement that defeats the dog: {:?}",
        result.events
    );
}

// ---------------------------------------------------------------------
// Case 2c — the attacker is defeated by the retaliate *during its own
// enemy-phase attack*, so there is nothing left to exhaust.
//
// #704 moved the exhaust off the attack's resolve step and onto the parked
// `AttackLoop` frame, which runs it after the whole sequence
// (`Appendix_II_Timing_and_Gameplay.md` step 3.3: "Upon completion of dealing
// the attack (and all abilities triggered by the attack), exhaust the enemy.").
// #727 made this path *more* reachable: the retaliate now resolves in the `when`
// cell, before the damage is placed, so an attacker can die mid-sequence even on
// the attack that would have killed the dog.
// ---------------------------------------------------------------------

#[test]
fn an_attacker_defeated_by_the_retaliate_mid_attack_has_nothing_to_exhaust() {
    let dog = CardInstanceId(1);
    let enemy_id = EnemyId(7);
    let inv = InvestigatorId(1);
    let loc = LocationId(101);
    // The attacker has 1 health, so Guard Dog's 1 retaliate damage defeats it
    // in the `when` cell of its own attack's damage. Enemy phase, so it *would*
    // have exhausted had it survived (unlike the AoO/Retaliate cases).
    let (mut state, inv_id, _) = soak_state(
        vec![(GUARD_DOG, dog)],
        vec![engaged_attacker(7, inv, loc, 2, 1)],
    );

    let result = take_turn_action(state, &TurnAction::EndTurn);
    let result = distribute_onto(result, dog);
    let result = fire_retaliate(result.state);
    state = result.state;

    assert!(
        !state.enemies.contains_key(&enemy_id),
        "the retaliate defeated the attacker during its own attack: {:?}",
        result.events
    );
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, Event::EnemyExhausted { enemy } if *enemy == enemy_id)),
        "a defeated attacker has nothing to exhaust: {:?}",
        result.events
    );
    // The rest of the sequence still ran: the damage it had already assigned is
    // placed even though the attacker is gone, because the assignment was
    // settled before the retaliate (RR step 1 precedes step 2).
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        2,
        "the dead attacker's assigned damage still lands on Guard Dog"
    );
    // And the loop drained rather than stalling on a missing attacker.
    assert!(
        !state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::AttackLoop { .. })),
        "the attack loop completed: {:?}",
        state.continuations
    );
}

// ---------------------------------------------------------------------
// Case 2b — overflow defeats Guard Dog AFTER it survives a prior attack
// (accumulated damage builds across attacks; the lethal one removes it)
// ---------------------------------------------------------------------

#[test]
fn guard_dog_defeated_on_overflow_is_discarded_from_play() {
    // Pre-load Guard Dog with 2 accumulated damage (a prior attack it
    // survived); a fresh 2-damage attack pushes it to 4 >= 3 → defeated.
    let dog = CardInstanceId(1);
    let enemy_id = EnemyId(7);
    let inv = InvestigatorId(1);
    let loc = LocationId(101);
    let (mut state, inv_id, _) = soak_state(
        vec![(GUARD_DOG, dog)],
        vec![engaged_attacker(7, inv, loc, 2, 3)],
    );
    // Survived a prior attack: 2 already accumulated (under health 3).
    state.investigators.get_mut(&inv_id).unwrap().cards_in_play[0].accumulated_damage = 2;

    let result = take_turn_action(state, &TurnAction::EndTurn);
    let result = distribute_onto(result, dog);
    // The assignment's `when` cell opens with the dog still in play; firing its
    // retaliate lets the deal proceed to the placement that defeats it.
    let result = fire_retaliate(result.state);
    state = result.state;

    assert!(
        !state.investigators[&inv_id]
            .cards_in_play
            .iter()
            .any(|c| c.instance_id == dog),
        "Guard Dog at accumulated 4 >= health 3 is discarded"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::CardDiscarded { code, from, .. }
                if *code == CardCode::new(GUARD_DOG) && *from == Zone::InPlay
        )),
        "Guard Dog discard emitted: {:?}",
        result.events
    );
    // The attacker (max_health 3, attack 2) took the retaliate the dog got in
    // before its own defeat.
    assert_eq!(
        state.enemies[&enemy_id].damage, 1,
        "the dog retaliated from the assignment, before the placement killed it"
    );
}

// ---------------------------------------------------------------------
// Case 3 — instance self-binding: only the soaked Guard Dog reacts
// ---------------------------------------------------------------------

#[test]
fn only_guard_dogs_reaction_is_offered_not_another_controlled_soaker() {
    // Two soaking assets controlled by the same investigator, soak-ordered
    // by CardInstanceId:
    //   - Guard Dog (Ally slot, health 3), instance 1 — soaks first.
    //   - Bulletproof Vest (Body slot, health 4), instance 2.
    // Different slots, so both are legally in play. A 2-damage attack soaks
    // entirely onto Guard Dog (2 < 3 → survives), never reaching the Vest.
    //
    // The self-binding point: the Vest is a controlled soaker too, but the
    // soak window is scoped to the *damaged* asset, and only Guard Dog has
    // an `EnemyAttackDamagedSelf` reaction. So exactly one window opens
    // — keyed to Guard Dog's instance — even though another soaker sits in
    // play. (Two surviving *damaged* soakers from a single attack isn't
    // constructible: `assign_attack` fills each soaker to capacity before
    // the next, and reaching capacity defeats it — so any non-final soaker
    // that takes damage is defeated. The honest demonstration is therefore
    // "the keyed instance is the one whose reaction fires," not two live
    // reacting allies — which two Ally slots also forbid.)
    let dog = CardInstanceId(1);
    let vest = CardInstanceId(2);
    let enemy_id = EnemyId(7);
    let inv = InvestigatorId(1);
    let loc = LocationId(101);
    let (mut state, inv_id, _) = soak_state(
        vec![(GUARD_DOG, dog), (BULLETPROOF_VEST, vest)],
        vec![engaged_attacker(7, inv, loc, 2, 3)],
    );

    let result = take_turn_action(state, &TurnAction::EndTurn);
    let result = distribute_onto(result, dog);
    state = result.state;

    // Guard Dog's reaction window suspended the loop.
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "Guard Dog's reaction window suspends the loop: {:?}",
        result.outcome
    );
    // The whole assignment went to Guard Dog; nothing is placed yet, and the
    // Vest is in neither the assignment nor the window.
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        0,
        "assigned to Guard Dog, not yet placed"
    );
    assert_eq!(
        state.investigators[&inv_id]
            .cards_in_play
            .iter()
            .find(|c| c.instance_id == vest)
            .map(|c| c.accumulated_damage),
        Some(0),
        "Bulletproof Vest is not in the assignment (Guard Dog absorbed it all)"
    );
    // Exactly one pending window, and the assignment it carries names Guard
    // Dog's instance — not the Vest's (a controlled soaker with no
    // `EnemyAttackDamagedSelf` ability).
    let soak_windows: Vec<_> = state
        .open_windows()
        .iter()
        .filter_map(|w| match w.window_timing_event() {
            Some(game_core::engine::TimingEvent::DamageAssigned { assignment, .. }) => {
                Some(assignment.asset_damage.keys().copied().collect::<Vec<_>>())
            }
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(
        soak_windows,
        vec![dog],
        "exactly the Guard Dog instance is in the open window's assignment, not the Vest's"
    );

    // Firing the single offered trigger retaliates (it's Guard Dog's, not
    // the Vest's — the Vest contributes no trigger at all).
    let result = fire_retaliate(state);
    state = result.state;
    assert_eq!(
        state.enemies[&enemy_id].damage, 1,
        "Guard Dog's reaction (not the Vest's) dealt 1 damage"
    );
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        2,
        "and the assigned damage then landed on Guard Dog"
    );
    assert_eq!(
        state.investigators[&inv_id]
            .cards_in_play
            .iter()
            .find(|c| c.instance_id == vest)
            .map(|c| c.accumulated_damage),
        Some(0),
        "Bulletproof Vest still untouched after placement"
    );
}

// ---------------------------------------------------------------------
// Case 4 — two attackers: suspend on the first, resume the second
// ---------------------------------------------------------------------

#[test]
fn two_attackers_suspend_on_first_soak_then_resume_second_attacker() {
    let dog = CardInstanceId(1);
    let inv = InvestigatorId(1);
    let loc = LocationId(101);
    let first = EnemyId(7);
    let second = EnemyId(8);
    // Two engaged attackers, each dealing 1 damage; Guard Dog (health 3)
    // soaks both. The first attack opens the soak window and suspends; after
    // resolving the reaction, the loop resumes and the second attacker
    // attacks too. Both end exhausted.
    let (mut state, inv_id, _) = soak_state(
        vec![(GUARD_DOG, dog)],
        vec![
            engaged_attacker(7, inv, loc, 1, 3),
            engaged_attacker(8, inv, loc, 1, 3),
        ],
    );

    // Two engaged attackers → the enemy phase first asks the player which
    // attacks next (#143). Pick the first attacker (EnemyId 7).
    let result = take_turn_action(state, &TurnAction::EndTurn);
    let pick_first = order_pick(&result.outcome, first);

    // The chosen first attacker attacks: its 1 damage prompts the soak
    // distribution (#44/K5b) — assign it to Guard Dog → suspend on the soak window.
    let result = resolve_pick(result.state, pick_first);
    let result = distribute_onto(result, dog);
    state = result.state;
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "first attack's soak window suspends the loop: {:?}",
        result.outcome
    );
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        0,
        "first attacker's 1 damage is assigned, not yet placed"
    );
    // Neither attacker has exhausted yet: since #704 the exhaust follows the
    // *whole* attack sequence — `Appendix_II_Timing_and_Gameplay.md` step 3.3's
    // "Upon completion of dealing the attack (and all abilities triggered by the
    // attack), exhaust the enemy." — and the first attack's soak reaction is one
    // of those abilities, still pending.
    assert!(
        !state.enemies[&first].exhausted,
        "first attacker exhausts only once its sequence completes"
    );
    assert!(
        !state.enemies[&second].exhausted,
        "second attacker not yet resolved"
    );
    // The parked loop still carries the attacking head plus the untouched
    // second attacker; the head comes off when its sequence pops.
    assert_eq!(
        state.continuations.iter().rev().find_map(|c| match c {
            Continuation::AttackLoop {
                remaining_attackers,
                ..
            } => Some(remaining_attackers.clone()),
            _ => None,
        }),
        Some(vec![first, second]),
        "the attacking head and the parked second attacker"
    );

    // Resolve the first reaction window → the first attacker takes the
    // retaliation, then the loop resumes the second attacker, whose 1 damage
    // prompts its own soak distribution (assign to Guard Dog). The second
    // attack ALSO soaks onto the (surviving) Guard Dog, opening a second
    // soak window and re-suspending — a clean demonstration that the
    // resumed loop suspends again on a later attacker.
    let result = resolve_pick(state, OptionId(0));
    let result = distribute_onto(result, dog);
    state = result.state;

    assert_eq!(
        state.enemies[&first].damage, 1,
        "first attacker took Guard Dog's retaliation"
    );
    // The first attack's damage has now landed; the second attack's is assigned
    // but not yet placed, its own `when` cell being the window we are parked on.
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        1,
        "first attacker's damage placed; the second's is only assigned"
    );
    assert!(
        state.enemies[&first].exhausted,
        "first attacker exhausts once its retaliate reaction has resolved"
    );
    assert!(
        !state.enemies[&second].exhausted,
        "the second attacker's own soak reaction is still pending"
    );
    // ...and the loop re-suspended on the second attacker's soak window.
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "second attacker's soak window re-suspends the resumed loop: {:?}",
        result.outcome
    );
    assert!(
        state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::AttackLoop { .. })),
        "loop is parked again after the second attack"
    );

    // Resolve the second reaction window → second attacker takes the
    // retaliation, the loop drains with no attackers left, the enemy phase
    // cascades onward, and nothing remains parked.
    let result = resolve_pick(state, OptionId(0));
    state = result.state;

    assert_eq!(
        state.enemies[&second].damage, 1,
        "second attacker took Guard Dog's retaliation on the second window"
    );
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        2,
        "both attacks' damage has landed once both sequences complete"
    );
    assert!(
        !state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::AttackLoop { .. })),
        "no parked attack after both attackers fully resolve"
    );
    assert!(
        state.open_windows().is_empty(),
        "no soak windows left open once both attacks resolve"
    );
}

// ---------------------------------------------------------------------
// Case 5 — attack of opportunity soaks onto Guard Dog; the soak window
// opens, Guard Dog retaliates, and the move completes after resume
// (#293 acceptance).
//
// Verified card text (ArkhamDB https://arkhamdb.com/card/01021, 2026-06-21):
//   Guard Dog (01021): "[reaction] When an enemy attack deals damage to
//   Guard Dog: Deal 1 damage to the attacking enemy." Health 3, Sanity 1.
//   FAQ: "You can use Guard Dog's ability when you assign lethal damage/
//   horror to it." Also confirmed via FAQ: Guard Dog's reaction fires
//   against attacks of opportunity (the trigger is 'when an enemy attack
//   deals damage', with no carve-out for AoO).
//
// The before-#293 behaviour was: `drive_aoo` dropped the survivor list so
// Guard Dog's window was never queued. After #293 the AoO runs through
// `drive_attack_loop` (which opens both the `EnemyAttacks` cancel window and
// the `DamageAssigned` window inside the attack's damage), so the full
// suspend/resume cycle now applies to AoO attacks.
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::too_many_lines)]
fn move_attack_of_opportunity_guard_dog_retaliates_and_move_completes() {
    // An investigator controlling Guard Dog, engaged by a ready enemy,
    // takes a Move action. The Move fires an attack of opportunity (through
    // the ActionResolution frame + drive_aoo path, #293). Guard Dog has no
    // cancel reaction so the before-attack window auto-skips; the AoO
    // damage soaks onto Guard Dog; the soak window opens and suspends;
    // the player fires Guard Dog's reaction; the attacker takes 1 damage;
    // the move then completes as the ActionResolution frame resumes.
    let dog = CardInstanceId(1);
    let enemy_id = EnemyId(7);
    let inv_id = InvestigatorId(1);
    let from = LocationId(101);
    let dest = LocationId(102);

    let mut study = test_location(101, "Study");
    study.connections = vec![dest];
    let mut hallway = test_location(102, "Hallway");
    hallway.connections = vec![from];

    let mut investigator = test_investigator(1);
    investigator.current_location = Some(from);
    investigator.cards_in_play = vec![CardInPlay::enter_play(CardCode::new(GUARD_DOG), dog)];

    // Engaged ready attacker dealing 2 damage; Guard Dog (health 3) soaks
    // all of it and survives (2 < 3).
    let attacker = engaged_attacker(7, inv_id, from, 2, 3);

    let state = game_core::test_support::GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(study)
        .with_location(hallway)
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_enemy(attacker)
        .build();

    // Step 1: take the Move — AoO runs; Guard Dog has no cancel reaction
    // so the before-attack window is skipped; damage soaks onto Guard Dog;
    // the soak window opens and suspends.
    let result = take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: dest,
        },
    );
    // The AoO prompts for the soak distribution (#44/K5b): assign both points
    // onto Guard Dog to reproduce the soak.
    let result = distribute_onto(result, dog);
    let mut state = result.state;

    // The AoO's soak window suspended the loop (the ActionResolution
    // frame is parked beneath the AttackLoop beneath the Resolution window).
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "AoO soak window must suspend the loop: {:?}",
        result.outcome
    );
    // The AoO damage is *assigned* to Guard Dog and not yet placed — the open
    // window is the one between the two steps.
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        0,
        "AoO damage assigned to Guard Dog, not yet placed"
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "investigator took no AoO damage (fully soaked)"
    );
    // Investigator has NOT moved yet (the ActionResolution frame is parked).
    assert_eq!(
        state.investigators[&inv_id].current_location,
        Some(from),
        "move not yet resolved while window is open"
    );
    // No retaliation damage on the attacker yet.
    assert_eq!(state.enemies[&enemy_id].damage, 0);

    // Step 2: fire Guard Dog's reaction (the single pending trigger).
    let result = fire_retaliate(state);
    state = result.state;

    // Guard Dog dealt 1 retaliate damage to the attacker.
    assert_eq!(
        state.enemies[&enemy_id].damage, 1,
        "Guard Dog's reaction dealt 1 damage to the AoO attacker"
    );
    // …and the assigned damage then landed on it.
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        2,
        "AoO damage placed on Guard Dog once the when cell has run"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == enemy_id
        )),
        "EnemyDamaged {{ amount: 1 }} emitted: {:?}",
        result.events
    );

    // The attacker did NOT exhaust (RR p.7: AoO attackers never exhaust).
    assert!(
        !state.enemies[&enemy_id].exhausted,
        "an attack of opportunity does not exhaust the attacker (RR p.7)"
    );

    // The move completed: investigator and engaged enemy are at the
    // destination, confirming the ActionResolution frame resumed correctly.
    assert_eq!(
        state.investigators[&inv_id].current_location,
        Some(dest),
        "move resolved: investigator reached the destination"
    );
    assert_eq!(
        state.enemies[&enemy_id].current_location,
        Some(dest),
        "engaged enemy moved with the investigator to the destination"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::InvestigatorMoved { investigator, from: f, to } if
                *investigator == inv_id && *f == from && *to == dest
        )),
        "InvestigatorMoved event emitted after window closed: {:?}",
        result.events
    );

    // No reaction windows remain after the full cycle.
    assert!(
        state.open_windows().is_empty(),
        "no windows stranded after Guard Dog reaction + move resume: {:?}",
        state.open_windows()
    );
}

// ---------------------------------------------------------------------
// Case 6 — the investigator card is the mandatory-remainder soaker
// (#448 cp2b). An asset soaks to capacity (and is defeated); the
// remainder lands on the *investigator card* via its `accumulated_damage`
// (not a bespoke field), exactly as the RR's "all damage that cannot be
// assigned to an asset must be assigned to the investigator" clause
// requires. This is the soaker-side half of the soak/defeat unification.
// ---------------------------------------------------------------------

#[test]
fn an_asset_soaks_first_then_the_investigator_card_takes_the_remainder() {
    let dog = CardInstanceId(1);
    let inv = InvestigatorId(1);
    let loc = LocationId(101);
    // Attack deals 5 damage. Guard Dog (printed health 3) soaks 3 and is
    // defeated by reaching its printed health; the remaining 2 must be
    // assigned to the investigator — landing on the investigator card's
    // `accumulated_damage`. Skids O'Toole (01003) has 8 health, so 2 < 8 →
    // the investigator survives.
    let (mut state, inv_id, _) = soak_state(
        vec![(GUARD_DOG, dog)],
        vec![engaged_attacker(7, inv, loc, 5, 3)],
    );

    let result = take_turn_action(state, &TurnAction::EndTurn);
    // Distribute soak-first: fill Guard Dog to capacity (3), then the rest
    // onto the investigator.
    let result = distribute_onto(result, dog);
    // The assignment gives Guard Dog damage, so its `when` cell opens before
    // anything is placed; fire the retaliate to let the deal reach step 2.
    let result = fire_retaliate(result.state);
    state = result.state;

    // Guard Dog reached printed health → defeated and discarded from play.
    assert!(
        !state.investigators[&inv_id]
            .cards_in_play
            .iter()
            .any(|c| c.instance_id == dog),
        "Guard Dog at accumulated 3 >= health 3 is discarded"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::CardDiscarded { code, from, .. }
                if *code == CardCode::new(GUARD_DOG) && *from == Zone::InPlay
        )),
        "Guard Dog discard emitted: {:?}",
        result.events
    );
    // The mandatory remainder (2) landed on the investigator *card* —
    // `investigator_card.accumulated_damage`, surfaced via `damage()`.
    assert_eq!(
        state.investigators[&inv_id]
            .investigator_card
            .accumulated_damage,
        2,
        "the 2-damage remainder lands on the investigator card's accumulated_damage"
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        2,
        "damage() reads the investigator card's accumulated_damage"
    );
    // The investigator is not defeated (2 < 8).
    assert_eq!(
        state.investigators[&inv_id].status,
        Status::Active,
        "investigator survives a sub-lethal remainder"
    );
}

// ---------------------------------------------------------------------
// Case 7 — investigator-card overflow triggers investigator elimination,
// not asset discard (#448 cp2b). The defeat half of the unification: the
// investigator card uses the same `accumulated >= printed capacity` rule
// as an asset, but the consequence is elimination (Status::Defeated) rather
// than discard-to-owner.
// ---------------------------------------------------------------------

#[test]
fn investigator_card_overflow_eliminates_the_investigator() {
    let dog = CardInstanceId(1);
    let inv = InvestigatorId(1);
    let loc = LocationId(101);
    // Skids O'Toole (01003) has 8 health. Pre-load the investigator card
    // with 7 damage (survived prior harm). A 4-damage attack: Guard Dog
    // soaks 3 (defeated), the remaining 1 lands on the investigator card →
    // 8 >= 8 → the investigator is eliminated (Defeated), not the card
    // discarded to a pile.
    let (mut state, inv_id, _) = soak_state(
        vec![(GUARD_DOG, dog)],
        vec![engaged_attacker(7, inv, loc, 4, 3)],
    );
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .investigator_card
        .accumulated_damage = 7;

    let result = take_turn_action(state, &TurnAction::EndTurn);
    let result = distribute_onto(result, dog);
    // Guard Dog is in the assignment, so its `when` cell opens first.
    let result = fire_retaliate(result.state);
    state = result.state;

    // The investigator card reached its printed health → elimination, with
    // the damage cause. The InvestigatorEliminated event fired.
    assert_eq!(
        state.investigators[&inv_id].status,
        Status::Defeated,
        "investigator-card overflow eliminates (Defeated), not asset-discards"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::InvestigatorEliminated { investigator, cause }
                if *investigator == inv_id && *cause == game_core::state::EliminationCause::Damage
        )),
        "InvestigatorEliminated {{ cause: Damage }} emitted: {:?}",
        result.events
    );
    // The investigator card was NOT discarded to a pile as if it were an
    // asset: no CardDiscarded for the investigator's own code, and the
    // investigator card itself is never the subject of an asset-defeat
    // discard (elimination removes the investigator's cards from the game
    // instead).
    assert!(
        !result.events.iter().any(|e| matches!(
            e,
            Event::CardDiscarded { code, .. } if *code == CardCode::new("01003")
        )),
        "investigator card is eliminated, not discarded as an asset: {:?}",
        result.events
    );
}

// ---------------------------------------------------------------------
// Case 8 — defeat ORDERING when the same attack overflows both an asset
// and the investigator card (#448 cp2b). Load-bearing invariant: the
// investigator-card overflow is resolved (elimination) *before* the asset
// overflow sweep, because RR p.10 Elimination step 1 removes every card
// the investigator controls *from the game* (into `removed_from_game`, NOT
// the discard pile). So a co-overflowing asset is removed-from-game
// silently — it never emits the asset-defeat `CardDiscarded`. This guards
// against a future "fold the investigator card into one uniform post-asset
// defeat sweep" refactor, which would emit that discard before elimination
// removed the card — a behaviour change.
// ---------------------------------------------------------------------

#[test]
fn co_overflowing_asset_is_removed_from_game_not_discarded_when_investigator_eliminated() {
    let dog = CardInstanceId(1);
    let inv = InvestigatorId(1);
    let loc = LocationId(101);
    // Skids O'Toole (01003): 8 health. Pre-load 5 onto the investigator card
    // and 2 onto Guard Dog (printed health 3). A 4-damage attack distributed
    // soak-first: Guard Dog takes 1 (→ 3 >= 3, would defeat) and the
    // remaining 3 land on the investigator card (→ 8 >= 8, eliminated). The
    // investigator is eliminated in step 2, draining cards_in_play to
    // removed_from_game before the asset sweep runs.
    let (mut state, inv_id, _) = soak_state(
        vec![(GUARD_DOG, dog)],
        vec![engaged_attacker(7, inv, loc, 4, 3)],
    );
    {
        let inv_mut = state.investigators.get_mut(&inv_id).unwrap();
        inv_mut.investigator_card.accumulated_damage = 5;
        inv_mut.cards_in_play[0].accumulated_damage = 2;
    }

    let result = take_turn_action(state, &TurnAction::EndTurn);
    let result = distribute_onto(result, dog);
    // Guard Dog is in the assignment, so its `when` cell opens first.
    let result = fire_retaliate(result.state);
    state = result.state;

    // Investigator eliminated (Defeated).
    assert_eq!(
        state.investigators[&inv_id].status,
        Status::Defeated,
        "investigator card overflow eliminates the investigator"
    );
    // Elimination step 1 removed all controlled cards from the game: the
    // Guard Dog is in removed_from_game, NOT the discard pile.
    assert!(
        state.investigators[&inv_id]
            .removed_from_game
            .contains(&CardCode::new(GUARD_DOG)),
        "co-overflowing Guard Dog removed from game by elimination: {:?}",
        state.investigators[&inv_id].removed_from_game
    );
    assert!(
        !state.investigators[&inv_id]
            .discard
            .contains(&CardCode::new(GUARD_DOG)),
        "co-overflowing Guard Dog is NOT in the discard pile"
    );
    // Crucially: NO asset-defeat CardDiscarded for the Guard Dog. The asset
    // sweep ran after elimination had already removed it from cards_in_play.
    assert!(
        !result.events.iter().any(|e| matches!(
            e,
            Event::CardDiscarded { code, from, .. }
                if *code == CardCode::new(GUARD_DOG) && *from == Zone::InPlay
        )),
        "no asset-defeat discard for the co-overflowing Guard Dog: {:?}",
        result.events
    );
}
