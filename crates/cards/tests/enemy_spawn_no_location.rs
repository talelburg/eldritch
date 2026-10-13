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

use cards::REGISTRY;
use game_core::state::{
    CardCode, GameState, GameStateBuilder, InvestigatorId, LocationId, MythosPhaseFrame,
    MythosResume, Phase,
};
use game_core::test_support::{self, seat, TestSession};

/// Flesh-Eater (01118) — Core enemy, "Spawn - Attic" (location 01113).
const FLESH_EATER: &str = "01118";
/// The Study (01111) — The Gathering's starting location.
const STUDY: &str = "01111";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(20);

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Mythos step 1.4: investigator 1, at the Study, to draw `state`'s top
/// encounter card.
fn drawing(top: &str) -> GameState {
    let mut study = test_support::test_location(HERE.0, "Study");
    study.code = CardCode::new(STUDY);
    let mut state = GameStateBuilder::new()
        .with_seat(seat(1).at(HERE))
        .with_location(study)
        .with_turn_order([INV])
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([INV])
        .build();
    state.encounter_deck.push_back(CardCode::new(top));
    state
}

#[test]
fn enemy_with_offboard_spawn_location_is_discarded_not_rejected() {
    // Investigator at the Study; the Attic (Flesh-Eater's spawn location) is
    // NOT in play. Flesh-Eater on top of the encounter deck.
    let session = TestSession::new(drawing(FLESH_EATER)).confirm();

    // An off-board spawn location must not reject the draw: play runs on out
    // of the Mythos phase to the turn.
    session.assert_at_turn_menu(INV);
    assert!(
        session.state().enemies.is_empty(),
        "the enemy does not spawn (its location is not in play)",
    );
    assert!(
        session
            .state()
            .encounter_discard
            .contains(&CardCode::new(FLESH_EATER)),
        "the enemy card is placed in the encounter discard pile (RR p.24)",
    );
}
