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
//! [`drive`] runs an action through the engine and drains any
//! `AwaitingInput` outcomes through the resolver until the engine
//! returns [`Done`](crate::EngineOutcome::Done) or
//! [`Rejected`](crate::EngineOutcome::Rejected). [`TestSession`] is the
//! fluent wrapper that pairs a [`GameState`] with a resolver script.
//!
//! # Status
//!
//! No engine path emits `AwaitingInput` yet — the first consumer is
//! the skill-test commit window (#63). This module ships the test
//! infrastructure ahead of that work so the API has a stable home.
//! [`ScriptedResolver::commit_cards`] is intentionally a stub until #63
//! finalizes the commit-window response shape.
//!
//! # Example
//!
//! ```
//! use game_core::action::{Action, InputResponse, PlayerAction};
//! use game_core::engine::EngineOutcome;
//! use game_core::test_support::TestGame;
//!
//! // `ResolveInput` rejects today (TODO(#63)); use it as a no-resolver
//! // smoke test for the fluent API.
//! let result = TestGame::new()
//!     .session()
//!     .apply(Action::Player(PlayerAction::ResolveInput {
//!         response: InputResponse::Confirm,
//!     }))
//!     .run();
//! assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
//! ```

use std::collections::VecDeque;

use crate::action::{Action, InputResponse, PlayerAction};
use crate::engine::{apply, ApplyResult, EngineOutcome, InputRequest};
use crate::event::Event;
use crate::state::{CardCode, GameState, InvestigatorId, LocationId};

/// Provide a response for an `AwaitingInput` prompt during a
/// [`drive`]-style session.
///
/// Tests use [`ScriptedResolver`]. Future hosts (server, web client)
/// will implement this trait against their own UI/transport layer; the
/// trait is the boundary between the engine's pause-and-resume protocol
/// and the consumer's input-collection mechanism.
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
/// [`skip`](Self::skip), [`pick`](Self::pick),
/// [`pick_investigator`](Self::pick_investigator),
/// [`pick_location`](Self::pick_location),
/// [`commit_cards`](Self::commit_cards)). When the engine prompts and the
/// script is empty, [`next`](Self::next) panics with the prompt text — a
/// useful failure mode in tests.
///
/// Helpers take `&mut self` and return `&mut Self` so they chain inside
/// a [`TestSession::resolve_choices`] closure.
#[derive(Debug, Default, Clone)]
pub struct ScriptedResolver {
    steps: VecDeque<ScriptedStep>,
}

#[derive(Debug, Clone)]
enum ScriptedStep {
    Response(InputResponse),
    /// Commit a set of cards from the active investigator's hand to a
    /// skill test. TODO(#63): replace with the real commit-window
    /// response variant(s) once finalized.
    CommitCards(Vec<CardCode>),
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

    /// Respond with [`InputResponse::PickIndex`].
    pub fn pick(&mut self, index: u32) -> &mut Self {
        self.push(InputResponse::PickIndex(index))
    }

    /// Respond with [`InputResponse::PickInvestigator`].
    pub fn pick_investigator(&mut self, id: InvestigatorId) -> &mut Self {
        self.push(InputResponse::PickInvestigator(id))
    }

    /// Respond with [`InputResponse::PickLocation`].
    pub fn pick_location(&mut self, id: LocationId) -> &mut Self {
        self.push(InputResponse::PickLocation(id))
    }

    /// Commit cards (by code) from the active investigator's hand to
    /// the current skill test.
    ///
    /// TODO(#63): stub until the commit-window prompt shape is
    /// finalized. The codes are recorded but never converted into
    /// responses; if the engine ever prompts a [`ScriptedResolver`]
    /// whose next step is `CommitCards`, [`next`](Self::next) panics
    /// with the request prompt and the recorded codes. Once #63 lands,
    /// this helper will translate codes to the real commit-window
    /// response variant(s) using the engine state passed into `next`.
    pub fn commit_cards(&mut self, codes: &[CardCode]) -> &mut Self {
        self.steps
            .push_back(ScriptedStep::CommitCards(codes.to_vec()));
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
    fn next(&mut self, request: &InputRequest, _state: &GameState) -> InputResponse {
        let step = self.steps.pop_front().unwrap_or_else(|| {
            panic!(
                "ScriptedResolver: no scripted response for prompt: {:?}",
                request.prompt,
            )
        });
        match step {
            ScriptedStep::Response(r) => r,
            ScriptedStep::CommitCards(codes) => panic!(
                "ScriptedResolver::commit_cards: TODO(#63) — commit-window response \
                 shape not yet finalized; helper is a stub. Codes recorded: {:?}, \
                 prompt was: {:?}",
                codes, request.prompt,
            ),
        }
    }
}

/// Run `action` against `state`, draining
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) outcomes through
/// `resolver` until the engine returns [`Done`](EngineOutcome::Done) or
/// [`Rejected`](EngineOutcome::Rejected).
///
/// The returned [`ApplyResult`] aggregates state, events, and outcome
/// across every sub-`apply` in the round trip:
///
/// - `state` is the final state after all sub-applies.
/// - `events` concatenate every sub-apply's events in order.
/// - `outcome` is the terminal [`Done`](EngineOutcome::Done) or
///   [`Rejected`](EngineOutcome::Rejected) — `drive` never returns
///   `AwaitingInput` (the resolver is consulted and the loop continues).
///
/// Panics if the resolver runs out of scripted responses while the
/// engine is still emitting `AwaitingInput`, or if the loop exceeds an
/// internal iteration cap (a sign of a broken resolver or engine cycle).
pub fn drive<R: ChoiceResolver>(state: GameState, action: Action, mut resolver: R) -> ApplyResult {
    drive_with_applier(state, action, &mut resolver, apply)
}

/// Loop body of [`drive`] with the engine entry point parameterized.
///
/// Tests in this module use this to substitute a fake `apply` that
/// produces `AwaitingInput` (which no real engine path emits yet),
/// exercising the drain logic without needing a real engine consumer.
pub(crate) fn drive_with_applier<R, F>(
    state: GameState,
    action: Action,
    resolver: &mut R,
    mut applier: F,
) -> ApplyResult
where
    R: ChoiceResolver + ?Sized,
    F: FnMut(GameState, Action) -> ApplyResult,
{
    const MAX_ITERATIONS: u32 = 1024;

    let mut state = state;
    let mut events: Vec<Event> = Vec::new();
    let mut next_action = Some(action);
    let mut iterations = 0u32;

    loop {
        iterations += 1;
        assert!(
            iterations <= MAX_ITERATIONS,
            "drive: exceeded {MAX_ITERATIONS} iterations without reaching Done/Rejected; \
             resolver or engine appears to be cycling",
        );

        let act = next_action
            .take()
            .expect("drive: internal invariant — next_action set on every loop entry");
        let result = applier(state, act);
        state = result.state;
        events.extend(result.events);

        match result.outcome {
            EngineOutcome::Done => {
                return ApplyResult {
                    state,
                    events,
                    outcome: EngineOutcome::Done,
                };
            }
            EngineOutcome::Rejected { reason } => {
                return ApplyResult {
                    state,
                    events,
                    outcome: EngineOutcome::Rejected { reason },
                };
            }
            EngineOutcome::AwaitingInput {
                request,
                resume_token: _,
            } => {
                let response = resolver.next(&request, &state);
                next_action = Some(Action::Player(PlayerAction::ResolveInput { response }));
            }
        }
    }
}

/// Fluent test driver: pair a [`GameState`] with an initial action and
/// a scripted resolver, then [`run`](Self::run) the engine through to a
/// terminal outcome.
///
/// Construct via [`TestGame::session`](super::TestGame::session) or
/// [`TestSession::new`].
#[derive(Debug)]
#[must_use = "TestSession does nothing until you call .run()"]
pub struct TestSession {
    state: GameState,
    action: Option<Action>,
    resolver: ScriptedResolver,
}

impl TestSession {
    /// Wrap a built state.
    pub fn new(state: GameState) -> Self {
        Self {
            state,
            action: None,
            resolver: ScriptedResolver::new(),
        }
    }

    /// Record the initial action to apply. Replaces any previous
    /// recorded action — `apply` is the single entry point per session,
    /// not a queue.
    pub fn apply(mut self, action: Action) -> Self {
        self.action = Some(action);
        self
    }

    /// Record the resolver script. The closure receives `&mut
    /// ScriptedResolver`; chain calls inside to build up the response
    /// sequence:
    ///
    /// ```
    /// # use game_core::test_support::TestGame;
    /// # use game_core::action::{Action, PlayerAction};
    /// let _session = TestGame::new()
    ///     .session()
    ///     .resolve_choices(|c| {
    ///         c.confirm();
    ///         c.skip();
    ///     });
    /// ```
    pub fn resolve_choices(mut self, f: impl FnOnce(&mut ScriptedResolver)) -> Self {
        f(&mut self.resolver);
        self
    }

    /// Execute. Panics if [`apply`](Self::apply) was not called.
    pub fn run(self) -> ApplyResult {
        let action = self
            .action
            .expect("TestSession::run: call .apply(action) before .run()");
        drive(self.state, action, self.resolver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{Action, InputResponse, PlayerAction};
    use crate::engine::{InputRequest, ResumeToken};
    use crate::state::{CardCode, InvestigatorId, LocationId, Phase};
    use crate::test_support::{test_investigator, test_location, TestGame};

    fn empty_state() -> GameState {
        TestGame::new().build()
    }

    fn req(prompt: &str) -> InputRequest {
        InputRequest {
            prompt: prompt.to_string(),
        }
    }

    #[test]
    fn scripted_resolver_returns_responses_in_fifo_order() {
        let mut r = ScriptedResolver::new();
        r.confirm()
            .skip()
            .pick(7)
            .pick_investigator(InvestigatorId(2))
            .pick_location(LocationId(99));
        assert_eq!(r.remaining(), 5);

        let state = empty_state();
        let p = req("pick");
        assert_eq!(r.next(&p, &state), InputResponse::Confirm);
        assert_eq!(r.next(&p, &state), InputResponse::Skip);
        assert_eq!(r.next(&p, &state), InputResponse::PickIndex(7));
        assert_eq!(
            r.next(&p, &state),
            InputResponse::PickInvestigator(InvestigatorId(2))
        );
        assert_eq!(
            r.next(&p, &state),
            InputResponse::PickLocation(LocationId(99))
        );
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    #[should_panic(expected = "no scripted response")]
    fn scripted_resolver_panics_when_script_exhausted() {
        let mut r = ScriptedResolver::new();
        let _ = r.next(&req("oops"), &empty_state());
    }

    #[test]
    #[should_panic(expected = "TODO(#63)")]
    fn commit_cards_is_a_stub_until_issue_63() {
        let mut r = ScriptedResolver::new();
        r.commit_cards(&[CardCode::new("01001")]);
        let _ = r.next(&req("commit"), &empty_state());
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
            Action::Player(PlayerAction::EndTurn),
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
                    assert!(matches!(action, Action::Player(PlayerAction::EndTurn)));
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
            Action::Player(PlayerAction::EndTurn),
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
            Action::Player(PlayerAction::EndTurn),
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
            Action::Player(PlayerAction::EndTurn),
            &mut resolver,
            applier,
        );
        assert_eq!(result.events.len(), 2);
        assert!(matches!(
            result.events[0],
            Event::ResourcesGained { amount: 1, .. }
        ));
        assert!(matches!(result.events[1], Event::TurnEnded { .. }));
    }

    #[test]
    fn drive_real_engine_passes_through_rejected_action() {
        // No `AwaitingInput` site exists in the engine yet, so the only
        // real-engine path this test exercises is the trivial pass-
        // through of a `Rejected` outcome (ResolveInput TODO-rejects).
        let result = drive(
            empty_state(),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Confirm,
            }),
            ScriptedResolver::new(),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    }

    #[test]
    fn test_session_fluent_round_trip() {
        let id = InvestigatorId(1);
        let result = TestGame::new()
            .with_phase(Phase::Investigation)
            .with_investigator(test_investigator(1))
            .with_location(test_location(10, "Study"))
            .with_active_investigator(id)
            .with_turn_order([id])
            .session()
            .apply(Action::Player(PlayerAction::EndTurn))
            .resolve_choices(|c| {
                // Stale script: engine reaches Done without prompting.
                c.confirm();
            })
            .run();
        assert!(matches!(result.outcome, EngineOutcome::Done));
    }

    #[test]
    #[should_panic(expected = "call .apply(action) before .run()")]
    fn test_session_run_without_apply_panics() {
        let _ = TestGame::new().session().run();
    }
}
