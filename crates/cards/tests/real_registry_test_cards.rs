//! The contract of `cards::TEST_REGISTRY` over the real corpus (#934): a test
//! that names it can seat a plain [`test_support::test_investigator`], while
//! every real card and the synthetic terminal acts/agendas still resolve.

use cards::{TEST_REGISTRIES, TEST_REGISTRY};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::scenario::ScenarioId;
use game_core::state::{
    Act, CardCode, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, MythosPhaseFrame,
    MythosResume, Phase, Skills, Status,
};
use game_core::test_support::{self, seat, terminal_code, TestSession, TEST_INV};

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(20);

/// Harm landing on the test investigator reads its capacity from the registry,
/// which panicked under the bare real registry and forced tests to borrow a
/// real investigator's code instead.
#[test]
fn test_investigator_takes_harm_under_the_real_registry() {
    // Mythos step 1.4: the test investigator draws Rotting Remains 01163.
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
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

    let session = TestSession::new(state, TEST_REGISTRIES)
        .confirm()
        .commit(&[]);

    // The horror landed and play went on into the investigator's turn.
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.horror(), 2);
    assert_eq!(inv.status, Status::Active);
    assert_eq!(
        (
            inv.max_health(TEST_REGISTRIES.cards),
            inv.max_sanity(TEST_REGISTRIES.cards)
        ),
        (8, 8)
    );
}

/// The composition adds to the real registry rather than shadowing it.
#[test]
fn a_real_investigator_keeps_its_corpus_capacity() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).as_card("01001"))
        .build();
    let roland = &state.investigators[&INV];
    // Roland Banks 01001 prints 9 health / 5 sanity
    // (`data/arkhamdb-snapshot/pack/core/core.json`).
    assert_eq!(
        (
            roland.max_health(TEST_REGISTRIES.cards),
            roland.max_sanity(TEST_REGISTRIES.cards)
        ),
        (9, 5)
    );
}

/// A terminal act's reverse is still served, so advancing it ends the scenario.
#[test]
fn a_terminal_act_still_ends_the_scenario() {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).clues(1))
        .open_turn(INV)
        .with_scenario_id(ScenarioId::new("unknown"))
        .build();
    // The builder stages no act deck.
    state.act_deck = vec![Act {
        code: test_support::terminal_code(1),
        clue_threshold: 1,
    }];

    let result = TestSession::new(state, TEST_REGISTRIES)
        .take(&TurnAction::AdvanceAct { investigator: INV })
        .finish();

    assert_event!(result.events, Event::ScenarioResolved { .. });
}

#[test]
fn a_real_investigator_is_seated_from_the_corpus() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).as_card("01001"))
        .build();
    let roland = &state.investigators[&INV];
    // Roland Banks 01001 prints willpower 3, intellect 3, combat 4, agility 2
    // (`data/arkhamdb-snapshot/pack/core/core.json`).
    assert_eq!(roland.name, "Roland Banks");
    assert_eq!(
        roland.skills,
        Skills {
            willpower: 3,
            intellect: 3,
            combat: 4,
            agility: 2,
        },
    );
}

#[test]
fn the_synthetic_test_investigator_resolves_next_to_the_corpus() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).as_card(TEST_INV))
        .build();
    assert_eq!(state.investigators[&INV].name, "Test Investigator");
}

#[test]
fn the_synthetic_terminal_cards_and_real_abilities_are_both_served() {
    assert!((TEST_REGISTRY.abilities_for)(&terminal_code(1)).is_some());
    // Roland Banks 01001's ability is implemented in `impls/roland_banks.rs`.
    assert!((TEST_REGISTRY.abilities_for)(&CardCode::new("01001")).is_some());
}
