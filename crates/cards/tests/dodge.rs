//! End-to-end Dodge 01023: cancel an enemy-phase attack, driven through the
//! public [`apply`] API with the real card corpus installed (#305 / Axis D
//! #336).
//!
//! `game-core`'s unit tests can't install `cards::REGISTRY` (the engine crate
//! can't depend on `cards`), so the before-attack cancel window's end-to-end
//! behaviour — Dodge offered from hand, played, the attack cancelled — is
//! covered here.
//!
//! Dodge 01023: "Fast. Play when an enemy attacks an investigator at your
//! location. Cancel that attack."

use game_core::engine::{apply, EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::state::{CardCode, Enemy, EnemyId, InvestigatorId, LocationId, Phase};
use game_core::test_support::{take_turn_action, test_enemy, test_investigator, test_location};
use game_core::{Action, GameState, InputResponse, PlayerAction, TurnAction};

/// Dodge (01023): Neutral Tactic, Fast, the before-attack cancel reaction.
const DODGE: &str = "01023";

#[ctor::ctor]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// An engaged ready enemy at `loc` dealing 2 damage / 0 horror.
fn engaged_attacker(id: u32, inv: InvestigatorId, loc: LocationId) -> Enemy {
    let mut e = test_enemy(id, format!("Attacker {id}"));
    e.attack_damage = 2;
    e.attack_horror = 0;
    e.current_location = Some(loc);
    e.engaged_with = Some(inv);
    e
}

/// Investigation-phase state: one active investigator at a location with
/// `DODGE` in hand, engaged by one ready attacker. `EndTurn` advances into the
/// Enemy phase; the `BeforeInvestigatorAttacked` player window auto-skips
/// (Dodge is a reaction event — `check_play_card` rejects a standalone play, so
/// it is not a framework Fast play), then the attack loop opens the
/// `BeforeEnemyAttack` cancel window and offers Dodge.
fn dodge_state() -> (GameState, InvestigatorId, EnemyId) {
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(101);
    let enemy_id = EnemyId(7);

    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    // Use a real investigator code so max_health()/max_sanity() can read from
    // the installed cards registry; TEST_INV is only in the game-core test
    // registry (#448 cp2a). Skids O'Toole (01003, 8/6) — no implemented abilities.
    inv.investigator_card.code = CardCode::new("01003");
    inv.hand = vec![CardCode::new(DODGE)];
    // A spare deck card so the round-ending cascade's upkeep step-4.4 draw has
    // something to draw. Without it, the empty deck reshuffles the discard —
    // and since a played Dodge is discarded the instant its effect completes
    // (RR Appendix I step 4, now flushed at completion rather than the apply
    // boundary — #348), the reshuffle would draw Dodge straight back into hand,
    // obscuring the "Dodge went to discard" assertion.
    inv.deck = vec![CardCode::new("01088")];

    let state = game_core::test_support::GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(inv)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_enemy(engaged_attacker(7, inv_id, loc_id))
        // Mid-Investigation invariant (slice 1a): the EndTurn cascade pops the
        // InvestigationPhase anchor at investigation_phase_end.
        .with_phase_anchor(game_core::state::Continuation::InvestigationPhase {
            resume: game_core::state::InvestigationResume::TurnBegins,
        })
        // Open-turn invariant (slice 2a-i, #393): the InvestigatorTurn frame the
        // EndTurn cascade pops before advancing into the Enemy phase.
        .with_investigator_turn(inv_id)
        .build();
    (state, inv_id, enemy_id)
}

#[test]
fn dodge_cancels_enemy_phase_attack_no_damage_attacker_exhausts() {
    let (state, inv_id, enemy_id) = dodge_state();

    // EndTurn → Enemy phase → the attack loop opens the before-attack cancel
    // window and suspends, offering Dodge from hand.
    let result = take_turn_action(state, &TurnAction::EndTurn);
    let mut state = result.state;
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "the before-attack cancel window suspends the loop: {:?}",
        result.outcome
    );
    // No damage dealt yet (the attack hasn't resolved).
    assert_eq!(state.investigators[&inv_id].damage(), 0);

    // Play Dodge (the single offered candidate) → cancel the attack.
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    state = result.state;

    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "the cancelled attack dealt no damage"
    );
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, Event::DamageTaken { .. })),
        "a cancelled attack deals no damage: {:?}",
        result.events
    );
    // The attacker still exhausts (RR p.6 + p.25) — asserted via the event,
    // since the enemy-phase cascade re-readies it at upkeep step 4.3.
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::EnemyExhausted { enemy } if *enemy == enemy_id
        )),
        "the attacker still exhausts after a cancelled attack: {:?}",
        result.events
    );
    // Dodge left hand and went to discard (a played event).
    assert!(
        !state.investigators[&inv_id]
            .hand
            .contains(&CardCode::new(DODGE)),
        "Dodge left hand"
    );
    assert!(
        state.investigators[&inv_id]
            .discard
            .contains(&CardCode::new(DODGE)),
        "Dodge is in the discard pile after being played"
    );
}

#[test]
fn declining_the_before_attack_window_lets_the_attack_land() {
    let (state, inv_id, enemy_id) = dodge_state();

    let result = take_turn_action(state, &TurnAction::EndTurn);
    let mut state = result.state;
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    // Skip the window → the attack resolves normally.
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );
    state = result.state;

    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::DamageTaken { investigator, amount: 2 } if *investigator == inv_id
        )),
        "the un-cancelled attack dealt its 2 damage: {:?}",
        result.events
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        2,
        "investigator carries the 2 damage"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::EnemyExhausted { enemy } if *enemy == enemy_id
        )),
        "attacker exhausts after attacking: {:?}",
        result.events
    );
    // Dodge was never played: still in hand, nothing in discard.
    assert!(
        state.investigators[&inv_id]
            .hand
            .contains(&CardCode::new(DODGE)),
        "Dodge stays in hand when the window is declined"
    );
}
