//! #128 integration: a spawn-engagement tie resolved through the real registry
//! and the Mythos encounter-draw path (option A).
//!
//! Two investigators share the spawn location, so `Prey::Default` ties and the
//! draw suspends for the lead's `PickSingle`; resolving the pick engages the
//! chosen investigator.
//!
//! What distinguishes this from `mythos_phase.rs`'s multi-investigator test is
//! the entry point: the frames the Mythos loop would have pushed are staged by
//! hand and resumed with a single `ResolveInput`, so the tie resolves without a
//! phase walk in front of it.
//!
//! Lives in `crates/cards/tests/` (ADR 0016) because it installs the real
//! `cards::REGISTRY`: `game-core` cannot reach the corpus by crate direction,
//! and each `tests/*.rs` is its own process, so this install does not collide
//! with the registries other integration binaries claim.
//!
//! The cards, all Core Set, all verified against
//! `data/arkhamdb-snapshot/pack/core/` and their rulings files:
//!
//! - **Flesh-Eater 01118** — *"**Spawn** - Attic."* Both investigators are
//!   therefore seated at the **Attic 01113**, which is what makes the tie.
//!   `Prey` is unprinted on the card, so the engine's default prey rule decides,
//!   and it finds two equally-close investigators. Its ruling — *"If an enemy
//!   should spawn at a location that is not currently in play … place that enemy
//!   card into the encounter discard pile without any further effects"*
//!   (<https://arkhamdb.com/card/01118>) — is the off-board case; here the Attic
//!   is in play.
//! - **Attic 01113** — the spawn location, built through the engine from its own
//!   corpus metadata (`GameState::add_location`) rather than hand-stamped onto a
//!   `test_location`, which is the impersonation ADR 0016 forbids. Its
//!   *"**Forced** - After you enter the Attic: Take 1 horror."* never fires —
//!   only `move_action` emits `EnteredLocation`, and this test places its
//!   investigators. Its ruling — *"The **Forced** ability triggers each time an
//!   investigator enters this location"* (<https://arkhamdb.com/card/01113>) —
//!   scopes that same entry.
//! - **Roland Banks 01001 / Daisy Walker 01002** — the seated investigators, so
//!   `max_health()` / `max_sanity()` resolve against the installed registry.
//!   Neither's printed ability has a trigger here. Roland's two rulings
//!   (<https://arkhamdb.com/card/01001>) scope his reaction; Daisy's single one
//!   (<https://arkhamdb.com/card/01002>) is about which action a lose-actions
//!   effect takes first, and no turn runs here.

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{apply, EngineOutcome};
use game_core::state::{
    CardCode, Continuation, GameState, InvestigatorId, LocationId, MythosResume, Phase,
};
use game_core::test_support::{test_investigator, GameStateBuilder};
use game_core::Action;

/// Flesh-Eater — *"**Spawn** - Attic."*
const FLESH_EATER: &str = "01118";
/// The Attic — Flesh-Eater's spawn location, and where both investigators sit.
const ATTIC: &str = "01113";
/// Roland Banks / Daisy Walker — two distinct seated investigators.
const ROLAND: &str = "01001";
const DAISY: &str = "01002";

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Seat `investigator_code` as `id` at `at`, appending to the turn order.
fn seat(state: &mut GameState, id: InvestigatorId, investigator_code: &str, at: LocationId) {
    let mut inv = test_investigator(id.0);
    inv.investigator_card.code = CardCode::new(investigator_code);
    inv.current_location = Some(at);
    state.investigators.insert(id, inv);
    state.turn_order.push(id);
}

#[test]
fn multi_investigator_spawn_engagement_resolves_via_lead_pick() {
    let inv1 = InvestigatorId(1);
    let mut state = GameStateBuilder::new().build();
    let attic = state.add_location(cards::by_code(ATTIC).expect("Attic 01113 in corpus"));
    state.starting_location = Some(attic);
    seat(&mut state, inv1, ROLAND, attic);
    state.active_investigator = Some(inv1);
    // Second investigator co-located at the spawn location, which is what ties.
    seat(&mut state, InvestigatorId(2), DAISY, attic);
    // Drive through the real Mythos draw path: stage the EncounterDraw loop
    // frame for inv1 so the ResolveInput(Confirm) below resumes it (#348).
    state.phase = Phase::Mythos;
    // Mythos anchor (slice 1a) sits beneath the EncounterDraw loop; the
    // post-1.4 MythosAfterDraws close routes to it.
    state.continuations.push(Continuation::MythosPhase {
        resume: MythosResume::AfterDraws,
    });
    state.continuations.push(Continuation::EncounterDraw {
        remaining: vec![InvestigatorId(1)],
    });
    state.encounter_deck.push_back(CardCode::new(FLESH_EATER));

    // 1) Drawing the enemy suspends for the lead's PickSingle.
    let r1 = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert!(
        matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }),
        "multi-investigator spawn suspends, got {:?}",
        r1.outcome,
    );
    assert!(
        matches!(
            r1.state.continuations.last(),
            Some(Continuation::SpawnEngage(_))
        ),
        "spawn engagement tie should be pending for the lead's pick",
    );
    let spawned = r1.state.enemies.values().next().expect("enemy placed");
    assert_eq!(spawned.engaged_with, None, "engagement deferred until pick");

    // 2) Lead picks investigator 2 (by its offered option id); engagement
    //    resolves, draw chain ends.
    let pick = {
        let EngineOutcome::AwaitingInput { request, .. } = &r1.outcome else {
            unreachable!("asserted AwaitingInput above");
        };
        request
            .options
            .iter()
            .find(|o| o.label == format!("{:?}", InvestigatorId(2)))
            .expect("InvestigatorId(2) among offered options")
            .id
    };
    let r2 = apply(
        r1.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(pick),
        }),
    );
    assert!(matches!(r2.outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(!matches!(
        r2.state.continuations.last(),
        Some(Continuation::SpawnEngage(_))
    ));
    let enemy = r2.state.enemies.values().next().expect("enemy in play");
    assert_eq!(enemy.engaged_with, Some(InvestigatorId(2)));
}
