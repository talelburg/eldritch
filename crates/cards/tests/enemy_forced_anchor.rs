//! Integration (#735): an attacking enemy's own forced ability anchors its
//! acknowledge to **that enemy**, not to the flat prompt bar. Own process →
//! installs the real `cards::REGISTRY`.
//!
//! The corpus card is Silver Twilight Acolyte 01102, whose printed text is
//! verbatim:
//!
//! ```text
//! Forced - After Silver Twilight Acolyte attacks: Place 1 doom on the current
//!   agenda.
//! ```
//!
//! Before the `CandidateSource::Board` split this candidate had nowhere to go:
//! `Board` covered the act, the agenda *and* an attacking enemy's own ability,
//! and the anchor was worked out by comparing the candidate's code against the
//! current act and agenda — which for an enemy matches neither, so the prompt
//! fell through to an un-anchored option. The source now names the enemy.
//!
//! Driven with `interactive_acknowledge` on, so the one-option "Resolve"
//! acknowledge surfaces *before* the effect (the #466 confirm-before-effect
//! pause), which is where the anchor is readable. Firing the `EnemyAttacks`
//! timing point deals the attack at its resolve step, and the `after` cell the
//! card prints then offers the acknowledge.

use cards::REGISTRY;
use game_core::engine::{OptionTarget, TimingEvent};
use game_core::state::{Agenda, CardCode, EnemyId, GameStateBuilder, InvestigatorId};
use game_core::test_support::{self, TestSession};

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

#[test]
fn enemy_01102_forced_ack_anchors_to_the_attacking_enemy() {
    let lead = InvestigatorId(1);
    let attacker_id = EnemyId(7);
    let inv = test_support::test_investigator(1);
    let mut attacker = test_support::test_enemy(7, "Silver Twilight Acolyte");
    attacker.code = CardCode::new("01102");
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([lead])
        .with_enemy(attacker)
        .build();
    // The card places doom on the current agenda, so one must be current for the
    // effect to have the potential to change the game state (RR p.2 initiation).
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01105"),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;
    state.interactive_acknowledge = true;

    let session = TestSession::new(state).fire_at(TimingEvent::EnemyAttacks {
        enemy: attacker_id,
        investigator: lead,
    });
    let request = session.prompt();
    assert_eq!(
        request.options.len(),
        1,
        "the interactive forced-acknowledge is a one-option 'Resolve' pick \
         before the effect resolves",
    );
    assert_eq!(
        request.options[0].target,
        Some(OptionTarget::Enemy(attacker_id)),
        "an attacking enemy's own forced ability anchors to that enemy, where \
         it used to anchor to nothing (#735)",
    );
    assert_eq!(
        session.state().agenda_doom,
        0,
        "the doom is not placed before the acknowledge"
    );
}
