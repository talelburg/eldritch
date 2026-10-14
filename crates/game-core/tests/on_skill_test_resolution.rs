//! End-to-end `Trigger::OnSkillTestResolution` flow with a mock
//! `CardRegistry` covering one success-gated and one failure-gated
//! ability.
//!
//! Lives at `crates/game-core/tests/` so it runs in its own integration-
//! test process (a process holds one `MockRegistry`), letting it build
//! a mock registry without colliding with game-core's in-crate tests
//! or with other `tests/*.rs` files. Mirrors `activate_ability.rs`.
//!
//! No real card carries `Trigger::OnSkillTestResolution` yet — the
//! follow-up issue (#39 Deduction) is the first consumer. Until then,
//! mock cards are the only way to exercise the full path.

use std::sync::LazyLock;

use card_dsl::dsl::{self, LocationTarget, ModifierScope, Stat, TestOutcome};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, SkillKind,
    TokenModifiers,
};
use game_core::test_support::{self, seat, MockRegistry, TestSession};
use game_core::{assert_event, assert_event_count, assert_no_event};

/// Mock: success-gated `OnSkillTestResolution` → discover 1 clue at
/// the tested location. The Deduction-shape (without kind narrowing,
/// which the trigger doesn't yet support and the consumer card will
/// add via an `If` over a kind condition).
const BONUS_CLUE_SUCCESS: &str = "MOCK-OSR-S";

/// Mock: failure-gated mirror.
const BONUS_CLUE_FAILURE: &str = "MOCK-OSR-F";

/// Mock: a constant +1 intellect modifier (passive, not a resolution
/// trigger) PLUS a success-gated `OnSkillTestResolution`. Exercises
/// the inner trigger-filter inside `fire_on_skill_test_resolution`:
/// the constant ability must NOT fire as a resolution trigger.
const MIXED_TRIGGERS: &str = "MOCK-OSR-MIXED";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(BONUS_CLUE_SUCCESS, || {
                vec![dsl::on_skill_test_resolution(
                    TestOutcome::Success,
                    dsl::discover_clue(LocationTarget::TestedLocation, 1),
                )]
            })
            .with_abilities(BONUS_CLUE_FAILURE, || {
                vec![dsl::on_skill_test_resolution(
                    TestOutcome::Failure,
                    dsl::discover_clue(LocationTarget::TestedLocation, 1),
                )]
            })
            .with_abilities(MIXED_TRIGGERS, || {
                vec![
                    dsl::constant(dsl::modify(Stat::Intellect, 1, ModifierScope::WhileInPlay)),
                    dsl::on_skill_test_resolution(
                        TestOutcome::Success,
                        dsl::discover_clue(LocationTarget::TestedLocation, 1),
                    ),
                ]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// `INV`'s open turn with `hand` in hand, standing at `HERE` (3 clues), a
/// single-`Numeric(0)` chaos bag (so token-modifier contribution is always 0),
/// and clean pending modifiers.
fn session_with_hand(hand: &[&str]) -> TestSession {
    let mut location = test_support::test_location(HERE.0, "Study");
    location.clues = 3;
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE).hand(hand.iter().copied()))
        .with_location(location)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
}

/// Run an Intellect plain skill test against `difficulty` from `INV`'s turn
/// with `hand` in hand, committing `commit`. The session comes back to the
/// turn menu.
fn skill_test_with_commits(hand: &[&str], difficulty: i8, commit: &[&str]) -> TestSession {
    session_with_hand(hand)
        .test_skill(INV, SkillKind::Intellect, difficulty)
        .commit(commit)
}

#[test]
fn success_gated_resolution_fires_on_a_passing_test() {
    // Base 3 + 0 (token) + 0 (mock has no icons) = 3 vs difficulty 2:
    // passes by 1. Committed BONUS_CLUE_SUCCESS triggers a 1-clue
    // discover at the tested location.
    let session = skill_test_with_commits(&[BONUS_CLUE_SUCCESS], 2, &[BONUS_CLUE_SUCCESS]);

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, .. }
            if *investigator == INV
    );
    // OnSkillTestResolution-driven discover lands at the tested
    // location. (Action follow-up is `None` for a bare
    // plain skill test — only the OnResolution effect runs.)
    assert_eq!(session.state().locations[&HERE].clues, 2);
    assert_eq!(session.state().investigators[&INV].clues, 1);
    assert_event!(
        session.events(),
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV
    );
}

#[test]
fn success_gated_resolution_does_not_fire_on_a_failing_test() {
    // Difficulty 99: base 3 + 0 = 3, total 3 << 99, fails by 96.
    // Even though BONUS_CLUE_SUCCESS is committed, its trigger is
    // gated on Success — no fire.
    let session = skill_test_with_commits(&[BONUS_CLUE_SUCCESS], 99, &[BONUS_CLUE_SUCCESS]);

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, .. }
            if *investigator == INV
    );
    assert_eq!(
        session.state().locations[&HERE].clues,
        3,
        "no clue discovered"
    );
    assert_eq!(session.state().investigators[&INV].clues, 0);
    assert_no_event!(session.events(), Event::CluePlaced { .. });
}

#[test]
fn failure_gated_resolution_fires_on_a_failing_test() {
    let session = skill_test_with_commits(&[BONUS_CLUE_FAILURE], 99, &[BONUS_CLUE_FAILURE]);

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, .. }
            if *investigator == INV
    );
    assert_eq!(session.state().locations[&HERE].clues, 2);
    assert_eq!(session.state().investigators[&INV].clues, 1);
}

#[test]
fn failure_gated_resolution_does_not_fire_on_a_passing_test() {
    let session = skill_test_with_commits(&[BONUS_CLUE_FAILURE], 2, &[BONUS_CLUE_FAILURE]);

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, .. }
            if *investigator == INV
    );
    assert_eq!(session.state().locations[&HERE].clues, 3);
    assert_eq!(session.state().investigators[&INV].clues, 0);
    assert_no_event!(session.events(), Event::CluePlaced { .. });
}

#[test]
fn uncommitted_resolution_triggered_card_does_not_fire() {
    // BONUS_CLUE_SUCCESS in hand but not committed → no fire.
    let session = skill_test_with_commits(&[BONUS_CLUE_SUCCESS], 2, &[]);

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, .. } if *investigator == INV
    );
    assert_eq!(session.state().locations[&HERE].clues, 3);
    assert_no_event!(session.events(), Event::CluePlaced { .. });
    // The uncommitted card stays in hand.
    assert_eq!(
        session.state().investigators[&INV].hand,
        vec![CardCode::new(BONUS_CLUE_SUCCESS)]
    );
}

#[test]
fn two_committed_success_triggers_both_fire() {
    // Two copies of the same trigger in hand, both committed: both
    // fire, discovering 2 clues total (1 each).
    let session = skill_test_with_commits(
        &[BONUS_CLUE_SUCCESS, BONUS_CLUE_SUCCESS],
        2,
        &[BONUS_CLUE_SUCCESS, BONUS_CLUE_SUCCESS],
    );

    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().locations[&HERE].clues, 1);
    assert_eq!(session.state().investigators[&INV].clues, 2);
    assert_event_count!(session.events(), 2, Event::CluePlaced { .. });
}

#[test]
fn mixed_triggers_card_only_fires_the_resolution_ability() {
    // A card carrying a constant modifier AND an OnSkillTestResolution
    // ability: only the resolution ability should fire from the
    // fire-loop. The constant modifier is already a passive
    // contribution (would be queried via `constant_skill_modifier`
    // if the card were in play, but it's in hand here so doesn't
    // contribute to the test total either — the test just confirms
    // the trigger-filter inside `fire_on_skill_test_resolution` skips
    // non-matching trigger variants on the same card).
    let session = skill_test_with_commits(&[MIXED_TRIGGERS], 2, &[MIXED_TRIGGERS]);

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, .. } if *investigator == INV
    );
    // Exactly one bonus clue from the OnSkillTestResolution ability;
    // the constant modifier doesn't emit anything.
    assert_eq!(session.state().locations[&HERE].clues, 2);
    assert_eq!(session.state().investigators[&INV].clues, 1);
    assert_event_count!(session.events(), 1, Event::CluePlaced { .. });
}

#[test]
fn investigate_canonical_event_order_with_on_resolution() {
    // Investigate (action follow-up = `Investigate`) plus a committed
    // BONUS_CLUE_SUCCESS. Verify the event order:
    //   SkillTestSucceeded
    //     → CluePlaced (action follow-up's 1 clue)
    //     → LocationCluesChanged (after follow-up)
    //     → CluePlaced (OnSkillTestResolution's bonus clue)
    //     → LocationCluesChanged (after bonus)
    //     → CardDiscarded (committed card)
    //     → SkillTestEnded
    //
    // The two CluePlaced events distinguish their source by ordering
    // alone (both name the same investigator + count = 1), so this
    // pin is essential as the spec for downstream listeners.
    let session = session_with_hand(&[BONUS_CLUE_SUCCESS])
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[BONUS_CLUE_SUCCESS]);

    session.assert_at_turn_menu(INV);
    assert_eq!(
        session.state().locations[&HERE].clues,
        1,
        "lost 2 clues total"
    );
    assert_eq!(session.state().investigators[&INV].clues, 2);

    // Find the indices of the milestone events to assert their order.
    let mut succeeded_idx: Option<usize> = None;
    let mut clue_indices: Vec<usize> = Vec::new();
    let mut location_clue_indices: Vec<usize> = Vec::new();
    let mut discarded_idx: Option<usize> = None;
    let mut ended_idx: Option<usize> = None;
    for (i, ev) in session.events().iter().enumerate() {
        match ev {
            Event::SkillTestSucceeded { .. } => succeeded_idx = Some(i),
            Event::CluePlaced { .. } => clue_indices.push(i),
            Event::LocationCluesChanged { .. } => location_clue_indices.push(i),
            Event::CardDiscarded { .. } => discarded_idx = Some(i),
            Event::SkillTestEnded { .. } => ended_idx = Some(i),
            _ => {}
        }
    }
    let succeeded = succeeded_idx.expect("expected SkillTestSucceeded");
    let discarded = discarded_idx.expect("expected CardDiscarded");
    let ended = ended_idx.expect("expected SkillTestEnded");
    assert_eq!(clue_indices.len(), 2, "two CluePlaced (follow-up + bonus)");
    assert_eq!(
        location_clue_indices.len(),
        2,
        "two LocationCluesChanged (one per clue)"
    );

    assert!(succeeded < clue_indices[0]);
    assert!(clue_indices[0] < clue_indices[1]);
    assert!(clue_indices[1] < discarded);
    assert!(discarded < ended);
    // First LocationCluesChanged sits between the two CluePlaced;
    // second sits between the second CluePlaced and CardDiscarded.
    assert!(clue_indices[0] < location_clue_indices[0]);
    assert!(location_clue_indices[0] < clue_indices[1]);
    assert!(clue_indices[1] < location_clue_indices[1]);
    assert!(location_clue_indices[1] < discarded);
}
