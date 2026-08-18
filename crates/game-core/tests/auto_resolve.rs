//! End-to-end `Effect::AutoResolve` — a card latching the in-flight skill
//! test's **determination** — with a mock `CardRegistry`.
//!
//! `data/rules-reference/rules/glossary/Automatic_Failure_Success.md`:
//!
//! > Some card or token abilities may cause a skill test to automatically
//! > fail or to automatically succeed.
//! >
//! > - If a skill test automatically fails, the investigator's total skill
//! >   value for that test is considered 0.
//! > - If a skill test automatically succeeds, the total difficulty of that
//! >   test is considered 0.
//!
//! Lives at `crates/game-core/tests/` (its own integration-test binary,
//! hence its own process and its own `OnceLock<CardRegistry>`) so the mock
//! registry doesn't collide with game-core's in-crate tests. Mirrors
//! `activate_ability.rs`.
//!
//! Every assertion is at the `apply()` boundary, on events crossing it —
//! what a player would see — rather than on the shape of the recorded-row
//! population or the name of the query behind it (#630's testing decisions).
//! The abilities are constructed inline from `card-dsl`, so no corpus card
//! is involved; the real consumers (Rex Murphy 02002, Stroke of Luck 02271)
//! are out of scope here.
//!
//! **Not covered here:** the ST.3/ST.4 skip. A determination latched before
//! the reveal still draws a chaos token today, which is rules-incorrect and
//! is #687's to fix.

use game_core::card_data::CardMetadata;
use game_core::card_data::{CardKind, Class, SkillIcons};
use game_core::card_registry::CardRegistry;
use game_core::dsl::{
    activated, auto_resolve, gain_resources, on_play, seq, Ability, Cost, Determination,
    InvestigatorTarget,
};
use game_core::engine::{legal_actions, EngineOutcome};
use game_core::event::{Event, FailureReason};
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, InvestigatorId, Phase, SkillKind,
    TokenModifiers,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, drive_skill_test, perform_skill_test_no_commits,
    test_investigator, GameStateBuilder, TakeOneFastPlay,
};
use game_core::TurnAction;
use game_core::{assert_event, assert_event_count, assert_no_event};

/// Mock asset: `[fast] Spend 1 resource: this skill test automatically
/// fails.` Rex-Murphy-shaped, minus the elder-sign trigger and the draw.
const AUTO_FAIL: &str = "MOCK-AF";

/// Mock asset: `[fast] Spend 1 resource: this skill test is automatically
/// successful.` Stroke-of-Luck-shaped, minus the exile and the parenthetical.
const AUTO_SUCCEED: &str = "MOCK-AS";

/// Mock asset latching **both** determinations in one activation, failure
/// first. Exists to reach the precedence rule from one player input.
const FAIL_THEN_SUCCEED: &str = "MOCK-AFS";

/// The same pair in the other order. Precedence is resolved at read time, so
/// the two must agree; a write-time suppression would not make them.
const SUCCEED_THEN_FAIL: &str = "MOCK-ASF";

/// Mock **event** card: `[fast]` *"this test is automatically successful"*,
/// played from hand at the running test's player window. Stroke of Luck
/// 02271's shape, and a *second trigger* — `OnPlay` rather than `Activated` —
/// reaching the same effect, which is what "no enumerated window list" buys.
/// It also has no in-play instance, so it exercises the unattributed latch.
const PLAY_AUTO_SUCCEED: &str = "MOCK-PLAY-AS";

/// Mock asset whose activated ability gains a resource — a control for the
/// initiation gate, which must keep offering an ability that *doesn't* need
/// a test in flight.
const PLAIN_GAIN: &str = "MOCK-GAIN";

/// Metadata for [`PLAY_AUTO_SUCCEED`] — the one mock here that is *played*
/// rather than activated, so the play gate reads its type, cost and `[fast]`
/// flag. Every other mock is an in-play instance the registry only needs
/// abilities for.
fn play_auto_succeed_metadata() -> CardMetadata {
    CardMetadata {
        code: PLAY_AUTO_SUCCEED.to_owned(),
        name: "Mock Stroke of Luck".to_owned(),
        traits: vec!["Fortune".to_owned()],
        text: Some("This test is automatically successful.".to_owned()),
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: false,
        kind: CardKind::Event {
            class: Class::Rogue,
            cost: Some(0),
            xp: Some(0),
            skill_icons: SkillIcons::default(),
            is_fast: true,
            deck_limit: 2,
            play_only_during_turn: false,
        },
    }
}

fn mock_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    static M: std::sync::OnceLock<CardMetadata> = std::sync::OnceLock::new();
    (code.as_str() == PLAY_AUTO_SUCCEED).then(|| M.get_or_init(play_auto_succeed_metadata))
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        AUTO_FAIL => Some(vec![activated(
            0,
            vec![Cost::Resources(1)],
            auto_resolve(Determination::AutomaticFailure),
        )]),
        AUTO_SUCCEED => Some(vec![activated(
            0,
            vec![Cost::Resources(1)],
            auto_resolve(Determination::AutomaticSuccess),
        )]),
        FAIL_THEN_SUCCEED => Some(vec![activated(
            0,
            vec![Cost::Resources(1)],
            seq([
                auto_resolve(Determination::AutomaticFailure),
                auto_resolve(Determination::AutomaticSuccess),
            ]),
        )]),
        SUCCEED_THEN_FAIL => Some(vec![activated(
            0,
            vec![Cost::Resources(1)],
            seq([
                auto_resolve(Determination::AutomaticSuccess),
                auto_resolve(Determination::AutomaticFailure),
            ]),
        )]),
        PLAY_AUTO_SUCCEED => Some(vec![on_play(auto_resolve(Determination::AutomaticSuccess))]),
        PLAIN_GAIN => Some(vec![activated(
            0,
            vec![Cost::Resources(1)],
            gain_resources(InvestigatorTarget::You, 1),
        )]),
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
        native_condition_for: |_| None,
    });
}

/// Board: the controller with one in-play instance of `code` (instance 0),
/// willpower 5, and a single-`Numeric(0)` chaos bag so the token contributes
/// nothing and every number below is the card's doing.
fn board(code: &str) -> (game_core::GameState, InvestigatorId, CardInstanceId) {
    let id = InvestigatorId(1);
    let instance_id = CardInstanceId(0);
    let mut inv = test_investigator(1);
    inv.skills.willpower = 5;
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(code), instance_id));

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_investigator_turn(id)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (state, id, instance_id)
}

/// Run a willpower test against `difficulty`, taking the card's offered fast
/// activation at the test's player window.
fn test_taking_the_fast_play(
    state: game_core::GameState,
    id: InvestigatorId,
    difficulty: i8,
) -> game_core::ApplyResult {
    drive_skill_test(
        state,
        id,
        SkillKind::Willpower,
        difficulty,
        TakeOneFastPlay::at_index(0),
    )
}

/// *"the investigator's total skill value for that test is considered 0"* —
/// so willpower 5 against difficulty 3 does not merely fail, it fails **by
/// 3**, which an "if you fail by 2 or more" clause on the same test reads.
#[test]
fn a_card_latched_automatic_failure_fails_the_test_with_the_real_margin() {
    let (state, id, _) = board(AUTO_FAIL);
    let result = test_taking_the_fast_play(state, id, 3);

    assert_event!(
        result.events,
        Event::SkillTestFailed {
            investigator,
            skill: SkillKind::Willpower,
            reason: FailureReason::AutoFail,
            by: 3,
        } if *investigator == id
    );
    assert_no_event!(result.events, Event::SkillTestSucceeded { .. });
}

/// *"the total difficulty of that test is considered 0"* — willpower 5
/// against difficulty 9 is a loss on the numbers and a success on the card.
#[test]
fn a_card_latched_automatic_success_passes_a_test_it_would_have_lost() {
    let (state, id, _) = board(AUTO_SUCCEED);
    let result = test_taking_the_fast_play(state, id, 9);

    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Willpower, .. }
            if *investigator == id
    );
    assert_no_event!(result.events, Event::SkillTestFailed { .. });
}

/// Automatic failure beats automatic success. Latched failure-first.
#[test]
fn automatic_failure_beats_automatic_success_latched_failure_first() {
    let (state, id, _) = board(FAIL_THEN_SUCCEED);
    let result = test_taking_the_fast_play(state, id, 3);

    assert_event!(
        result.events,
        Event::SkillTestFailed {
            reason: FailureReason::AutoFail,
            by: 3,
            ..
        }
    );
    assert_no_event!(result.events, Event::SkillTestSucceeded { .. });
}

/// The same board with the two latches swapped. Precedence is resolved at
/// **read** time, so neither row suppresses nor overwrites the other and the
/// verdict cannot depend on which card resolved first — a write-time
/// suppression would pass the test above and fail this one.
#[test]
fn automatic_failure_beats_automatic_success_latched_success_first() {
    let (state, id, _) = board(SUCCEED_THEN_FAIL);
    let result = test_taking_the_fast_play(state, id, 3);

    assert_event!(
        result.events,
        Event::SkillTestFailed {
            reason: FailureReason::AutoFail,
            by: 3,
            ..
        }
    );
    assert_no_event!(result.events, Event::SkillTestSucceeded { .. });
    assert_event_count!(
        result.events,
        2,
        Event::SkillTestDeterminationLatched { .. }
    );
}

/// The engine enumerates no legal latch windows: the moment comes from the
/// declaring card's own trigger. Here that is `OnPlay` — a `[fast]` event
/// played from hand at the running test's player window, Stroke of Luck
/// 02271's shape — rather than the `Activated` trigger every other test in
/// this file uses. The corpus range is wider still: Delusory Evils 52065
/// reacts at ST.6.
#[test]
fn a_determination_can_be_latched_from_a_play_trigger() {
    let id = InvestigatorId(1);
    let mut inv = test_investigator(1);
    inv.skills.willpower = 5;
    inv.hand = vec![CardCode::new(PLAY_AUTO_SUCCEED)];
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_investigator_turn(id)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    let result = test_taking_the_fast_play(state, id, 9);

    // No in-play instance to name: an event resolving out of hand latches an
    // unattributed determination, exactly as the row records it.
    assert_event!(
        result.events,
        Event::SkillTestDeterminationLatched {
            determination: Determination::AutomaticSuccess,
            source: None,
            ..
        }
    );
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, .. } if *investigator == id
    );
    assert_no_event!(result.events, Event::SkillTestFailed { .. });
}

/// A determination with no test in flight has no skill-test identity to be
/// stamped with, so it is **refused** rather than banked — otherwise it would
/// surface as an unexplained automatic result on whatever test came next.
/// State and events are unchanged: the cost is not paid.
#[test]
fn latching_a_determination_outside_a_test_is_rejected() {
    let (state, id, instance_id) = board(AUTO_SUCCEED);
    let resources_before = state.investigators[&id].resources;
    let action = TurnAction::ActivateAbility {
        investigator: id,
        instance_id,
        ability_index: 0,
    };

    assert!(
        !legal_actions(&state).contains(&action),
        "the open-turn menu must not offer an activation that would reject",
    );

    let result = dispatch_turn_action_unchecked(state, &action);
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty(), "a rejection emits no events");
    assert_eq!(
        result.state.investigators[&id].resources, resources_before,
        "a rejection leaves state unchanged — the cost is not paid",
    );
    assert!(
        result.state.recorded_modifiers.is_empty(),
        "no determination was banked for a later test",
    );
}

/// The initiation gate above narrows on *this* effect, not on activation in
/// general: an ability with nothing to say about skill tests is still offered
/// outside one.
#[test]
fn an_unrelated_activation_is_still_offered_outside_a_test() {
    let (state, id, instance_id) = board(PLAIN_GAIN);
    assert!(
        legal_actions(&state).contains(&TurnAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: 0,
        })
    );
}

/// A determination is a test-scoped row, so it expires with the test that
/// carried it. A leak here would automatically succeed the *next* test —
/// the same guard `an_autofail_determination_does_not_outlive_its_test`
/// gives the token's row.
#[test]
fn a_card_latched_determination_does_not_survive_into_a_later_test() {
    let (state, id, _) = board(AUTO_SUCCEED);
    let first = test_taking_the_fast_play(state, id, 9);
    assert_event!(first.events, Event::SkillTestSucceeded { .. });
    assert!(
        first.state.recorded_modifiers.is_empty(),
        "the determination expires with the test's teardown",
    );

    // The second test declines the fast window, so nothing latches: willpower
    // 5 against difficulty 9 fails by 4 on the numbers.
    let second = perform_skill_test_no_commits(first.state, id, SkillKind::Willpower, 9);
    assert_event!(
        second.events,
        Event::SkillTestFailed {
            reason: FailureReason::Total,
            by: 4,
            ..
        }
    );
    assert_no_event!(second.events, Event::SkillTestDeterminationLatched { .. });
}

/// The event log is the client's only narrative channel: without this event
/// an automatic success renders as a win with a dash where the chaos token
/// would be and no stated cause. It names the determination and the in-play
/// instance that latched it.
#[test]
fn the_latch_event_names_the_determination_and_its_source() {
    let (state, id, instance_id) = board(AUTO_SUCCEED);
    let result = test_taking_the_fast_play(state, id, 9);

    assert_event!(
        result.events,
        Event::SkillTestDeterminationLatched {
            investigator,
            determination: Determination::AutomaticSuccess,
            source: Some(src),
        } if *investigator == id && *src == instance_id
    );
    assert_event_count!(
        result.events,
        1,
        Event::SkillTestDeterminationLatched { .. }
    );
}

/// The `[auto_fail]` chaos token latches the same determination through the
/// same row (#685), but emits no latch event: its cause is already on the log
/// as the `ChaosTokenRevealed` carrying `TokenResolution::AutoFail`, and a
/// second event for the same moment would read as two determinations.
#[test]
fn the_auto_fail_token_latches_no_second_event() {
    let (state, id, _) = board(PLAIN_GAIN);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(state.investigators[&id].clone())
        .with_active_investigator(id)
        .with_investigator_turn(id)
        .with_chaos_bag(ChaosBag::new([ChaosToken::AutoFail]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    let result = perform_skill_test_no_commits(state, id, SkillKind::Willpower, 3);

    assert_event!(
        result.events,
        Event::SkillTestFailed {
            reason: FailureReason::AutoFail,
            by: 3,
            ..
        }
    );
    assert_no_event!(result.events, Event::SkillTestDeterminationLatched { .. });
}
