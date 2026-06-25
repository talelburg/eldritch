//! Attacks of opportunity provoked by **playing an action-cost card** (#378, K3
//! of the keystone arc), driven through the public [`apply`] API with the real
//! card corpus installed.
//!
//! Per the Rules Reference (p.5, "Attack of Opportunity"), playing a card is an
//! action, so playing a **non-fast** card (asset or event) while engaged with a
//! ready enemy provokes one `AoO` from each — after the action cost is paid and
//! before the card's effect resolves. The Dynamite Blast 01024 FAQ pins the
//! order: "first you spend an action and pay the cost, then each engaged enemy
//! makes an attack of opportunity against you, and then the effects of the card
//! resolve — but only if you're still alive." Fast events/assets are not
//! actions and never provoke.
//!
//! This also covers the missing non-fast play-action charge folded into #378:
//! before this slice, playing a non-fast card spent no action at all.
//!
//! `game-core`'s unit tests can't install `cards::REGISTRY`, so this lives in
//! `crates/cards/tests/`. The `AoO` window/suspend mechanism is covered by
//! `dodge_aoo.rs` / `guard_dog_soak.rs` (K1); these tests prove the **new
//! firing site** routes through it.
//!
//! ## Verified card text (`ArkhamDB`, 2026-06-21)
//!
//! **Emergency Cache (01088):** "Gain 3 resources." A non-fast event → provokes.
//! **Working a Hunch (01037):** "Fast. Play only during your turn. … discover 1
//! clue …" A fast event → `AoO`-exempt.
#![allow(clippy::too_many_lines)]

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, Enemy, InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{test_enemy, test_investigator, test_location, GameStateBuilder};
use game_core::{Action, PlayerAction};

/// Emergency Cache (01088): non-fast event, `OnPlay` gain 3 resources → provokes.
const EMERGENCY_CACHE: &str = "01088";
/// Guard Dog (01021): Guardian Ally, health 3 / sanity 1, damage-retaliate soak.
const GUARD_DOG: &str = "01021";
/// Working a Hunch (01037): Fast event, `OnPlay` discover 1 clue → `AoO`-exempt.
const WORKING_A_HUNCH: &str = "01037";
/// Machete (01020): non-fast Guardian weapon asset → playing it provokes.
const MACHETE: &str = "01020";

fn install_real_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Resolve a soak-distribution prompt (#44/K5b — an `AoO` against an investigator
/// with a soaker prompts for the damage distribution) by assigning every point
/// onto the soaker asset, then to the investigator once it is full. Returns the
/// first result that is no longer a distribution prompt.
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
                response: game_core::InputResponse::PickSingle(id),
            }),
        );
    }
    result
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

// -----------------------------------------------------------------------
// Playing a non-fast event provokes an AoO (the new #378 firing site).
// -----------------------------------------------------------------------

/// Playing Emergency Cache (a non-fast event) while engaged with a ready enemy
/// provokes an `AoO`. With Guard Dog in play and no Dodge, the `AoO` damage soaks
/// onto Guard Dog and its reaction window opens — before the card's "gain 3
/// resources" effect resolves.
#[test]
fn playing_a_non_fast_event_while_engaged_provokes_an_aoo() {
    install_real_registry();

    let dog = CardInstanceId(1);
    let inv_id = InvestigatorId(1);
    let loc = LocationId(101);

    let mut investigator = test_investigator(1);
    // Real investigator code so max_health() reads from installed registry (#448 cp2a).
    investigator.investigator_card.code = CardCode::new("01003"); // Skids O'Toole: 8/6
    investigator.current_location = Some(loc);
    investigator.hand = vec![CardCode::new(EMERGENCY_CACHE)];
    investigator.cards_in_play = vec![CardInPlay::enter_play(CardCode::new(GUARD_DOG), dog)];
    let resources_before = investigator.resources;

    let attacker = engaged_attacker(7, inv_id, loc, 2, 5);

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_enemy(attacker)
        .build();

    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: inv_id,
            hand_index: 0,
        }),
    );
    // The AoO prompts for the soak distribution (#44/K5b): assign onto Guard Dog.
    let result = soak_onto_asset(result);
    let state = result.state;

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "the AoO soak window must suspend the play: {:?}",
        result.outcome
    );
    let dog_in_play = state.investigators[&inv_id]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == dog)
        .expect("Guard Dog still in play");
    assert_eq!(
        dog_in_play.accumulated_damage, 2,
        "AoO damage from the play soaked onto Guard Dog"
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "investigator took no AoO damage"
    );
    // The "gain 3 resources" effect has NOT run yet — it resolves only after the
    // AoO window closes.
    assert_eq!(
        state.investigators[&inv_id].resources, resources_before,
        "the card's effect resolves after the AoO, not before"
    );
}

// -----------------------------------------------------------------------
// Playing a non-fast card costs one action (the folded-in charge, #378).
// -----------------------------------------------------------------------

/// Playing a non-fast event spends one action. With no engaged enemy the play
/// completes immediately (no `AoO`), so we observe the spent action and the
/// resolved effect together.
#[test]
fn playing_a_non_fast_event_spends_one_action() {
    install_real_registry();

    let inv_id = InvestigatorId(1);
    let loc = LocationId(101);

    let mut investigator = test_investigator(1);
    // Real investigator code so max_health() reads from installed registry (#448 cp2a).
    investigator.investigator_card.code = CardCode::new("01003"); // Skids O'Toole: 8/6
    investigator.current_location = Some(loc);
    investigator.actions_remaining = 3;
    investigator.hand = vec![CardCode::new(EMERGENCY_CACHE)];
    let resources_before = investigator.resources;

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .build();

    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: inv_id,
            hand_index: 0,
        }),
    );
    let state = result.state;

    assert!(matches!(result.outcome, EngineOutcome::Done));
    assert_eq!(
        state.investigators[&inv_id].actions_remaining, 2,
        "playing a non-fast card spent one action"
    );
    assert_eq!(
        state.investigators[&inv_id].resources,
        resources_before + 3,
        "the effect still resolved (gain 3 resources)"
    );
}

/// Playing a non-fast card with no actions left is rejected (validate-first).
#[test]
fn playing_a_non_fast_card_with_no_actions_is_rejected() {
    install_real_registry();

    let inv_id = InvestigatorId(1);
    let loc = LocationId(101);

    let mut investigator = test_investigator(1);
    // Real investigator code so max_health() reads from installed registry (#448 cp2a).
    investigator.investigator_card.code = CardCode::new("01003"); // Skids O'Toole: 8/6
    investigator.current_location = Some(loc);
    investigator.actions_remaining = 0;
    investigator.hand = vec![CardCode::new(EMERGENCY_CACHE)];

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .build();

    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: inv_id,
            hand_index: 0,
        }),
    );

    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "a non-fast play with 0 actions must be rejected: {:?}",
        result.outcome
    );
    // Card stays in hand, untouched.
    assert!(
        result.state.investigators[&inv_id]
            .hand
            .contains(&CardCode::new(EMERGENCY_CACHE)),
        "the rejected play left the card in hand"
    );
}

// -----------------------------------------------------------------------
// A fast event is not an action — no AoO, no action spent (RR p.11).
// -----------------------------------------------------------------------

/// Playing Working a Hunch (a Fast event) while engaged with a ready enemy
/// provokes no `AoO` and spends no action; its effect (discover 1 clue) resolves.
#[test]
fn playing_a_fast_event_while_engaged_provokes_no_aoo_and_spends_no_action() {
    install_real_registry();

    let dog = CardInstanceId(1);
    let inv_id = InvestigatorId(1);
    let loc = LocationId(101);

    let mut investigator = test_investigator(1);
    // Real investigator code so max_health() reads from installed registry (#448 cp2a).
    investigator.investigator_card.code = CardCode::new("01003"); // Skids O'Toole: 8/6
    investigator.current_location = Some(loc);
    investigator.actions_remaining = 3;
    investigator.hand = vec![CardCode::new(WORKING_A_HUNCH)];
    investigator.cards_in_play = vec![CardInPlay::enter_play(CardCode::new(GUARD_DOG), dog)];

    let mut location = test_location(101, "Study");
    location.clues = 1;

    let attacker = engaged_attacker(7, inv_id, loc, 2, 5);

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(location)
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_enemy(attacker)
        .build();

    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: inv_id,
            hand_index: 0,
        }),
    );
    let state = result.state;

    // A fast event provokes no AoO: the play resolves without ever suspending
    // on an AoO window (a window would have surfaced as AwaitingInput).
    assert!(
        !matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "a fast event provokes no AoO — the play must not suspend on a window: {:?}",
        result.outcome
    );
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
        state.investigators[&inv_id].actions_remaining, 3,
        "a fast play spends no action"
    );
    assert_eq!(
        state.locations[&loc].clues, 0,
        "the fast event's effect resolved (discovered the clue)"
    );
}

// -----------------------------------------------------------------------
// An AoO that defeats the player mid-play suppresses the card's effect.
// -----------------------------------------------------------------------

/// Per the Dynamite Blast FAQ ("…the effects of the card resolve — but only if
/// you're still alive"): if the play's `AoO` defeats the investigator, the event's
/// effect does not resolve — though the card was still played (it goes to
/// discard).
#[test]
fn aoo_that_defeats_the_player_suppresses_the_event_effect() {
    install_real_registry();

    let inv_id = InvestigatorId(1);
    let loc = LocationId(101);

    let mut investigator = test_investigator(1);
    // Real investigator code so max_health() reads from installed registry (#448 cp2a).
    investigator.investigator_card.code = CardCode::new("01003"); // Skids O'Toole: 8/6
    investigator.current_location = Some(loc);
    investigator.actions_remaining = 3;
    investigator.hand = vec![CardCode::new(EMERGENCY_CACHE)];

    // Lethal AoO, no soaker / no Dodge → the actor is defeated before the
    // "gain 3 resources" effect can resolve.
    let attacker = engaged_attacker(7, inv_id, loc, 50, 5);

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_enemy(attacker)
        .build();

    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: inv_id,
            hand_index: 0,
        }),
    );
    let state = result.state;

    assert_ne!(
        state.investigators[&inv_id].status,
        game_core::state::Status::Active,
        "the lethal AoO defeated the actor"
    );
    // Elimination zeroes the wallet, so assert on the absent event, not the value.
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, Event::ResourcesGained { .. })),
        "the event's effect did not resolve (no ResourcesGained) — defeated mid-play: {:?}",
        result.events
    );
    // The card was still played: it left hand and went to discard.
    assert!(
        state.investigators[&inv_id]
            .discard
            .contains(&CardCode::new(EMERGENCY_CACHE)),
        "the played event still went to discard even though its effect was suppressed"
    );
}

// -----------------------------------------------------------------------
// Playing a non-fast ASSET also provokes, then the asset enters play on resume.
// -----------------------------------------------------------------------

/// Playing Machete (a non-fast asset) while engaged provokes an `AoO` (soaking
/// onto Guard Dog); after the soak window closes, the asset enters play.
#[test]
fn playing_a_non_fast_asset_provokes_an_aoo_then_enters_play() {
    install_real_registry();

    let dog = CardInstanceId(1);
    let inv_id = InvestigatorId(1);
    let loc = LocationId(101);

    let mut investigator = test_investigator(1);
    // Real investigator code so max_health() reads from installed registry (#448 cp2a).
    investigator.investigator_card.code = CardCode::new("01003"); // Skids O'Toole: 8/6
    investigator.current_location = Some(loc);
    investigator.actions_remaining = 3;
    investigator.hand = vec![CardCode::new(MACHETE)];
    investigator.cards_in_play = vec![CardInPlay::enter_play(CardCode::new(GUARD_DOG), dog)];

    let attacker = engaged_attacker(7, inv_id, loc, 2, 5);

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_enemy(attacker)
        .build();

    // Step 1: play Machete → AoO soaks onto Guard Dog → soak window; Machete is
    // still in hand (it enters play only after the play completes).
    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: inv_id,
            hand_index: 0,
        }),
    );
    // The AoO prompts for the soak distribution (#44/K5b): assign onto Guard Dog.
    let result = soak_onto_asset(result);
    let state = result.state;
    assert_eq!(
        state.investigators[&inv_id].actions_remaining, 2,
        "playing the asset spent one action before the AoO"
    );
    assert!(
        state.investigators[&inv_id]
            .hand
            .contains(&CardCode::new(MACHETE)),
        "the asset is still in hand while the AoO window is open"
    );

    // Step 2: fire Guard Dog's reaction (closes the soak window) → the play
    // resumes → Machete enters play.
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: game_core::InputResponse::PickSingle(game_core::OptionId(0)),
        }),
    );
    let state = result.state;
    assert!(
        state.investigators[&inv_id]
            .cards_in_play
            .iter()
            .any(|c| c.code == CardCode::new(MACHETE)),
        "Machete entered play after the AoO soak window closed"
    );
    assert!(
        !state.investigators[&inv_id]
            .hand
            .contains(&CardCode::new(MACHETE)),
        "Machete left hand once it entered play"
    );
}

/// If the asset play's `AoO` defeats the player, the asset never enters play (the
/// §D gate suppresses the resume before the enter-play step). Elimination
/// cleanup then sweeps the un-entered card out of hand — it is neither in play
/// nor stranded in a live hand.
#[test]
fn aoo_that_defeats_the_player_mid_asset_play_leaves_no_asset_in_play() {
    install_real_registry();

    let inv_id = InvestigatorId(1);
    let loc = LocationId(101);

    let mut investigator = test_investigator(1);
    // Real investigator code so max_health() reads from installed registry (#448 cp2a).
    investigator.investigator_card.code = CardCode::new("01003"); // Skids O'Toole: 8/6
    investigator.current_location = Some(loc);
    investigator.actions_remaining = 3;
    investigator.hand = vec![CardCode::new(MACHETE)];

    // Lethal AoO, no soaker / no Dodge → defeated before Machete enters play.
    let attacker = engaged_attacker(7, inv_id, loc, 50, 5);

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(investigator)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_enemy(attacker)
        .build();

    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: inv_id,
            hand_index: 0,
        }),
    );
    let state = result.state;

    assert_ne!(
        state.investigators[&inv_id].status,
        game_core::state::Status::Active,
        "the lethal AoO defeated the actor"
    );
    assert!(
        !state.investigators[&inv_id]
            .cards_in_play
            .iter()
            .any(|c| c.code == CardCode::new(MACHETE)),
        "Machete never entered play — the defeat suppressed the enter-play step"
    );
    assert!(
        !state.investigators[&inv_id]
            .hand
            .contains(&CardCode::new(MACHETE)),
        "the un-entered asset was swept out of hand by elimination cleanup, not stranded"
    );
}
