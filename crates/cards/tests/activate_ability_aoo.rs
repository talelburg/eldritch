//! Attacks of opportunity provoked by **activated abilities** (#361, K3 of the
//! keystone arc), driven through the public [`apply`] API with the real card
//! corpus installed.
//!
//! Per the Rules Reference (p.5, "Attack of Opportunity"): an investigator
//! engaged with one or more ready enemies who takes an action **other than to
//! fight, to evade, or to activate a parley or resign ability** provokes one
//! `AoO` from each such enemy, after costs are paid and before the action's
//! effect resolves. So a non-fight/evade action-cost activated ability (First
//! Aid 01019, Flashlight 01087, Medical Texts 01035, Old Book of Lore 01031)
//! provokes; a Fight ability (Machete 01020, .45 Automatic) is exempt; and a
//! fast ability (`action_cost == 0`, e.g. Beat Cop 01018) is not an action and
//! never provokes.
//!
//! `game-core`'s unit tests can't install `cards::REGISTRY` (the engine crate
//! can't depend on `cards`), so this lives in `crates/cards/tests/`. The `AoO`
//! window/suspend mechanism itself is covered by `dodge_aoo.rs` /
//! `guard_dog_soak.rs` (K1); these tests prove the **new firing sites** route
//! through it.
//!
//! ## Verified card text (`ArkhamDB`, 2026-06-21)
//!
//! **First Aid (01019):** "[action] Spend 1 supply: Heal 1 damage or 1 horror
//! from an investigator at your location." A non-fight action ability → provokes.
//!
//! **Machete (01020):** "[action]: Fight. You get +1 [combat] for this attack.
//! …" A Fight ability → `AoO`-exempt (RR p.5).
#![allow(clippy::too_many_lines)]

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, Enemy, EnemyId, InvestigatorId,
    LocationId, Phase, UseKind,
};
use game_core::test_support::{
    take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder,
};
use game_core::{Action, InputResponse, PlayerAction, TurnAction};

/// First Aid (01019): Guardian Item, `[action] Spend 1 supply: Heal …`. A
/// non-fight action ability → provokes an `AoO`.
const FIRST_AID: &str = "01019";
/// Machete (01020): Guardian weapon, `[action]: Fight …`. `AoO`-exempt.
const MACHETE: &str = "01020";
/// Guard Dog (01021): Guardian Ally, health 3 / sanity 1, damage-retaliate soak.
const GUARD_DOG: &str = "01021";
/// Beat Cop (01018): Guardian Ally; ability 1 is `[fast] Discard Beat Cop: Deal
/// 1 damage to an enemy at your location` — fast, so never provokes.
const BEAT_COP: &str = "01018";
/// Dodge (01023): Neutral Tactic, Fast, before-attack cancel reaction.
const DODGE: &str = "01023";

fn install_real_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// An engaged ready enemy at `loc` dealing `damage` / 0 horror with `max_health`.
fn engaged_attacker(
    id: u32,
    inv: InvestigatorId,
    loc: LocationId,
    damage: u8,
    max_health: u8,
) -> Enemy {
    let mut e = test_enemy(id, format!("Attacker {id}"));
    e.attack_damage = damage;
    e.attack_horror = 0;
    e.max_health = max_health;
    e.current_location = Some(loc);
    e.engaged_with = Some(inv);
    e
}

/// The distribution-prompt `PickSingle` `OptionId` for the soaker asset option
/// (#44/K5b — an `AoO` against an investigator with a soaker prompts for the
/// damage distribution before placing it).
fn pick_soaker(outcome: &EngineOutcome) -> OptionId {
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("expected a distribution prompt, got {outcome:?}");
    };
    request
        .options
        .iter()
        .find(|o| o.label.contains("Asset"))
        .unwrap_or_else(|| panic!("no soaker option in {:?}", request.options))
        .id
}

/// First Aid in play (with `supplies`) + Guard Dog in play (the soaker).
fn first_aid_and_guard_dog(
    inv: &mut game_core::state::Investigator,
    dog: CardInstanceId,
    kit: CardInstanceId,
) {
    let guard_dog = CardInPlay::enter_play(CardCode::new(GUARD_DOG), dog);
    let mut first_aid = CardInPlay::enter_play(CardCode::new(FIRST_AID), kit);
    first_aid.uses.insert(UseKind::Supplies, 3);
    inv.cards_in_play = vec![guard_dog, first_aid];
}

// -----------------------------------------------------------------------
// A non-fight action ability provokes an AoO (the new #361 firing site).
// -----------------------------------------------------------------------

/// Activating First Aid (a non-fight `[action]` ability) while engaged with a
/// ready enemy provokes an `AoO`. With Guard Dog in play and no Dodge, the `AoO`
/// damage soaks onto Guard Dog and its reaction window opens — the same
/// suspend/resume path Move uses (K1), proving `activate_ability` now routes
/// through `drive_aoo`.
#[test]
fn activating_a_non_fight_ability_while_engaged_provokes_an_aoo() {
    install_real_registry();

    let dog = CardInstanceId(1);
    let kit = CardInstanceId(2);
    let inv_id = InvestigatorId(1);
    let loc = LocationId(101);

    let mut investigator = test_investigator(1);
    investigator.current_location = Some(loc);
    first_aid_and_guard_dog(&mut investigator, dog, kit);

    let attacker = engaged_attacker(7, inv_id, loc, 2, 5);

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_enemy(attacker)
        .build();

    // Activate First Aid (ability 0). Action-cost, non-fight → provokes an AoO
    // after the supply cost is paid and before the heal effect resolves.
    let result = take_turn_action(
        state,
        &TurnAction::ActivateAbility {
            investigator: inv_id,
            instance_id: kit,
            ability_index: 0,
        },
    );
    // The AoO provokes a soak distribution prompt (Guard Dog has capacity, #44/
    // K5b): assign both AoO damage points onto Guard Dog to reproduce the soak.
    let r2 = apply(
        result.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(pick_soaker(&result.outcome)),
        }),
    );
    let result = apply(
        r2.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(pick_soaker(&r2.outcome)),
        }),
    );
    let state = result.state;

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "the AoO soak window must suspend the activation: {:?}",
        result.outcome
    );
    let dog_in_play = state.investigators[&inv_id]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == dog)
        .expect("Guard Dog still in play");
    assert_eq!(
        dog_in_play.accumulated_damage, 2,
        "AoO damage from the activation soaked onto Guard Dog"
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "investigator took no AoO damage (soaked onto Guard Dog)"
    );
    // First Aid spent its supply (the cost paid before the AoO).
    let kit_in_play = state.investigators[&inv_id]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == kit)
        .expect("First Aid still in play");
    assert_eq!(
        kit_in_play.uses.get(&UseKind::Supplies).copied(),
        Some(2),
        "First Aid spent 1 supply as the activation cost, before the AoO"
    );
    // The heal effect has NOT run yet — it resolves only after the AoO window closes.
}

// -----------------------------------------------------------------------
// A Fight ability is AoO-exempt (RR p.5).
// -----------------------------------------------------------------------

/// Activating Machete (a `[action]: Fight` ability) while engaged with a ready
/// enemy provokes **no** `AoO` — Fight is on the exempt list. The activation goes
/// straight to the Fight skill test (its commit window), and Guard Dog (in play)
/// soaks nothing.
#[test]
fn activating_a_fight_ability_while_engaged_provokes_no_aoo() {
    install_real_registry();

    let dog = CardInstanceId(1);
    let blade = CardInstanceId(2);
    let inv_id = InvestigatorId(1);
    let loc = LocationId(101);

    let mut investigator = test_investigator(1);
    investigator.current_location = Some(loc);
    investigator.cards_in_play = vec![
        CardInPlay::enter_play(CardCode::new(GUARD_DOG), dog),
        CardInPlay::enter_play(CardCode::new(MACHETE), blade),
    ];

    let attacker = engaged_attacker(7, inv_id, loc, 2, 5);

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_enemy(attacker)
        // The Fight starts a Combat skill test, which needs a non-empty bag.
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();

    let result = take_turn_action(
        state,
        &TurnAction::ActivateAbility {
            investigator: inv_id,
            instance_id: blade,
            ability_index: 0,
        },
    );
    let state = result.state;

    // Guard Dog soaked nothing; the investigator took no AoO damage.
    let dog_in_play = state.investigators[&inv_id]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == dog)
        .expect("Guard Dog still in play");
    assert_eq!(
        dog_in_play.accumulated_damage, 0,
        "no AoO soaked onto Guard Dog"
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "no AoO damage to the investigator"
    );
    // The activation went straight to the Fight skill test.
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, Event::SkillTestStarted { .. })),
        "the Fight ability began its skill test directly (no AoO first): {:?}",
        result.events
    );
}

// -----------------------------------------------------------------------
// A fast ability is not an action and never provokes (RR p.11).
// -----------------------------------------------------------------------

/// Activating Beat Cop's `[fast]` ability (`action_cost == 0`) while engaged
/// with a ready enemy provokes **no** `AoO` — fast is not an action. The ability
/// resolves (deals 1 damage to the enemy) without the investigator taking any
/// `AoO` damage.
#[test]
fn activating_a_fast_ability_while_engaged_provokes_no_aoo() {
    install_real_registry();

    let cop = CardInstanceId(1);
    let inv_id = InvestigatorId(1);
    let loc = LocationId(101);
    let enemy_id = EnemyId(7);

    let mut investigator = test_investigator(1);
    investigator.current_location = Some(loc);
    investigator.cards_in_play = vec![CardInPlay::enter_play(CardCode::new(BEAT_COP), cop)];

    let attacker = engaged_attacker(7, inv_id, loc, 2, 5);

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_enemy(attacker)
        .build();

    // Beat Cop ability 1 is the `[fast]` deal-1-damage (ability 0 is its
    // constant +1 combat).
    let result = take_turn_action(
        state,
        &TurnAction::ActivateAbility {
            investigator: inv_id,
            instance_id: cop,
            ability_index: 1,
        },
    );
    let state = result.state;

    assert!(
        matches!(result.outcome, EngineOutcome::Done),
        "the fast ability resolves without suspending on an AoO window: {:?}",
        result.outcome
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "no AoO damage to the investigator"
    );
    // The fast ability's own effect resolved: the enemy took 1 damage.
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == enemy_id
        )),
        "Beat Cop's fast ability dealt its 1 damage to the engaged enemy: {:?}",
        result.events
    );
}

// -----------------------------------------------------------------------
// The activation's AoO can be cancelled, then the ability's effect resumes.
// -----------------------------------------------------------------------

/// Dodge cancels the `AoO` provoked by activating First Aid; the activation then
/// resumes and runs First Aid's heal effect (which itself suspends on its
/// damage-or-horror choice). Proves `resume_activate_ability` runs the parked
/// effect after the `AoO` window closes.
#[test]
fn dodge_cancels_the_activations_aoo_then_the_ability_effect_resumes() {
    install_real_registry();

    let kit = CardInstanceId(2);
    let inv_id = InvestigatorId(1);
    let loc = LocationId(101);

    let mut investigator = test_investigator(1);
    investigator.current_location = Some(loc);
    // Use a real investigator code so max_health()/max_sanity() can read from
    // the installed cards registry (test_investigator uses TEST_INV which only
    // the game-core test registry knows about, #448 cp2a).
    investigator.investigator_card.code = CardCode::new("01003"); // Skids O'Toole: 8/6
    investigator.investigator_card.accumulated_damage = 2; // something for First Aid to heal
    investigator.hand = vec![CardCode::new(DODGE)];
    let mut first_aid = CardInPlay::enter_play(CardCode::new(FIRST_AID), kit);
    first_aid.uses.insert(UseKind::Supplies, 3);
    investigator.cards_in_play = vec![first_aid];

    let attacker = engaged_attacker(7, inv_id, loc, 2, 5);

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_enemy(attacker)
        .build();

    // Activate First Aid → AoO → Dodge is in hand, so the BeforeEnemyAttack
    // cancel window opens.
    let result = take_turn_action(
        state,
        &TurnAction::ActivateAbility {
            investigator: inv_id,
            instance_id: kit,
            ability_index: 0,
        },
    );
    let state = result.state;

    // Play Dodge (the single candidate) → cancel the AoO.
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    let state = result.state;

    // The AoO was cancelled — no damage — and the activation resumed into First
    // Aid's heal choice (the effect ran after the window closed).
    assert_eq!(
        state.investigators[&inv_id].damage(),
        2,
        "the cancelled AoO dealt no damage"
    );
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "First Aid's heal choice opened — the parked effect resumed: {:?}",
        result.outcome
    );

    // Pick the damage branch → 1 damage healed (2 → 1), proving the resumed
    // effect actually resolves.
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    let state = result.state;
    assert_eq!(
        state.investigators[&inv_id].damage(),
        1,
        "First Aid healed 1 damage after the AoO was dodged and the effect resumed"
    );
    assert!(
        state.open_windows().is_empty(),
        "no windows stranded after the dodge + resume + heal cycle: {:?}",
        state.open_windows()
    );
}

// -----------------------------------------------------------------------
// An AoO that defeats the actor mid-activation suppresses the ability effect.
// -----------------------------------------------------------------------

/// If the activation's `AoO` defeats the investigator, the §D re-validation gate
/// suppresses the parked ability effect: First Aid's heal never runs (no heal
/// choice opens), though the supply spent as the activation cost stays spent.
#[test]
fn aoo_that_defeats_the_actor_suppresses_the_ability_effect() {
    install_real_registry();

    let kit = CardInstanceId(2);
    let inv_id = InvestigatorId(1);
    let loc = LocationId(101);

    let mut investigator = test_investigator(1);
    investigator.current_location = Some(loc);
    // Use a real investigator code so max_health()/max_sanity() can read from
    // the installed cards registry (#448 cp2a).
    investigator.investigator_card.code = CardCode::new("01003"); // Skids O'Toole: 8/6
    investigator.investigator_card.accumulated_damage = 2;
    let mut first_aid = CardInPlay::enter_play(CardCode::new(FIRST_AID), kit);
    first_aid.uses.insert(UseKind::Supplies, 3);
    investigator.cards_in_play = vec![first_aid];

    // A lethal AoO and no soaker / no Dodge → the actor is defeated before the
    // heal effect can resume.
    let attacker = engaged_attacker(7, inv_id, loc, 50, 5);

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_enemy(attacker)
        .build();

    let result = take_turn_action(
        state,
        &TurnAction::ActivateAbility {
            investigator: inv_id,
            instance_id: kit,
            ability_index: 0,
        },
    );
    let state = result.state;

    // The actor was defeated by the AoO.
    assert_ne!(
        state.investigators[&inv_id].status,
        game_core::state::Status::Active,
        "the lethal AoO defeated the actor"
    );
    // The heal effect was suppressed: had it run, First Aid's `choose_one` would
    // have suspended on its damage-or-horror choice (`AwaitingInput`). Resolving
    // to `Done` instead proves the §D gate aborted the parked effect.
    assert!(
        matches!(result.outcome, EngineOutcome::Done),
        "the suppressed activation resolves to Done (no heal choice opened): {:?}",
        result.outcome
    );
}
