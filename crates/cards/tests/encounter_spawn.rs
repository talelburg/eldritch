//! End-to-end test of the spawn-on-reveal path (#127), against a real Core Set
//! enemy.
//!
//! Drives [`EngineRecord::EncounterCardRevealed`] **directly** — not through a
//! player action — so what is under test is the reveal → spawn → engage path
//! itself rather than the Mythos phase that normally calls it
//! (`mythos_phase.rs` owns the phase walk). The test exercises:
//!
//! - Happy path: revealing the enemy from the encounter deck emits
//!   `Event::CardRevealed` (kind Enemy), then `Event::EnemySpawned` at its
//!   printed spawn location, engaged with the drawing investigator. The enemy
//!   lands in `state.enemies` and does NOT appear in `encounter_discard`.
//! - Multi-investigator suspend: two investigators at the spawn location → the
//!   spawn suspends (`AwaitingInput`) for the lead investigator's `PickSingle`
//!   (#128, option A), leaving the enemy in play but unengaged until the pick
//!   resolves.
//!
//! Default-spawn and location-not-in-play coverage lives in
//! `enemy_spawn_no_location.rs` and `enemy_spawn_unrepresented.rs`.
//!
//! Lives in `crates/cards/tests/` (ADR 0016) because it installs the real
//! `cards::REGISTRY`: `game-core` cannot reach the corpus by crate direction,
//! and each `tests/*.rs` is its own process, so this install does not collide
//! with the registries other integration binaries claim.
//!
//! The cards, all Core Set, all verified against
//! `data/arkhamdb-snapshot/pack/core/` and their rulings files:
//!
//! - **Flesh-Eater 01118** — *"**Spawn** - Attic."* The board therefore seats
//!   its investigators at the **Attic 01113** so the spawn engages them. Its
//!   ruling — *"If an enemy should spawn at a location that is not currently in
//!   play … place that enemy card into the encounter discard pile without any
//!   further effects"* (<https://arkhamdb.com/card/01118>) — is the off-board
//!   case, pinned separately by `enemy_spawn_no_location.rs`; here the Attic is
//!   in play.
//! - **Attic 01113** — the spawn location, built through the engine from its own
//!   corpus metadata (`GameState::add_location`) rather than hand-stamped onto a
//!   `test_location`, which is the impersonation ADR 0016 forbids. Its
//!   *"**Forced** - After you enter the Attic: Take 1 horror."* is live in the
//!   installed registry and simply never fires — only `move_action` emits
//!   `EnteredLocation`, and these tests place their investigators. Its ruling —
//!   *"The **Forced** ability triggers each time an investigator enters this
//!   location"* (<https://arkhamdb.com/card/01113>) — scopes that same entry.
//! - **Roland Banks 01001 / Daisy Walker 01002** — the seated investigators, so
//!   `max_health()` / `max_sanity()` resolve against the installed registry.
//!   Neither's printed ability has a trigger here — no turn is taken and no
//!   enemy is defeated. Roland's two rulings
//!   (<https://arkhamdb.com/card/01001>) scope his reaction; Daisy's single one
//!   (<https://arkhamdb.com/card/01002>) is about which action a lose-actions
//!   effect takes first, and no turn runs here.

use card_dsl::card_data::CardType;
use cards::REGISTRY;
use game_core::action::{Action, EngineRecord};
use game_core::engine::{self, EngineOutcome};
use game_core::event::Event;
use game_core::state::{CardCode, Continuation, GameState, InvestigatorId, LocationId, Phase};
use game_core::test_support::{self, GameStateBuilder};
use game_core::{assert_event_sequence, card_registry};

/// Flesh-Eater — *"**Spawn** - Attic."*
const FLESH_EATER: &str = "01118";
/// The Attic — Flesh-Eater's spawn location, and where the board seats everyone.
const ATTIC: &str = "01113";
/// Roland Banks / Daisy Walker — two distinct seated investigators.
const ROLAND: &str = "01001";
const DAISY: &str = "01002";

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = card_registry::install(REGISTRY);
}

/// The board both tests start from: the Attic in play with one Flesh-Eater on
/// the encounter deck and no investigators yet. Returns the minted
/// [`LocationId`] alongside, since `add_location` mints it.
///
/// Investigators are seated by hand rather than via `seat_and_open` because
/// these tests drive [`EngineRecord::EncounterCardRevealed`] directly.
fn board() -> (GameState, LocationId) {
    let mut state = GameStateBuilder::new().build();
    let attic = state.add_location(cards::by_code(ATTIC).expect("Attic 01113 in corpus"));
    state.phase = Phase::Mythos;
    state.encounter_deck.push_back(CardCode::new(FLESH_EATER));
    (state, attic)
}

/// Seat `investigator_code` as `id` at `at`, appending to the turn order.
fn seat(state: &mut GameState, id: InvestigatorId, investigator_code: &str, at: LocationId) {
    let mut inv = test_support::test_investigator(id.0);
    inv.investigator_card.code = CardCode::new(investigator_code);
    inv.current_location = Some(at);
    state.investigators.insert(id, inv);
    state.turn_order.push(id);
}

#[test]
fn revealing_flesh_eater_spawns_at_the_attic_engaged_with_drawer() {
    let inv1 = InvestigatorId(1);
    let (mut state, attic) = board();
    seat(&mut state, inv1, ROLAND, attic);
    state.active_investigator = Some(inv1);

    let result = engine::apply(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed { investigator: inv1 }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);

    // CardRevealed (Enemy) fires first; EnemySpawned follows.
    assert_event_sequence!(
        result.events,
        Event::CardRevealed { card_type, code, .. }
            if *card_type == CardType::Enemy
                && *code == CardCode::new(FLESH_EATER),
        Event::EnemySpawned { code, location, engaged_with, .. }
            if *code == CardCode::new(FLESH_EATER)
                && *location == attic
                && *engaged_with == Some(inv1),
        Event::EnemyEngaged { investigator, .. }
            if *investigator == inv1,
    );

    // Enemy is in play.
    assert_eq!(
        result.state.enemies.len(),
        1,
        "exactly one enemy should be in play after spawn",
    );
    let enemy = result.state.enemies.values().next().unwrap();
    assert_eq!(enemy.current_location, Some(attic));
    assert_eq!(enemy.engaged_with, Some(inv1));

    // Enemy is NOT in encounter_discard (enemies stay in play; only
    // treacheries discard after Revelation).
    assert!(
        !result
            .state
            .encounter_discard
            .contains(&CardCode::new(FLESH_EATER)),
        "spawned enemy must not appear in encounter_discard",
    );

    // Sanity: the Attic's code is what spawn_enemy looked up.
    let loc = result.state.locations.get(&attic).unwrap();
    assert_eq!(loc.code, CardCode::new(ATTIC));
}

#[test]
fn revealing_flesh_eater_with_two_investigators_at_the_attic_suspends_for_lead_pick() {
    let inv1 = InvestigatorId(1);
    let (mut state, attic) = board();
    seat(&mut state, inv1, ROLAND, attic);
    state.active_investigator = Some(inv1);
    // Second investigator co-located at the spawn location.
    seat(&mut state, InvestigatorId(2), DAISY, attic);

    let result = engine::apply(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed { investigator: inv1 }),
    );

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "multi-investigator spawn now suspends for the lead's PickSingle, got {:?}",
        result.outcome,
    );
    assert!(matches!(
        result.state.continuations.last(),
        Some(Continuation::SpawnEngage(_))
    ));
    let enemy = result.state.enemies.values().next().expect("enemy placed");
    assert_eq!(
        enemy.engaged_with, None,
        "engagement deferred until the lead picks",
    );
}
