//! Deterministic [`ChoiceResolver`] for driving `AwaitingInput` round-trips.
//!
//! When the engine returns
//! [`EngineOutcome::AwaitingInput`],
//! it pauses mid-resolution waiting for a player response. A test needs
//! a way to script that response without leaking engine internals into
//! every assertion. The [`ChoiceResolver`] trait is the seam;
//! [`ScriptedResolver`] is the deterministic test impl that feeds
//! pre-recorded responses in FIFO order.
//!
//! [`TestSession`](super::TestSession) is the one driver: it settles a built
//! state, then applies one step at a time and drains to the next rest through
//! its reply policy, a resolver script by default. The drain loop it runs,
//! `drain_with_applier`, lives here.
//!
//! # Engine consumers
//!
//! The skill-test commit window (#63) is the prompt tests answer most: the
//! active investigator replies with [`InputResponse::PickMultiple`] (each
//! `OptionId` a hand index). [`ScriptedResolver::commit_cards`] is the
//! ergonomic helper: tests pass card codes, the resolver translates them to
//! hand indices using [`GameState`] at resolve time. Other prompts are
//! answered by the board entity an option anchors to
//! ([`ScriptedResolver::pick`]) or by a literal response.
//!
//! [`InputResponse::PickMultiple`]: crate::action::InputResponse::PickMultiple
//!
//! # Example
//!
//! ```
//! use game_core::action::{Action, InputResponse, PlayerAction};
//! use game_core::engine::EngineOutcome;
//! use game_core::state::GameStateBuilder;
//!
//! // A `ResolveInput` against a bare state with no outstanding prompt
//! // rejects — a tiny smoke test for the fluent API without needing a
//! // real chaos bag or any setup.
//! let session = GameStateBuilder::new()
//!     .session()
//!     .apply(Action::Player(PlayerAction::ResolveInput {
//!         response: InputResponse::Skip,
//!     }));
//! assert!(!session.expect_rejected().is_empty());
//! ```

use std::collections::VecDeque;

use crate::action::{Action, InputResponse, PlayerAction};
use crate::engine::enumerate::TurnAction;
use crate::engine::{
    enumerate, ApplyResult, ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget,
};
use crate::state::{CardCode, GameState, InvestigatorId};

/// The test harness's reply-policy seam: provide a response for an
/// `AwaitingInput` prompt while a test drives the engine.
///
/// A [`TestSession`](super::TestSession) answers prompts through a
/// [`ScriptedResolver`] by default; a flow that needs a stateful reply rule
/// installs its own implementation with
/// [`answer_with`](super::TestSession::answer_with). Every implementation is
/// test-side — hosts answer prompts over the wire, not through this trait.
pub trait ChoiceResolver {
    /// Produce the next response for the given prompt.
    ///
    /// `state` is the engine state at the moment of the prompt — useful
    /// for resolvers that translate symbolic intents ("commit this card
    /// by code") into engine indices.
    fn next(&mut self, request: &InputRequest, state: &GameState) -> InputResponse;
}

/// Replayable [`ChoiceResolver`] backed by a FIFO of pre-recorded steps.
///
/// Build the script with the fluent helpers ([`confirm`](Self::confirm),
/// [`skip`](Self::skip), [`pick_single`](Self::pick_single),
/// [`pick`](Self::pick), [`commit_cards`](Self::commit_cards)). When the engine prompts and the
/// script is empty, [`next`](Self::next) panics with the prompt text — a
/// useful failure mode in tests.
///
/// Helpers take `&mut self` and return `&mut Self` so they chain inside
/// a [`TestSession::resolve_choices`](super::TestSession::resolve_choices) closure.
#[derive(Debug, Default, Clone)]
pub struct ScriptedResolver {
    steps: VecDeque<ScriptedStep>,
}

#[derive(Debug, Clone)]
enum ScriptedStep {
    Response(InputResponse),
    /// Commit a set of cards from the active investigator's hand to a
    /// skill test. A by-`CardCode` convenience over the real index-based
    /// commit flow (resolves codes to hand indices at replay time).
    CommitCards(Vec<CardCode>),
    /// Pick the one offered option anchored to this target, resolved to
    /// `PickSingle(option.id)` at replay time.
    Pick(OptionTarget),
}

impl ScriptedResolver {
    /// Empty script.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a literal [`InputResponse`] to the script.
    pub fn push(&mut self, response: InputResponse) -> &mut Self {
        self.steps.push_back(ScriptedStep::Response(response));
        self
    }

    /// Respond with [`InputResponse::Confirm`].
    pub fn confirm(&mut self) -> &mut Self {
        self.push(InputResponse::Confirm)
    }

    /// Respond with [`InputResponse::Skip`].
    pub fn skip(&mut self) -> &mut Self {
        self.push(InputResponse::Skip)
    }

    /// Respond with [`InputResponse::PickSingle`] (the Axis-A choice contract).
    pub fn pick_single(&mut self, id: OptionId) -> &mut Self {
        self.push(InputResponse::PickSingle(id))
    }

    /// Respond with [`InputResponse::PickSingle`] of the one offered option
    /// anchored to `target`, resolved at replay time — the script analogue of
    /// [`TestSession::pick`](super::TestSession::pick). Panics at replay unless
    /// exactly one option is anchored there.
    pub fn pick(&mut self, target: OptionTarget) -> &mut Self {
        self.steps.push_back(ScriptedStep::Pick(target));
        self
    }

    /// Commit cards (by code) from the in-flight skill test's
    /// investigator's hand. The resolver translates each code to the
    /// first matching not-yet-committed hand index at resolve time
    /// using the [`GameState`] passed into [`next`](Self::next).
    /// Duplicate codes pick distinct indices in left-to-right order;
    /// a missing code panics.
    ///
    /// Pass `&[]` to commit nothing — the canonical empty-commit
    /// helper for tests that aren't exercising the commit-window
    /// itself.
    pub fn commit_cards(&mut self, codes: &[&str]) -> &mut Self {
        self.steps
            .push_back(ScriptedStep::CommitCards(card_codes(codes)));
        self
    }

    /// Number of scripted steps remaining (literal responses plus
    /// unexpanded `commit_cards` entries). Useful for asserting a test
    /// consumed the script it set up.
    pub fn remaining(&self) -> usize {
        self.steps.len()
    }
}

impl ChoiceResolver for ScriptedResolver {
    fn next(&mut self, request: &InputRequest, state: &GameState) -> InputResponse {
        let step = self.steps.pop_front().unwrap_or_else(|| {
            panic!(
                "ScriptedResolver: no scripted response for prompt: {:?}",
                request.prompt,
            )
        });
        match step {
            ScriptedStep::Response(r) => r,
            ScriptedStep::CommitCards(codes) => InputResponse::PickMultiple {
                selected: resolve_commit_codes(
                    &codes,
                    state,
                    &request.prompt,
                    "ScriptedResolver::commit_cards",
                )
                .into_iter()
                .map(OptionId)
                .collect(),
            },
            ScriptedStep::Pick(target) => InputResponse::PickSingle(sole_option_at(
                request,
                &target,
                "ScriptedResolver::pick",
            )),
        }
    }
}

/// The [`CardCode`]s a by-code hand step names. The steps take `&[&str]`
/// rather than generic `Into<CardCode>` items so `&[]` (choose nothing) needs
/// no type annotation.
pub(super) fn card_codes(codes: &[&str]) -> Vec<CardCode> {
    codes.iter().copied().map(CardCode::new).collect()
}

/// Translate scripted commit-card codes to hand indices using the
/// in-flight skill test's investigator on `state`, through
/// [`hand_indices_of`].
pub(super) fn resolve_commit_codes(
    codes: &[CardCode],
    state: &GameState,
    prompt: &str,
    caller: &str,
) -> Vec<u32> {
    if codes.is_empty() {
        return Vec::new();
    }
    let in_flight = state.current_skill_test().unwrap_or_else(|| {
        panic!(
            "{caller}: state has no in-flight skill test at \
             resolve time; prompt was: {prompt:?}",
        )
    });
    hand_indices_of(codes, state, in_flight.investigator, prompt, caller)
}

/// Translate card codes to indices into `investigator`'s hand — the one
/// code-to-hand-index translation behind a commit by code and a mulligan or
/// hand-size discard by code.
///
/// Returns an empty vec for an empty code list (the canonical
/// choose-nothing case). Each code is matched against the
/// investigator's hand left-to-right; duplicate codes claim distinct
/// indices so `&[CardCode("X"), CardCode("X")]` against a hand with
/// two `X`s yields `[i, j]`. Panics with the prompt and remaining
/// state if any code can't be matched — that's a test-author error.
pub(super) fn hand_indices_of(
    codes: &[CardCode],
    state: &GameState,
    investigator: InvestigatorId,
    prompt: &str,
    caller: &str,
) -> Vec<u32> {
    let inv = state.investigators.get(&investigator).unwrap_or_else(|| {
        panic!(
            "{caller}: investigator {investigator:?} not in \
             state.investigators; prompt was: {prompt:?}",
        )
    });
    let mut used = vec![false; inv.hand.len()];
    let mut indices = Vec::with_capacity(codes.len());
    for code in codes {
        let idx = inv
            .hand
            .iter()
            .enumerate()
            .find_map(|(i, c)| (!used[i] && c == code).then_some(i))
            .unwrap_or_else(|| {
                panic!(
                    "{caller}: code {code:?} not found in \
                     {investigator:?}'s hand (hand = {:?}, already-used indices = {:?}); \
                     prompt was: {prompt:?}",
                    inv.hand, used,
                )
            });
        used[idx] = true;
        indices.push(u32::try_from(idx).expect("hand index fits in u32"));
    }
    indices
}

/// Whether `request` is the open-turn action menu (2b, #447): the prompt the
/// engine anchors to the acting investigator's
/// [`TurnControl`](OptionTarget::TurnControl). Read off the outcome alone — the
/// menu is the only prompt anchored there (ADR 0011) — so no driver needs to
/// look at the continuation stack to know where the engine stopped. Drivers
/// treat it as a rest: it is the *next* action's prompt, not a window to
/// resolve, so driving past it would silently consume another turn action.
pub(super) fn is_turn_menu(request: &InputRequest) -> bool {
    matches!(request.target, Some(OptionTarget::TurnControl(_)))
}

/// The options of `request` anchored to `target`, in the order the prompt
/// offers them — the one anchor filter behind every by-anchor step and reader.
pub(super) fn options_at<'a>(
    request: &'a InputRequest,
    target: &'a OptionTarget,
) -> impl Iterator<Item = &'a ChoiceOption> {
    request
        .options
        .iter()
        .filter(move |option| option.target.as_ref() == Some(target))
}

/// The id of the one option of `request` anchored to `target`, or a panic
/// naming `caller` and why there isn't one. The anchor lookup behind
/// [`ScriptedResolver::pick`] and the session's `pick` step.
pub(super) fn sole_option_at(
    request: &InputRequest,
    target: &OptionTarget,
    caller: &str,
) -> OptionId {
    let mut matching = options_at(request, target);
    let Some(option) = matching.next() else {
        panic!(
            "{caller}: no option anchored to {target:?}; prompt {:?} offers {:?}",
            request.prompt, request.options,
        );
    };
    assert!(
        matching.next().is_none(),
        "{caller}: several options are anchored to {target:?}, so the anchor does not say which; \
         prompt {:?} offers {:?}",
        request.prompt,
        request.options,
    );
    option.id
}

/// The turn-menu [`OptionId`] of the open-turn action `action` on `state`, or a
/// panic naming `caller` and listing what is offered. Behind the session's
/// `take` step.
pub(super) fn turn_action_option(state: &GameState, action: &TurnAction, caller: &str) -> OptionId {
    let actions = enumerate::legal_actions(state);
    let idx = actions
        .iter()
        .position(|a| a == action)
        .unwrap_or_else(|| panic!("{caller}: {action:?} is not legal; offered: {actions:?}"));
    OptionId(u32::try_from(idx).expect("action index fits u32"))
}

/// Apply `action` against `state` through `applier`, then drain every
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) outcome through `resolver`
/// with the one drain loop, [`drain_with_applier`].
///
/// The returned [`ApplyResult`] aggregates state, events, and outcome across
/// every sub-`apply` in the round trip. Panics if the resolver runs out of
/// scripted responses while the engine is still prompting, or if the loop
/// exceeds an internal iteration cap.
///
/// Tests in this module use this to substitute a fake `apply` that scripts
/// the `AwaitingInput` sequence, exercising the drain logic independently of
/// any particular engine prompt.
#[cfg(test)]
fn drive_with_applier<R, F>(
    state: GameState,
    action: Action,
    resolver: &mut R,
    mut applier: F,
) -> ApplyResult
where
    R: ChoiceResolver + ?Sized,
    F: FnMut(GameState, Action) -> ApplyResult,
{
    let first = applier(state, action);
    drain_with_applier(first, answering(resolver), applier)
}

/// A reply policy that answers every prompt through `resolver` — the policy
/// [`drive_with_applier`] runs the drain loop under.
#[cfg(test)]
fn answering<R: ChoiceResolver + ?Sized>(
    resolver: &mut R,
) -> impl FnMut(&InputRequest, &GameState) -> Option<InputResponse> + '_ {
    |request, state| Some(resolver.next(request, state))
}

/// The harness's one drain loop. Continues from an already-applied
/// [`ApplyResult`], answering every [`AwaitingInput`](EngineOutcome::AwaitingInput)
/// through `policy` (re-applying its `ResolveInput` responses via `applier`)
/// until the engine comes to rest: `Done`, `Rejected`, the open-turn menu, or a
/// prompt the policy declines to answer (`None`).
///
/// Each [`TestSession`](super::TestSession) step, and [`drive_with_applier`],
/// is this loop under a different reply policy; none re-implements it.
pub(crate) fn drain_with_applier<P, F>(
    first: ApplyResult,
    mut policy: P,
    mut applier: F,
) -> ApplyResult
where
    P: FnMut(&InputRequest, &GameState) -> Option<InputResponse>,
    F: FnMut(GameState, Action) -> ApplyResult,
{
    const MAX_ITERATIONS: u32 = 1024;

    let ApplyResult {
        mut state,
        mut events,
        mut outcome,
    } = first;
    let mut iterations = 0u32;

    loop {
        let response = match &outcome {
            EngineOutcome::AwaitingInput { request, .. } if !is_turn_menu(request) => {
                policy(request, &state)
            }
            _ => None,
        };
        let Some(response) = response else {
            return ApplyResult {
                state,
                events,
                outcome,
            };
        };
        iterations += 1;
        assert!(
            iterations <= MAX_ITERATIONS,
            "drain_with_applier: exceeded {MAX_ITERATIONS} iterations without reaching Done/Rejected; \
             resolver or engine appears to be cycling",
        );
        let result = applier(
            state,
            Action::Player(PlayerAction::ResolveInput { response }),
        );
        state = result.state;
        events.extend(result.events);
        outcome = result.outcome;
    }
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::SkillTestKind;

    use super::*;
    use crate::engine::{self, InputKind, ResumeToken};
    use crate::event::Event;
    use crate::state::SkillKind;
    use crate::state::{ChaosBag, ChaosToken, Continuation, GameStateBuilder, SkillTestId};
    use crate::test_support::{self, TestSession};

    fn empty_state() -> GameState {
        GameStateBuilder::new().build()
    }

    fn req(prompt: &str) -> InputRequest {
        // The resolver returns scripted responses regardless of `kind`; the
        // constructor choice here is arbitrary.
        InputRequest::confirm(prompt)
    }

    #[test]
    fn scripted_resolver_returns_responses_in_fifo_order() {
        let mut r = ScriptedResolver::new();
        r.confirm()
            .skip()
            .pick_single(OptionId(2))
            .pick_single(OptionId(5));
        assert_eq!(r.remaining(), 4);

        let state = empty_state();
        let p = req("pick");
        assert_eq!(r.next(&p, &state), InputResponse::Confirm);
        assert_eq!(r.next(&p, &state), InputResponse::Skip);
        assert_eq!(r.next(&p, &state), InputResponse::PickSingle(OptionId(2)));
        assert_eq!(r.next(&p, &state), InputResponse::PickSingle(OptionId(5)));
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    #[should_panic(expected = "no scripted response")]
    fn scripted_resolver_panics_when_script_exhausted() {
        let mut r = ScriptedResolver::new();
        let _ = r.next(&req("oops"), &empty_state());
    }

    /// Build a tiny state with one investigator who has a non-empty
    /// hand and an in-flight skill test parked on it. Used by the
    /// commit-card resolution tests below.
    fn state_with_in_flight_hand(hand: &[&str]) -> GameState {
        let id = InvestigatorId(1);
        let mut inv = test_support::test_investigator(1);
        inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
        let mut state = GameStateBuilder::new().with_investigator(inv).build();
        state
            .continuations
            .push(Continuation::SkillTest(test_support::test_skill_test(
                SkillTestId(0),
                id,
                SkillKind::Intellect,
                SkillTestKind::Plain,
                1,
            )));
        state
    }

    #[test]
    fn commit_cards_empty_resolves_to_empty_indices() {
        let mut r = ScriptedResolver::new();
        r.commit_cards(&[]);
        // Empty doesn't even consult `in_flight_skill_test` — symmetric
        // with the engine's "empty commits is the no-op" semantics.
        let state = empty_state();
        let response = r.next(&req("commit"), &state);
        assert_eq!(response, InputResponse::PickMultiple { selected: vec![] });
    }

    #[test]
    fn commit_cards_translates_codes_to_hand_indices_in_order() {
        let mut r = ScriptedResolver::new();
        r.commit_cards(&["X", "Y"]);
        let state = state_with_in_flight_hand(&["X", "Y", "Z"]);
        let response = r.next(&req("commit"), &state);
        assert_eq!(
            response,
            InputResponse::PickMultiple {
                selected: vec![OptionId(0), OptionId(1)]
            }
        );
    }

    #[test]
    fn commit_cards_duplicate_code_picks_distinct_indices_left_to_right() {
        let mut r = ScriptedResolver::new();
        r.commit_cards(&["X", "X"]);
        let state = state_with_in_flight_hand(&["A", "X", "B", "X"]);
        let response = r.next(&req("commit"), &state);
        assert_eq!(
            response,
            InputResponse::PickMultiple {
                selected: vec![OptionId(1), OptionId(3)]
            }
        );
    }

    #[test]
    #[should_panic(expected = "no in-flight skill test")]
    fn commit_cards_panics_when_no_in_flight_skill_test() {
        let mut r = ScriptedResolver::new();
        r.commit_cards(&["X"]);
        let _ = r.next(&req("commit"), &empty_state());
    }

    #[test]
    #[should_panic(expected = "not found")]
    fn commit_cards_panics_when_code_not_in_hand() {
        let mut r = ScriptedResolver::new();
        r.commit_cards(&["MISSING"]);
        let state = state_with_in_flight_hand(&["X", "Y"]);
        let _ = r.next(&req("commit"), &state);
    }

    #[test]
    fn drive_passes_through_done_without_consulting_resolver() {
        // Use `ResolveInput` purely as a no-op shape — it rejects today,
        // but we substitute the applier so the test doesn't depend on
        // engine specifics.
        let applier = |state, action: Action| {
            assert!(matches!(
                action,
                Action::Player(PlayerAction::ResolveInput { .. })
            ));
            ApplyResult {
                state,
                events: vec![],
                outcome: EngineOutcome::Done,
            }
        };
        let mut resolver = ScriptedResolver::new();
        resolver.confirm(); // stale; must not be consumed
        let result = drive_with_applier(
            empty_state(),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Confirm,
            }),
            &mut resolver,
            applier,
        );
        assert!(matches!(result.outcome, EngineOutcome::Done));
        assert_eq!(resolver.remaining(), 1);
    }

    #[test]
    fn drive_passes_through_rejected_with_reason() {
        let applier = |state, _action: Action| ApplyResult {
            state,
            events: vec![],
            outcome: EngineOutcome::Rejected {
                reason: "test rejection".into(),
            },
        };
        let mut resolver = ScriptedResolver::new();
        let result = drive_with_applier(
            empty_state(),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Skip,
            }),
            &mut resolver,
            applier,
        );
        match result.outcome {
            EngineOutcome::Rejected { reason } => assert_eq!(reason, "test rejection"),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn drive_drains_awaiting_input_until_done() {
        let mut step = 0;
        let applier = |state, action: Action| {
            step += 1;
            match step {
                1 => {
                    // The initial action is the opaque payload threaded to the
                    // (fake) applier — any surviving variant works; `ResolveInput`
                    // is the only gameplay-bearing one post-OptionId-routing (#447).
                    assert!(matches!(
                        action,
                        Action::Player(PlayerAction::ResolveInput {
                            response: InputResponse::Skip
                        })
                    ));
                    ApplyResult {
                        state,
                        events: vec![],
                        outcome: EngineOutcome::AwaitingInput {
                            request: req("pick first"),
                            resume_token: ResumeToken(0),
                        },
                    }
                }
                2 => {
                    assert!(matches!(
                        action,
                        Action::Player(PlayerAction::ResolveInput {
                            response: InputResponse::Confirm
                        })
                    ));
                    ApplyResult {
                        state,
                        events: vec![],
                        outcome: EngineOutcome::AwaitingInput {
                            request: req("pick second"),
                            resume_token: ResumeToken(1),
                        },
                    }
                }
                3 => {
                    assert!(matches!(
                        action,
                        Action::Player(PlayerAction::ResolveInput {
                            response: InputResponse::Skip
                        })
                    ));
                    ApplyResult {
                        state,
                        events: vec![],
                        outcome: EngineOutcome::Done,
                    }
                }
                _ => panic!("unexpected step {step}"),
            }
        };
        let mut resolver = ScriptedResolver::new();
        resolver.confirm().skip();
        let result = drive_with_applier(
            empty_state(),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Skip,
            }),
            &mut resolver,
            applier,
        );
        assert!(matches!(result.outcome, EngineOutcome::Done));
        assert_eq!(resolver.remaining(), 0);
    }

    #[test]
    #[should_panic(expected = "no scripted response")]
    fn drive_panics_with_useful_message_on_unhandled_prompt() {
        let applier = |state, _action: Action| ApplyResult {
            state,
            events: vec![],
            outcome: EngineOutcome::AwaitingInput {
                request: req("nothing scripted for me"),
                resume_token: ResumeToken(42),
            },
        };
        let mut resolver = ScriptedResolver::new();
        let _ = drive_with_applier(
            empty_state(),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Skip,
            }),
            &mut resolver,
            applier,
        );
    }

    #[test]
    fn drive_accumulates_events_across_sub_applies() {
        let mut step = 0;
        let id = InvestigatorId(1);
        let applier = |state, _action: Action| {
            step += 1;
            match step {
                1 => ApplyResult {
                    state,
                    events: vec![Event::ResourcesGained {
                        investigator: id,
                        amount: 1,
                    }],
                    outcome: EngineOutcome::AwaitingInput {
                        request: req("pick"),
                        resume_token: ResumeToken(0),
                    },
                },
                2 => ApplyResult {
                    state,
                    events: vec![Event::TurnEnded { investigator: id }],
                    outcome: EngineOutcome::Done,
                },
                _ => panic!("unexpected step {step}"),
            }
        };
        let mut resolver = ScriptedResolver::new();
        resolver.confirm();
        let result = drive_with_applier(
            empty_state(),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Skip,
            }),
            &mut resolver,
            applier,
        );
        crate::assert_total_event_count!(result.events, 2);
        assert!(matches!(
            result.events[0],
            Event::ResourcesGained { amount: 1, .. }
        ));
        assert!(matches!(result.events[1], Event::TurnEnded { .. }));
    }

    #[test]
    fn drive_real_engine_passes_through_rejected_action() {
        // A `ResolveInput` against a state with no outstanding prompt rejects;
        // this exercises the real engine's pass-through of that `Rejected`
        // outcome.
        let result = drive_with_applier(
            empty_state(),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Confirm,
            }),
            &mut ScriptedResolver::new(),
            engine::apply,
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    }

    /// Flag on: the acknowledge `Confirm` (#478) surfaces after the commit
    /// window, and confirming it resolves the skill test to teardown (the
    /// dispatch-level Confirm routing, end-to-end through `apply`).
    #[test]
    fn flag_on_confirming_the_acknowledge_resolves_the_test() {
        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_seat(test_support::seat(1))
            .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
            .open_turn(inv)
            .build();
        // No builder setter for the acknowledge flag.
        state.interactive_acknowledge = true;

        let session = TestSession::new(state)
            .test_skill(inv, SkillKind::Willpower, 2)
            .commit(&[]);
        assert_eq!(
            session.prompt().kind,
            InputKind::Confirm,
            "the acknowledge pause is up"
        );
        let session = session.confirm();

        session.assert_at_turn_menu(inv);
        crate::assert_event!(session.events(), Event::SkillTestEnded { .. });
    }
}
