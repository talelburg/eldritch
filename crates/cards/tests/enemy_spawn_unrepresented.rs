//! Integration (#635): an encounter enemy whose printed Spawn clause the
//! pipeline could not model refuses loudly instead of spawning at the drawing
//! investigator's location. Driven through the real `cards` registry, so the
//! metadata under test is the shipped corpus entry rather than a fixture.
//!
//! `data/rules-reference/rules/glossary/Spawn.md`:
//!
//! > If an enemy has no spawn instruction, it spawns engaged with the
//! > investigator who drew it.
//!
//! > If an enemy's spawn instruction has multiple valid locations, the
//! > investigator spawning that enemy decides among those locations.
//!
//! Acolyte 01169 prints "**Spawn** - Any empty location.", and
//! `Empty_Location.md` defines an empty location as "a location with no
//! enemies or investigators at it" — so the drawer's own location is the one
//! location that can never satisfy the clause. Modelling the choice among
//! empty locations is deliberately out of scope here; the engine refuses
//! until it lands.

use game_core::action::EngineRecord;
use game_core::state::{CardCode, InvestigatorId, LocationId};
use game_core::test_support::{
    drive, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::{Action, EngineOutcome};

/// Acolyte (01169) — Core enemy, "Spawn - Any empty location".
const ACOLYTE: &str = "01169";
/// Ghoul Minion (01160) — Core enemy printing no Spawn line at all.
const GHOUL_MINION: &str = "01160";
/// The Study (01111) — The Gathering's starting location.
const STUDY: &str = "01111";

#[ctor::ctor(unsafe)]
fn install_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// One investigator in the Study, with `top` on top of the encounter deck.
fn state_with_top_encounter(top: &str) -> game_core::GameState {
    let mut study = test_location(20, "Study");
    study.code = CardCode::new(STUDY);
    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_investigator(1), LocationId(20))
        .with_location(study)
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.encounter_deck.push_back(CardCode::new(top));
    state
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
fn acolyte_refuses_rather_than_spawning_at_the_drawers_location() {
    let result = reveal_top(state_with_top_encounter(ACOLYTE));

    let EngineOutcome::Rejected { reason } = &result.outcome else {
        panic!(
            "an unmodelled spawn instruction must reject, got {:?}",
            result.outcome
        );
    };
    assert!(
        reason.contains("Any empty location"),
        "the rejection must name the clause it could not model: {reason}",
    );
    assert!(
        result.state.enemies.is_empty(),
        "the Acolyte must not be placed anywhere — least of all at the \
         drawing investigator's (non-empty) location",
    );
    // The remaining rejection invariants (no discard, no events, pristine
    // state) belong to the engine seam and are asserted there, in
    // `encounter.rs`'s `spawn_with_unrepresented_instruction_rejects_without_mutating`.
    // What only this test can show is that the *shipped corpus entry* for
    // 01169 carries a clause that reaches that refusal at all.
}

#[test]
fn an_enemy_with_no_spawn_line_still_spawns_engaged_with_the_drawer() {
    // Regression guard for the other half of the distinction: #635 must not
    // turn the no-instruction rule into a refusal too.
    let result = reveal_top(state_with_top_encounter(GHOUL_MINION));

    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "a card printing no Spawn line resolves normally: {:?}",
        result.outcome,
    );
    let (_, enemy) = result
        .state
        .enemies
        .iter()
        .next()
        .expect("the Ghoul Minion spawns");
    assert_eq!(
        enemy.current_location,
        Some(LocationId(20)),
        "spawns at the drawing investigator's location",
    );
    assert_eq!(
        enemy.engaged_with,
        Some(InvestigatorId(1)),
        "and engaged with the drawing investigator",
    );
}
