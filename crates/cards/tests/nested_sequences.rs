//! The Rules Reference's worked example of nested sequences, traced cell for
//! cell against the real corpus (#727/#722).
//!
//! `data/rules-reference/rules/glossary/Nested_Sequences.md`, verbatim:
//!
//! > Roland and Agnes are embroiled in a fierce battle. Roland has a Guard Dog in
//! > his play area, and is engaged with a Goat Spawn with 2 damage on it. […]
//! > Roland wishes to play a .45 Automatic, which provokes an attack of
//! > opportunity from the Goat Spawn, dealing 1 damage to Roland. Roland assigns
//! > this damage to his Guard Dog, which has a [reaction] ability: “When an enemy
//! > attack deals damage to Guard Dog: Deal 1 damage to the attacking enemy.”
//! > Before resolving the playing of Roland’s .45 Automatic, Guard Dog’s ability
//! > resolves, and 1 damage is dealt to the Goat Spawn, which would defeat it.
//! > […] Before resolving the damage dealt to the Guard Dog, 1 horror is dealt to
//! > each investigator at the location, including Agnes, who has a [reaction]
//! > ability: “After 1 or more horror is placed on Agnes Baker: Deal 1 damage to
//! > an enemy at your location.” Before resolving the Goat Spawn’s defeat, Agnes
//! > deals 1 damage to the Ghoul Minion engaged with her. […] Now that there are
//! > no further [reaction] or **Forced** abilities to trigger, the players return
//! > to the previous triggering condition and resolve the Goat Spawn’s defeat,
//! > and resolve any “After...” effects that might occur when it is defeated.
//! > Then, the players resolve the damage dealt to the Guard Dog, and resolve any
//! > “After...” effects that might occur from that damage. Finally, the players
//! > return to the original triggering condition, and Roland is able to put his
//! > .45 Automatic into play.
//!
//! Quoted with the source's own emphasis, elisions marked; the emphasis from
//! here on is ours. Every sentence of that paragraph is a cell, and the
//! load-bearing one is *"Before resolving the damage dealt to the Guard Dog"*:
//! the damage the Guard Dog reacted to is **assigned but not placed** for the
//! whole of the nested sequence its reaction spawns. That is what
//! `docs/adr/0009-damage-is-assigned-then-placed.md` splits `DamageAssigned`
//! from `DamagePlaced` to model, and it is the whole of what this file asserts.
//!
//! **Two substitutions**, and neither is a convenience. Both of the example's
//! other cards are in the corpus but **unimplemented** — they have no
//! `abilities()` impl, so the engine sees only their metadata:
//!
//! - Goat Spawn 01180 (`pack/core/core_encounter.json`), *"**Forced** - When
//!   Goat Spawn is defeated: Each investigator at this location takes 1
//!   horror."* So the nested sequence hanging off the defeat is **Roland Banks
//!   01001's own** *"[reaction] After you defeat an enemy: Discover 1 clue at
//!   your location. (Limit once per round.)"* — a different ability in the same
//!   structural position, spawned by the same defeat, and it has to complete
//!   before the original deal of damage resolves for exactly the same reason.
//! - Agnes Baker 01004 (`pack/core/core.json`), *"[reaction] After 1 or more
//!   horror is placed on Agnes Baker: Deal 1 damage to an enemy at your
//!   location. (Limit once per phase.)"* — the example's third nesting level.
//!   Implementing it would need a card-facing pattern for `DamagePlaced`, which
//!   #727 deliberately did **not** build ("a card-facing pattern with no corpus
//!   declaration would be decoration"), so that level is not reproduced. The
//!   LIFO discipline it demonstrates is the same one asserted here one level up.
//!
//! Implementing either card is card work neither #727 nor #722 asked for; what
//! this file owes them is the *shape*, and the shape does not depend on which
//! ability the defeat spawns.
//!
//! Sibling files: `guard_dog_soak.rs` (the soak pipeline itself),
//! `enemy_attack_cells.rs` (#704's condition), `clue_discovery_cells.rs` (#703's).

use game_core::engine::{apply, ApplyResult, EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, Enemy, EnemyId, InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{take_turn_action, test_enemy, test_investigator, test_location};
use game_core::{Action, InputResponse, PlayerAction, TurnAction};

/// Roland Banks (01001) — the example's investigator, and the source of the
/// nested sequence that hangs off the Goat Spawn's defeat.
const ROLAND: &str = "01001";
/// Guard Dog (01021): Ally, health 3 / sanity 1, the retaliate the example runs on.
const GUARD_DOG: &str = "01021";
/// .45 Automatic (01016): the Hand asset whose play provokes the attack of
/// opportunity, and which enters play last of all.
const AUTOMATIC_45: &str = "01016";

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// The Goat Spawn's stand-in: engaged, ready, **2 damage already on it** and 3
/// health, so Guard Dog's 1 retaliate damage defeats it — the example's setup
/// exactly. Its attack deals 1 damage and no horror.
fn goat_spawn(id: u32, inv: InvestigatorId, loc: LocationId) -> Enemy {
    let mut e = test_enemy(id, "Goat Spawn");
    e.max_health = 3;
    e.damage = 2;
    e.attack_damage = 1;
    e.attack_horror = 0;
    e.current_location = Some(loc);
    e.engaged_with = Some(inv);
    e
}

/// Resume the top prompt with `PickSingle(id)`.
fn resolve(state: game_core::GameState, id: OptionId) -> ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(id),
        }),
    )
}

/// The soak distribution's per-point prompt (#44/K5b), as opposed to a window.
fn is_distribution_prompt(outcome: &EngineOutcome) -> bool {
    matches!(
        outcome,
        EngineOutcome::AwaitingInput { request, .. } if request.prompt.contains("to which target")
    )
}

/// Assign every contested point to `inst`, reproducing the example's "Roland
/// assigns this damage to his Guard Dog".
fn assign_to(mut result: ApplyResult, inst: CardInstanceId) -> ApplyResult {
    while is_distribution_prompt(&result.outcome) {
        let EngineOutcome::AwaitingInput { request, .. } = &result.outcome else {
            unreachable!()
        };
        let needle = format!("CardInstanceId({})", inst.0);
        let id = request
            .options
            .iter()
            .find(|o| o.label.contains(&needle))
            .expect("the Guard Dog is an eligible target for the point")
            .id;
        result = resolve(result.state, id);
    }
    result
}

fn guard_dog_damage(state: &game_core::GameState, inv: InvestigatorId, inst: CardInstanceId) -> u8 {
    state.investigators[&inv]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == inst)
        .expect("Guard Dog in play")
        .accumulated_damage
}

fn in_play(state: &game_core::GameState, inv: InvestigatorId, code: &str) -> bool {
    state.investigators[&inv]
        .cards_in_play
        .iter()
        .any(|c| c.code == CardCode::new(code))
}

#[test]
#[allow(clippy::too_many_lines)]
fn the_damage_dealt_to_the_guard_dog_resolves_last() {
    let dog = CardInstanceId(1);
    let roland_card = CardInstanceId(2);
    let inv_id = InvestigatorId(1);
    let loc = LocationId(101);
    let spawn = EnemyId(7);

    let mut roland = test_investigator(1);
    roland.investigator_card.code = CardCode::new(ROLAND); // 9 health / 5 sanity
    roland.current_location = Some(loc);
    roland.resources = 5; // the .45 costs 4
    roland.hand = vec![CardCode::new(AUTOMATIC_45)];
    roland.cards_in_play = vec![
        CardInPlay::enter_play(CardCode::new(GUARD_DOG), dog),
        // Roland's own card in play is how the reaction scan reaches his
        // investigator ability (the `roland_banks.rs` fixture's convention).
        CardInPlay::enter_play(CardCode::new(ROLAND), roland_card),
    ];

    // A clue at the location for Roland's reaction to discover — the marker
    // that tells us the nested sequence resolved, and when.
    let mut study = test_location(101, "Study");
    study.clues = 1;

    let state = game_core::test_support::GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(study)
        .with_investigator(roland)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_enemy(goat_spawn(7, inv_id, loc))
        .build();

    // ── "Roland wishes to play a .45 Automatic, which provokes an attack of
    //    opportunity from the Goat Spawn, dealing 1 damage to Roland. Roland
    //    assigns this damage to his Guard Dog" ────────────────────────────────
    let result = take_turn_action(
        state,
        &TurnAction::PlayCard {
            investigator: inv_id,
            hand_index: 0,
        },
    );
    let result = assign_to(result, dog);
    // The event log is per-`apply`, and the trace below spans three of them, so
    // it is accumulated as we go.
    let mut log: Vec<Event> = result.events.clone();
    let state = result.state;

    // The assignment is made and **nothing is placed**: what is open is Guard
    // Dog's `when` cell, the window the Rules Reference puts between assigning
    // and placing.
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "Guard Dog's when cell suspends the AoO: {:?}",
        result.outcome
    );
    assert_eq!(
        guard_dog_damage(&state, inv_id, dog),
        0,
        "the damage is assigned to the Guard Dog, not yet dealt to it"
    );
    assert_eq!(
        state.enemies[&spawn].damage, 2,
        "the Goat Spawn is untouched"
    );
    assert!(
        !in_play(&state, inv_id, AUTOMATIC_45),
        "\"Before resolving the playing of Roland's .45 Automatic\""
    );

    // ── "Guard Dog's ability resolves, and 1 damage is dealt to the Goat
    //    Spawn, which would defeat it" ─────────────────────────────────────────
    let result = resolve(state, OptionId(0));
    log.extend(result.events.iter().cloned());
    let state = result.state;

    assert!(
        !state.enemies.contains_key(&spawn),
        "the retaliate defeated the Goat Spawn: {:?}",
        result.events
    );
    // The defeat opened its own sequence, nested inside the deal of damage that
    // has still not been placed. Roland's after-defeat reaction is the corpus
    // stand-in for the example's Forced horror, and it is what is pending now.
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "the defeat's own sequence nests inside the unplaced damage: {:?}",
        result.outcome
    );
    // This is the example's load-bearing sentence.
    assert_eq!(
        guard_dog_damage(&state, inv_id, dog),
        0,
        "\"Before resolving the damage dealt to the Guard Dog\" — still unplaced \
         while the nested sequence runs"
    );
    assert_eq!(
        state.locations[&loc].clues, 1,
        "Roland's reaction has not resolved yet either"
    );
    assert!(
        !in_play(&state, inv_id, AUTOMATIC_45),
        "and the original action is still parked beneath all of it"
    );

    // ── "the players return to the previous triggering condition and resolve
    //    the Goat Spawn's defeat […] Then, the players resolve the damage dealt
    //    to the Guard Dog […] Finally […] Roland is able to put his .45
    //    Automatic into play." ─────────────────────────────────────────────────
    let result = resolve(state, OptionId(0));
    log.extend(result.events.iter().cloned());
    let state = result.state;

    // LIFO, innermost first: the nested sequence completed…
    assert_eq!(
        state.locations[&loc].clues, 0,
        "Roland discovered the clue — the nested sequence resolved first"
    );
    assert_eq!(state.investigators[&inv_id].clues, 1, "…onto Roland");
    // …then the damage that spawned it was finally placed…
    assert_eq!(
        guard_dog_damage(&state, inv_id, dog),
        1,
        "\"Then, the players resolve the damage dealt to the Guard Dog\""
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "Roland himself took none of it (it was assigned to the dog)"
    );
    // …and only then did the action that started everything complete.
    assert!(
        in_play(&state, inv_id, AUTOMATIC_45),
        "\"Finally […] Roland is able to put his .45 Automatic into play\""
    );

    // The same order, read off the event log: the retaliate's damage, then the
    // defeat, then the nested reaction's clue. The Guard Dog's own placement
    // emits nothing (asset damage is state, not an event) and the .45's entry
    // into play is a zone move, so both are asserted on state above rather than
    // here.
    game_core::assert_event_sequence!(
        log,
        Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == spawn,
        Event::EnemyDefeated { enemy, .. } if *enemy == spawn,
        Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id,
    );
}
