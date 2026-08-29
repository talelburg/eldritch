//! #774 integration: the Parlor 01115's movement barrier, end to end against
//! the real `cards::REGISTRY`.
//!
//! ## Verified card text (`data/arkhamdb-snapshot`, 2026-08-29)
//!
//! **Parlor (01115)**, `back_text` verbatim
//! (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
//!
//! > The entrance to the Parlor is blocked by a darkly glowing unfathomable
//! > barrier. You cannot move into the Parlor.
//!
//! **The Barrier (01109)**, `back_text` verbatim, is what lifts it:
//!
//! > The barrier blocking passage into the parlor has vanished. Reveal the
//! > Parlor.
//!
//! So the barrier is a property of the Parlor being **unrevealed**, and the
//! act-2 advance lifts it by revealing. Nothing else on either card conditions
//! it.
//!
//! ## The ruling the two-predicate split rests on
//!
//! `data/arkhamdb-faq/core/01115.md`, the card's only ruling:
//!
//! > **Q:** Can enemies move into Parlor even when investigators are blocked by
//! > the barrier? **A:** Yes; in The Gathering scenario, enemies can move into
//! > The Parlor even when the investigators are blocked by the barrier.
//! > (March 2024)
//!
//! (<https://arkhamdb.com/card/01115>)
//!
//! That is why `Restriction::InvestigatorMovementBlocked` is a second
//! restriction beside Barricade 01038's `EnemyMovementBlocked` rather than one
//! shared block, and both halves are asserted below: an enemy walks into the
//! unrevealed Parlor, and a non-Elite enemy still cannot walk into a
//! Barricaded location.
//!
//! ## Where the block bites
//!
//! `data/rules-reference/rules/glossary/Nearest.md`: *"Nearest refers to the
//! entity of the specified kind at a location that can be reached in the fewest
//! number of connections, **even if one or more of those connections are
//! blocked by another card ability**."* — so the connection stays on the map and
//! the block is checked against the step, never baked into the graph (#651).
//!
//! Lives in `crates/cards/tests/` because every assertion needs the real
//! corpus: the Parlor's front and back abilities, resolved through
//! `cards::REGISTRY`.
//!
//! Own process → installs `cards::REGISTRY`.

use game_core::engine::{legal_actions, EngineOutcome, TurnAction};
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, Enemy, InvestigatorId, Location, LocationId, Phase,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, take_turn_action, test_enemy, test_investigator, test_location,
    GameStateBuilder,
};
use game_core::{enemy_can_enter_location, GameState};

/// The Parlor.
const PARLOR_CODE: &str = "01115";
/// Barricade 01038 — the enemy-side block, for the contrast case.
const BARRICADE: &str = "01038";
/// Ghoul Minion 01160 — Humanoid. Monster. Ghoul., non-Elite.
const GHOUL_MINION: &str = "01160";

const INV: InvestigatorId = InvestigatorId(1);
/// The Hallway — where act 1 leaves the investigators.
const HALLWAY: LocationId = LocationId(1);
/// The Parlor.
const PARLOR: LocationId = LocationId(2);
/// The Attic — an ordinary connected neighbour, to prove the barrier is not a
/// blanket ban on moving.
const ATTIC: LocationId = LocationId(3);
const ATT_INST: CardInstanceId = CardInstanceId(900);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// The Parlor as the board actually holds it: the real card code (so the
/// registry resolves its front and back), `revealed` set by the caller.
fn parlor(revealed: bool) -> Location {
    let mut loc = test_location(PARLOR.0, "Parlor");
    loc.code = CardCode::new(PARLOR_CODE);
    loc.revealed = revealed;
    loc.connections = vec![HALLWAY];
    loc
}

/// Hallway ─ Parlor and Hallway ─ Attic, with the investigator in the Hallway
/// and an open Investigation-phase turn.
fn board(parlor_revealed: bool) -> GameState {
    let mut hallway = test_location(HALLWAY.0, "Hallway");
    hallway.connections = vec![PARLOR, ATTIC];
    let mut attic = test_location(ATTIC.0, "Attic");
    attic.connections = vec![HALLWAY];

    let mut inv = test_investigator(INV.0);
    inv.current_location = Some(HALLWAY);

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_location(hallway)
        .with_location(parlor(parlor_revealed))
        .with_location(attic)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .build()
}

fn move_to(destination: LocationId) -> TurnAction {
    TurnAction::Move {
        investigator: INV,
        destination,
    }
}

// ---------------------------------------------------------------------------
// The side in effect
// ---------------------------------------------------------------------------

/// The mechanism itself: a location's `back_text` abilities apply while it is
/// unrevealed and its front's while it is revealed. Read through the two
/// surfaces that observe the difference — the barrier (back) and the Resign
/// **action designator** (front, #644/#805).
#[test]
fn unrevealed_parlor_shows_its_back_and_revealed_shows_its_front() {
    // Unrevealed: the barrier is in effect, the Resign is not offered.
    let state = board(false);
    let actions = legal_actions(&state);
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, TurnAction::ActivateAbility { .. })),
        "an unrevealed Parlor shows only its back, which prints no activated \
         ability — the front's [action] Resign must not be reachable; got {actions:?}",
    );

    // Revealed: the front is in effect, so the Resign is offered — and the
    // barrier is gone (asserted on the move, below).
    let mut inv = test_investigator(INV.0);
    inv.current_location = Some(PARLOR);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_location(parlor(true))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .build();
    let actions = legal_actions(&state);
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, TurnAction::ActivateAbility { .. })),
        "a revealed Parlor shows its front, whose [action] Resign is reachable \
         by co-location (ADR 0010); got {actions:?}",
    );
}

// ---------------------------------------------------------------------------
// The investigator side
// ---------------------------------------------------------------------------

/// *"You cannot move into the Parlor."* — the destination is not offered, and
/// the Attic beside it still is, so the barrier is a per-destination block and
/// not a suppression of the Move action.
#[test]
fn move_does_not_offer_the_unrevealed_parlor_but_still_offers_its_neighbours() {
    let state = board(false);
    let actions = legal_actions(&state);
    assert!(
        !actions.contains(&move_to(PARLOR)),
        "the unrevealed Parlor's barrier blocks the move; got {actions:?}",
    );
    assert!(
        actions.contains(&move_to(ATTIC)),
        "every other connected destination is unaffected; got {actions:?}",
    );
}

/// The `apply` seam refuses it too: a client that never read the menu, or read
/// a stale one, is still refused rather than walking through the barrier.
#[test]
fn submitting_a_move_into_the_unrevealed_parlor_is_rejected() {
    // Submitted straight at the `apply` seam, bypassing the menu — which is
    // exactly the case the handler's own check exists for.
    let r = dispatch_turn_action_unchecked(board(false), &move_to(PARLOR));
    assert!(
        matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "a submitted move into the barrier is Rejected; got {:?}",
        r.outcome,
    );
    assert_eq!(
        r.state.investigators[&INV].current_location,
        Some(HALLWAY),
        "a rejected move leaves the investigator where they stood",
    );
    assert_eq!(
        r.state.investigators[&INV].actions_remaining,
        board(false).investigators[&INV].actions_remaining,
        "and spends nothing — the barrier is checked before the action is charged",
    );
}

/// Act 01109b's *"Reveal the Parlor"* lifts it: same board, same connection,
/// `revealed` flipped, and the move is legal and lands.
#[test]
fn revealing_the_parlor_makes_the_move_legal() {
    let state = board(true);
    assert!(
        legal_actions(&state).contains(&move_to(PARLOR)),
        "a revealed Parlor is an ordinary destination",
    );

    let r = take_turn_action(state, &move_to(PARLOR));
    assert!(
        !matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "the move resolves; got {:?}",
        r.outcome,
    );
    assert_eq!(
        r.state.investigators[&INV].current_location,
        Some(PARLOR),
        "the investigator is in the Parlor",
    );
}

/// The reveal is what changes the answer, and nothing else about the board is:
/// flipping `revealed` on the very state whose move was refused — which is all
/// act 01109b's `reveal_location` does to it — makes the same move legal.
#[test]
fn the_reveal_alone_flips_the_answer() {
    let mut r = dispatch_turn_action_unchecked(board(false), &move_to(PARLOR));
    assert!(matches!(r.outcome, EngineOutcome::Rejected { .. }));

    r.state
        .locations
        .get_mut(&PARLOR)
        .expect("the Parlor is on the board")
        .revealed = true;
    assert!(
        legal_actions(&r.state).contains(&move_to(PARLOR)),
        "nothing moved but the Parlor's `revealed` flag, and the barrier is gone",
    );
}

// ---------------------------------------------------------------------------
// The enemy side — 01115's ruling
// ---------------------------------------------------------------------------

/// *"enemies can move into The Parlor even when the investigators are blocked
/// by the barrier."* The Parlor's restriction is investigator-side only, so the
/// enemy predicate answers `true` on the same unrevealed location that refuses
/// the investigator.
#[test]
fn an_enemy_can_enter_the_unrevealed_parlor() {
    let state = board(false);
    let mut ghoul: Enemy = test_enemy(1, "Ghoul");
    ghoul.code = CardCode::new(GHOUL_MINION);
    ghoul.traits = vec!["Humanoid".into(), "Monster".into(), "Ghoul".into()];
    ghoul.current_location = Some(HALLWAY);

    assert!(
        enemy_can_enter_location(&state, &ghoul, PARLOR),
        "01115's ruling: the barrier stops investigators, not enemies",
    );
    assert!(
        !legal_actions(&state).contains(&move_to(PARLOR)),
        "and the investigator is still blocked on the same board — the point of \
         the ruling is that the two answers differ",
    );
}

/// The converse, so the split is proved in both directions: Barricade 01038 is
/// enemy-side only. A non-Elite enemy cannot enter the attached location, and
/// an investigator still can.
#[test]
fn a_barricaded_location_blocks_a_non_elite_enemy_and_not_an_investigator() {
    let mut state = board(true);
    state
        .locations
        .get_mut(&ATTIC)
        .expect("Attic is on the board")
        .attachments
        .push(CardInPlay::enter_play(CardCode::new(BARRICADE), ATT_INST));

    let mut ghoul: Enemy = test_enemy(1, "Ghoul");
    ghoul.code = CardCode::new(GHOUL_MINION);
    ghoul.traits = vec!["Humanoid".into(), "Monster".into(), "Ghoul".into()];
    ghoul.current_location = Some(HALLWAY);

    assert!(
        !enemy_can_enter_location(&state, &ghoul, ATTIC),
        "*\"Non-Elite enemies cannot move into attached location.\"*",
    );
    assert!(
        legal_actions(&state).contains(&move_to(ATTIC)),
        "Barricade says nothing about investigators, so the move stays legal",
    );
}
