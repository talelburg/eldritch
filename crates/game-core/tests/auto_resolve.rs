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

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use card_dsl::dsl::{self, Cost, Determination, InvestigatorTarget, TestOutcome};
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::{OptionTarget, Registries};
use game_core::event::{Event, FailureReason};
use game_core::scenario::{
    ScenarioEnding, ScenarioId, ScenarioModule, ScenarioRegistry, SymbolCtx, SymbolOutcome,
    TokenEffect,
};
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInstanceId, ChaosBag, ChaosToken, DiscardPile,
    GameState, GameStateBuilder, InvestigatorId, LocationId, SkillKind, TokenModifiers,
    TokenResolution, Zone,
};
use game_core::test_support::{self, seat, MockRegistry, SeatLookup as _, TestSession};
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

fn mock_apply_resolution(_: ScenarioEnding, _: &mut GameState, _: &mut Vec<Event>) {}

static SYMBOL_MODULE: ScenarioModule = ScenarioModule {
    resolve_symbol: Some(mock_resolve_symbol),
    setup: mock_setup,
    apply_resolution: mock_apply_resolution,
    layout: &[],
};

fn mock_module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    (id.as_str() == SYMBOL_SCENARIO).then_some(&SYMBOL_MODULE)
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        // `TEST_INV` rides `build`'s composed `metadata_for_test_inv`: the symbol
        // token's damage reads the tester's `max_health()` through the registry,
        // which would otherwise not know the fixture investigator's card.
        cards: MockRegistry::new()
            .with_card(play_auto_succeed_metadata())
            .with_abilities(AUTO_FAIL, || {
                vec![dsl::activated(
                    0,
                    vec![Cost::Resources(1)],
                    dsl::auto_resolve(Determination::AutomaticFailure),
                )]
            })
            .with_abilities(AUTO_SUCCEED, || {
                vec![dsl::activated(
                    0,
                    vec![Cost::Resources(1)],
                    dsl::auto_resolve(Determination::AutomaticSuccess),
                )]
            })
            .with_abilities(FAIL_THEN_SUCCEED, || {
                vec![dsl::activated(
                    0,
                    vec![Cost::Resources(1)],
                    dsl::seq([
                        dsl::auto_resolve(Determination::AutomaticFailure),
                        dsl::auto_resolve(Determination::AutomaticSuccess),
                    ]),
                )]
            })
            .with_abilities(SUCCEED_THEN_FAIL, || {
                vec![dsl::activated(
                    0,
                    vec![Cost::Resources(1)],
                    dsl::seq([
                        dsl::auto_resolve(Determination::AutomaticSuccess),
                        dsl::auto_resolve(Determination::AutomaticFailure),
                    ]),
                )]
            })
            .with_abilities(PLAY_AUTO_SUCCEED, || {
                vec![dsl::on_play(dsl::auto_resolve(
                    Determination::AutomaticSuccess,
                ))]
            })
            .with_abilities(PLAIN_GAIN, || {
                vec![dsl::activated(
                    0,
                    vec![Cost::Resources(1)],
                    dsl::gain_resources(InvestigatorTarget::You, 1),
                )]
            })
            .with_abilities(ON_RESOLUTION_GAIN, || {
                vec![dsl::on_skill_test_resolution(
                    TestOutcome::Success,
                    dsl::gain_resources(InvestigatorTarget::You, 1),
                )]
            })
            .build(),
        scenarios: &ScenarioRegistry {
            module_for: mock_module_for,
        },
    });
    *REGISTRIES
}

const INV: InvestigatorId = InvestigatorId(1);
/// The investigator's location on the Investigate board.
const HERE: LocationId = LocationId(10);

/// The one board every test here builds on: the controller at willpower 5,
/// holding `hand`, with an in-play instance of each code in `in_play`,
/// drawing from `bag`, on their open turn.
fn board_with(in_play: &[&str], hand: &[&str], bag: ChaosBag) -> (GameState, InvestigatorId) {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(1)
                .skill(SkillKind::Willpower, 5)
                .hand(hand.iter().copied())
                .in_play(in_play.iter().copied()),
        )
        .open_turn(INV)
        .with_chaos_bag(bag)
        .with_token_modifiers(TokenModifiers::default())
        // Named so the mock module's symbol hook is reachable; only a
        // `[skull]` in the bag actually consults it.
        .with_scenario_id(ScenarioId::new(SYMBOL_SCENARIO))
        .build();
    (state, INV)
}

/// [`board_with`] plus a location the investigator stands at, holding one
/// clue behind `shroud` — the board an Investigate action needs. Bag is a
/// single `Numeric(0)`.
fn investigate_board(hand: &[&str], shroud: u8) -> (GameState, InvestigatorId, LocationId) {
    let mut location = test_support::test_location(HERE.0, "Study");
    location.clues = 1;
    location.shroud = shroud;
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE).hand(hand.iter().copied()))
        .open_turn(INV)
        .with_location(location)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .with_scenario_id(ScenarioId::new(SYMBOL_SCENARIO))
        .build();
    (state, INV, HERE)
}

/// [`board_with`] for the common case: one in-play instance of `code`, empty
/// hand, and a single-`Numeric(0)` chaos bag so the token contributes nothing
/// and every number below is the card's doing. Returns the instance id too.
fn board(code: &str) -> (GameState, InvestigatorId, CardInstanceId) {
    let (state, id) = board_with(&[code], &[], ChaosBag::new([ChaosToken::Numeric(0)]));
    let instance_id = state.in_play(id, code);
    (state, id, instance_id)
}

/// The fast play of the hand card `code` on `state`: a player window anchors
/// a hand play to the card's place in the hand.
fn play_from_hand(state: &GameState, code: &str) -> OptionTarget {
    let hand_index = state.investigators[&INV]
        .hand
        .iter()
        .position(|c| c.as_str() == code)
        .unwrap_or_else(|| panic!("{code} is not in hand"));
    OptionTarget::HandCard {
        investigator: INV,
        hand_index: u8::try_from(hand_index).expect("a short hand"),
    }
}

/// Resolve a skill test the session has just started, activating the fast
/// ability on the in-play card `card` at the ST.1 player window and
/// committing `commit`. The ability stays offered, so the ST.1 window re-opens
/// after it resolves and the ST.2 (pre-token) window opens too; both are
/// declined. The test resolves back to the turn menu.
fn activating_through_the_test(
    session: TestSession,
    card: CardInstanceId,
    commit: &[&str],
) -> TestSession {
    let session = session
        .pick(OptionTarget::CardInstance(card))
        .skip()
        .commit(commit)
        .skip();
    session.assert_at_turn_menu(INV);
    session
}

/// Resolve a skill test the session has just started, playing the fast event
/// `play` from hand at the ST.1 player window and committing `commit`. The
/// played event leaves nothing to offer, so no window opens after it. The test
/// resolves back to the turn menu.
fn playing_through_the_test(
    session: TestSession,
    play: OptionTarget,
    commit: &[&str],
) -> TestSession {
    let session = session.pick(play).commit(commit);
    session.assert_at_turn_menu(INV);
    session
}

/// Run a willpower test against `difficulty`, activating the in-play `card`'s
/// fast ability at the test's player window and committing nothing.
fn test_activating(state: GameState, card: CardInstanceId, difficulty: i8) -> TestSession {
    activating_through_the_test(
        TestSession::new(state, registries()).test_skill(INV, SkillKind::Willpower, difficulty),
        card,
        &[],
    )
}

/// Run a willpower test against `difficulty` on a board with nothing to play
/// or commit: no player window opens, and the test resolves through an empty
/// commit back to the turn menu.
fn test_with_nothing_to_play(state: GameState, difficulty: i8) -> TestSession {
    let session = TestSession::new(state, registries())
        .test_skill(INV, SkillKind::Willpower, difficulty)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

/// *"the investigator's total skill value for that test is considered 0"* —
/// so willpower 5 against difficulty 3 does not merely fail, it fails **by
/// 3**, which an "if you fail by 2 or more" clause on the same test reads.
#[test]
fn a_card_latched_automatic_failure_fails_the_test_with_the_real_margin() {
    let (state, id, card) = board(AUTO_FAIL);
    let session = test_activating(state, card, 3);

    assert_event!(
        session.events(),
        Event::SkillTestFailed {
            investigator,
            skill: SkillKind::Willpower,
            reason: FailureReason::AutoFail,
            by: 3,
        } if *investigator == id
    );
    assert_no_event!(session.events(), Event::SkillTestSucceeded { .. });
}

/// *"the total difficulty of that test is considered 0"* — willpower 5
/// against difficulty 9 is a loss on the numbers and a success on the card.
#[test]
fn a_card_latched_automatic_success_passes_a_test_it_would_have_lost() {
    let (state, id, card) = board(AUTO_SUCCEED);
    let session = test_activating(state, card, 9);

    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Willpower, .. }
            if *investigator == id
    );
    assert_no_event!(session.events(), Event::SkillTestFailed { .. });
}

/// Automatic failure beats automatic success. Latched failure-first.
#[test]
fn automatic_failure_beats_automatic_success_latched_failure_first() {
    let (state, _, card) = board(FAIL_THEN_SUCCEED);
    let session = test_activating(state, card, 3);

    assert_event!(
        session.events(),
        Event::SkillTestFailed {
            reason: FailureReason::AutoFail,
            by: 3,
            ..
        }
    );
    assert_no_event!(session.events(), Event::SkillTestSucceeded { .. });
}

/// The same board with the two latches swapped. Precedence is resolved at
/// **read** time, so neither row suppresses nor overwrites the other and the
/// verdict cannot depend on which card resolved first — a write-time
/// suppression would pass the test above and fail this one.
#[test]
fn automatic_failure_beats_automatic_success_latched_success_first() {
    let (state, _, card) = board(SUCCEED_THEN_FAIL);
    let session = test_activating(state, card, 3);

    assert_event!(
        session.events(),
        Event::SkillTestFailed {
            reason: FailureReason::AutoFail,
            by: 3,
            ..
        }
    );
    assert_no_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_event_count!(
        session.events(),
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

    let play = play_from_hand(&state, PLAY_AUTO_SUCCEED);
    let session = playing_through_the_test(
        TestSession::new(state, registries()).test_skill(id, SkillKind::Willpower, 9),
        play,
        &[],
    );

    // No in-play instance to name: an event resolving out of hand latches an
    // unattributed determination, exactly as the row records it.
    assert_event!(
        session.events(),
        Event::SkillTestDeterminationLatched {
            determination: Determination::AutomaticSuccess,
            source: None,
            ..
        }
    );
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, .. } if *investigator == id
    );
    assert_no_event!(session.events(), Event::SkillTestFailed { .. });
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
        address: AbilityAddress::Printed(0),
    };

    assert!(
        !enumerate::legal_actions(&state, registries().cards).contains(&action),
        "the open-turn menu must not offer an activation that would reject",
    );

    // Off the menu, as a client could send it over the wire: the handler
    // itself refuses it.
    let refused = TestSession::new(state, registries()).submit_off_menu(&action);
    refused.expect_rejected();
    let result = refused.finish();
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
        enumerate::legal_actions(&state, registries().cards).contains(
            &TurnAction::ActivateAbility {
                investigator: id,
                source: AbilitySource::InPlay(instance_id),
                address: AbilityAddress::Printed(0),
            }
        )
    );
}

/// A determination is a test-scoped row, so it expires with the test that
/// carried it. A leak here would automatically succeed the *next* test —
/// the same guard `an_autofail_determination_does_not_outlive_its_test`
/// gives the token's row.
#[test]
fn a_card_latched_determination_does_not_survive_into_a_later_test() {
    let (state, _, card) = board(AUTO_SUCCEED);
    let first = test_activating(state, card, 9);
    assert_event!(first.events(), Event::SkillTestSucceeded { .. });
    assert!(
        first.state().recorded_modifiers.is_empty(),
        "the determination expires with the test's teardown",
    );

    // The second test declines the fast window, so nothing latches: willpower
    // 5 against difficulty 9 fails by 4 on the numbers. A fresh session from
    // the first test's end, so its events are the second test's alone.
    let second = TestSession::new(first.state().clone(), registries())
        .test_skill(INV, SkillKind::Willpower, 9)
        .skip()
        .commit(&[])
        .skip();
    second.assert_at_turn_menu(INV);
    assert_event!(
        second.events(),
        Event::SkillTestFailed {
            reason: FailureReason::Total,
            by: 4,
            ..
        }
    );
    assert_no_event!(second.events(), Event::SkillTestDeterminationLatched { .. });
}

/// The event log is the client's only narrative channel: without this event
/// an automatic success renders as a win with a dash where the chaos token
/// would be and no stated cause. It names the determination and the in-play
/// instance that latched it.
#[test]
fn the_latch_event_names_the_determination_and_its_source() {
    let (state, id, instance_id) = board(AUTO_SUCCEED);
    let session = test_activating(state, instance_id, 9);

    assert_event!(
        session.events(),
        Event::SkillTestDeterminationLatched {
            investigator,
            determination: Determination::AutomaticSuccess,
            source: Some(src),
        } if *investigator == id && *src == instance_id
    );
    assert_event_count!(
        session.events(),
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
    let (state, _) = board_with(&[], &[], ChaosBag::new([ChaosToken::AutoFail]));
    let session = test_with_nothing_to_play(state, 3);

    assert_event!(
        session.events(),
        Event::SkillTestFailed {
            reason: FailureReason::AutoFail,
            by: 3,
            ..
        }
    );
    assert_no_event!(
        session.events(),
        Event::SkillTestDeterminationLatched { .. }
    );
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
    let (state, id, card) = board(AUTO_SUCCEED);
    let bag_before = state.chaos_bag.clone();
    let session = test_activating(state, card, 9);

    assert_no_event!(session.events(), Event::ChaosTokenRevealed { .. });
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, .. } if *investigator == id
    );
    // The load-bearing assertion is the absent reveal above; this one pins
    // the bag's contents, which a draw never mutates anyway.
    assert_eq!(session.state().chaos_bag, bag_before);
}

/// *"that step is skipped, along with Step 4"* — the `[skull]`'s immediate
/// damage never lands, because the token that would have carried it was
/// never revealed.
#[test]
fn a_skipped_draw_pushes_no_chaos_symbol_effects() {
    let (state, id) = board_with(&[AUTO_SUCCEED], &[], ChaosBag::new([ChaosToken::Skull]));
    let card = state.in_play(id, AUTO_SUCCEED);
    let session = test_activating(state, card, 9);

    assert_no_event!(session.events(), Event::ChaosTokenRevealed { .. });
    assert_no_event!(session.events(), Event::DamageTaken { .. });
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
}

/// The control for the test above: with nothing latched, the same board
/// reveals the `[skull]` and takes its ST.4 damage. Without this, the
/// absence asserted there could be a mock that never fires.
#[test]
fn the_symbol_effects_still_run_when_nothing_is_latched() {
    let (state, _) = board_with(&[], &[], ChaosBag::new([ChaosToken::Skull]));
    let session = test_with_nothing_to_play(state, 9);

    assert_event!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Skull,
            ..
        }
    );
    assert_event!(session.events(), Event::DamageTaken { amount: 1, .. });
}

/// *"If a chaos token effect causes an investigator to automatically
/// succeed or fail at a skill test, continue with Steps 3 and 4, as
/// normal."* The `[auto_fail]` token's determination arrives *from* the
/// reveal, so there is nothing left to skip: the token is revealed and the
/// test fails on it.
#[test]
fn a_determination_from_the_revealed_token_skips_nothing() {
    let (state, _) = board_with(&[], &[], ChaosBag::new([ChaosToken::AutoFail]));
    let session = test_with_nothing_to_play(state, 3);

    assert_event!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::AutoFail,
            resolution: TokenResolution::AutoFail,
        }
    );
    assert_event!(
        session.events(),
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
    let card = state.in_play(id, AUTO_SUCCEED);
    let session = activating_through_the_test(
        TestSession::new(state, registries()).test_skill(id, SkillKind::Willpower, 9),
        card,
        &[FILLER],
    );

    assert_no_event!(session.events(), Event::ChaosTokenRevealed { .. });
    assert_event!(
        session.events(),
        Event::CardDiscarded { code, from: Zone::Hand, to: DiscardPile::Investigator(investigator) }
            if *investigator == id && code.as_str() == FILLER
    );
    assert!(
        session.state().investigators[&id]
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
    let play = play_from_hand(&state, PLAY_AUTO_SUCCEED);
    let session = playing_through_the_test(
        TestSession::new(state, registries()).take(&TurnAction::Investigate { investigator: id }),
        play,
        &[ON_RESOLUTION_GAIN],
    );

    assert_no_event!(session.events(), Event::ChaosTokenRevealed { .. });
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    // ST.7 action follow-up: the shroud-9 location gives up its clue.
    assert_event!(session.events(), Event::CluePlaced { .. });
    assert_eq!(session.state().locations[&loc].clues, 0);
    assert_eq!(session.state().investigators[&id].clues, 1);
    // The committed card's end-of-test trigger.
    assert_event!(
        session.events(),
        Event::ResourcesGained { investigator, amount: 1 } if *investigator == id
    );
    assert_eq!(
        session.state().investigators[&id].resources,
        resources_before + 1
    );
    // ST.8 teardown.
    assert_event!(session.events(), Event::SkillTestEnded { investigator } if *investigator == id);
}
