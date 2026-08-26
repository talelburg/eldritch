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
//! pause), which is where the anchor is readable. The `after` cell is the one
//! the card prints, and the module's own header quotes it.

use game_core::dsl::EventTiming;
use game_core::engine::{EngineOutcome, OptionTarget};
use game_core::state::{Agenda, CardCode, EnemyId, InvestigatorId};
use game_core::test_support::{
    fire_forced_on_enemy_attack, test_enemy, test_investigator, GameStateBuilder,
};

#[ctor::ctor(unsafe)]
fn install_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

#[test]
fn enemy_01102_forced_ack_anchors_to_the_attacking_enemy() {
    let lead = InvestigatorId(1);
    let attacker_id = EnemyId(7);
    // Skids O'Toole (01003) has no implemented abilities — a real code so any
    // registry-backed lookup resolves (mirrors `agenda_forced_anchor.rs`).
    let mut inv = test_investigator(1);
    inv.investigator_card.code = CardCode::new("01003");
    let mut attacker = test_enemy(7, "Silver Twilight Acolyte");
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
        resolution: None,
    }];
    state.agenda_index = 0;
    state.interactive_acknowledge = true;

    let mut events = Vec::new();
    let out = fire_forced_on_enemy_attack(
        &mut state,
        &mut events,
        attacker_id,
        lead,
        EventTiming::After,
    );
    match out {
        EngineOutcome::AwaitingInput { request, .. } => {
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
        }
        other => panic!("expected the forced-acknowledge suspend, got {other:?}"),
    }
}
