//! The contract of [`test_support::install_registry_with_test_cards`] over the
//! real corpus (#934): a binary that installs `cards::REGISTRY` through it can
//! seat a plain [`test_support::test_investigator`], while every real card and
//! the synthetic terminal acts/agendas still resolve.
//!
//! Own process, so it can install the process-global registry.

use cards::REGISTRY;
use game_core::action::{Action, EngineRecord};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::scenario::ScenarioId;
use game_core::state::{
    Act, CardCode, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, Status,
};
use game_core::test_support::{self, ScriptedResolver};

const INV: InvestigatorId = InvestigatorId(1);

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Harm landing on the test investigator reads its capacity from the registry,
/// which panicked under the bare real registry and forced tests to borrow a
/// real investigator's code instead.
#[test]
fn test_investigator_takes_harm_under_the_real_registry() {
    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_support::test_investigator(1), LocationId(20))
        .with_location(test_support::test_location(20, "Here"))
        .with_turn_order([INV])
        .build();
    // Rotting Remains 01163: willpower 3 + (-2) = 1 vs difficulty 3 → fail by 2
    // → 2 horror.
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(-2)];
    state.encounter_deck.push_back(CardCode::new("01163"));
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);

    let result = test_support::drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed { investigator: INV }),
        resolver,
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = &result.state.investigators[&INV];
    assert_eq!(inv.horror(), 2);
    assert_eq!(inv.status, Status::Active);
    assert_eq!((inv.max_health(), inv.max_sanity()), (8, 8));
}

/// The composition adds to the real registry rather than shadowing it.
#[test]
fn a_real_investigator_keeps_its_corpus_capacity() {
    let mut roland = test_support::test_investigator(1);
    roland.investigator_card.code = CardCode::new("01001");
    // Roland Banks 01001 prints 9 health / 5 sanity
    // (`data/arkhamdb-snapshot/pack/core/core.json`).
    assert_eq!((roland.max_health(), roland.max_sanity()), (9, 5));
}

/// A terminal act's reverse is still served, so advancing it ends the scenario.
#[test]
fn a_terminal_act_still_ends_the_scenario() {
    let mut investigator = test_support::test_investigator(1);
    investigator.clues = 1;
    let mut state = GameStateBuilder::new()
        .with_investigator(investigator)
        .open_turn(INV)
        .with_scenario_id(ScenarioId::new("unknown"))
        .build();
    state.act_deck = vec![Act {
        code: test_support::terminal_code(1),
        clue_threshold: 1,
    }];

    let result =
        test_support::take_turn_action(state, &TurnAction::AdvanceAct { investigator: INV });

    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, Event::ScenarioResolved { .. })),
        "events = {:?}",
        result.events
    );
}
