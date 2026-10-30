//! Integration: an attacking enemy's own forced ability anchors its acknowledge
//! to **that enemy**, not to the flat prompt bar.
//!
//! Runs against `cards::TEST_REGISTRIES`.
//!
//! The corpus card is Silver Twilight Acolyte 01102, whose printed text is
//! verbatim:
//!
//! ```text
//! Forced - After Silver Twilight Acolyte attacks: Place 1 doom on the current
//!   agenda.
//! ```
//!
//! The candidate's source names the enemy, so the anchor is read from it
//! rather than by comparing the candidate's code against the current act and
//! agenda, which for an enemy matches neither.
//!
//! Driven with `interactive_acknowledge` on, so the one-option "Resolve"
//! acknowledge surfaces *before* the effect (the confirm-before-effect pause),
//! which is where the anchor is readable. The investigator ends their
//! turn engaged with the Acolyte, the Enemy phase deals its attack, and the
//! `after` cell the card prints then offers the acknowledge.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::state::{EnemyId, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, seat, TestSession};

const LEAD: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ATTACKER: EnemyId = EnemyId(7);

#[test]
fn enemy_01102_forced_ack_anchors_to_the_attacking_enemy() {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HERE))
        .with_seat(seat(1).at(HERE))
        .with_enemy(
            test_support::enemy(ATTACKER)
                .as_card("01102")
                .engaged_with(LEAD),
        )
        .open_turn(LEAD)
        // The card places doom on the current agenda, so one must be current
        // for the effect to have the potential to change the game state (RR
        // p.2 initiation).
        .with_agenda_deck(["01105"])
        .build();
    state.interactive_acknowledge = true;

    // Ending the turn runs into the Enemy phase, where the Acolyte attacks.
    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::EndTurn);
    let request = session.prompt();
    assert_eq!(
        request.options.len(),
        1,
        "the interactive forced-acknowledge is a one-option 'Resolve' pick \
         before the effect resolves: {request:?}",
    );
    assert_eq!(
        session.offered_at(&OptionTarget::Enemy(ATTACKER)),
        1,
        "an attacking enemy's own forced ability anchors to that enemy, where \
         it used to anchor to nothing (#735)",
    );
    assert_eq!(
        session.state().agenda_doom,
        0,
        "the doom is not placed before the acknowledge"
    );
}
