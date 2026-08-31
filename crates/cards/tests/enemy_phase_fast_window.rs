//! The Enemy phase's per-investigator `BeforeInvestigatorAttacked` player
//! window **pauses** when a Fast play is eligible (#842), driven end-to-end
//! through the public [`apply`] API with the real card corpus installed.
//!
//! Rules Reference, Appendix II — the flow chart's Enemy phase, verbatim:
//!
//! ```text
//! 3.2 Hunter enemies move.
//! [free] PLAYER WINDOW
//! 3.3 Next investigator resolves engaged enemy attacks. If an investigator
//!     has not yet resolved enemy attacks this phase, return to previous
//!     player window. …
//! ```
//!
//! `game-core`'s own unit test for that window covers the **Skip-resume** path
//! against a synthetic (empty) window, because the engine crate cannot install
//! `cards::REGISTRY` and so cannot make anything Fast-eligible. The other half
//! — the window pausing for input instead of auto-skipping — needs a real
//! eligible play, which is what this file supplies.
//!
//! The fixture is Beat Cop 01018's zero-action ability rather than the Fast
//! event the ticket imagined: every Fast **event** in the Core corpus is either
//! a reaction (Dodge 01023 — `check_play_card` refuses a standalone play) or
//! carries *"Play only during your turn"* (Working a Hunch 01037), and the
//! Enemy phase is nobody's turn. A `[fast]` ability has no such restriction and
//! is enumerated by the same `enumerate_fast_plays` scan that gates the window.
//!
//! Beat Cop 01018, verbatim from the pinned snapshot:
//!
//! ```text
//! You get +1 [combat].
//! [fast] Discard Beat Cop: Deal 1 damage to an enemy at your location.
//! ```
//!
//! with its ruling, verbatim (<https://arkhamdb.com/card/01018>):
//!
//! > You cannot use Beat Cop's ability after you assign lethal damage/horror to
//! > him (there is no Player Window to use [fast]free abilities in between
//! > assigning and applying damage).
//!
//! — which is the same claim from the other side: whether a `[fast]` ability
//! can be used is exactly the question of whether a player window stands open
//! there. Step 3.3's does.

use game_core::engine::{apply, EngineOutcome, InputKind, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, Continuation, Enemy, EnemyId, FastActorScope,
    FastWindowKind, InvestigationResume, InvestigatorId, LocationId, Phase, PhaseStep,
};
use game_core::test_support::{
    take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder,
};
use game_core::{Action, GameState, InputResponse, PlayerAction, TurnAction};

/// Beat Cop (01018): Guardian Ally, `[fast]` *"Discard Beat Cop: Deal 1 damage
/// to an enemy at your location."*
const BEAT_COP: &str = "01018";
const BEAT_COP_INSTANCE: CardInstanceId = CardInstanceId(1);

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// A ready enemy engaged with `inv` at `loc`, dealing 1 damage. `max_health` is
/// the caller's to choose: 2 leaves it alive through a single point of Beat Cop
/// damage, 1 lets the Fast play defeat it before it ever attacks.
fn engaged_attacker(inv: InvestigatorId, loc: LocationId, max_health: u8) -> Enemy {
    let mut e = test_enemy(7, "Attacker");
    e.max_health = max_health;
    e.attack_damage = 1;
    e.current_location = Some(loc);
    e.engaged_with = Some(inv);
    e
}

/// Mid-Investigation state for one investigator engaged by one ready attacker,
/// poised so that `EndTurn` cascades Investigation → Enemy and runs into the
/// step-3.3 window. `beat_cop` decides whether anything is Fast-eligible there.
fn board(beat_cop: bool, enemy_health: u8) -> (GameState, InvestigatorId, EnemyId) {
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(101);

    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    // A real investigator code so max_health()/max_sanity() resolve against the
    // installed corpus; TEST_INV lives only in game-core's test registry.
    // Skids O'Toole (01003, 8/6) — no implemented abilities of his own.
    inv.investigator_card.code = CardCode::new("01003");
    if beat_cop {
        inv.cards_in_play.push(CardInPlay::enter_play(
            CardCode::new(BEAT_COP),
            BEAT_COP_INSTANCE,
        ));
    }
    // Something for the round-ending cascade's Upkeep step-4.4 draw to take.
    inv.deck = vec![CardCode::new("01088")];

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(101, "Study"))
        .with_investigator(inv)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_enemy(engaged_attacker(inv_id, loc_id, enemy_health))
        // Mid-Investigation invariant (slice 1a): the EndTurn cascade pops the
        // InvestigationPhase anchor at investigation_phase_end.
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        })
        // Open-turn invariant (slice 2a-i, #393): the InvestigatorTurn frame the
        // EndTurn cascade pops before advancing into the Enemy phase.
        .with_investigator_turn(inv_id)
        .build();
    (state, inv_id, EnemyId(7))
}

/// `EndTurn`, which cascades out of the Investigation phase and into the Enemy
/// phase's step-3.3 loop.
fn end_turn(state: GameState) -> game_core::ApplyResult {
    take_turn_action(state, &TurnAction::EndTurn)
}

#[test]
fn before_investigator_attacked_pauses_when_a_fast_play_is_eligible() {
    let (state, _, enemy_id) = board(true, 2);
    let result = end_turn(state);

    assert_eq!(
        result.state.phase,
        Phase::Enemy,
        "the EndTurn cascade should stop inside the Enemy phase, not run through it; \
         events = {:?}",
        result.events
    );
    assert_eq!(
        result.state.open_windows(),
        vec![&Continuation::FastWindow {
            candidates: Vec::new(),
            fast_actors: FastActorScope::Any,
            kind: FastWindowKind::Phase(PhaseStep::BeforeInvestigatorAttacked),
        }],
        "the step-3.3 window is the one left standing open",
    );

    let EngineOutcome::AwaitingInput { ref request, .. } = result.outcome else {
        panic!(
            "the window should pause for input rather than auto-skipping, got {:?}",
            result.outcome
        );
    };
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert!(
        request.skippable,
        "a player window is passable: {request:?}"
    );
    assert_eq!(
        request
            .options
            .iter()
            .map(|o| o.target.clone())
            .collect::<Vec<_>>(),
        vec![Some(OptionTarget::CardInstance(BEAT_COP_INSTANCE))],
        "Beat Cop's [fast] ability is the one eligible play on this board: {request:?}",
    );

    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, Event::DamageTaken { .. })),
        "the pause is *before* the attack — nothing has been dealt yet; events = {:?}",
        result.events
    );
    assert!(
        !result.state.enemies[&enemy_id].exhausted,
        "the attacker has not attacked, so it has not exhausted",
    );
}

#[test]
fn skipping_the_pause_resumes_into_the_attack() {
    let (state, inv_id, enemy_id) = board(true, 2);
    let paused = end_turn(state);
    let resumed = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );

    // Passing the window drops straight into step 3.3's attack, which stops to
    // ask where its damage goes — Beat Cop is a 2-health Ally in play and so a
    // legal soak target beside the investigator himself.
    let EngineOutcome::AwaitingInput { ref request, .. } = resumed.outcome else {
        panic!(
            "expected the attack's damage-assignment prompt, got {:?}",
            resumed.outcome
        );
    };
    let to_investigator = request
        .options
        .iter()
        .find(|o| o.label == "Investigator")
        .expect("the attacked investigator is always an assignment target")
        .id;

    let assigned = apply(
        resumed.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(to_investigator),
        }),
    );
    assert!(
        assigned.events.iter().any(|e| matches!(
            e,
            Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
        )),
        "passing the window resolves step 3.3's attack; events = {:?}",
        assigned.events
    );
    assert!(
        assigned
            .events
            .iter()
            .any(|e| matches!(e, Event::EnemyExhausted { enemy } if *enemy == enemy_id)),
        "the attacker exhausts on completing its attack; events = {:?}",
        assigned.events
    );
}

#[test]
fn taking_the_fast_play_resolves_before_the_attack() {
    // The attacker dies to Beat Cop's single point of damage, so "the window
    // opens before the attack" is provable by consequence and not only by
    // event order: a defeated enemy never attacks.
    let (state, inv_id, enemy_id) = board(true, 1);
    let paused = end_turn(state);
    let EngineOutcome::AwaitingInput { ref request, .. } = paused.outcome else {
        panic!("expected the step-3.3 pause, got {:?}", paused.outcome);
    };
    let option = request
        .options
        .iter()
        .find(|o| o.target == Some(OptionTarget::CardInstance(BEAT_COP_INSTANCE)))
        .expect("Beat Cop's zero-action ability is the offered play")
        .id;

    let played = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(option),
        }),
    );

    assert!(
        !played.state.enemies.contains_key(&enemy_id),
        "Beat Cop's 1 damage defeats the 1-health attacker; enemies = {:?}",
        played.state.enemies
    );
    assert!(
        !played.events.iter().any(
            |e| matches!(e, Event::DamageTaken { investigator, .. } if *investigator == inv_id)
        ),
        "with its attacker gone, the investigator takes no attack damage; events = {:?}",
        played.events
    );
    let inv = &played.state.investigators[&inv_id];
    assert!(
        inv.cards_in_play.is_empty(),
        "Beat Cop left play as its own cost; cards_in_play = {:?}",
        inv.cards_in_play
    );
    assert!(
        inv.discard.contains(&CardCode::new(BEAT_COP)),
        "…and landed in the discard pile; discard = {:?}",
        inv.discard
    );
}

#[test]
fn the_window_auto_skips_when_nothing_is_fast_eligible() {
    // The control for the three above: the same board without Beat Cop makes
    // the same window auto-skip, so the pause is attributable to Fast
    // eligibility and not to the phase step itself.
    let (state, inv_id, enemy_id) = board(false, 2);
    let result = end_turn(state);

    assert!(
        !result
            .state
            .open_windows()
            .iter()
            .any(|w| matches!(w, Continuation::FastWindow { .. })),
        "no window is left open on the auto-skip path; windows = {:?}",
        result.state.open_windows()
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
        )),
        "the attack resolves in the same cascade; events = {:?}",
        result.events
    );
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, Event::EnemyExhausted { enemy } if *enemy == enemy_id)),
        "…and the attacker exhausts; events = {:?}",
        result.events
    );
}
