//! Regression for #379: a failed Fight against a ready retaliate enemy (a) lets
//! Dodge cancel the resulting retaliate attack, and (b) lets Guard Dog retaliate
//! against the retaliating enemy.
//!
//! These are registry-backed proofs of the full
//! `drive_retaliate` → suspend (`BeforeEnemyAttack` or
//! `AfterEnemyAttackDamagedAsset` window) → `ResolveInput` → resume →
//! `advance` (teardown: `SkillTestEnded`, pop `SkillTest` frame)
//! cycle.  That resume seam is not exercised by the unit tests in
//! `game-core` (those can't reach the real card registry and so the
//! windows never open).
//!
//! ## Verified card text (`ArkhamDB`, 2026-06-21)
//!
//! **Dodge (01023)** — Event. Tactic. Cost 1. Fast.
//! "Fast. Play when an enemy attacks an investigator at your location.
//! Cancel that attack."
//! FAQ:
//!   - "Dodge can cancel any type of enemy attack: a normal attack during
//!     the Enemy phase, an attack of opportunity, or a Retaliate attack."
//!   - "If the attacking enemy has a Forced ability that says 'When attacks'
//!     or 'After attacks', that ability does not trigger if an attack is
//!     Dodged."
//!   - "When a Massive enemy attacks each investigator in its location,
//!     Dodge will cancel only one of these attacks, not all of them."
//!   - "If an attack was cancelled during the Enemy phase, the attacking
//!     enemy still exhausts."
//!
//! **Guard Dog (01021)** — Asset. Ally. Creature. Cost 3. Health 3, Sanity 1.
//! "[reaction] When an enemy attack deals damage to Guard Dog: Deal 1 damage
//! to the attacking enemy."
//! FAQ:
//!   - "You can use Guard Dog's ability when you assign lethal damage/horror
//!     to it."
//!   - Guard Dog's ability triggers only on damage, not horror.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, Continuation, EnemyId, GameState, GameStateBuilder,
    InvestigatorId, LocationId, SkillKind,
};
use game_core::test_support::{enemy, location, seat, Seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_no_event};

/// Dodge (01023): Neutral Tactic, Fast, before-attack cancel reaction.
const DODGE: &str = "01023";

/// Guard Dog (01021): Guardian Ally, health 3 / sanity 1, damage-retaliate.
const GUARD_DOG: &str = "01021";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);
const RETALIATOR: EnemyId = EnemyId(7);

/// Investigator 1 at the Study with combat 1, so the Fight always fails:
/// 1 + Numeric(0) = 1 < 5 (enemy fight).
fn fighter() -> Seat {
    seat(1).at(HERE).skill(SkillKind::Combat, 1)
}

/// An Investigation-phase turn with `investigator` at the Study, engaged with
/// a ready Retaliate enemy, and a deterministically-failing chaos bag
/// (`Numeric(0)`; investigator combat = 1; enemy fight = 5 → 1 < 5 → fail).
/// `attack_damage` is the retaliate attack's damage payload; health 5 lets
/// the enemy survive Guard Dog's 1-point retaliation.
fn fight_board(investigator: Seat, attack_damage: u8) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(investigator)
        .open_turn(INV)
        .with_enemy_engaged(
            enemy(RETALIATOR)
                .retaliate()
                .fight(5)
                .health(5)
                .attack_damage(attack_damage)
                .attack_horror(0),
            INV,
        )
        // Single `Numeric(0)` token → total = combat(1) + 0 = 1 < fight(5) → fail.
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .session()
}

/// Drive the Fight through its commit window with no commits, so that the
/// chaos token is drawn and the skill test is resolved (failed → retaliate).
fn fight_committing_nothing(session: TestSession) -> TestSession {
    // Step 1: initiate the Fight — suspends at the commit window.
    // Step 2: submit an empty commit — the chaos token is drawn, the test
    // resolves (failed), and fire_retaliate_if_any is called.
    session
        .take(&TurnAction::Fight {
            investigator: INV,
            enemy: RETALIATOR,
        })
        .commit(&[])
}

/// The skill test tore down cleanly: no `SkillTest` frame and no window left
/// stranded on the stack. A stack invariant — no stranded frames — so it reads
/// the stack.
fn assert_nothing_stranded(state: &GameState) {
    assert!(
        !state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::SkillTest(_))),
        "no SkillTest frame on the stack after the skill test tore down: {:?}",
        state.continuations
    );
    assert!(
        state.open_windows().is_empty(),
        "no windows stranded after the skill-test teardown: {:?}",
        state.open_windows()
    );
}

// ---------------------------------------------------------------------------
// Test 1 — Guard Dog retaliates against the retaliate attack
// ---------------------------------------------------------------------------

/// Investigator controls Guard Dog, fails a Fight against a ready retaliate
/// enemy → the retaliate runs through the attack loop → Guard
/// Dog soaks the retaliate's 1 damage → the
/// `AfterEnemyAttackDamagedAsset` window opens → fire Guard Dog's reaction
/// → the retaliating enemy takes 1 → the Fight's skill test tears down
/// cleanly (`SkillTestEnded` emitted, no `SkillTest` frame on the stack).
#[test]
fn guard_dog_retaliates_against_retaliate_and_skill_test_ends() {
    // Enemy fight 5, damage 1 (soaks onto Guard Dog health 3), max_health 5
    // (survives Guard Dog's 1-point retaliation after absorbing 0 prior damage).
    let session = fight_board(fighter().in_play([GUARD_DOG]), 1);
    let dog_id = session.state().in_play(INV, GUARD_DOG);
    let dog = OptionTarget::CardInstance(dog_id);

    // Initiate Fight → empty commit → fails → fire_retaliate_if_any → drive_retaliate
    // → Guard Dog has no cancel reaction so the attack's `when` cell auto-skips →
    // its damage is assigned to Guard Dog → the `DamageDealt` window opens →
    // suspend, before anything is placed.
    // The retaliate's damage prompts the soak distribution: assign it
    // onto Guard Dog → soak window opens.
    let session = fight_committing_nothing(session).pick(dog.clone());
    let state = session.state();

    assert_ne!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV)),
        "Guard Dog soak window must suspend after the failed Fight"
    );

    // The retaliate's 1 damage is assigned to Guard Dog and not yet placed: the
    // open window is Guard Dog's own `when` cell, between the two rules steps.
    let dog_in_play = state.investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == dog_id)
        .expect("Guard Dog still in play before reaction fires");
    assert_eq!(
        dog_in_play.accumulated_damage, 0,
        "retaliate's 1 damage assigned to Guard Dog, not yet placed"
    );
    assert_eq!(
        state.investigators[&INV].damage(),
        0,
        "investigator took no damage from the retaliate (Guard Dog soaked it)"
    );
    // Guard Dog has not yet dealt its retaliate damage to the enemy.
    assert_eq!(state.enemies[&RETALIATOR].damage, 0);

    // The skill test is still in flight while the soak window is open
    // (teardown only happens after the window closes via the resume seam).
    assert_no_event!(session.events(), Event::SkillTestEnded { .. });

    // Fire Guard Dog's reaction, offered on the dog.
    let session = session.pick(dog);
    session.assert_at_turn_menu(INV);
    let result = session.finish();
    let state = result.state;

    // Guard Dog dealt 1 damage to the retaliating enemy.
    assert_eq!(
        state.enemies[&RETALIATOR].damage, 1,
        "Guard Dog's reaction dealt 1 damage to the retaliating enemy"
    );
    // …and only then did the retaliate's own damage land on Guard Dog.
    assert_eq!(
        state.investigators[&INV]
            .cards_in_play
            .iter()
            .find(|c| c.instance_id == dog_id)
            .map(|c| c.accumulated_damage),
        Some(1),
        "the retaliate's damage is placed once Guard Dog's when cell has run"
    );
    assert_event!(
        result.events,
        Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == RETALIATOR
    );

    // The retaliating enemy did NOT exhaust (RR p.18: retaliate attacks do
    // not exhaust the attacker; Dodge FAQ confirms this is not Enemy-phase).
    assert!(
        !state.enemies[&RETALIATOR].exhausted,
        "a retaliate attack does not exhaust the attacker (RR p.18)"
    );

    // The Fight's skill test tore down cleanly after the window closed:
    // SkillTestEnded was emitted and the SkillTest frame was popped.
    assert_event!(
        result.events,
        Event::SkillTestEnded { investigator } if *investigator == INV
    );
    assert_nothing_stranded(&state);
}

// ---------------------------------------------------------------------------
// Test 2 — Dodge cancels the retaliate attack; skill test tears down cleanly
// ---------------------------------------------------------------------------

/// Investigator has Dodge in hand, fails a Fight against a ready retaliate
/// enemy → the retaliate runs through the attack loop → the
/// `BeforeEnemyAttack` cancel window opens → play Dodge to cancel →
/// no damage/horror from the retaliate → the Fight's skill test tears down
/// cleanly (`SkillTestEnded` emitted, no `SkillTest` frame left).
#[test]
fn dodge_cancels_retaliate_and_skill_test_ends() {
    // Enemy fight 5, damage 2, max_health 5.  No Guard Dog — Dodge is the
    // only reaction card.
    let session = fight_board(fighter().hand([DODGE]), 2);

    // Initiate Fight → empty commit → fails → fire_retaliate_if_any → drive_retaliate
    // → BeforeEnemyAttack window opens (Dodge in hand) → suspend.
    let session = fight_committing_nothing(session);
    let dodge = OptionTarget::HandCardByCode {
        investigator: INV,
        code: CardCode::new(DODGE),
    };
    assert_eq!(
        session.offered_at(&dodge),
        1,
        "BeforeEnemyAttack window must suspend after the failed Fight, offering \
         Dodge: {:?}",
        session.prompt(),
    );
    let state = session.state();

    // No damage yet; Dodge still in hand.
    assert_eq!(
        state.investigators[&INV].damage(),
        0,
        "no damage before Dodge resolves"
    );
    assert_eq!(
        state.investigators[&INV].horror(),
        0,
        "no horror before Dodge resolves"
    );
    assert!(
        state.investigators[&INV]
            .hand
            .contains(&CardCode::new(DODGE)),
        "Dodge is still in hand while the cancel window is open"
    );

    // The skill test is still in flight while the window is open.
    assert_no_event!(session.events(), Event::SkillTestEnded { .. });

    // Play Dodge, the single cancel candidate.
    let session = session.pick(dodge);
    session.assert_at_turn_menu(INV);
    let result = session.finish();
    let state = result.state;

    // The retaliate was cancelled: no damage or horror dealt to the investigator.
    assert_eq!(
        state.investigators[&INV].damage(),
        0,
        "Dodge cancelled the retaliate: no damage to the investigator"
    );
    assert_eq!(
        state.investigators[&INV].horror(),
        0,
        "Dodge cancelled the retaliate: no horror to the investigator"
    );
    // A Dodge-cancelled retaliate deals no damage.
    assert_no_event!(result.events, Event::DamagePlacedOnInvestigator { .. });

    // The retaliating enemy did NOT exhaust: a retaliate attacker never
    // exhausts, regardless of a cancel (RR p.18).
    assert!(
        !state.enemies[&RETALIATOR].exhausted,
        "a retaliate attacker does not exhaust, even after a Dodge cancel (RR p.18)"
    );

    // Dodge left the investigator's hand and went to discard.
    assert!(
        !state.investigators[&INV]
            .hand
            .contains(&CardCode::new(DODGE)),
        "Dodge left hand after being played"
    );
    assert!(
        state.investigators[&INV]
            .discard
            .contains(&CardCode::new(DODGE)),
        "Dodge is in the discard pile after being played"
    );

    // The Fight's skill test tore down cleanly after the cancel window closed:
    // SkillTestEnded was emitted and the SkillTest frame was popped.
    assert_event!(
        result.events,
        Event::SkillTestEnded { investigator } if *investigator == INV
    );
    assert_nothing_stranded(&state);
}
