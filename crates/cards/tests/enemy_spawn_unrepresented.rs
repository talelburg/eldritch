//! Regression for #635: an encounter enemy whose printed Spawn clause the
//! pipeline could not model refuses loudly instead of spawning at the drawing
//! investigator's location.
//!
//! Driven through the real `cards` registry, so the metadata under test is the
//! shipped corpus entry rather than a fixture.
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

use cards::TEST_REGISTRIES;
use game_core::state::{
    CardCode, GameState, GameStateBuilder, InvestigatorId, LocationId, MythosPhaseFrame,
    MythosResume, Phase,
};
use game_core::test_support::{self, seat, TestSession};

/// Acolyte (01169) — Core enemy, "Spawn - Any empty location".
const ACOLYTE: &str = "01169";
/// Ghoul Minion (01160) — Core enemy printing no Spawn line at all.
const GHOUL_MINION: &str = "01160";
/// The Study (01111) — The Gathering's starting location.
const STUDY: &str = "01111";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(20);

/// Mythos step 1.4: one investigator in the Study to draw, with `top` on top
/// of the encounter deck.
fn state_with_top_encounter(top: &str) -> GameState {
    let mut study = test_support::test_location(HERE.0, "Study");
    study.code = CardCode::new(STUDY);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
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

/// Draw the top encounter card for investigator 1.
fn reveal_top(state: GameState) -> TestSession {
    TestSession::new(state, TEST_REGISTRIES).confirm()
}

#[test]
fn acolyte_refuses_rather_than_spawning_at_the_drawers_location() {
    let session = reveal_top(state_with_top_encounter(ACOLYTE));

    let reason = session.expect_rejected();
    assert!(
        reason.contains("Any empty location"),
        "the rejection must name the clause it could not model: {reason}",
    );
    assert!(
        session.state().enemies.is_empty(),
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
    // The other half of the distinction: refusing an unmodelled clause must
    // not turn the no-instruction rule into a refusal too.
    let session = reveal_top(state_with_top_encounter(GHOUL_MINION));

    // A card printing no Spawn line resolves normally: play runs on out of the
    // Mythos phase to the turn.
    session.assert_at_turn_menu(INV);
    let (_, enemy) = session
        .state()
        .enemies
        .iter()
        .next()
        .expect("the Ghoul Minion spawns");
    assert_eq!(
        enemy.current_location,
        Some(HERE),
        "spawns at the drawing investigator's location",
    );
    assert_eq!(
        enemy.engaged_with,
        Some(INV),
        "and engaged with the drawing investigator",
    );
}
