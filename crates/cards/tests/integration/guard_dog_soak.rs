//! End-to-end enemy-attack damage soak + Guard Dog reaction, driven
//! through the public [`apply`] API against the real card corpus.
//!
//! These cover the coverage deferred from `game-core`'s unit tests, which
//! can't reach the real `cards::REGISTRY` (the engine crate can't depend
//! on `cards`):
//!
//! 1. Happy path: an enemy attack assigns damage to Guard Dog; the
//!    `DamageDealt` window opens **before anything is placed**; firing
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
//! **The retaliate resolves in the `when` cell of `DamageDealt`**,
//! which is why every case here reads the dog's `accumulated_damage` as 0 at
//! the window and non-zero only after the reaction has resolved. That order is
//! the Rules Reference's — `glossary/Nested_Sequences.md` runs its worked
//! example on this card, and `glossary/Dealing_Damage_Horror.md` puts the
//! window between assigning and placing. See
//! `docs/adr/0009-damage-is-assigned-then-placed.md`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, DiscardPile, EliminationCause, EnemyId, GameState,
    GameStateBuilder, InvestigatorId, LocationId, Status, Zone,
};
use game_core::test_support::{self, seat, BoardLookup as _, StagedEnemy, TestSession};

/// Guard Dog (01021): Guardian Ally, health 3 / sanity 1, with the
/// damage-retaliate reaction.
const GUARD_DOG: &str = "01021";

/// Bulletproof Vest (01094): Body-slot asset, health 4 / no sanity, no
/// reaction. A damage soaker that legally co-exists with Guard Dog (Body
/// vs Ally slot) and never reacts — used for the self-binding case.
const BULLETPROOF_VEST: &str = "01094";

/// A ready enemy dealing `damage` / 0 horror, with `max_health`. Engage it
/// with `with_enemy_engaged`, which places it at the investigator's location.
fn ready_attacker(id: u32, damage: u8, max_health: u8) -> StagedEnemy {
    test_support::enemy(id)
        .attack_damage(damage)
        .health(max_health)
}

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);

/// Build an Investigation-phase state with one active investigator at a
/// location carrying `damage` damage, `assets` in play, and `enemies` engaged.
/// Driving `EndTurn` from here advances Investigation → Enemy and runs the
/// per-investigator attack loop (the `BeforeInvestigatorAttacked` player
/// window auto-skips — Guard Dog has no zero-action ability — so the loop runs
/// inline and suspends on the soak reaction window).
fn soak_state(assets: &[&str], damage: u8, enemies: Vec<StagedEnemy>) -> GameState {
    let investigator = seat(1)
        .at(HERE)
        .harm(damage, 0)
        .in_play(assets.iter().copied())
        // A deck with cards left in it. The Upkeep draw these cases cascade
        // through otherwise draws from an empty deck, whose penalty is 1
        // horror — which offers Guard Dog's 1 printed sanity the soak,
        // in the middle of a test measuring what the *attack* did to it.
        .deck(["01087"; 5]);
    enemies
        .into_iter()
        .fold(
            GameStateBuilder::new()
                .with_registries(TEST_REGISTRIES)
                .with_location(test_support::location(HERE))
                .with_seat(investigator)
                .open_turn(INV),
            |builder, enemy| builder.with_enemy_engaged(enemy, INV),
        )
        .build()
}

/// Put `damage` on the seated Guard Dog — the damage a prior attack it
/// survived left on it. A seat enters its cards undamaged, so this is a
/// built-state edit.
fn preload_guard_dog(state: &mut GameState, damage: u8) {
    let dog = state.in_play(INV, GUARD_DOG);
    state
        .investigators
        .get_mut(&INV)
        .expect("seated")
        .cards_in_play
        .iter_mut()
        .find(|c| c.instance_id == dog)
        .expect("Guard Dog in play")
        .accumulated_damage = damage;
}

/// Find the investigator's Guard Dog instance.
fn guard_dog_card(state: &GameState, inv: InvestigatorId, inst: CardInstanceId) -> &CardInPlay {
    state.investigators[&inv]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == inst)
        .expect("Guard Dog still in play")
}

/// Resolve a soak distribution by assigning every point to the soaker `inst`
/// while it has capacity, then to the investigator once it is full — soak
/// first. Returns the session at the first
/// prompt that is no longer a distribution prompt: the distribution must be
/// answered, a reaction window may be passed.
fn distribute_onto(mut session: TestSession, inst: CardInstanceId) -> TestSession {
    while !session.prompt().skippable {
        let soaker = OptionTarget::CardInstance(inst);
        let offered = session
            .prompt()
            .options
            .iter()
            .any(|o| o.target.as_ref() == Some(&soaker));
        session = if offered {
            session.pick(soaker)
        } else {
            // The sole defender: every fixture here seats investigator 1.
            let me = session.state().investigators[&INV].card_anchor();
            session.pick(me)
        };
    }
    session
}

/// Fire Guard Dog's retaliate from its open reaction window. The window is
/// `DamageDealt`'s `when` cell, so this is also what lets the deal
/// proceed to its placement.
fn fire_retaliate(session: TestSession, dog: CardInstanceId) -> TestSession {
    session.pick(OptionTarget::CardInstance(dog))
}

// ---------------------------------------------------------------------
// Case 1 — happy path
// ---------------------------------------------------------------------

#[test]
fn enemy_attack_soaks_onto_guard_dog_then_retaliate_damages_attacker() {
    let enemy_id = EnemyId(7);
    let inv_id = INV;
    // Attack deals 2 damage; Guard Dog (health 3) soaks all of it, the
    // investigator takes none.
    let state = soak_state(&[GUARD_DOG], 0, vec![ready_attacker(7, 2, 3)]);
    let dog = state.in_play(INV, GUARD_DOG);

    // Distribute the attack: assign both points onto Guard Dog.
    let session = distribute_onto(
        TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::EndTurn),
        dog,
    );
    let state = session.state();

    // The attack-loop suspended on Guard Dog's `when`-cell window.
    assert!(
        session.prompt().skippable,
        "the assignment's when-cell window must suspend the attack loop: {:?}",
        session.prompt()
    );
    // Nothing is placed yet: the damage is *assigned* to Guard Dog — tokens
    // "next to" it, in the Rules Reference's words — and the window between the
    // two steps is what is open.
    assert_eq!(
        guard_dog_card(state, inv_id, dog).accumulated_damage,
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
    let session = fire_retaliate(session, dog);
    let state = session.state();

    // The attacker took exactly 1 damage.
    assert_eq!(
        state.enemies[&enemy_id].damage, 1,
        "Guard Dog dealt 1 damage to the attacker"
    );
    // …and only then did the assignment land on Guard Dog.
    assert_eq!(
        guard_dog_card(state, inv_id, dog).accumulated_damage,
        2,
        "Guard Dog takes its assigned 2 damage once the when cell has run"
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "investigator still took none"
    );
    assert!(
        session.events().iter().any(|e| matches!(
            e,
            Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == enemy_id
        )),
        "EnemyDamaged {{ amount: 1 }} emitted: {:?}",
        session.events()
    );
}

// ---------------------------------------------------------------------
// Case 2a — overflow defeats Guard Dog by the SAME attack: it retaliates
// anyway, because the assignment is announced while it is still in play.
// ---------------------------------------------------------------------

/// Regression for #727: a Guard Dog killed by the attack that damaged it still
/// bites back, because the assignment is announced before the placement — its
/// ruling, `data/arkhamdb-faq/core/01021.md`: "You can use Guard Dog's ability
/// when you assign lethal damage/horror to it."
#[test]
fn guard_dog_retaliates_on_a_lethal_assignment_then_is_defeated() {
    let enemy_id = EnemyId(7);
    let inv_id = INV;
    // Attack deals 3 damage = Guard Dog's printed health → the assignment is
    // lethal, and the dog is defeated once it is placed.
    let state = soak_state(&[GUARD_DOG], 0, vec![ready_attacker(7, 3, 3)]);
    let dog = state.in_play(INV, GUARD_DOG);

    let session = distribute_onto(
        TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::EndTurn),
        dog,
    );
    let state = session.state();

    // The lethal assignment opened Guard Dog's `when` cell, with the dog still
    // in play and undamaged.
    assert!(
        session.prompt().skippable,
        "a lethal assignment still opens the when cell: {:?}",
        session.prompt()
    );
    assert_eq!(
        guard_dog_card(state, inv_id, dog).accumulated_damage,
        0,
        "nothing placed yet — the dog is alive and about to bite"
    );

    let session = fire_retaliate(session, dog);
    let state = session.state();

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
        session.events().iter().any(|e| matches!(
            e,
            Event::CardDiscarded { code, from, to }
                if *code == CardCode::new(GUARD_DOG)
                    && *from == Zone::InPlay
                    && *to == DiscardPile::Investigator(InvestigatorId(1))
        )),
        "Guard Dog discard emitted: {:?}",
        session.events()
    );
    // The retaliate preceded the damage it reacted to, in the log as in the
    // rules: `EnemyDamaged` before the dog's `CardDiscarded`.
    let retaliate_at = session
        .events()
        .iter()
        .position(|e| matches!(e, Event::EnemyDamaged { enemy, .. } if *enemy == enemy_id));
    let discard_at = session.events().iter().position(|e| {
        matches!(e, Event::CardDiscarded { code, from, .. }
            if *code == CardCode::new(GUARD_DOG) && *from == Zone::InPlay)
    });
    assert!(
        retaliate_at < discard_at,
        "the when-cell retaliate precedes the placement that defeats the dog: {:?}",
        session.events()
    );
}

// ---------------------------------------------------------------------
// Case 2c — the attacker is defeated by the retaliate *during its own
// enemy-phase attack*, so there is nothing left to exhaust.
//
// The exhaust runs on the parked `AttackLoop` frame, after the whole sequence
// (`Appendix_II_Timing_and_Gameplay.md` step 3.3: "Upon completion of dealing
// the attack (and all abilities triggered by the attack), exhaust the enemy.").
// The retaliate resolves in the `when` cell, before the damage is placed, so an
// attacker can die mid-sequence even on the attack that would kill the dog.
// ---------------------------------------------------------------------

#[test]
fn an_attacker_defeated_by_the_retaliate_mid_attack_has_nothing_to_exhaust() {
    let enemy_id = EnemyId(7);
    let inv_id = INV;
    // The attacker has 1 health, so Guard Dog's 1 retaliate damage defeats it
    // in the `when` cell of its own attack's damage. Enemy phase, so it *would*
    // have exhausted had it survived (unlike the AoO/Retaliate cases).
    let state = soak_state(&[GUARD_DOG], 0, vec![ready_attacker(7, 2, 1)]);
    let dog = state.in_play(INV, GUARD_DOG);

    let session = fire_retaliate(
        distribute_onto(
            TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::EndTurn),
            dog,
        ),
        dog,
    );
    let state = session.state();

    assert!(
        !state.enemies.contains_key(&enemy_id),
        "the retaliate defeated the attacker during its own attack: {:?}",
        session.events()
    );
    assert!(
        !session
            .events()
            .iter()
            .any(|e| matches!(e, Event::EnemyExhausted { enemy } if *enemy == enemy_id)),
        "a defeated attacker has nothing to exhaust: {:?}",
        session.events()
    );
    // The rest of the sequence still ran: the damage it had already assigned is
    // placed even though the attacker is gone, because the assignment was
    // settled before the retaliate (RR step 1 precedes step 2).
    assert_eq!(
        guard_dog_card(state, inv_id, dog).accumulated_damage,
        2,
        "the dead attacker's assigned damage still lands on Guard Dog"
    );
    // And the loop drained rather than stalling on a missing attacker.
    session.assert_at_encounter_draw();
}

// ---------------------------------------------------------------------
// Case 2b — overflow defeats Guard Dog AFTER it survives a prior attack
// (accumulated damage builds across attacks; the lethal one removes it)
// ---------------------------------------------------------------------

#[test]
fn guard_dog_defeated_on_overflow_is_discarded_from_play() {
    // Pre-load Guard Dog with 2 accumulated damage (a prior attack it
    // survived); a fresh 2-damage attack pushes it to 4 >= 3 → defeated.
    let enemy_id = EnemyId(7);
    let inv_id = INV;
    let mut state = soak_state(&[GUARD_DOG], 0, vec![ready_attacker(7, 2, 3)]);
    let dog = state.in_play(INV, GUARD_DOG);
    // Survived a prior attack: 2 already accumulated (under health 3).
    preload_guard_dog(&mut state, 2);

    // The assignment's `when` cell opens with the dog still in play; firing its
    // retaliate lets the deal proceed to the placement that defeats it.
    let session = fire_retaliate(
        distribute_onto(
            TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::EndTurn),
            dog,
        ),
        dog,
    );
    let state = session.state();

    assert!(
        !state.investigators[&inv_id]
            .cards_in_play
            .iter()
            .any(|c| c.instance_id == dog),
        "Guard Dog at accumulated 4 >= health 3 is discarded"
    );
    assert!(
        session.events().iter().any(|e| matches!(
            e,
            Event::CardDiscarded { code, from, to }
                if *code == CardCode::new(GUARD_DOG)
                    && *from == Zone::InPlay
                    && *to == DiscardPile::Investigator(InvestigatorId(1))
        )),
        "Guard Dog discard emitted: {:?}",
        session.events()
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
    // by CardInstanceId (seated in this order, so minted in it):
    //   - Guard Dog (Ally slot, health 3) — soaks first.
    //   - Bulletproof Vest (Body slot, health 4).
    // Different slots, so both are legally in play. A 2-damage attack soaks
    // entirely onto Guard Dog (2 < 3 → survives), never reaching the Vest.
    //
    // The self-binding point: the Vest is a controlled soaker too, but the
    // soak window is scoped to the *damaged* asset, and only Guard Dog has
    // an `EnemyAttackDamagedSelf` reaction. So exactly one window opens
    // — keyed to Guard Dog's instance — even though another soaker sits in
    // play. (The demonstration is "the keyed instance is the one whose
    // reaction fires," not two live reacting allies — which two Ally slots
    // forbid.)
    let enemy_id = EnemyId(7);
    let inv_id = INV;
    let state = soak_state(
        &[GUARD_DOG, BULLETPROOF_VEST],
        0,
        vec![ready_attacker(7, 2, 3)],
    );
    let dog = state.in_play(INV, GUARD_DOG);
    let vest = state.in_play(INV, BULLETPROOF_VEST);

    let session = distribute_onto(
        TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::EndTurn),
        dog,
    );
    let state = session.state();

    // Guard Dog's reaction window suspended the loop.
    assert!(
        session.prompt().skippable,
        "Guard Dog's reaction window suspends the loop: {:?}",
        session.prompt()
    );
    // The whole assignment went to Guard Dog; nothing is placed yet, and the
    // Vest is in neither the assignment nor the window.
    assert_eq!(
        guard_dog_card(state, inv_id, dog).accumulated_damage,
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
    // Exactly one reaction is offered, Guard Dog's — not the Vest's (a
    // controlled soaker with no `EnemyAttackDamagedSelf` ability). That the
    // assignment names Guard Dog's instance alone is what its placement shows
    // below: the dog takes all 2, the Vest none.
    assert_eq!(
        (
            session.offered_at(&OptionTarget::CardInstance(dog)),
            session.offered_at(&OptionTarget::CardInstance(vest)),
            session.prompt().options.len(),
        ),
        (1, 0, 1),
        "the window offers Guard Dog's reaction alone: {:?}",
        session.prompt()
    );

    // Firing the single offered trigger retaliates (it's Guard Dog's, not
    // the Vest's — the Vest contributes no trigger at all).
    let session = fire_retaliate(session, dog);
    let state = session.state();
    assert_eq!(
        state.enemies[&enemy_id].damage, 1,
        "Guard Dog's reaction (not the Vest's) dealt 1 damage"
    );
    assert_eq!(
        guard_dog_card(state, inv_id, dog).accumulated_damage,
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
    let first = EnemyId(7);
    let second = EnemyId(8);
    let inv_id = INV;
    // Two engaged attackers, each dealing 1 damage; Guard Dog (health 3)
    // soaks both. The first attack opens the soak window and suspends; after
    // resolving the reaction, the loop resumes and the second attacker
    // attacks too. Both end exhausted.
    let state = soak_state(
        &[GUARD_DOG],
        0,
        vec![ready_attacker(7, 1, 3), ready_attacker(8, 1, 3)],
    );
    let dog = state.in_play(INV, GUARD_DOG);

    // Two engaged attackers → the enemy phase first asks the player which
    // attacks next. Pick the first attacker (EnemyId 7).
    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::EndTurn);

    // The chosen first attacker attacks: its 1 damage prompts the soak
    // distribution — assign it to Guard Dog → suspend on the soak window.
    let session = distribute_onto(session.pick(OptionTarget::Enemy(first)), dog);
    let state = session.state();
    assert!(
        session.prompt().skippable,
        "first attack's soak window suspends the loop: {:?}",
        session.prompt()
    );
    assert_eq!(
        guard_dog_card(state, inv_id, dog).accumulated_damage,
        0,
        "first attacker's 1 damage is assigned, not yet placed"
    );
    // Neither attacker has exhausted yet: the exhaust follows the
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
    // The untouched second attacker is still to come: it attacks once the
    // first attack's sequence completes, below.
    assert_eq!(
        state.enemies[&second].damage, 0,
        "second attacker untouched"
    );

    // Resolve the first reaction window → the first attacker takes the
    // retaliation, then the loop resumes the second attacker, whose 1 damage
    // prompts its own soak distribution (assign to Guard Dog). The second
    // attack ALSO soaks onto the (surviving) Guard Dog, opening a second
    // soak window and re-suspending — a clean demonstration that the
    // resumed loop suspends again on a later attacker.
    let session = distribute_onto(fire_retaliate(session, dog), dog);
    let state = session.state();

    assert_eq!(
        state.enemies[&first].damage, 1,
        "first attacker took Guard Dog's retaliation"
    );
    // The first attack's damage has now landed; the second attack's is assigned
    // but not yet placed, its own `when` cell being the window we are parked on.
    assert_eq!(
        guard_dog_card(state, inv_id, dog).accumulated_damage,
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
    // ...and the loop re-suspended on the second attacker's soak window,
    // Guard Dog's reaction offered again.
    assert!(
        session.prompt().skippable,
        "second attacker's soak window re-suspends the resumed loop: {:?}",
        session.prompt()
    );
    assert_eq!(
        session.offered_at(&OptionTarget::CardInstance(dog)),
        1,
        "Guard Dog's reaction is offered on the second attack's window"
    );

    // Resolve the second reaction window → second attacker takes the
    // retaliation, the loop drains with no attackers left, the enemy phase
    // cascades onward, and nothing remains parked.
    let session = fire_retaliate(session, dog);
    let state = session.state();

    assert_eq!(
        state.enemies[&second].damage, 1,
        "second attacker took Guard Dog's retaliation on the second window"
    );
    assert_eq!(
        guard_dog_card(state, inv_id, dog).accumulated_damage,
        2,
        "both attacks' damage has landed once both sequences complete"
    );
    // No attack is parked and no soak window left open once both attacks
    // resolve: play goes on past the Enemy phase.
    session.assert_at_encounter_draw();
}

// ---------------------------------------------------------------------
// Case 5 — attack of opportunity soaks onto Guard Dog; the soak window
// opens, Guard Dog retaliates, and the move completes after resume.
//
// Verified card text (ArkhamDB https://arkhamdb.com/card/01021, 2026-06-21):
//   Guard Dog (01021): "[reaction] When an enemy attack deals damage to
//   Guard Dog: Deal 1 damage to the attacking enemy." Health 3, Sanity 1.
//   FAQ: "You can use Guard Dog's ability when you assign lethal damage/
//   horror to it." The trigger is "when an enemy attack deals damage", with
//   no carve-out for an attack of opportunity.
//
// The AoO runs through `drive_attack_loop` (which opens both the
// `EnemyAttacks` cancel window and the `DamageDealt` window inside the
// attack's damage), so the full suspend/resume cycle applies to AoO attacks.
// ---------------------------------------------------------------------

#[test]
fn move_attack_of_opportunity_guard_dog_retaliates_and_move_completes() {
    // An investigator controlling Guard Dog, engaged by a ready enemy,
    // takes a Move action. The Move fires an attack of opportunity (through
    // the ActionResolution frame + drive_aoo path). Guard Dog has no
    // cancel reaction so the before-attack window auto-skips; the AoO
    // damage soaks onto Guard Dog; the soak window opens and suspends;
    // the player fires Guard Dog's reaction; the attacker takes 1 damage;
    // the move then completes as the ActionResolution frame resumes.
    let enemy_id = EnemyId(7);
    let inv_id = INV;
    let from = HERE;
    let dest = LocationId(102);

    let study = test_support::location(HERE).connections(vec![dest]);
    let hallway = test_support::location(dest).connections(vec![from]);

    // Engaged ready attacker dealing 2 damage; Guard Dog (health 3) soaks
    // all of it and survives (2 < 3).
    let attacker = ready_attacker(7, 2, 3);

    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(study)
        .with_location(hallway)
        .with_seat(seat(1).at(from).in_play([GUARD_DOG]))
        .open_turn(inv_id)
        .with_enemy_engaged(attacker, inv_id)
        .build();
    let dog = state.in_play(INV, GUARD_DOG);

    // Step 1: take the Move — AoO runs; Guard Dog has no cancel reaction
    // so the before-attack window is skipped; damage soaks onto Guard Dog;
    // the soak window opens and suspends.
    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::Move {
        investigator: inv_id,
        destination: dest,
    });
    // The AoO prompts for the soak distribution: assign both points
    // onto Guard Dog to reproduce the soak.
    let session = distribute_onto(session, dog);
    let state = session.state();

    // The AoO's soak window suspended the loop (the ActionResolution
    // frame is parked beneath the AttackLoop beneath the Resolution window).
    assert!(
        session.prompt().skippable,
        "AoO soak window must suspend the loop: {:?}",
        session.prompt()
    );
    // The AoO damage is *assigned* to Guard Dog and not yet placed — the open
    // window is the one between the two steps.
    assert_eq!(
        guard_dog_card(state, inv_id, dog).accumulated_damage,
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
    let session = fire_retaliate(session, dog);
    let state = session.state();

    // Guard Dog dealt 1 retaliate damage to the attacker.
    assert_eq!(
        state.enemies[&enemy_id].damage, 1,
        "Guard Dog's reaction dealt 1 damage to the AoO attacker"
    );
    // …and the assigned damage then landed on it.
    assert_eq!(
        guard_dog_card(state, inv_id, dog).accumulated_damage,
        2,
        "AoO damage placed on Guard Dog once the when cell has run"
    );
    assert!(
        session.events().iter().any(|e| matches!(
            e,
            Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == enemy_id
        )),
        "EnemyDamaged {{ amount: 1 }} emitted: {:?}",
        session.events()
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
        session.events().iter().any(|e| matches!(
            e,
            Event::InvestigatorMoved { investigator, from: f, to } if
                *investigator == inv_id && *f == from && *to == dest
        )),
        "InvestigatorMoved event emitted after window closed: {:?}",
        session.events()
    );

    // No reaction windows remain after the full cycle: the turn goes on.
    session.assert_at_turn_menu(inv_id);
}

// ---------------------------------------------------------------------
// Case 6 — the investigator card is the mandatory-remainder soaker
// An asset soaks to capacity (and is defeated); the
// remainder lands on the *investigator card* via its `accumulated_damage`
// (not a bespoke field), exactly as the RR's "All damage/horror that cannot
// be assigned to an asset must be assigned to the investigator" clause
// requires. This is the soaker-side half of the soak/defeat unification.
// ---------------------------------------------------------------------

#[test]
fn an_asset_soaks_first_then_the_investigator_card_takes_the_remainder() {
    let inv_id = INV;
    // Attack deals 5 damage. Guard Dog (printed health 3) soaks 3 and is
    // defeated by reaching its printed health; the remaining 2 must be
    // assigned to the investigator — landing on the investigator card's
    // `accumulated_damage`. The investigator has 8 health, so 2 < 8 →
    // the investigator survives.
    let state = soak_state(&[GUARD_DOG], 0, vec![ready_attacker(7, 5, 3)]);
    let dog = state.in_play(INV, GUARD_DOG);

    // Distribute soak-first: fill Guard Dog to capacity (3), then the rest
    // onto the investigator.
    let session = distribute_onto(
        TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::EndTurn),
        dog,
    );
    // The assignment gives Guard Dog damage, so its `when` cell opens before
    // anything is placed; fire the retaliate to let the deal reach step 2.
    let session = fire_retaliate(session, dog);
    let state = session.state();

    // Guard Dog reached printed health → defeated and discarded from play.
    assert!(
        !state.investigators[&inv_id]
            .cards_in_play
            .iter()
            .any(|c| c.instance_id == dog),
        "Guard Dog at accumulated 3 >= health 3 is discarded"
    );
    assert!(
        session.events().iter().any(|e| matches!(
            e,
            Event::CardDiscarded { code, from, to }
                if *code == CardCode::new(GUARD_DOG)
                    && *from == Zone::InPlay
                    && *to == DiscardPile::Investigator(InvestigatorId(1))
        )),
        "Guard Dog discard emitted: {:?}",
        session.events()
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
// not asset discard. The defeat half of the unification: the
// investigator card uses the same `accumulated >= printed capacity` rule
// as an asset, but the consequence is elimination (Status::Defeated) rather
// than discard-to-owner.
// ---------------------------------------------------------------------

#[test]
fn investigator_card_overflow_eliminates_the_investigator() {
    let inv_id = INV;
    // The investigator has 8 health. Pre-load the investigator card
    // with 7 damage (survived prior harm). A 4-damage attack: Guard Dog
    // soaks 3 (defeated), the remaining 1 lands on the investigator card →
    // 8 >= 8 → the investigator is eliminated (Defeated), not the card
    // discarded to a pile.
    let state = soak_state(&[GUARD_DOG], 7, vec![ready_attacker(7, 4, 3)]);
    let dog = state.in_play(INV, GUARD_DOG);

    let session = distribute_onto(
        TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::EndTurn),
        dog,
    );
    // Guard Dog is in the assignment, so its `when` cell opens first.
    let session = fire_retaliate(session, dog);
    let state = session.state();

    // The investigator card reached its printed health → elimination, with
    // the damage cause. The InvestigatorEliminated event fired.
    assert_eq!(
        state.investigators[&inv_id].status,
        Status::Defeated,
        "investigator-card overflow eliminates (Defeated), not asset-discards"
    );
    assert!(
        session.events().iter().any(|e| matches!(
            e,
            Event::InvestigatorEliminated { investigator, cause }
                if *investigator == inv_id && *cause == EliminationCause::Damage
        )),
        "InvestigatorEliminated {{ cause: Damage }} emitted: {:?}",
        session.events()
    );
    // The investigator card was NOT discarded to a pile as if it were an
    // asset: no CardDiscarded for the investigator's own code, and the
    // investigator card itself is never the subject of an asset-defeat
    // discard (elimination removes the investigator's cards from the game
    // instead).
    assert!(
        !session.events().iter().any(|e| matches!(
            e,
            Event::CardDiscarded { code, .. } if *code == CardCode::new(test_support::TEST_INV)
        )),
        "investigator card is eliminated, not discarded as an asset: {:?}",
        session.events()
    );
}

// ---------------------------------------------------------------------
// Case 8 — defeat ORDERING when the same attack overflows both an asset
// and the investigator card. Load-bearing invariant: the
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
    let inv_id = INV;
    // The investigator has 8 health. Pre-load 5 onto the investigator card
    // and 2 onto Guard Dog (printed health 3). A 4-damage attack distributed
    // soak-first: Guard Dog takes 1 (→ 3 >= 3, would defeat) and the
    // remaining 3 land on the investigator card (→ 8 >= 8, eliminated). The
    // investigator is eliminated in step 2, draining cards_in_play to
    // removed_from_game before the asset sweep runs.
    let mut state = soak_state(&[GUARD_DOG], 5, vec![ready_attacker(7, 4, 3)]);
    let dog = state.in_play(INV, GUARD_DOG);
    preload_guard_dog(&mut state, 2);

    let session = distribute_onto(
        TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::EndTurn),
        dog,
    );
    // Guard Dog is in the assignment, so its `when` cell opens first.
    let session = fire_retaliate(session, dog);
    let state = session.state();

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
        !session.events().iter().any(|e| matches!(
            e,
            Event::CardDiscarded { code, from, .. }
                if *code == CardCode::new(GUARD_DOG) && *from == Zone::InPlay
        )),
        "no asset-defeat discard for the co-overflowing Guard Dog: {:?}",
        session.events()
    );
}
