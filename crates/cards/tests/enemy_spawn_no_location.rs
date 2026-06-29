//! Integration (#517): an encounter enemy whose designated spawn location is
//! not in play is placed in the encounter discard pile — the draw must NOT
//! reject. Driven through the real `cards` registry and the full `apply` →
//! resolve → disposal pipeline. Own process so it can install the
//! process-global registry against the real corpus.
//!
//! Rules Reference p.24: "If an enemy has no legal location to spawn at (for
//! example, if its spawn instruction directs it to a specific location that is
//! not in play …), it does not spawn, and is discarded instead." Flesh-Eater
//! (01118) FAQ: "If an enemy should spawn at a location that is not currently
//! in play (i.e. while you're at the Study), place that enemy card into the
//! encounter discard pile without any further effects."

use game_core::action::EngineRecord;
use game_core::state::{CardCode, InvestigatorId, LocationId};
use game_core::test_support::{
    drive, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::{Action, EngineOutcome};

/// Flesh-Eater (01118) — Core enemy, "Spawn - Attic" (location 01113).
const FLESH_EATER: &str = "01118";
/// The Study (01111) — The Gathering's starting location.
const STUDY: &str = "01111";

#[ctor::ctor(unsafe)]
fn install_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Reveal the top encounter card for investigator 1.
fn reveal_top(state: game_core::GameState) -> game_core::ApplyResult {
    drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
        ScriptedResolver::new(),
    )
}

#[test]
fn enemy_with_offboard_spawn_location_is_discarded_not_rejected() {
    // Investigator at the Study; the Attic (Flesh-Eater's spawn location) is
    // NOT in play. Flesh-Eater on top of the encounter deck.
    let mut study = test_location(20, "Study");
    study.code = CardCode::new(STUDY);
    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_investigator(1), LocationId(20))
        .with_location(study)
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.encounter_deck.push_back(CardCode::new(FLESH_EATER));

    let result = reveal_top(state);

    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "an off-board spawn location must not reject the draw: {:?}",
        result.outcome,
    );
    assert!(
        result.state.enemies.is_empty(),
        "the enemy does not spawn (its location is not in play)",
    );
    assert!(
        result
            .state
            .encounter_discard
            .contains(&CardCode::new(FLESH_EATER)),
        "the enemy card is placed in the encounter discard pile (RR p.24)",
    );
}
