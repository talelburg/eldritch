//! End-to-end `Trigger::OnSkillTestResolution` flow with a mock
//! `CardRegistry` covering one success-gated and one failure-gated
//! ability.
//!
//! Lives at `crates/game-core/tests/` so it runs in its own integration-
//! test process (separate `OnceLock<CardRegistry>`), letting it install
//! a mock registry without colliding with game-core's in-crate tests
//! or with other `tests/*.rs` files. Mirrors `activate_ability.rs`.
//!
//! No real card carries `Trigger::OnSkillTestResolution` yet — the
//! follow-up issue (#39 Deduction) is the first consumer. Until then,
//! mock cards are the only way to exercise the full path.

use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::dsl::{
    constant, discover_clue, modify, on_skill_test_resolution, Ability, LocationTarget,
    ModifierScope, Stat, TestOutcome,
};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, InvestigatorId, LocationId, Phase, SkillKind, TokenModifiers,
};
use game_core::test_support::{
    drive, drive_skill_test, perform_skill_test_no_commits, test_investigator, test_location,
    GameStateBuilder, ScriptedResolver,
};
use game_core::{
    assert_event, assert_event_count, assert_no_event, Action, InputResponse, PlayerAction,
};

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

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        BONUS_CLUE_SUCCESS => Some(vec![on_skill_test_resolution(
            TestOutcome::Success,
            discover_clue(LocationTarget::TestedLocation, 1),
        )]),
        BONUS_CLUE_FAILURE => Some(vec![on_skill_test_resolution(
            TestOutcome::Failure,
            discover_clue(LocationTarget::TestedLocation, 1),
        )]),
        MIXED_TRIGGERS => Some(vec![
            constant(modify(Stat::Intellect, 1, ModifierScope::WhileInPlay)),
            on_skill_test_resolution(
                TestOutcome::Success,
                discover_clue(LocationTarget::TestedLocation, 1),
            ),
        ]),
        _ => None,
    }
}

#[ctor::ctor(unsafe)]
fn install_mock_registry() {
    let _ = game_core::card_registry::install(CardRegistry {
        metadata_for: mock_metadata_for,
        abilities_for: mock_abilities_for,
        native_effect_for: |_| None,
        native_eligibility_for: |_| None,
    });
}

/// Build a state with the given hand, the investigator standing at
/// `LocationId(10)` (3 clues by default), a single-`Numeric(0)` chaos
/// bag (so token-modifier contribution is always 0), and clean
/// pending modifiers.
fn state_with_hand_and_location(
    hand: &[&str],
    initial_clues: u8,
) -> (game_core::GameState, InvestigatorId, LocationId) {
    let id = InvestigatorId(1);
    let loc = LocationId(10);
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc);
    inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
    let mut location = test_location(10, "Study");
    location.clues = initial_clues;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_turn_order([id])
        .with_investigator_turn(id)
        .with_investigator(inv)
        .with_location(location)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (state, id, loc)
}

/// Start an Intellect plain skill test (the [`perform_skill_test`] synthetic
/// entry point) against `difficulty` and drive it through with the supplied
/// commit codes.
fn skill_test_with_commits(
    state: game_core::GameState,
    id: InvestigatorId,
    difficulty: i8,
    commit: &[&str],
) -> game_core::ApplyResult {
    let mut resolver = ScriptedResolver::new();
    let codes: Vec<CardCode> = commit.iter().map(|c| CardCode::new(*c)).collect();
    resolver.commit_cards(&codes);
    drive_skill_test(state, id, SkillKind::Intellect, difficulty, resolver)
}

/// Drive a pre-built skill-test-initiating `action` (e.g. an Investigate
/// `ResolveInput`) through with the supplied commit codes.
fn drive_with_commits(
    state: game_core::GameState,
    action: Action,
    commit: &[&str],
) -> game_core::ApplyResult {
    let mut resolver = ScriptedResolver::new();
    let codes: Vec<CardCode> = commit.iter().map(|c| CardCode::new(*c)).collect();
    resolver.commit_cards(&codes);
    drive(state, action, resolver)
}

#[test]
fn success_gated_resolution_fires_on_a_passing_test() {
    // Base 3 + 0 (token) + 0 (mock has no icons) = 3 vs difficulty 2:
    // passes by 1. Committed BONUS_CLUE_SUCCESS triggers a 1-clue
    // discover at the tested location.
    let (state, id, loc) = state_with_hand_and_location(&[BONUS_CLUE_SUCCESS], 3);
    let result = skill_test_with_commits(state, id, 2, &[BONUS_CLUE_SUCCESS]);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, .. }
            if *investigator == id
    );
    // OnSkillTestResolution-driven discover lands at the tested
    // location. (Action follow-up is `None` for a bare
    // PerformSkillTest — only the OnResolution effect runs.)
    assert_eq!(result.state.locations[&loc].clues, 2);
    assert_eq!(result.state.investigators[&id].clues, 1);
    assert_event!(
        result.events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == id
    );
}

#[test]
fn success_gated_resolution_does_not_fire_on_a_failing_test() {
    // Difficulty 99: base 3 + 0 = 3, total 3 << 99, fails by 96.
    // Even though BONUS_CLUE_SUCCESS is committed, its trigger is
    // gated on Success — no fire.
    let (state, id, loc) = state_with_hand_and_location(&[BONUS_CLUE_SUCCESS], 3);
    let result = skill_test_with_commits(state, id, 99, &[BONUS_CLUE_SUCCESS]);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, .. }
            if *investigator == id
    );
    assert_eq!(result.state.locations[&loc].clues, 3, "no clue discovered");
    assert_eq!(result.state.investigators[&id].clues, 0);
    assert_no_event!(result.events, Event::CluePlaced { .. });
}

#[test]
fn failure_gated_resolution_fires_on_a_failing_test() {
    let (state, id, loc) = state_with_hand_and_location(&[BONUS_CLUE_FAILURE], 3);
    let result = skill_test_with_commits(state, id, 99, &[BONUS_CLUE_FAILURE]);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, .. }
            if *investigator == id
    );
    assert_eq!(result.state.locations[&loc].clues, 2);
    assert_eq!(result.state.investigators[&id].clues, 1);
}

#[test]
fn failure_gated_resolution_does_not_fire_on_a_passing_test() {
    let (state, id, loc) = state_with_hand_and_location(&[BONUS_CLUE_FAILURE], 3);
    let result = skill_test_with_commits(state, id, 2, &[BONUS_CLUE_FAILURE]);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, .. }
            if *investigator == id
    );
    assert_eq!(result.state.locations[&loc].clues, 3);
    assert_eq!(result.state.investigators[&id].clues, 0);
    assert_no_event!(result.events, Event::CluePlaced { .. });
}

#[test]
fn uncommitted_resolution_triggered_card_does_not_fire() {
    // BONUS_CLUE_SUCCESS in hand but not committed → no fire.
    let (state, id, loc) = state_with_hand_and_location(&[BONUS_CLUE_SUCCESS], 3);
    // Apply with an empty commit list (apply_no_commits drives the
    // commit window with `[]`).
    let result = perform_skill_test_no_commits(state, id, SkillKind::Intellect, 2);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, .. } if *investigator == id
    );
    assert_eq!(result.state.locations[&loc].clues, 3);
    assert_no_event!(result.events, Event::CluePlaced { .. });
    // The uncommitted card stays in hand.
    assert_eq!(
        result.state.investigators[&id].hand,
        vec![CardCode::new(BONUS_CLUE_SUCCESS)]
    );
}

#[test]
fn two_committed_success_triggers_both_fire() {
    // Two copies of the same trigger in hand, both committed: both
    // fire, discovering 2 clues total (1 each).
    let (state, id, loc) =
        state_with_hand_and_location(&[BONUS_CLUE_SUCCESS, BONUS_CLUE_SUCCESS], 3);
    let result = skill_test_with_commits(state, id, 2, &[BONUS_CLUE_SUCCESS, BONUS_CLUE_SUCCESS]);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(result.state.locations[&loc].clues, 1);
    assert_eq!(result.state.investigators[&id].clues, 2);
    assert_event_count!(result.events, 2, Event::CluePlaced { .. });
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
    let (state, id, loc) = state_with_hand_and_location(&[MIXED_TRIGGERS], 3);
    let result = skill_test_with_commits(state, id, 2, &[MIXED_TRIGGERS]);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, .. } if *investigator == id
    );
    // Exactly one bonus clue from the OnSkillTestResolution ability;
    // the constant modifier doesn't emit anything.
    assert_eq!(result.state.locations[&loc].clues, 2);
    assert_eq!(result.state.investigators[&id].clues, 1);
    assert_event_count!(result.events, 1, Event::CluePlaced { .. });
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
    let (state, id, loc) = state_with_hand_and_location(&[BONUS_CLUE_SUCCESS], 3);
    // Submit the open-turn Investigate via the enumeration round-trip (the typed
    // `PlayerAction::Investigate` removed in 2b, #447).
    let investigate = {
        use game_core::engine::enumerate::legal_actions;
        use game_core::engine::OptionId;
        use game_core::TurnAction;
        let idx = legal_actions(&state)
            .iter()
            .position(|a| a == &TurnAction::Investigate { investigator: id })
            .expect("Investigate must be a legal open-turn action");
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        })
    };
    let result = drive_with_commits(state, investigate, &[BONUS_CLUE_SUCCESS]);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(result.state.locations[&loc].clues, 1, "lost 2 clues total");
    assert_eq!(result.state.investigators[&id].clues, 2);

    // Find the indices of the milestone events to assert their order.
    let mut succeeded_idx: Option<usize> = None;
    let mut clue_indices: Vec<usize> = Vec::new();
    let mut location_clue_indices: Vec<usize> = Vec::new();
    let mut discarded_idx: Option<usize> = None;
    let mut ended_idx: Option<usize> = None;
    for (i, ev) in result.events.iter().enumerate() {
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
