//! #323 integration: Barricade 01038's attach / non-Elite movement block /
//! leave-location self-discard, end-to-end against the real `cards::REGISTRY`.
//!
//! Since #721 the self-discard is declared in the **`when`** cell of the
//! `LeftLocation` condition, as the card prints it — *"**Forced** - When an
//! investigator leaves attached location: Discard Barricade."* — so it resolves
//! before the departure lands. That ordering spans the move handler, the
//! registry lookup and the card's own effect, which is why it is proved here
//! rather than in `game-core`.
//!
//! The movement tests drive the real Enemy phase via `EndTurn` (hunter
//! movement is step 3.2) — the same entry `dodge.rs` uses.
//!
//! Own process → installs `cards::REGISTRY`.

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, Enemy, EnemyId, InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{
    take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder,
};
use game_core::{assert_event, assert_event_sequence, assert_no_event, TurnAction};

const BARRICADE: &str = "01038";
const GHOUL_PRIEST: &str = "01116"; // Humanoid. Monster. Ghoul. Elite. + Hunter
const GHOUL_MINION: &str = "01160"; // Humanoid. Monster. Ghoul. (non-Elite)
const INV: InvestigatorId = InvestigatorId(1);
const A: LocationId = LocationId(1);
const B: LocationId = LocationId(2);
const ATT_INST: CardInstanceId = CardInstanceId(900);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// A ready, unengaged ghoul (code `code`) at location `at`, with the printed
/// traits of that card (Elite-ness drives the movement block, read off
/// `Enemy.traits` as spawns populate it). Hunter-ness and engagement are the
/// caller's to set — the two scenarios here want opposite answers.
fn ghoul(id: u32, code: &str, at: LocationId) -> Enemy {
    let mut e = test_enemy(id, "Ghoul");
    e.code = CardCode::new(code);
    e.traits = if code == GHOUL_PRIEST {
        vec![
            "Humanoid".into(),
            "Monster".into(),
            "Ghoul".into(),
            "Elite".into(),
        ]
    } else {
        vec!["Humanoid".into(), "Monster".into(), "Ghoul".into()]
    };
    e.current_location = Some(at);
    e.engaged_with = None;
    e.exhausted = false;
    e
}

#[test]
fn playing_barricade_attaches_one_card_and_does_not_discard_the_event() {
    let mut inv = test_investigator(1);
    inv.current_location = Some(A);
    inv.hand = vec![CardCode::new(BARRICADE)];
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_location(test_location(1, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .build();

    let r = take_turn_action(
        state,
        &TurnAction::PlayCard {
            investigator: INV,
            hand_index: 0,
        },
    );
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    // Exactly one Barricade: attached to the location, none in hand/discard.
    assert_eq!(
        r.state.locations[&A]
            .attachments
            .iter()
            .filter(|c| c.code == CardCode::new(BARRICADE))
            .count(),
        1,
        "attached once",
    );
    assert!(r.state.investigators[&INV].hand.is_empty(), "left hand");
    assert!(
        r.state.investigators[&INV].discard.is_empty(),
        "not discarded (re-homed, not duplicated)",
    );
    assert_event!(r.events, Event::CardAttachedToLocation { .. });
}

/// Linear map A—B with a Barricade attached at B, the investigator at `inv_at`,
/// and `enemy` on the board.
fn map_with_barricade_at_b(inv_at: LocationId, enemy: Enemy) -> game_core::GameState {
    let mut inv = test_investigator(1);
    // Use a real investigator code so max_health()/max_sanity() can read from
    // the installed cards registry; TEST_INV is only in the game-core test
    // registry (#448 cp2a). Skids O'Toole (01003, 8/6) — no implemented abilities.
    inv.investigator_card.code = CardCode::new("01003");
    inv.current_location = Some(inv_at);
    let mut a = test_location(1, "A");
    a.connections = vec![B];
    let mut b = test_location(2, "B");
    b.connections = vec![A];
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_location(a)
        .with_location(b)
        .with_enemy(enemy)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        // Mid-Investigation invariant (slice 1a): the EndTurn cascade pops the
        // InvestigationPhase anchor at investigation_phase_end.
        .with_phase_anchor(game_core::state::Continuation::InvestigationPhase {
            resume: game_core::state::InvestigationResume::TurnBegins,
        })
        // Open-turn invariant (slice 2a-i, #393): the InvestigatorTurn frame the
        // EndTurn cascade pops before rotating / cascading.
        .with_investigator_turn(INV)
        .build();
    state
        .locations
        .get_mut(&B)
        .unwrap()
        .attachments
        .push(CardInPlay::enter_play(CardCode::new(BARRICADE), ATT_INST));
    state
}

/// The hunter-movement scenario: the investigator (prey) at B, a hunter at A,
/// driven via `EndTurn` into the Enemy phase.
fn hunter_at_a_moving_toward_b(enemy_code: &str) -> game_core::GameState {
    let mut enemy = ghoul(100, enemy_code, A);
    enemy.hunter = true;
    map_with_barricade_at_b(B, enemy)
}

#[test]
fn non_elite_hunter_cannot_enter_the_barricaded_location() {
    let r = take_turn_action(
        hunter_at_a_moving_toward_b(GHOUL_MINION),
        &TurnAction::EndTurn,
    );
    assert_eq!(
        r.state.enemies[&EnemyId(100)].current_location,
        Some(A),
        "non-Elite hunter stayed (only path is into the barricaded location)",
    );
}

#[test]
fn elite_hunter_enters_the_barricaded_location() {
    let r = take_turn_action(
        hunter_at_a_moving_toward_b(GHOUL_PRIEST),
        &TurnAction::EndTurn,
    );
    assert_eq!(
        r.state.enemies[&EnemyId(100)].current_location,
        Some(B),
        "Elite hunter ignores the barricade",
    );
}

/// Linear map A—B with a Barricade attached at B, the investigator at A, and a
/// ready enemy (code `enemy_code`, 1 damage) engaged with them at A. The move
/// A→B is the drag-along case: the engaged enemy would ride along, but B is
/// barricaded.
fn engaged_map_with_barricade_at_b(enemy_code: &str) -> game_core::GameState {
    let mut enemy = ghoul(100, enemy_code, A);
    enemy.engaged_with = Some(INV);
    enemy.attack_damage = 1;
    enemy.attack_horror = 0;
    map_with_barricade_at_b(A, enemy)
}

/// Barricade 01038: "Non-[[Elite]] enemies cannot move into attached location."
/// Its ruling (<https://arkhamdb.com/card/01038>): "If an investigator that is
/// engaged with an enemy moves to a Barricaded location, the engaged enemy will
/// disengage and remain in the investigator's previous location (after making an
/// attack of opportunity)."
#[test]
fn engaged_non_elite_enemy_disengages_and_stays_behind_at_a_barricade() {
    let r = take_turn_action(
        engaged_map_with_barricade_at_b(GHOUL_MINION),
        &TurnAction::Move {
            investigator: INV,
            destination: B,
        },
    );
    assert!(!matches!(r.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(
        r.state.investigators[&INV].current_location,
        Some(B),
        "the investigator still moves",
    );
    let enemy = &r.state.enemies[&EnemyId(100)];
    assert_eq!(enemy.current_location, Some(A), "enemy stayed behind");
    assert_eq!(enemy.engaged_with, None, "engagement broke");
    assert_event!(
        r.events,
        Event::EnemyDisengaged { enemy, investigator }
            if *enemy == EnemyId(100) && *investigator == INV
    );
}

/// The attack of opportunity resolves *before* the disengage (per the ruling's
/// parenthetical), which in turn precedes the investigator's move.
#[test]
fn the_attack_of_opportunity_resolves_before_the_disengage() {
    let r = take_turn_action(
        engaged_map_with_barricade_at_b(GHOUL_MINION),
        &TurnAction::Move {
            investigator: INV,
            destination: B,
        },
    );
    assert_event_sequence!(
        r.events,
        Event::DamageTaken { .. },
        Event::EnemyDisengaged { .. },
        Event::InvestigatorMoved { .. },
    );
    assert_eq!(r.state.investigators[&INV].damage(), 1, "AoO still landed");
}

/// Barricade names non-Elite only, so an Elite enemy is dragged along as before.
#[test]
fn engaged_elite_enemy_is_dragged_into_the_barricaded_location() {
    let r = take_turn_action(
        engaged_map_with_barricade_at_b(GHOUL_PRIEST),
        &TurnAction::Move {
            investigator: INV,
            destination: B,
        },
    );
    assert!(!matches!(r.outcome, EngineOutcome::Rejected { .. }));
    let enemy = &r.state.enemies[&EnemyId(100)];
    assert_eq!(enemy.current_location, Some(B), "Elite enemy followed");
    assert_eq!(enemy.engaged_with, Some(INV), "still engaged");
}

/// Control: with no Barricade at the destination, the non-Elite enemy is dragged
/// along and stays engaged.
#[test]
fn engaged_non_elite_enemy_follows_when_the_destination_is_unbarricaded() {
    let mut state = engaged_map_with_barricade_at_b(GHOUL_MINION);
    state.locations.get_mut(&B).unwrap().attachments.clear();
    let r = take_turn_action(
        state,
        &TurnAction::Move {
            investigator: INV,
            destination: B,
        },
    );
    assert!(!matches!(r.outcome, EngineOutcome::Rejected { .. }));
    let enemy = &r.state.enemies[&EnemyId(100)];
    assert_eq!(enemy.current_location, Some(B), "enemy followed");
    assert_eq!(enemy.engaged_with, Some(INV), "still engaged");
}

#[test]
fn leaving_the_barricaded_location_discards_barricade() {
    let mut inv = test_investigator(1);
    inv.current_location = Some(A);
    let mut a = test_location(1, "A");
    a.connections = vec![B];
    let mut b = test_location(2, "B");
    b.connections = vec![A];
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_location(a)
        .with_location(b)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .build();
    state
        .locations
        .get_mut(&A)
        .unwrap()
        .attachments
        .push(CardInPlay::enter_play(CardCode::new(BARRICADE), ATT_INST));

    let r = take_turn_action(
        state,
        &TurnAction::Move {
            investigator: INV,
            destination: B,
        },
    );
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(
        r.state.locations[&A].attachments.is_empty(),
        "Barricade discarded on leave",
    );
    assert!(
        r.state.investigators[&INV]
            .discard
            .contains(&CardCode::new(BARRICADE)),
        "to the owner's player discard",
    );
}

/// The `when` cell, end to end: Barricade discards **before** the departure
/// lands (#721). `glossary/When.md` pins the printed word to *"the moment
/// immediately after the specified timing point or triggering condition
/// initiates, but before its impact upon the game state resolves"*, and the
/// departure's impact is the investigator arriving — `InvestigatorMoved`.
///
/// Before the migration `LeftLocation` was caller-owned: the move handler
/// assigned the location and *then* emitted, so the card was tagged `After` and
/// discarded with the investigator already at B.
#[test]
fn barricade_discards_before_the_departure_lands() {
    let r = take_turn_action(
        map_leaving_barricaded_a(None),
        &TurnAction::Move {
            investigator: INV,
            destination: B,
        },
    );
    assert_event_sequence!(
        r.events,
        Event::CardDiscarded { .. },
        Event::InvestigatorMoved { .. },
    );
    assert!(
        r.state.locations[&A].attachments.is_empty(),
        "Barricade discarded on leave",
    );
    assert_eq!(
        r.state.investigators[&INV].current_location,
        Some(B),
        "and the departure still lands",
    );
}

/// The drag-along is part of the departure's impact, so it happens at the
/// resolve step — after the discard, not before it. An engaged non-Elite enemy
/// still follows into an unbarricaded destination on the very move that
/// discards Barricade (`glossary/Enemy_Engagement.md`: such an enemy *"remains
/// engaged and moves to the new location simultaneously with the
/// investigator"*).
#[test]
fn an_engaged_enemy_still_follows_the_move_that_discards_barricade() {
    let mut enemy = ghoul(100, GHOUL_MINION, A);
    enemy.engaged_with = Some(INV);
    enemy.attack_damage = 0;
    enemy.attack_horror = 0;
    let r = take_turn_action(
        map_leaving_barricaded_a(Some(enemy)),
        &TurnAction::Move {
            investigator: INV,
            destination: B,
        },
    );
    assert_event_sequence!(
        r.events,
        Event::CardDiscarded { .. },
        Event::InvestigatorMoved { .. },
    );
    let enemy = &r.state.enemies[&EnemyId(100)];
    assert_eq!(enemy.current_location, Some(B), "the enemy followed");
    assert_eq!(enemy.engaged_with, Some(INV), "still engaged");
    assert_no_event!(r.events, Event::EnemyDisengaged { .. });
}

/// The other half of the ruling on the very move that discards Barricade: with
/// a Barricade at **both** ends, leaving A discards A's copy in the `when` cell
/// and B's copy still blocks, so the engaged non-Elite enemy disengages and
/// stays behind — *"the engaged enemy will disengage and remain in the
/// investigator's previous location"* (<https://arkhamdb.com/card/01038>).
///
/// The block is read at the resolve step, *after* the `when` cell has run, so
/// this is the case that proves the discard does not disturb it.
#[test]
fn an_engaged_enemy_still_disengages_on_the_move_that_discards_barricade() {
    let mut enemy = ghoul(100, GHOUL_MINION, A);
    enemy.engaged_with = Some(INV);
    enemy.attack_damage = 0;
    enemy.attack_horror = 0;
    let mut state = map_leaving_barricaded_a(Some(enemy));
    state
        .locations
        .get_mut(&B)
        .unwrap()
        .attachments
        .push(CardInPlay::enter_play(
            CardCode::new(BARRICADE),
            CardInstanceId(901),
        ));

    let r = take_turn_action(
        state,
        &TurnAction::Move {
            investigator: INV,
            destination: B,
        },
    );
    assert_event_sequence!(
        r.events,
        Event::CardDiscarded { .. },
        Event::EnemyDisengaged { .. },
        Event::InvestigatorMoved { .. },
    );
    let enemy = &r.state.enemies[&EnemyId(100)];
    assert_eq!(enemy.current_location, Some(A), "the enemy stayed behind");
    assert_eq!(enemy.engaged_with, None, "engagement broke");
    assert!(
        r.state.locations[&A].attachments.is_empty(),
        "A's Barricade discarded on leave",
    );
    assert_eq!(
        r.state.locations[&B].attachments.len(),
        1,
        "B's Barricade is untouched — nobody left B",
    );
}

/// Linear map A—B with a Barricade attached at **A**, the investigator there,
/// and `enemy` optionally on the board. The move A→B is the one that fires the
/// card's own forced self-discard.
fn map_leaving_barricaded_a(enemy: Option<Enemy>) -> game_core::GameState {
    let mut inv = test_investigator(1);
    // A real investigator code, so max_health()/max_sanity() read from the
    // installed cards registry (see `map_with_barricade_at_b`).
    inv.investigator_card.code = CardCode::new("01003");
    inv.current_location = Some(A);
    let mut a = test_location(1, "A");
    a.connections = vec![B];
    let mut b = test_location(2, "B");
    b.connections = vec![A];
    let mut builder = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_location(a)
        .with_location(b)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV);
    if let Some(enemy) = enemy {
        builder = builder.with_enemy(enemy);
    }
    let mut state = builder.build();
    state
        .locations
        .get_mut(&A)
        .unwrap()
        .attachments
        .push(CardInPlay::enter_play(CardCode::new(BARRICADE), ATT_INST));
    state
}

// ---------------------------------------------------------------------------
// #651: a movement block never changes *who is nearest* or *what the shortest
// path is* — it only turns the compelled step into a non-move.
//
// `data/rules-reference/rules/glossary/Nearest.md`: *"Nearest refers to the
// entity of the specified kind at a location that can be reached in the fewest
// number of connections, **even if one or more of those connections are blocked
// by another card ability**. The path to the nearest entity is the 'shortest'
// path to that entity."*
//
// `data/rules-reference/rules/glossary/Hunter.md`: *"If a hunter enemy would be
// compelled to a location to which the move is blocked by a card ability, the
// enemy does not move."*
// ---------------------------------------------------------------------------

const INV2: InvestigatorId = InvestigatorId(2);

/// An investigator at `at` with a real card code, so `max_health()` /
/// `max_sanity()` read from the installed registry (see
/// `map_with_barricade_at_b`).
fn inv_at(id: u32, at: LocationId) -> game_core::state::Investigator {
    let mut inv = test_investigator(id);
    inv.investigator_card.code = CardCode::new("01003");
    inv.current_location = Some(at);
    inv
}

/// A location with the given id, name and connections.
fn linked(id: u32, name: &str, connections: &[LocationId]) -> game_core::state::Location {
    let mut loc = test_location(id, name);
    loc.connections = connections.to_vec();
    loc
}

/// A ready, unengaged, non-Elite hunter at `at` — the mover every scenario
/// below shares, since it is the non-Elite case the barricade actually stops.
fn hunter_at(at: LocationId) -> Enemy {
    let mut enemy = ghoul(100, GHOUL_MINION, at);
    enemy.hunter = true;
    enemy
}

/// Assemble a board out of ready-made locations / investigators / one hunter,
/// staging the mid-Investigation invariants the `EndTurn` cascade expects, and
/// attaching a Barricade at each of `barricaded`.
fn board(
    locations: Vec<game_core::state::Location>,
    investigators: Vec<game_core::state::Investigator>,
    hunter: Enemy,
    barricaded: &[LocationId],
) -> game_core::GameState {
    let turn_order: Vec<InvestigatorId> = investigators.iter().map(|i| i.id).collect();
    let mut builder = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_enemy(hunter)
        .with_active_investigator(turn_order[0])
        .with_turn_order(turn_order.clone())
        .with_phase_anchor(game_core::state::Continuation::InvestigationPhase {
            resume: game_core::state::InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(turn_order[0]);
    for loc in locations {
        builder = builder.with_location(loc);
    }
    for inv in investigators {
        builder = builder.with_investigator(inv);
    }
    let mut state = builder.build();
    for (i, loc) in barricaded.iter().enumerate() {
        state
            .locations
            .get_mut(loc)
            .unwrap()
            .attachments
            .push(CardInPlay::enter_play(
                CardCode::new(BARRICADE),
                CardInstanceId(900 + u32::try_from(i).unwrap()),
            ));
    }
    state
}

/// Run `EndTurn` once per investigator in turn order, which cascades the
/// Investigation phase into the Enemy phase and its step-3.2 hunter movement.
fn end_the_investigation_phase(
    mut state: game_core::GameState,
    turns: usize,
) -> game_core::GameState {
    for _ in 0..turns {
        state = take_turn_action(state, &TurnAction::EndTurn).state;
    }
    state
}

/// The issue's divergence scenario. The Gathering's own map is a star around
/// the Hallway, which cannot express "two connections away", so this is a
/// synthetic board: Hub(1) — Blocked(2, barricaded), and Hub — Mid(3) — Far(4).
/// Investigator 1 is at Blocked, one connection away; investigator 2 is at
/// Far, two connections away past an unblocked route.
fn hallway_attic_cellar() -> game_core::GameState {
    board(
        vec![
            linked(1, "Hub", &[LocationId(2), LocationId(3)]),
            linked(2, "Blocked", &[LocationId(1)]),
            linked(3, "Mid", &[LocationId(1), LocationId(4)]),
            linked(4, "Far", &[LocationId(3)]),
        ],
        vec![inv_at(1, LocationId(2)), inv_at(2, LocationId(4))],
        hunter_at(LocationId(1)),
        &[LocationId(2)],
    )
}

#[test]
fn a_blocked_nearest_investigator_is_still_the_nearest_and_the_hunter_does_not_move() {
    let state = end_the_investigation_phase(hallway_attic_cellar(), 2);
    assert_eq!(
        state.enemies[&EnemyId(100)].current_location,
        Some(LocationId(1)),
        "the barricaded location is still one connection away, so its investigator is \
         nearest; the compelled step is blocked, so the hunter does not move — it \
         must not reroute toward the farther investigator",
    );
    assert_eq!(
        state.investigators[&INV2].current_location,
        Some(LocationId(4)),
        "sanity: the far investigator never moved",
    );
}

#[test]
fn an_unblocked_board_moves_the_hunter_toward_the_nearest_investigator() {
    let mut state = hallway_attic_cellar();
    state
        .locations
        .get_mut(&LocationId(2))
        .unwrap()
        .attachments
        .clear();
    let state = end_the_investigation_phase(state, 2);
    assert_eq!(
        state.enemies[&EnemyId(100)].current_location,
        Some(LocationId(2)),
        "with no barricade the hunter steps to the nearest investigator's location",
    );
}

#[test]
fn a_block_off_the_shortest_path_leaves_hunter_movement_alone() {
    // Hub(1) — Target(2) [investigator], and a longer detour
    // Hub — Side(3, barricaded) — Target. The block sits on that detour.
    let state = board(
        vec![
            linked(1, "Hub", &[LocationId(2), LocationId(3)]),
            linked(2, "Target", &[LocationId(1), LocationId(3)]),
            linked(3, "Side", &[LocationId(1), LocationId(2)]),
        ],
        vec![inv_at(1, LocationId(2))],
        hunter_at(LocationId(1)),
        &[LocationId(3)],
    );
    let state = end_the_investigation_phase(state, 1);
    assert_eq!(
        state.enemies[&EnemyId(100)].current_location,
        Some(LocationId(2)),
        "the barricade is not on the shortest path, so the hunter takes it as usual",
    );
}

#[test]
fn a_blocked_step_on_the_only_shortest_path_is_not_rerouted_around() {
    // Hub(1) — Blocked(2, barricaded) — Target(3) [investigator] is the
    // shortest path (2 connections). A longer unblocked detour exists:
    // Hub — Detour1(4) — Detour2(5) — Target. `Nearest.md` measures the
    // shortest path *through* the block, so the compelled first step is
    // Blocked; `Hunter.md` then makes that a non-move.
    let state = board(
        vec![
            linked(1, "Hub", &[LocationId(2), LocationId(4)]),
            linked(2, "Blocked", &[LocationId(1), LocationId(3)]),
            linked(3, "Target", &[LocationId(2), LocationId(5)]),
            linked(4, "Detour1", &[LocationId(1), LocationId(5)]),
            linked(5, "Detour2", &[LocationId(4), LocationId(3)]),
        ],
        vec![inv_at(1, LocationId(3))],
        hunter_at(LocationId(1)),
        &[LocationId(2)],
    );
    let state = end_the_investigation_phase(state, 1);
    assert_eq!(
        state.enemies[&EnemyId(100)].current_location,
        Some(LocationId(1)),
        "the hunter does not take the longer unblocked detour",
    );
}

#[test]
fn one_blocked_shortest_step_of_two_leaves_the_other_open() {
    // Diamond: Hub(1) — Blocked(2, barricaded) — Target(4) [investigator]
    // and Hub — Open(3) — Target. Both first steps are shortest; only the
    // unblocked one survives as an offered destination.
    let state = board(
        vec![
            linked(1, "Hub", &[LocationId(2), LocationId(3)]),
            linked(2, "Blocked", &[LocationId(1), LocationId(4)]),
            linked(3, "Open", &[LocationId(1), LocationId(4)]),
            linked(4, "Target", &[LocationId(2), LocationId(3)]),
        ],
        vec![inv_at(1, LocationId(4))],
        hunter_at(LocationId(1)),
        &[LocationId(2)],
    );
    let state = end_the_investigation_phase(state, 1);
    assert_eq!(
        state.enemies[&EnemyId(100)].current_location,
        Some(LocationId(3)),
        "the blocked step is dropped from the destination set, not the graph",
    );
}
