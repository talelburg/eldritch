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

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, Enemy, EnemyId, InvestigatorId, LocationId, Phase, Zone,
};
use game_core::test_support::{test_enemy, test_investigator, test_location};
use game_core::{Action, InputResponse, PlayerAction};

/// Guard Dog (01021): Guardian Ally, health 3 / sanity 1, with the
/// damage-retaliate reaction.
const GUARD_DOG: &str = "01021";

/// Bulletproof Vest (01094): Body-slot asset, health 4 / no sanity, no
/// reaction. A damage soaker that legally co-exists with Guard Dog (Body
/// vs Ally slot) and never reacts — used for the self-binding case.
const BULLETPROOF_VEST: &str = "01094";

fn install_real_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
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
    install_real_registry();

    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(101);

    let mut inv = test_investigator(1);
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

    let result = apply(state, Action::Player(PlayerAction::EndTurn));
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
        state.investigators[&inv_id].damage, 0,
        "investigator took no damage"
    );
    // The soak window opened.
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, Event::WindowOpened { kind }
                if matches!(kind, game_core::state::WindowKind::AfterEnemyAttackDamagedAsset { .. }))),
        "AfterEnemyAttackDamagedAsset window opened: {:?}",
        result.events
    );
    // No damage on the attacker yet (reaction not fired).
    assert_eq!(state.enemies[&enemy_id].damage, 0);

    // Fire Guard Dog's reaction (the single pending trigger).
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickIndex(0),
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

    let result = apply(state, Action::Player(PlayerAction::EndTurn));
    state = result.state;

    // Guard Dog left play (discarded), so no soak window suspended the loop;
    // the enemy phase cascaded onward.
    assert!(
        !matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "no soak window should park the loop when Guard Dog is defeated: {:?}",
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
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, Event::WindowOpened { kind }
            if matches!(kind, game_core::state::WindowKind::AfterEnemyAttackDamagedAsset { .. }))),
        "no soak window opens for a same-attack-defeated soaker"
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

    let result = apply(state, Action::Player(PlayerAction::EndTurn));
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

    let result = apply(state, Action::Player(PlayerAction::EndTurn));
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
        .open_windows
        .iter()
        .filter_map(|w| match w.kind {
            game_core::state::WindowKind::AfterEnemyAttackDamagedAsset { asset, .. } => Some(asset),
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
            response: InputResponse::PickIndex(0),
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

    // First attack soaks → suspend on the soak window.
    let result = apply(state, Action::Player(PlayerAction::EndTurn));
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
    // The remaining attacker is parked.
    assert_eq!(
        state
            .pending_enemy_attack
            .as_ref()
            .map(|p| p.remaining_attackers.clone()),
        Some(vec![second]),
        "second attacker parked for resume"
    );

    // Resolve the first reaction window → the first attacker takes the
    // retaliation, then the loop resumes the second attacker. The second
    // attack ALSO soaks onto the (surviving) Guard Dog, opening a second
    // soak window and re-suspending — a clean demonstration that the
    // resumed loop suspends again on a later attacker.
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickIndex(0),
        }),
    );
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
        state.pending_enemy_attack.is_some(),
        "loop is parked again after the second attack"
    );

    // Resolve the second reaction window → second attacker takes the
    // retaliation, the loop drains with no attackers left, the enemy phase
    // cascades onward, and nothing remains parked.
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickIndex(0),
        }),
    );
    state = result.state;

    assert_eq!(
        state.enemies[&second].damage, 1,
        "second attacker took Guard Dog's retaliation on the second window"
    );
    assert!(
        state.pending_enemy_attack.is_none(),
        "no parked attack after both attackers fully resolve"
    );
    assert!(
        state.open_windows.is_empty(),
        "no soak windows left open once both attacks resolve"
    );
}

// ---------------------------------------------------------------------
// Case 5 — attack of opportunity soaks onto Guard Dog but strands no
// reaction window (regression guard for the AoO seam; C5b #237)
// ---------------------------------------------------------------------

#[test]
fn move_attack_of_opportunity_soaks_onto_guard_dog_without_stranding_a_window() {
    // An investigator controlling Guard Dog, engaged by a ready enemy,
    // takes a Move action. The Move fires an attack of opportunity BEFORE
    // resolving, and the AoO's damage soaks onto Guard Dog. The bug this
    // guards: `enemy_attack` used to queue an `AfterEnemyAttackDamagedAsset`
    // reaction window unconditionally, so the AoO would leave an undriven
    // window on `open_windows` after `fire_attacks_of_opportunity` returns.
    // AoO now drops the soak-survivor list (Guard Dog does not retaliate
    // against AoO yet — deferred fast-follow), so the move resolves cleanly
    // with no stranded window.
    let dog = CardInstanceId(1);
    let enemy_id = EnemyId(7);
    let inv = InvestigatorId(1);
    let from = LocationId(101);
    let dest = LocationId(102);

    install_real_registry();
    let inv_id = InvestigatorId(1);

    let mut study = test_location(101, "Study");
    study.connections = vec![dest];
    let mut hallway = test_location(102, "Hallway");
    hallway.connections = vec![from];

    let mut investigator = test_investigator(1);
    investigator.current_location = Some(from);
    investigator.cards_in_play = vec![CardInPlay::enter_play(CardCode::new(GUARD_DOG), dog)];

    // Engaged ready attacker dealing 2 damage; Guard Dog (health 3) soaks
    // all of it.
    let attacker = engaged_attacker(7, inv, from, 2, 3);

    let state = game_core::test_support::GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(study)
        .with_location(hallway)
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_enemy(attacker)
        .build();

    let result = apply(
        state,
        Action::Player(PlayerAction::Move {
            investigator: inv_id,
            destination: dest,
        }),
    );
    let state = result.state;

    // The AoO soaked its damage onto Guard Dog.
    assert_eq!(
        guard_dog_card(&state, inv_id, dog).accumulated_damage,
        2,
        "AoO damage soaked onto Guard Dog"
    );
    assert_eq!(
        state.investigators[&inv_id].damage, 0,
        "investigator took no AoO damage (fully soaked)"
    );

    // The bug guard: no stranded soak window, and the outcome is NOT a
    // dangling AwaitingInput on a soak window — AoO does not open one.
    assert!(
        state.open_windows.is_empty(),
        "no reaction window stranded by the AoO: {:?}",
        state.open_windows
    );
    assert!(
        !matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "AoO must not suspend on a soak window: {:?}",
        result.outcome
    );
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, Event::WindowOpened { kind }
            if matches!(kind, game_core::state::WindowKind::AfterEnemyAttackDamagedAsset { .. }))),
        "no soak window opened for the AoO: {:?}",
        result.events
    );

    // The move resolved: the engaged enemy is NOT exhausted (AoO does not
    // exhaust the attacker), the investigator and the engaged enemy both
    // moved to the destination, and no retaliation hit the attacker.
    assert!(
        !state.enemies[&enemy_id].exhausted,
        "an attack of opportunity does not exhaust the attacker"
    );
    assert_eq!(
        state.investigators[&inv_id].current_location,
        Some(dest),
        "investigator moved to the destination"
    );
    assert_eq!(
        state.enemies[&enemy_id].current_location,
        Some(dest),
        "engaged enemy moved with the investigator"
    );
    assert_eq!(
        state.enemies[&enemy_id].damage, 0,
        "Guard Dog does not retaliate against an attack of opportunity (yet)"
    );
}
