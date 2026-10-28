//! Integration: Barricade 01038's attach / non-Elite movement block /
//! leave-location self-discard, end-to-end against the real `cards::REGISTRY`.
//!
//! The self-discard is declared in the **`when`** cell of the
//! `LeftLocation` condition, as the card prints it — *"**Forced** - When an
//! investigator leaves attached location: Discard Barricade."* — so it resolves
//! before the departure lands. That ordering spans the move handler, the
//! registry lookup and the card's own effect, which is why it is proved here
//! rather than in `game-core`.
//!
//! The movement tests drive the real Enemy phase via `EndTurn` (hunter
//! movement is step 3.2) — the same entry `dodge.rs` uses.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, DiscardPile, Enemy, EnemyId, GameStateBuilder,
    InvestigatorId, Location, LocationId, Owner, Zone,
};
use game_core::test_support::{self, seat, Seat, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

const BARRICADE: &str = "01038";
const GHOUL_PRIEST: &str = "01116"; // Humanoid. Monster. Ghoul. Elite. + Hunter
const GHOUL_MINION: &str = "01160"; // Humanoid. Monster. Ghoul. (non-Elite)
const INV: InvestigatorId = InvestigatorId(1);
const A: LocationId = LocationId(1);
const B: LocationId = LocationId(2);
/// The ghoul every scenario here moves.
const GHOUL: EnemyId = EnemyId(100);
/// The first Barricade attached on a board. A seat cannot attach a card to a
/// location, so a Barricade's id is named here, and a board with several
/// numbers them on from this one.
const ATT_INST: CardInstanceId = CardInstanceId(900);

/// The move most tests take: from A to B.
const MOVE_TO_B: TurnAction = TurnAction::Move {
    investigator: INV,
    destination: B,
};

/// A ready, unengaged ghoul (code `code`) at location `at`. Elite-ness drives
/// the movement block, and it comes from the real card's printed traits
/// (looked up by code through the registry), so pass a real code: Ghoul
/// Priest 01116 is Elite, Ghoul Minion 01160 is not. Hunter-ness and
/// engagement are the caller's to set — the two scenarios here want opposite
/// answers.
fn ghoul(code: &str, at: LocationId) -> Enemy {
    let mut e = test_support::test_enemy(GHOUL.0, "Ghoul");
    e.code = CardCode::new(code);
    e.current_location = Some(at);
    e.engaged_with = None;
    e.exhausted = false;
    e
}

/// `location` with a Barricade attached, played by `INV`, under instance `id`.
fn barricaded(mut location: Location, id: CardInstanceId) -> Location {
    location.attachments.push(CardInPlay::enter_play(
        CardCode::new(BARRICADE),
        id,
        Owner::Investigator(INV),
    ));
    location
}

/// A location with the given id, name and connections.
fn linked(id: LocationId, name: &str, connections: &[LocationId]) -> Location {
    let mut loc = test_support::test_location(id.0, name);
    loc.connections = connections.to_vec();
    loc
}

/// `INV`'s turn at the Study (`A`) with Barricade in hand, having played it.
fn play_barricade() -> TestSession {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(A).hand([BARRICADE]))
        .with_location(test_support::test_location(A.0, "Study"))
        .open_turn(INV)
        .session()
        .take(&TurnAction::PlayCard {
            investigator: INV,
            source: PlaySource::Hand(0),
        });
    session.assert_at_turn_menu(INV);
    session
}

/// Take `action` on `INV`'s turn; it resolves back to the turn menu.
fn take(session: TestSession, action: &TurnAction) -> TestSession {
    let session = session.take(action);
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn playing_barricade_attaches_one_card_and_does_not_discard_the_event() {
    let r = play_barricade().finish();
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

/// **A played Barricade is its player's**, although it sits in a location's
/// attachments beside encounter cards: it came from their hand.
#[test]
fn a_played_barricade_is_owned_by_its_player() {
    let r = play_barricade().finish();
    let barricade = r.state.locations[&A]
        .attachments
        .iter()
        .find(|c| c.code == CardCode::new(BARRICADE))
        .expect("Barricade attached");
    assert_eq!(barricade.owner, Owner::Investigator(INV));
}

/// Linear map `a`—`b` (locations A and B), the investigator at `inv_at` and
/// `enemy` optionally on the board, on the investigator's turn.
fn linear_map(inv_at: LocationId, a: Location, b: Location, enemy: Option<Enemy>) -> TestSession {
    let mut builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(inv_at))
        .with_location(a)
        .with_location(b)
        .open_turn(INV);
    if let Some(enemy) = enemy {
        builder = builder.with_enemy(enemy);
    }
    builder.session()
}

/// Linear map A—B with a Barricade attached at B, the investigator at `inv_at`,
/// and `enemy` on the board, on the investigator's turn.
fn map_with_barricade_at_b(inv_at: LocationId, enemy: Enemy) -> TestSession {
    linear_map(
        inv_at,
        linked(A, "A", &[B]),
        barricaded(linked(B, "B", &[A]), ATT_INST),
        Some(enemy),
    )
}

/// The hunter-movement scenario: the investigator (prey) at B, a hunter at A,
/// driven via `EndTurn` into the Enemy phase.
fn hunter_at_a_moving_toward_b(enemy_code: &str) -> TestSession {
    let mut enemy = ghoul(enemy_code, A);
    enemy.hunter = true;
    map_with_barricade_at_b(B, enemy)
}

#[test]
fn non_elite_hunter_cannot_enter_the_barricaded_location() {
    let r = hunter_at_a_moving_toward_b(GHOUL_MINION).take(&TurnAction::EndTurn);
    assert_phase_reached_enemy(&r);
    assert_eq!(
        r.state().enemies[&GHOUL].current_location,
        Some(A),
        "non-Elite hunter stayed (only path is into the barricaded location)",
    );
}

#[test]
fn elite_hunter_enters_the_barricaded_location() {
    let r = hunter_at_a_moving_toward_b(GHOUL_PRIEST).take(&TurnAction::EndTurn);
    assert_phase_reached_enemy(&r);
    assert_eq!(
        r.state().enemies[&GHOUL].current_location,
        Some(B),
        "Elite hunter ignores the barricade",
    );
}

/// Linear map A—B with a Barricade attached at B, the investigator at A, and a
/// ready enemy (code `enemy_code`, 1 damage) engaged with them at A. The move
/// A→B is the drag-along case: the engaged enemy would ride along, but B is
/// barricaded.
fn engaged_map_with_barricade_at_b(enemy_code: &str) -> TestSession {
    map_with_barricade_at_b(A, engaged_ghoul(enemy_code))
}

/// A ready ghoul (code `code`, 1 damage) engaged with the investigator at A.
fn engaged_ghoul(code: &str) -> Enemy {
    let mut enemy = ghoul(code, A);
    enemy.engaged_with = Some(INV);
    enemy.attack_damage = 1;
    enemy.attack_horror = 0;
    enemy
}

/// Barricade 01038: "Non-[[Elite]] enemies cannot move into attached location."
/// Its ruling (<https://arkhamdb.com/card/01038>): "If an investigator that is
/// engaged with an enemy moves to a Barricaded location, the engaged enemy will
/// disengage and remain in the investigator's previous location (after making an
/// attack of opportunity)."
#[test]
fn engaged_non_elite_enemy_disengages_and_stays_behind_at_a_barricade() {
    let r = take(engaged_map_with_barricade_at_b(GHOUL_MINION), &MOVE_TO_B).finish();
    assert_eq!(
        r.state.investigators[&INV].current_location,
        Some(B),
        "the investigator still moves",
    );
    let enemy = &r.state.enemies[&GHOUL];
    assert_eq!(enemy.current_location, Some(A), "enemy stayed behind");
    assert_eq!(enemy.engaged_with, None, "engagement broke");
    assert_event!(
        r.events,
        Event::EnemyDisengaged { enemy, investigator }
            if *enemy == GHOUL && *investigator == INV
    );
}

/// The attack of opportunity resolves *before* the disengage (per the ruling's
/// parenthetical), which in turn precedes the investigator's move.
#[test]
fn the_attack_of_opportunity_resolves_before_the_disengage() {
    let r = take(engaged_map_with_barricade_at_b(GHOUL_MINION), &MOVE_TO_B).finish();
    assert_event_sequence!(
        r.events,
        Event::DamagePlacedOnInvestigator { .. },
        Event::EnemyDisengaged { .. },
        Event::InvestigatorMoved { .. },
    );
    assert_eq!(r.state.investigators[&INV].damage(), 1, "AoO still landed");
}

/// Barricade names non-Elite only, so an Elite enemy is dragged along as before.
#[test]
fn engaged_elite_enemy_is_dragged_into_the_barricaded_location() {
    let r = take(engaged_map_with_barricade_at_b(GHOUL_PRIEST), &MOVE_TO_B).finish();
    let enemy = &r.state.enemies[&GHOUL];
    assert_eq!(enemy.current_location, Some(B), "Elite enemy followed");
    assert_eq!(enemy.engaged_with, Some(INV), "still engaged");
}

/// Control: with no Barricade at the destination, the non-Elite enemy is dragged
/// along and stays engaged.
#[test]
fn engaged_non_elite_enemy_follows_when_the_destination_is_unbarricaded() {
    let unbarricaded = linear_map(
        A,
        linked(A, "A", &[B]),
        linked(B, "B", &[A]),
        Some(engaged_ghoul(GHOUL_MINION)),
    );
    let r = take(unbarricaded, &MOVE_TO_B).finish();
    let enemy = &r.state.enemies[&GHOUL];
    assert_eq!(enemy.current_location, Some(B), "enemy followed");
    assert_eq!(enemy.engaged_with, Some(INV), "still engaged");
}

#[test]
fn leaving_the_barricaded_location_discards_barricade() {
    let r = take(map_leaving_barricaded_a(None), &MOVE_TO_B).finish();
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
/// lands. `glossary/When.md` pins the printed word to *"the moment
/// immediately after the specified timing point or triggering condition
/// initiates, but before its impact upon the game state resolves"*, and the
/// departure's impact is the investigator arriving — `InvestigatorMoved`.
#[test]
fn barricade_discards_before_the_departure_lands() {
    let r = take(map_leaving_barricaded_a(None), &MOVE_TO_B).finish();
    assert_event_sequence!(
        r.events,
        Event::CardDiscarded { .. },
        Event::InvestigatorMoved { .. },
    );
    // A player card attached to a location goes to its player's discard, not
    // the encounter discard.
    assert_event!(
        r.events,
        Event::CardDiscarded { code, from: Zone::LocationAttachment, to: DiscardPile::Investigator(INV) }
            if code.as_str() == BARRICADE
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
    let mut enemy = ghoul(GHOUL_MINION, A);
    enemy.engaged_with = Some(INV);
    enemy.attack_damage = 0;
    enemy.attack_horror = 0;
    let r = take(map_leaving_barricaded_a(Some(enemy)), &MOVE_TO_B).finish();
    assert_event_sequence!(
        r.events,
        Event::CardDiscarded { .. },
        Event::InvestigatorMoved { .. },
    );
    let enemy = &r.state.enemies[&GHOUL];
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
    let mut enemy = ghoul(GHOUL_MINION, A);
    enemy.engaged_with = Some(INV);
    enemy.attack_damage = 0;
    enemy.attack_horror = 0;
    let both_barricaded = map_leaving_barricaded_a_with(
        Some(enemy),
        barricaded(linked(B, "B", &[A]), CardInstanceId(ATT_INST.0 + 1)),
    );
    let r = take(both_barricaded, &MOVE_TO_B).finish();
    assert_event_sequence!(
        r.events,
        Event::CardDiscarded { .. },
        Event::EnemyDisengaged { .. },
        Event::InvestigatorMoved { .. },
    );
    let enemy = &r.state.enemies[&GHOUL];
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
/// and `enemy` optionally on the board, on the investigator's turn. The move
/// A→B is the one that fires the card's own forced self-discard.
fn map_leaving_barricaded_a(enemy: Option<Enemy>) -> TestSession {
    map_leaving_barricaded_a_with(enemy, linked(B, "B", &[A]))
}

/// [`map_leaving_barricaded_a`] with `b` as location B.
fn map_leaving_barricaded_a_with(enemy: Option<Enemy>, b: Location) -> TestSession {
    linear_map(A, barricaded(linked(A, "A", &[B]), ATT_INST), b, enemy)
}

// ---------------------------------------------------------------------------
// A movement block never changes *who is nearest* or *what the shortest
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

/// A ready, unengaged, non-Elite hunter at `at` — the mover every scenario
/// below shares, since it is the non-Elite case the barricade actually stops.
fn hunter_at(at: LocationId) -> Enemy {
    let mut enemy = ghoul(GHOUL_MINION, at);
    enemy.hunter = true;
    enemy
}

/// Assemble a board out of ready-made locations / seated investigators / one
/// hunter, on the first investigator's turn, attaching a Barricade at each of
/// `barricaded`.
fn board(
    locations: Vec<Location>,
    investigators: Vec<Seat>,
    hunter: Enemy,
    barricaded: &[LocationId],
) -> TestSession {
    let mut builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_enemy(hunter);
    let mut next_barricade = ATT_INST.0;
    for loc in locations {
        if barricaded.contains(&loc.id) {
            builder = builder.with_location(self::barricaded(loc, CardInstanceId(next_barricade)));
            next_barricade += 1;
        } else {
            builder = builder.with_location(loc);
        }
    }
    let mut turn_order = Vec::new();
    for (i, inv) in investigators.into_iter().enumerate() {
        turn_order.push(InvestigatorId(
            u32::try_from(i + 1).expect("seat count fits u32"),
        ));
        builder = builder.with_seat(inv);
    }
    builder
        .with_turn_order(turn_order.clone())
        .open_turn(turn_order[0])
        .session()
}

/// Take `EndTurn` once per investigator in turn order, which cascades the
/// Investigation phase into the Enemy phase and its step-3.2 hunter movement.
fn end_the_investigation_phase(mut session: TestSession, turns: usize) -> TestSession {
    for _ in 0..turns {
        session = session.take(&TurnAction::EndTurn);
    }
    assert_phase_reached_enemy(&session);
    session
}

/// The `EndTurn` cascade really ran the Enemy phase, so its step-3.2 hunter
/// movement had its chance.
fn assert_phase_reached_enemy(session: &TestSession) {
    assert_event!(
        session.events(),
        Event::PhaseStarted {
            phase: game_core::state::Phase::Enemy
        }
    );
}

/// The hub every hunter board below starts its hunter at.
const HUB: LocationId = LocationId(1);
/// The barricaded location next to [`HUB`].
const BLOCKED: LocationId = LocationId(2);
/// The divergence board's unblocked middle step.
const MID: LocationId = LocationId(3);
/// The divergence board's far end, two connections from [`HUB`].
const FAR: LocationId = LocationId(4);

/// The issue's divergence scenario. The Gathering's own map is a star around
/// the Hallway, which cannot express "two connections away", so this is a
/// synthetic board: Hub(1) — Blocked(2, barricaded), and Hub — Mid(3) — Far(4).
/// Investigator 1 is at Blocked, one connection away; investigator 2 is at
/// Far, two connections away past an unblocked route.
fn hallway_attic_cellar() -> TestSession {
    hallway_attic_cellar_barricading(&[BLOCKED])
}

/// [`hallway_attic_cellar`] with Barricades at `barricaded`.
fn hallway_attic_cellar_barricading(barricaded: &[LocationId]) -> TestSession {
    board(
        vec![
            linked(HUB, "Hub", &[BLOCKED, MID]),
            linked(BLOCKED, "Blocked", &[HUB]),
            linked(MID, "Mid", &[HUB, FAR]),
            linked(FAR, "Far", &[MID]),
        ],
        vec![seat(1).at(BLOCKED), seat(2).at(FAR)],
        hunter_at(HUB),
        barricaded,
    )
}

/// Regression for #651: a hunter whose nearest investigator stands behind a
/// block does not move, rather than rerouting toward a farther investigator.
#[test]
fn a_blocked_nearest_investigator_is_still_the_nearest_and_the_hunter_does_not_move() {
    let session = end_the_investigation_phase(hallway_attic_cellar(), 2);
    let state = session.state();
    assert_eq!(
        state.enemies[&GHOUL].current_location,
        Some(HUB),
        "the barricaded location is still one connection away, so its investigator is \
         nearest; the compelled step is blocked, so the hunter does not move — it \
         must not reroute toward the farther investigator",
    );
    assert_eq!(
        state.investigators[&INV2].current_location,
        Some(FAR),
        "sanity: the far investigator never moved",
    );
}

#[test]
fn an_unblocked_board_moves_the_hunter_toward_the_nearest_investigator() {
    let session = end_the_investigation_phase(hallway_attic_cellar_barricading(&[]), 2);
    assert_eq!(
        session.state().enemies[&GHOUL].current_location,
        Some(BLOCKED),
        "with no barricade the hunter steps to the nearest investigator's location",
    );
}

#[test]
fn a_block_off_the_shortest_path_leaves_hunter_movement_alone() {
    const TARGET: LocationId = LocationId(2);
    const SIDE: LocationId = LocationId(3);
    // Hub(1) — Target(2) [investigator], and a longer detour
    // Hub — Side(3, barricaded) — Target. The block sits on that detour.
    let state = board(
        vec![
            linked(HUB, "Hub", &[TARGET, SIDE]),
            linked(TARGET, "Target", &[HUB, SIDE]),
            linked(SIDE, "Side", &[HUB, TARGET]),
        ],
        vec![seat(1).at(TARGET)],
        hunter_at(HUB),
        &[SIDE],
    );
    let session = end_the_investigation_phase(state, 1);
    assert_eq!(
        session.state().enemies[&GHOUL].current_location,
        Some(TARGET),
        "the barricade is not on the shortest path, so the hunter takes it as usual",
    );
}

/// Regression for #651: a blocked step on the only shortest path is a non-move,
/// not a reroute along a longer unblocked detour.
#[test]
fn a_blocked_step_on_the_only_shortest_path_is_not_rerouted_around() {
    const TARGET: LocationId = LocationId(3);
    const DETOUR_1: LocationId = LocationId(4);
    const DETOUR_2: LocationId = LocationId(5);
    // Hub(1) — Blocked(2, barricaded) — Target(3) [investigator] is the
    // shortest path (2 connections). A longer unblocked detour exists:
    // Hub — Detour1(4) — Detour2(5) — Target. `Nearest.md` measures the
    // shortest path *through* the block, so the compelled first step is
    // Blocked; `Hunter.md` then makes that a non-move.
    let state = board(
        vec![
            linked(HUB, "Hub", &[BLOCKED, DETOUR_1]),
            linked(BLOCKED, "Blocked", &[HUB, TARGET]),
            linked(TARGET, "Target", &[BLOCKED, DETOUR_2]),
            linked(DETOUR_1, "Detour1", &[HUB, DETOUR_2]),
            linked(DETOUR_2, "Detour2", &[DETOUR_1, TARGET]),
        ],
        vec![seat(1).at(TARGET)],
        hunter_at(HUB),
        &[BLOCKED],
    );
    let session = end_the_investigation_phase(state, 1);
    assert_eq!(
        session.state().enemies[&GHOUL].current_location,
        Some(HUB),
        "the hunter does not take the longer unblocked detour",
    );
}

#[test]
fn one_blocked_shortest_step_of_two_leaves_the_other_open() {
    const OPEN: LocationId = LocationId(3);
    const TARGET: LocationId = LocationId(4);
    // Diamond: Hub(1) — Blocked(2, barricaded) — Target(4) [investigator]
    // and Hub — Open(3) — Target. Both first steps are shortest; only the
    // unblocked one survives as an offered destination.
    let state = board(
        vec![
            linked(HUB, "Hub", &[BLOCKED, OPEN]),
            linked(BLOCKED, "Blocked", &[HUB, TARGET]),
            linked(OPEN, "Open", &[HUB, TARGET]),
            linked(TARGET, "Target", &[BLOCKED, OPEN]),
        ],
        vec![seat(1).at(TARGET)],
        hunter_at(HUB),
        &[BLOCKED],
    );
    let session = end_the_investigation_phase(state, 1);
    assert_eq!(
        session.state().enemies[&GHOUL].current_location,
        Some(OPEN),
        "the blocked step is dropped from the destination set, not the graph",
    );
}

/// Regression for #371: **Barricade goes to the player who played it**, not to
/// whoever left.
/// Investigator 1 played it at A; investigator 2 leaves A, which fires its
/// *"**Forced** - When an investigator leaves attached location: Discard
/// Barricade."* `glossary/Discard_Piles.md`: *"Any time a card is discarded, it
/// is placed faceup on top of its owner's discard pile."*
#[test]
fn another_investigator_leaving_discards_barricade_to_the_player_who_played_it() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(A))
        .with_seat(seat(2).at(A))
        .with_location(barricaded(linked(A, "A", &[B]), ATT_INST))
        .with_location(linked(B, "B", &[A]))
        .open_turn(INV2)
        .session()
        .take(&TurnAction::Move {
            investigator: INV2,
            destination: B,
        });
    session.assert_at_turn_menu(INV2);
    let r = session.finish();
    assert!(r.state.locations[&A].attachments.is_empty(), "discarded");
    assert_eq!(
        r.state.investigators[&INV].discard,
        vec![CardCode::new(BARRICADE)],
        "in the discard of the player who played it",
    );
    assert!(
        r.state.investigators[&INV2].discard.is_empty(),
        "not in the leaver's discard",
    );
    assert_event!(
        r.events,
        Event::CardDiscarded { code, from: Zone::LocationAttachment, to: DiscardPile::Investigator(INV) }
            if code.as_str() == BARRICADE
    );
}
