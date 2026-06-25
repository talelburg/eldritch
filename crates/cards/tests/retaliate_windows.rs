//! Integration tests proving that a failed Fight against a ready retaliate
//! enemy (a) lets Dodge cancel the resulting retaliate attack, and (b) lets
//! Guard Dog retaliate against the retaliating enemy — the acceptance test
//! for #379 (K2b).
//!
//! These are registry-backed proofs of the full
//! `drive_retaliate` → suspend (`BeforeEnemyAttack` or
//! `AfterEnemyAttackDamagedAsset` window) → `ResolveInput` → resume →
//! `advance` (teardown: `SkillTestEnded`, pop `SkillTest` frame)
//! cycle.  That resume seam is not exercised by the unit tests in
//! `game-core` (those don't install the real card registry and so the
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
#![allow(clippy::too_many_lines)]

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, Continuation, Enemy, EnemyId,
    InvestigatorId, LocationId, Phase, TokenModifiers,
};
use game_core::test_support::{
    take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder,
};
use game_core::{Action, InputResponse, PlayerAction, TurnAction};

/// Dodge (01023): Neutral Tactic, Fast, before-attack cancel reaction.
const DODGE: &str = "01023";

/// Guard Dog (01021): Guardian Ally, health 3 / sanity 1, damage-retaliate.
const GUARD_DOG: &str = "01021";

fn install_real_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Resolve a soak-distribution prompt (#44/K5b — a retaliate attack against an
/// investigator with a soaker prompts for the damage distribution) by assigning
/// every point onto the soaker asset. Returns the first result that is no longer
/// a distribution prompt.
fn soak_onto_asset(mut result: game_core::ApplyResult) -> game_core::ApplyResult {
    while let EngineOutcome::AwaitingInput { request, .. } = &result.outcome {
        if !request.prompt.contains("to which target") {
            break;
        }
        let id = request
            .options
            .iter()
            .find(|o| o.label.contains("Asset"))
            .or_else(|| request.options.iter().find(|o| o.label == "Investigator"))
            .expect("a distribution option")
            .id;
        result = apply(
            result.state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickSingle(id),
            }),
        );
    }
    result
}

/// Build a ready retaliate enemy at `loc`, engaged with `inv`.
/// `fight` is set high enough that the investigator (combat 1) will fail
/// the test with a `Numeric(0)` token (1 + 0 = 1 < fight).
/// `attack_damage` is the retaliate attack's damage payload;
/// `max_health` ensures the enemy can survive Guard Dog's 1-point retaliation.
fn retaliate_enemy(
    id: u32,
    inv: InvestigatorId,
    loc: LocationId,
    fight: i8,
    attack_damage: u8,
    max_health: u8,
) -> Enemy {
    let mut e = test_enemy(id, format!("Retaliate Enemy {id}"));
    e.fight = fight;
    e.max_health = max_health;
    e.attack_damage = attack_damage;
    e.attack_horror = 0;
    e.retaliate = true;
    e.current_location = Some(loc);
    e.engaged_with = Some(inv);
    e
}

/// Build a minimal Investigation-phase state with one active investigator
/// at a location, `enemy` engaged, and a deterministically-failing chaos bag
/// (`Numeric(0)`; investigator combat = 1; enemy fight = 5 → 1 < 5 → fail).
fn fight_state(
    enemy: Enemy,
    hand: Vec<CardCode>,
    cards_in_play: Vec<CardInPlay>,
) -> (game_core::GameState, InvestigatorId, LocationId) {
    install_real_registry();

    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(101);

    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    // Low combat so the test always fails: 1 + Numeric(0) = 1 < 5 (enemy fight).
    inv.skills.combat = 1;
    inv.hand = hand;
    inv.cards_in_play = cards_in_play;

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(inv)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_enemy(enemy)
        // Single `Numeric(0)` token → total = combat(1) + 0 = 1 < fight(5) → fail.
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    (state, inv_id, loc_id)
}

/// Drive the Fight through its commit window with no commits, so that the
/// chaos token is drawn and the skill test is resolved (failed → retaliate).
/// Returns the `ApplyResult` from submitting the empty commit.
fn submit_empty_commit(
    state: game_core::GameState,
    investigator: InvestigatorId,
    enemy: EnemyId,
) -> game_core::engine::ApplyResult {
    // Step 1: initiate the Fight — suspends at the commit window.
    let result = take_turn_action(
        state,
        &TurnAction::Fight {
            investigator,
            enemy,
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "Fight must suspend at the commit window: {:?}",
        result.outcome
    );

    // Step 2: submit an empty commit — the chaos token is drawn, the test
    // resolves (failed), and fire_retaliate_if_any is called.
    apply(
        result.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
}

// ---------------------------------------------------------------------------
// Test 1 — Guard Dog retaliates against the retaliate attack
// ---------------------------------------------------------------------------

/// Investigator controls Guard Dog, fails a Fight against a ready retaliate
/// enemy → the retaliate runs through the attack loop (K2a, #379) → Guard
/// Dog soaks the retaliate's 1 damage → the
/// `AfterEnemyAttackDamagedAsset` window opens → fire Guard Dog's reaction
/// → the retaliating enemy takes 1 → the Fight's skill test tears down
/// cleanly (`SkillTestEnded` emitted, no `SkillTest` frame on the stack).
#[test]
fn guard_dog_retaliates_against_retaliate_and_skill_test_ends() {
    let dog = CardInstanceId(1);
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(101);
    let enemy_id = EnemyId(7);

    // Enemy fight 5, damage 1 (soaks onto Guard Dog health 3), max_health 5
    // (survives Guard Dog's 1-point retaliation after absorbing 0 prior damage).
    let enemy = retaliate_enemy(7, inv_id, loc_id, 5, 1, 5);
    let guard_dog_in_play = CardInPlay::enter_play(CardCode::new(GUARD_DOG), dog);

    let (state, _, _) = fight_state(enemy, vec![], vec![guard_dog_in_play]);

    // Initiate Fight → empty commit → fails → fire_retaliate_if_any → drive_retaliate
    // → Guard Dog has no cancel reaction so BeforeEnemyAttack auto-skips → damage
    // lands on Guard Dog → AfterEnemyAttackDamagedAsset window opens → suspend.
    let result = submit_empty_commit(state, inv_id, enemy_id);
    // The retaliate's damage prompts the soak distribution (#44/K5b): assign it
    // onto Guard Dog → soak window opens.
    let result = soak_onto_asset(result);
    let mut state = result.state;

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "Guard Dog soak window must suspend after the failed Fight: {:?}",
        result.outcome
    );

    // Guard Dog soaked the retaliate's 1 damage; investigator took none.
    let dog_in_play = state.investigators[&inv_id]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == dog)
        .expect("Guard Dog still in play before reaction fires");
    assert_eq!(
        dog_in_play.accumulated_damage, 1,
        "retaliate's 1 damage soaked onto Guard Dog"
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "investigator took no damage from the retaliate (Guard Dog soaked it)"
    );
    // Guard Dog has not yet dealt its retaliate damage to the enemy.
    assert_eq!(state.enemies[&enemy_id].damage, 0);

    // The SkillTest frame must still be present while the soak window is open
    // (teardown only happens after the window closes via the resume seam).
    assert!(
        state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::SkillTest(_))),
        "SkillTest frame must still be on the stack while the Guard Dog window is open"
    );

    // Fire Guard Dog's reaction (PickSingle(0) = the single offered trigger).
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    state = result.state;

    // Guard Dog dealt 1 damage to the retaliating enemy.
    assert_eq!(
        state.enemies[&enemy_id].damage, 1,
        "Guard Dog's reaction dealt 1 damage to the retaliating enemy"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == enemy_id
        )),
        "EnemyDamaged {{ amount: 1 }} emitted: {:?}",
        result.events
    );

    // The retaliating enemy did NOT exhaust (RR p.18: retaliate attacks do
    // not exhaust the attacker; Dodge FAQ confirms this is not Enemy-phase).
    assert!(
        !state.enemies[&enemy_id].exhausted,
        "a retaliate attack does not exhaust the attacker (RR p.18)"
    );

    // The Fight's skill test tore down cleanly after the window closed:
    // SkillTestEnded was emitted and the SkillTest frame was popped.
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::SkillTestEnded { investigator } if *investigator == inv_id
        )),
        "SkillTestEnded emitted after Guard Dog window closed (K2 resume seam): {:?}",
        result.events
    );
    assert!(
        !state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::SkillTest(_))),
        "no SkillTest frame on the stack after the skill test tore down: {:?}",
        state.continuations
    );

    // No windows remain stranded.
    assert!(
        state.open_windows().is_empty(),
        "no windows stranded after Guard Dog reaction + skill-test teardown: {:?}",
        state.open_windows()
    );
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
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(101);
    let enemy_id = EnemyId(7);

    // Enemy fight 5, damage 2, max_health 5.  No Guard Dog — Dodge is the
    // only reaction card.
    let enemy = retaliate_enemy(7, inv_id, loc_id, 5, 2, 5);

    let (state, _, _) = fight_state(enemy, vec![CardCode::new(DODGE)], vec![]);

    // Initiate Fight → empty commit → fails → fire_retaliate_if_any → drive_retaliate
    // → BeforeEnemyAttack window opens (Dodge in hand) → suspend.
    let result = submit_empty_commit(state, inv_id, enemy_id);
    let mut state = result.state;

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "BeforeEnemyAttack window must suspend after the failed Fight: {:?}",
        result.outcome
    );

    // No damage yet; Dodge still in hand.
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "no damage before Dodge resolves"
    );
    assert_eq!(
        state.investigators[&inv_id].horror(),
        0,
        "no horror before Dodge resolves"
    );
    assert!(
        state.investigators[&inv_id]
            .hand
            .contains(&CardCode::new(DODGE)),
        "Dodge is still in hand while the cancel window is open"
    );

    // The SkillTest frame must still be present while the window is open.
    assert!(
        state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::SkillTest(_))),
        "SkillTest frame present while BeforeEnemyAttack window is open"
    );

    // Play Dodge (PickSingle(0) = the single cancel candidate).
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    state = result.state;

    // The retaliate was cancelled: no damage or horror dealt to the investigator.
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "Dodge cancelled the retaliate: no damage to the investigator"
    );
    assert_eq!(
        state.investigators[&inv_id].horror(),
        0,
        "Dodge cancelled the retaliate: no horror to the investigator"
    );
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, Event::DamageTaken { .. })),
        "a Dodge-cancelled retaliate deals no damage: {:?}",
        result.events
    );

    // The retaliating enemy did NOT exhaust: a retaliate attacker never
    // exhausts, regardless of a cancel (RR p.18).
    assert!(
        !state.enemies[&enemy_id].exhausted,
        "a retaliate attacker does not exhaust, even after a Dodge cancel (RR p.18)"
    );

    // Dodge left the investigator's hand and went to discard.
    assert!(
        !state.investigators[&inv_id]
            .hand
            .contains(&CardCode::new(DODGE)),
        "Dodge left hand after being played"
    );
    assert!(
        state.investigators[&inv_id]
            .discard
            .contains(&CardCode::new(DODGE)),
        "Dodge is in the discard pile after being played"
    );

    // The Fight's skill test tore down cleanly after the cancel window closed:
    // SkillTestEnded was emitted and the SkillTest frame was popped.
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::SkillTestEnded { investigator } if *investigator == inv_id
        )),
        "SkillTestEnded emitted after Dodge cancel (K2 resume seam): {:?}",
        result.events
    );
    assert!(
        !state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::SkillTest(_))),
        "no SkillTest frame on the stack after the skill test tore down: {:?}",
        state.continuations
    );

    // No windows remain stranded.
    assert!(
        state.open_windows().is_empty(),
        "no windows stranded after Dodge cancel + skill-test teardown: {:?}",
        state.open_windows()
    );
}
