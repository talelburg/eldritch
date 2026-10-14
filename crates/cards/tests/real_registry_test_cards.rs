//! The contract of [`test_support::install_registry_with_test_cards`] over the
//! real corpus (#934): a binary that installs `cards::REGISTRY` through it can
//! seat a plain [`test_support::test_investigator`], while every real card and
//! the synthetic terminal acts/agendas still resolve.
//!
//! Own process, so it can install the process-global registry.

use cards::REGISTRY;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::scenario::ScenarioId;
use game_core::state::{
    Act, CardCode, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, MythosPhaseFrame,
    MythosResume, Phase, Status,
};
use game_core::test_support::{self, seat, TestSession};

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(20);

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Harm landing on the test investigator reads its capacity from the registry,
/// which panicked under the bare real registry and forced tests to borrow a
/// real investigator's code instead.
#[test]
fn test_investigator_takes_harm_under_the_real_registry() {
    // Mythos step 1.4: the test investigator draws Rotting Remains 01163.
    let mut state = GameStateBuilder::new()
        .with_seat(seat(1).at(HERE))
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_turn_order([INV])
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([INV])
        .build();
    // Rotting Remains 01163: willpower 3 + (-2) = 1 vs difficulty 3 → fail by 2
    // → 2 horror.
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(-2)];
    state.encounter_deck.push_back(CardCode::new("01163"));

    let session = TestSession::new(state).confirm().commit(&[]);

    // The horror landed and play went on into the investigator's turn.
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.horror(), 2);
    assert_eq!(inv.status, Status::Active);
    assert_eq!(
        (
            inv.max_health(game_core::card_registry::current().expect("card registry installed")),
            inv.max_sanity(game_core::card_registry::current().expect("card registry installed"))
        ),
        (8, 8)
    );
}

/// The composition adds to the real registry rather than shadowing it.
#[test]
fn a_real_investigator_keeps_its_corpus_capacity() {
    let state = GameStateBuilder::new()
        .with_seat(seat(1).as_card("01001"))
        .build();
    let roland = &state.investigators[&INV];
    // Roland Banks 01001 prints 9 health / 5 sanity
    // (`data/arkhamdb-snapshot/pack/core/core.json`).
    assert_eq!(
        (
            roland
                .max_health(game_core::card_registry::current().expect("card registry installed")),
            roland
                .max_sanity(game_core::card_registry::current().expect("card registry installed"))
        ),
        (9, 5)
    );
}

/// A terminal act's reverse is still served, so advancing it ends the scenario.
#[test]
fn a_terminal_act_still_ends_the_scenario() {
    let mut state = GameStateBuilder::new()
        .with_seat(seat(1).clues(1))
        .open_turn(INV)
        .with_scenario_id(ScenarioId::new("unknown"))
        .build();
    // The builder stages no act deck.
    state.act_deck = vec![Act {
        code: test_support::terminal_code(1),
        clue_threshold: 1,
    }];

    let result = TestSession::new(state)
        .take(&TurnAction::AdvanceAct { investigator: INV })
        .finish();

    assert_event!(result.events, Event::ScenarioResolved { .. });
}
