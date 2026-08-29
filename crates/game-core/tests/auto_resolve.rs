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
//! The ST.3/ST.4 **skip** lives here too (#687):
//!
//! > If it is known that an investigator automatically succeeds or fails at
//! > a skill test before Step 3 ("Reveal chaos token") occurs, that step is
//! > skipped, along with Step 4. No chaos token(s) are revealed from the
//! > chaos bag, and the investigator immediately moves to Step 5. All other
//! > steps of the skill test resolve as normal.
//! >
//! > If a chaos token effect causes an investigator to automatically succeed
//! > or fail at a skill test, continue with Steps 3 and 4, as normal.
//!
//! The two clauses are the same determination query asked at two different
//! moments, so they are asserted the same way: on the presence or absence of
//! `ChaosTokenRevealed` and of the ST.4 symbol effect, never on the RNG
//! cursor. The `[skull]` symbol effect comes from a mock scenario module
//! installed alongside the mock card registry, for the same
//! process-isolation reason.

use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::card_data::CardMetadata;
use game_core::card_data::{CardKind, Class, SkillIcons};
use game_core::card_registry::CardRegistry;
use game_core::dsl::{
    activated, auto_resolve, gain_resources, on_play, on_skill_test_resolution, seq, Ability, Cost,
    Determination, InvestigatorTarget, TestOutcome,
};
use game_core::engine::{legal_actions, EngineOutcome, InputKind, InputRequest, OptionId};
use game_core::event::{Event, FailureReason};
use game_core::scenario::{
    ScenarioId, ScenarioModule, ScenarioRegistry, SymbolCtx, SymbolOutcome, TokenEffect,
};
use game_core::state::{
    AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, GameState,
    InvestigatorId, LocationId, Phase, SkillKind, TokenModifiers, TokenResolution, Zone,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, drive, drive_skill_test, metadata_for_test_inv,
    perform_skill_test_no_commits, test_investigator, test_location, ChoiceResolver,
    GameStateBuilder, TakeOneFastPlay,
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

/// Mock card with no abilities at all, committed to a test purely to be
/// discarded at ST.8.
const FILLER: &str = "MOCK-FILLER";

/// Mock card whose `OnSkillTestResolution` trigger gains a resource on a
/// successful test — an end-of-test step that must still run when the draw
/// was skipped.
const ON_RESOLUTION_GAIN: &str = "MOCK-OSR-GAIN";

/// The mock scenario whose `[skull]` deals 1 damage immediately, so a
/// skipped ST.4 has an observable absence. The Gathering's `[tablet]`
/// shape, minus the Ghoul condition.
const SYMBOL_SCENARIO: &str = "_mock_symbols";

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
    // `TEST_INV` first: the symbol token's damage reads the tester's
    // `max_health()` through the registry, which would otherwise not know
    // the fixture investigator's card.
    metadata_for_test_inv(code).or_else(|| {
        (code.as_str() == PLAY_AUTO_SUCCEED).then(|| M.get_or_init(play_auto_succeed_metadata))
    })
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
        ON_RESOLUTION_GAIN => Some(vec![on_skill_test_resolution(
            TestOutcome::Success,
            gain_resources(InvestigatorTarget::You, 1),
        )]),
        _ => None,
    }
}

/// Mock symbol hook: a `[skull]` contributes nothing to the total and deals
/// 1 damage at ST.4.
fn mock_resolve_symbol(token: ChaosToken, _ctx: &SymbolCtx) -> SymbolOutcome {
    match token {
        ChaosToken::Skull => SymbolOutcome {
            modifier: 0,
            immediate: vec![TokenEffect::Damage(1)],
            on_fail: vec![],
        },
        _ => SymbolOutcome::default(),
    }
}

fn mock_setup() -> GameState {
    GameStateBuilder::new().build()
}

fn mock_apply_resolution(
    _: game_core::scenario::ScenarioEnding,
    _: &mut GameState,
    _: &mut Vec<Event>,
) {
}

static SYMBOL_MODULE: ScenarioModule = ScenarioModule {
    resolve_symbol: Some(mock_resolve_symbol),
    setup: mock_setup,
    apply_resolution: mock_apply_resolution,
};

fn mock_module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    (id.as_str() == SYMBOL_SCENARIO).then_some(&SYMBOL_MODULE)
}

#[ctor::ctor(unsafe)]
fn install_mock_registry() {
    let _ = game_core::card_registry::install(CardRegistry {
        metadata_for: mock_metadata_for,
        abilities_for: mock_abilities_for,
        ..CardRegistry::EMPTY
    });
    let _ = game_core::scenario_registry::install(ScenarioRegistry {
        module_for: mock_module_for,
    });
}

/// The one board every test here builds on: the controller at willpower 5,
/// holding `hand`, with an in-play instance of each code in `in_play`
/// (instance ids ascending from 0), drawing from `bag`.
fn board_with(
    in_play: &[&str],
    hand: &[&str],
    bag: ChaosBag,
) -> (game_core::GameState, InvestigatorId) {
    let id = InvestigatorId(1);
    let mut inv = test_investigator(1);
    inv.skills.willpower = 5;
    inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
    for (i, code) in in_play.iter().enumerate() {
        inv.cards_in_play.push(CardInPlay::enter_play(
            CardCode::new(*code),
            CardInstanceId(u32::try_from(i).expect("a handful of mocks")),
        ));
    }

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_investigator_turn(id)
        .with_chaos_bag(bag)
        .with_token_modifiers(TokenModifiers::default())
        // Named so the mock module's symbol hook is reachable; only a
        // `[skull]` in the bag actually consults it.
        .with_scenario_id(ScenarioId::new(SYMBOL_SCENARIO))
        .build();
    (state, id)
}

/// [`board_with`] plus a location the investigator stands at, holding one
/// clue behind `shroud` — the board an Investigate action needs. Bag is a
/// single `Numeric(0)`.
fn investigate_board(hand: &[&str], shroud: u8) -> (GameState, InvestigatorId, LocationId) {
    let id = InvestigatorId(1);
    let loc = LocationId(10);
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc);
    inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
    let mut location = test_location(10, "Study");
    location.clues = 1;
    location.shroud = shroud;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_turn_order([id])
        .with_investigator_turn(id)
        .with_location(location)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .with_scenario_id(ScenarioId::new(SYMBOL_SCENARIO))
        .build();
    (state, id, loc)
}

/// A [`ChoiceResolver`] that takes the first offered fast play *and* commits
/// a named set of cards at the commit window, declining every other window.
///
/// [`TakeOneFastPlay`] commits nothing and [`ScriptedResolver`] cannot pin
/// down a fast window's prompt count; the skipped-draw tests need both at
/// once — a card latching the determination before ST.3, and a card
/// committed to the test it resolves.
///
/// [`ScriptedResolver`]: game_core::test_support::ScriptedResolver
struct FastPlayAndCommit {
    commit: Vec<CardCode>,
    used: bool,
}

impl FastPlayAndCommit {
    fn committing(codes: &[&str]) -> Self {
        Self {
            commit: codes.iter().map(|c| CardCode::new(*c)).collect(),
            used: false,
        }
    }
}

impl ChoiceResolver for FastPlayAndCommit {
    fn next(&mut self, request: &InputRequest, state: &GameState) -> InputResponse {
        // Assumes the first skippable `PickSingle` offering anything is the
        // test's fast window — true for every board built in this file, where
        // nothing else opens one.
        if request.skippable {
            if !self.used && request.kind == InputKind::PickSingle && !request.options.is_empty() {
                self.used = true;
                return InputResponse::PickSingle(request.options[0].id);
            }
            return InputResponse::Skip;
        }
        match request.kind {
            InputKind::Confirm => InputResponse::Confirm,
            // The commit window: hand indices, resolved against the hand as
            // it stands now (the fast play above already left it).
            InputKind::PickMultiple => {
                let inv = state
                    .current_skill_test()
                    .and_then(|t| state.investigators.get(&t.investigator))
                    .expect("the commit window implies an in-flight test");
                let mut used = vec![false; inv.hand.len()];
                let selected = self
                    .commit
                    .iter()
                    .map(|code| {
                        let i = inv
                            .hand
                            .iter()
                            .enumerate()
                            .find_map(|(i, c)| (!used[i] && c == code).then_some(i))
                            .unwrap_or_else(|| {
                                panic!("commit code {code:?} not in hand {:?}", inv.hand)
                            });
                        used[i] = true;
                        OptionId(u32::try_from(i).expect("hand index fits u32"))
                    })
                    .collect();
                InputResponse::PickMultiple { selected }
            }
            other => panic!(
                "FastPlayAndCommit: unexpected non-skippable {other:?}: {:?}",
                request.prompt
            ),
        }
    }
}

/// [`board_with`] for the common case: one in-play instance of `code`
/// (instance 0), empty hand, and a single-`Numeric(0)` chaos bag so the token
/// contributes nothing and every number below is the card's doing.
fn board(code: &str) -> (game_core::GameState, InvestigatorId, CardInstanceId) {
    let (state, id) = board_with(&[code], &[], ChaosBag::new([ChaosToken::Numeric(0)]));
    (state, id, CardInstanceId(0))
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
/// this file uses. The snapshot's latch range is wider still: Delusory Evils
/// 52065 reacts at ST.6.
#[test]
fn a_determination_can_be_latched_from_a_play_trigger() {
    let (state, id) = board_with(
        &[],
        &[PLAY_AUTO_SUCCEED],
        ChaosBag::new([ChaosToken::Numeric(0)]),
    );

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
        source: AbilitySource::InPlay(instance_id),
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
            source: AbilitySource::InPlay(instance_id),
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
    let (state, id) = board_with(&[], &[], ChaosBag::new([ChaosToken::AutoFail]));
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

// ---------------------------------------------------------------------------
// The ST.3/ST.4 skip (#687)
// ---------------------------------------------------------------------------

/// A determination latched before the driver reaches the resolution step
/// skips ST.3 outright: *"No chaos token(s) are revealed from the chaos
/// bag"*. Asserted as the absence of the reveal the player would have seen,
/// not by reading the RNG cursor — and the bag itself is untouched.
#[test]
fn a_determination_latched_before_the_reveal_draws_no_token() {
    let (state, id, _) = board(AUTO_SUCCEED);
    let bag_before = state.chaos_bag.clone();
    let result = test_taking_the_fast_play(state, id, 9);

    assert_no_event!(result.events, Event::ChaosTokenRevealed { .. });
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, .. } if *investigator == id
    );
    // The load-bearing assertion is the absent reveal above; this one pins
    // the bag's contents, which a draw never mutates anyway.
    assert_eq!(result.state.chaos_bag, bag_before);
}

/// *"that step is skipped, along with Step 4"* — the `[skull]`'s immediate
/// damage never lands, because the token that would have carried it was
/// never revealed.
#[test]
fn a_skipped_draw_pushes_no_chaos_symbol_effects() {
    let (state, id) = board_with(&[AUTO_SUCCEED], &[], ChaosBag::new([ChaosToken::Skull]));
    let result = test_taking_the_fast_play(state, id, 9);

    assert_no_event!(result.events, Event::ChaosTokenRevealed { .. });
    assert_no_event!(result.events, Event::DamageTaken { .. });
    assert_event!(result.events, Event::SkillTestSucceeded { .. });
}

/// The control for the test above: with nothing latched, the same board
/// reveals the `[skull]` and takes its ST.4 damage. Without this, the
/// absence asserted there could be a mock that never fires.
#[test]
fn the_symbol_effects_still_run_when_nothing_is_latched() {
    let (state, id) = board_with(&[], &[], ChaosBag::new([ChaosToken::Skull]));
    let result = perform_skill_test_no_commits(state, id, SkillKind::Willpower, 9);

    assert_event!(
        result.events,
        Event::ChaosTokenRevealed {
            token: ChaosToken::Skull,
            ..
        }
    );
    assert_event!(result.events, Event::DamageTaken { amount: 1, .. });
}

/// *"If a chaos token effect causes an investigator to automatically
/// succeed or fail at a skill test, continue with Steps 3 and 4, as
/// normal."* The `[auto_fail]` token's determination arrives *from* the
/// reveal, so there is nothing left to skip: the token is revealed and the
/// test fails on it.
#[test]
fn a_determination_from_the_revealed_token_skips_nothing() {
    let (state, id) = board_with(&[], &[], ChaosBag::new([ChaosToken::AutoFail]));
    let result = perform_skill_test_no_commits(state, id, SkillKind::Willpower, 3);

    assert_event!(
        result.events,
        Event::ChaosTokenRevealed {
            token: ChaosToken::AutoFail,
            resolution: TokenResolution::AutoFail,
        }
    );
    assert_event!(
        result.events,
        Event::SkillTestFailed {
            reason: FailureReason::AutoFail,
            by: 3,
            ..
        }
    );
}

/// *"However, the skill test still takes place. Cards may still be
/// committed to the test"* — and a committed card is still discarded at
/// ST.8 when the draw was skipped.
#[test]
fn committed_cards_are_still_discarded_when_the_draw_is_skipped() {
    let (state, id) = board_with(
        &[AUTO_SUCCEED],
        &[FILLER],
        ChaosBag::new([ChaosToken::Numeric(0)]),
    );
    let result = drive_skill_test(
        state,
        id,
        SkillKind::Willpower,
        9,
        FastPlayAndCommit::committing(&[FILLER]),
    );

    assert_no_event!(result.events, Event::ChaosTokenRevealed { .. });
    assert_event!(
        result.events,
        Event::CardDiscarded { investigator, code, from: Zone::Hand }
            if *investigator == id && code.as_str() == FILLER
    );
    assert!(
        result.state.investigators[&id]
            .discard
            .contains(&CardCode::new(FILLER)),
        "the committed card reaches the discard pile",
    );
}

/// *"All other steps of the skill test resolve as normal."* An Investigate
/// whose draw is skipped still runs its ST.7 action follow-up (the clue) and
/// its end-of-test steps: the committed card's `OnSkillTestResolution`
/// trigger fires and the test still ends with `SkillTestEnded`.
#[test]
fn the_follow_up_and_end_of_test_steps_still_run_on_a_skipped_draw() {
    let (state, id, loc) = investigate_board(&[PLAY_AUTO_SUCCEED, ON_RESOLUTION_GAIN], 9);
    let resources_before = state.investigators[&id].resources;
    let investigate = TurnAction::Investigate { investigator: id };
    let idx = legal_actions(&state)
        .iter()
        .position(|a| a == &investigate)
        .expect("Investigate must be a legal open-turn action");
    let result = drive(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(
                u32::try_from(idx).expect("action index fits u32"),
            )),
        }),
        FastPlayAndCommit::committing(&[ON_RESOLUTION_GAIN]),
    );

    assert_no_event!(result.events, Event::ChaosTokenRevealed { .. });
    assert_event!(result.events, Event::SkillTestSucceeded { .. });
    // ST.7 action follow-up: the shroud-9 location gives up its clue.
    assert_event!(result.events, Event::CluePlaced { .. });
    assert_eq!(result.state.locations[&loc].clues, 0);
    assert_eq!(result.state.investigators[&id].clues, 1);
    // The committed card's end-of-test trigger.
    assert_event!(
        result.events,
        Event::ResourcesGained { investigator, amount: 1 } if *investigator == id
    );
    assert_eq!(
        result.state.investigators[&id].resources,
        resources_before + 1
    );
    // ST.8 teardown.
    assert_event!(result.events, Event::SkillTestEnded { investigator } if *investigator == id);
}
