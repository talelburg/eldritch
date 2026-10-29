//! Regression for #650: an enemy with no Spawn instruction spawns engaged with
//! the investigator who drew it, driven through a real encounter draw against the corpus.
//!
//! `glossary/Spawn.md`, *Spawning an Enemy*: *"If the enemy does not have a
//! "Spawn –" instruction, the investigator drawing the enemy spawns it engaged
//! with him/her, unless it is aloof."* and *"The only time "Prey –"
//! instructions will impact this process is when an enemy spawns unengaged at a
//! location with multiple investigators"*. So a second investigator at the
//! drawer's location neither ties the spawn nor wins it on prey. The `Specific`
//! spawn, which does resolve prey there, is `encounter_spawn.rs`'s.
//!
//! The card, verified against `data/arkhamdb-snapshot/pack/core/`:
//!
//! - **Ravenous Ghoul 01161** — *"<b>Prey</b> - Lowest remaining health."*, no
//!   Spawn instruction. No rulings (`data/arkhamdb-faq/no-rulings.txt`).

use cards::TEST_REGISTRIES;
use game_core::event::Event;
use game_core::state::{
    CardCode, GameStateBuilder, InvestigatorId, LocationId, MythosPhaseFrame, MythosResume, Phase,
};
use game_core::test_support::{self, seat, TestSession};
use game_core::{assert_event_sequence, assert_no_event};

/// Ravenous Ghoul — *"<b>Prey</b> - Lowest remaining health."*
const RAVENOUS_GHOUL: &str = "01161";

/// The first investigator, wounded so the Ghoul's prey would pick them.
const LEAD: InvestigatorId = InvestigatorId(1);
/// The investigator who draws the Ghoul.
const DRAWER: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(20);

#[test]
fn a_drawn_enemy_without_a_spawn_instruction_engages_its_drawer_whatever_its_prey() {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat(1).at(HERE).harm(2, 0))
        .with_seat(seat(2).at(HERE))
        .with_seating([LEAD, DRAWER])
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([DRAWER])
        .build();
    state
        .encounter_deck
        .push_back(CardCode::new(RAVENOUS_GHOUL));

    let session = TestSession::new(state, TEST_REGISTRIES).confirm();

    // No prompt for the lead: play runs on out of the Mythos phase.
    session.assert_at_turn_menu(LEAD);
    let ghoul = session
        .state()
        .enemies
        .values()
        .next()
        .expect("Ravenous Ghoul spawned");
    assert_eq!(ghoul.current_location, Some(HERE));
    assert_eq!(ghoul.engaged_with, Some(DRAWER));
    assert_event_sequence!(
        session.events(),
        Event::EnemySpawned { code, engaged_with, .. }
            if *code == CardCode::new(RAVENOUS_GHOUL) && *engaged_with == Some(DRAWER),
        Event::EnemyEngaged { investigator, .. } if *investigator == DRAWER,
    );
    assert_no_event!(
        session.events(),
        Event::EnemyEngaged { investigator, .. } if *investigator == LEAD
    );
}
