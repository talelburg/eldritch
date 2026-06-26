//! End-to-end enemy-attack damage soak + Guard Dog reaction, driven
//! through the public [`apply`] API with the real card corpus installed.
//!
//! These cover the coverage deferred from `game-core`'s unit tests, which
//! can't install the real `cards::REGISTRY` (the engine crate can't depend
//! on `cards`):
//!
//! 1. Happy path: an enemy attack soaks onto Guard Dog; the
//!    `AfterEnemyAttackDamagedAsset` window opens; firing Guard Dog's
//!    reaction deals 1 damage to the attacker.
//! 2. Asset defeat on overflow: damage reaching Guard Dog's printed
//!    health (3) defeats it (`CardDiscarded`, removed from
//!    `cards_in_play`), and a Guard Dog killed by the *same* attack gets
//!    no reaction window (survivor filter).
//! 3. Instance self-binding: with a second damage-soaking asset in play
//!    that has no reaction, only Guard Dog's reaction is offered.
//! 4. Two-attacker suspend/resume: an investigator engaged by two enemies
//!    suspends on the first attack's soak window and resumes the second
//!    attacker after the reaction resolves.
//!
//! Guard Dog 01021: "[reaction] When an enemy attack deals damage to Guard
//! Dog: Deal 1 damage to the attacking enemy." Health 3, sanity 1, Ally.

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

#[ctor::ctor]
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

    // The attack-loop suspended on Guard Dog's soak reaction window.
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "soak window must suspend the attack loop: {:?}",
        result.outcome
    );
    // Damage soaked onto Guard Dog, investigator took none.
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        2,
        "Guard Dog soaked all 2 damage"
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "investigator took no damage"
    );
    // No damage on the attacker yet (reaction not fired).
    assert_eq!(state.enemies[&enemy_id].damage, 0);

    // Fire Guard Dog's reaction (the single pending trigger).
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    state = result.state;

    // The attacker took exactly 1 damage.
    assert_eq!(
        state.enemies[&enemy_id].damage, 1,
        "Guard Dog dealt 1 damage to the attacker"
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
// Case 2a — overflow defeats Guard Dog by the SAME attack: no reaction
// ---------------------------------------------------------------------

#[test]
fn attack_reaching_printed_health_defeats_guard_dog_with_no_reaction_window() {
    let dog = CardInstanceId(1);
    let enemy_id = EnemyId(7);
    let inv = InvestigatorId(1);
    let loc = LocationId(101);
    // Attack deals 3 damage = Guard Dog's printed health → defeated by the
    // same attack. Survivor filter: no reaction window, no retaliation.
    let (mut state, inv_id, _) = soak_state(
        vec![(GUARD_DOG, dog)],
        vec![engaged_attacker(7, inv, loc, 3, 3)],
    );

    let result = take_turn_action(state, &TurnAction::EndTurn);
    let result = distribute_onto(result, dog);
    state = result.state;

    // Guard Dog left play (discarded), so no soak window suspended the loop;
    // the enemy phase cascaded onward, all the way through Upkeep into the
    // round-ending Mythos step-1.4 encounter-draw prompt (#348). Reaching the
    // Mythos draw prompt (rather than parking on a soak window) is the
    // definitive "no soak window opened" proof.
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. })
            && state.phase == Phase::Mythos
            && state.current_encounter_drawer().is_some(),
        "the loop cascaded onward to the Mythos draw prompt (not a soak park): {:?}",
        result.outcome
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
    // No reaction fired: attacker undamaged, no soak window ever opened.
    assert_eq!(
        state.enemies.get(&enemy_id).map(|e| e.damage),
        Some(0),
        "defeated-this-attack Guard Dog does not retaliate"
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
    // Defeated by this attack → no retaliation (survivor filter). The
    // attacker (max_health 3, attack 2) is never defeated, so index directly.
    assert_eq!(
        state.enemies[&enemy_id].damage, 0,
        "attacker took no retaliation"
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
    // an `EnemyAttackDamagedSelf` reaction. So exactly one soak window opens
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

    // The surviving Guard Dog's reaction window suspended the loop.
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "the surviving Guard Dog's reaction window suspends the loop: {:?}",
        result.outcome
    );
    // Guard Dog soaked the damage; the Vest sits untouched.
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        2,
        "Guard Dog soaked the attack and survived"
    );
    assert_eq!(
        state.investigators[&inv_id]
            .cards_in_play
            .iter()
            .find(|c| c.instance_id == vest)
            .map(|c| c.accumulated_damage),
        Some(0),
        "Bulletproof Vest took no damage (attack fully soaked by Guard Dog)"
    );
    // Exactly one pending soak window, keyed to Guard Dog's instance — not
    // the Vest's (a controlled soaker with no `EnemyAttackDamagedSelf`).
    let soak_windows: Vec<_> = state
        .open_windows()
        .iter()
        .filter_map(|w| match w.window_timing_event() {
            Some(game_core::engine::TimingEvent::EnemyAttackDamagedSelf { asset, .. }) => {
                Some(*asset)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        soak_windows,
        vec![dog],
        "exactly the Guard Dog instance's soak window is open, not the Vest's"
    );

    // Firing the single offered trigger retaliates (it's Guard Dog's, not
    // the Vest's — the Vest contributes no trigger at all).
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    state = result.state;
    assert_eq!(
        state.enemies[&enemy_id].damage, 1,
        "Guard Dog's reaction (not the Vest's) dealt 1 damage"
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
        1,
        "first attacker's 1 damage soaked"
    );
    // First attacker resolved + exhausted; second not yet.
    assert!(
        state.enemies[&first].exhausted,
        "first attacker exhausted before suspend"
    );
    assert!(
        !state.enemies[&second].exhausted,
        "second attacker not yet resolved"
    );
    // The remaining attacker is parked on the AttackLoop frame.
    assert_eq!(
        state.continuations.iter().rev().find_map(|c| match c {
            Continuation::AttackLoop {
                remaining_attackers,
                ..
            } => Some(remaining_attackers.clone()),
            _ => None,
        }),
        Some(vec![second]),
        "second attacker parked for resume"
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
    // The second attacker resolved on resume: its damage soaked, it exhausted.
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        2,
        "second attacker's 1 damage also soaked onto Guard Dog"
    );
    assert!(
        state.enemies[&second].exhausted,
        "second attacker resolved and exhausted after resume"
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
// Guard Dog's soak window was never queued. After #293 the AoO runs through
// `drive_attack_loop` (which opens both the BeforeEnemyAttack cancel window
// and the AfterEnemyAttackDamagedAsset soak window), so the full suspend/
// resume cycle now applies to AoO attacks.
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
    // AoO damage fully soaked onto Guard Dog; investigator took none.
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        2,
        "AoO damage soaked onto Guard Dog"
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

    // Step 2: fire Guard Dog's soak reaction (the single pending trigger).
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    state = result.state;

    // Guard Dog dealt 1 retaliate damage to the attacker.
    assert_eq!(
        state.enemies[&enemy_id].damage, 1,
        "Guard Dog's reaction dealt 1 damage to the AoO attacker"
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
// as an asset, but the consequence is elimination (Status::Killed) rather
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
    // 8 >= 8 → the investigator is eliminated (Killed), not the card
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
    state = result.state;

    // The investigator card reached its printed health → elimination, with
    // the damage cause (Killed). The InvestigatorDefeated event fired.
    assert_eq!(
        state.investigators[&inv_id].status,
        Status::Killed,
        "investigator-card overflow eliminates (Killed), not asset-discards"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::InvestigatorDefeated { investigator, cause }
                if *investigator == inv_id && *cause == game_core::state::DefeatCause::Damage
        )),
        "InvestigatorDefeated {{ cause: Damage }} emitted: {:?}",
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
    state = result.state;

    // Investigator eliminated (Killed).
    assert_eq!(
        state.investigators[&inv_id].status,
        Status::Killed,
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
