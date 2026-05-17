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
//! # Engine consumers
//!
//! The first (and currently only) `AwaitingInput` site is the skill-
//! test commit window (#63). When the engine prompts, the active
//! investigator must reply with [`InputResponse::CommitCards`].
//! [`ScriptedResolver::commit_cards`] is the ergonomic helper: tests
//! pass card codes, the resolver translates them to hand indices using
//! [`GameState`] at resolve time.
//!
//! [`InputResponse::CommitCards`]: crate::action::InputResponse::CommitCards
//!
//! # Example
//!
//! ```
//! use game_core::action::{Action, PlayerAction};
//! use game_core::engine::EngineOutcome;
//! use game_core::test_support::TestGame;
//!
//! // A skill-test action with no investigator in state still rejects
//! // — useful as a tiny smoke test for the fluent API without needing
//! // a real chaos bag.
//! let result = TestGame::new()
//!     .session()
//!     .apply(Action::Player(PlayerAction::EndTurn))
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
    fn next(&mut self, request: &InputRequest, state: &GameState) -> InputResponse {
        let step = self.steps.pop_front().unwrap_or_else(|| {
            panic!(
                "ScriptedResolver: no scripted response for prompt: {:?}",
                request.prompt,
            )
        });
        match step {
            ScriptedStep::Response(r) => r,
            ScriptedStep::CommitCards(codes) => InputResponse::CommitCards {
                indices: resolve_commit_codes(&codes, state, &request.prompt),
            },
        }
    }
}

/// Translate scripted commit-card codes to hand indices using the
/// in-flight skill test's investigator on `state`.
///
/// Returns an empty vec for an empty code list (the canonical
/// commit-nothing case). Each code is matched against the
/// investigator's hand left-to-right; duplicate codes claim distinct
/// indices so `&[CardCode("X"), CardCode("X")]` against a hand with
/// two `X`s yields `[i, j]`. Panics with the prompt and remaining
/// state if any code can't be matched — that's a test-author error.
fn resolve_commit_codes(codes: &[CardCode], state: &GameState, prompt: &str) -> Vec<u32> {
    if codes.is_empty() {
        return Vec::new();
    }
    let in_flight = state.in_flight_skill_test.as_ref().unwrap_or_else(|| {
        panic!(
            "ScriptedResolver::commit_cards: state has no in-flight skill test at \
             resolve time; prompt was: {prompt:?}",
        )
    });
    let inv = state
        .investigators
        .get(&in_flight.investigator)
        .unwrap_or_else(|| {
            panic!(
                "ScriptedResolver::commit_cards: in-flight investigator {:?} not in \
                 state.investigators; prompt was: {prompt:?}",
                in_flight.investigator,
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
                    "ScriptedResolver::commit_cards: code {code:?} not found in \
                     {:?}'s hand (hand = {:?}, already-used indices = {:?}); \
                     prompt was: {prompt:?}",
                    in_flight.investigator, inv.hand, used,
                )
            });
        used[idx] = true;
        indices.push(u32::try_from(idx).expect("hand index fits in u32"));
    }
    indices
}

/// Drive a single skill-test-initiating action through the engine
/// with an empty commit submitted to the commit window.
///
/// Tests that don't care about the commit window (they're exercising
/// the rest of skill-test resolution) call this instead of
/// [`apply`] and treat the returned
/// [`ApplyResult`] exactly as they used to — `events` accumulates
/// across `SkillTestStarted`, the empty `ResolveInput`, and the
/// post-commit resolution chain; `outcome` is the terminal
/// [`Done`](EngineOutcome::Done) or [`Rejected`](EngineOutcome::Rejected).
///
/// Equivalent to:
/// ```ignore
/// drive(state, action, { let mut r = ScriptedResolver::new(); r.commit_cards(&[]); r })
/// ```
pub fn apply_no_commits(state: GameState, action: Action) -> ApplyResult {
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    drive(state, action, resolver)
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

    /// Build a tiny state with one investigator who has a non-empty
    /// hand and an in-flight skill test parked on it. Used by the
    /// commit-card resolution tests below.
    fn state_with_in_flight_hand(hand: &[&str]) -> GameState {
        use crate::dsl::SkillTestKind;
        use crate::state::{InFlightSkillTest, SkillTestFollowUp};
        let id = InvestigatorId(1);
        let mut inv = crate::test_support::test_investigator(1);
        inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
        let mut state = TestGame::new().with_investigator(inv).build();
        state.in_flight_skill_test = Some(InFlightSkillTest {
            investigator: id,
            skill: crate::state::SkillKind::Intellect,
            kind: SkillTestKind::Plain,
            difficulty: 1,
            committed_by_active: Vec::new(),
            follow_up: SkillTestFollowUp::None,
        });
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
        assert_eq!(response, InputResponse::CommitCards { indices: vec![] });
    }

    #[test]
    fn commit_cards_translates_codes_to_hand_indices_in_order() {
        let mut r = ScriptedResolver::new();
        r.commit_cards(&[CardCode::new("X"), CardCode::new("Y")]);
        let state = state_with_in_flight_hand(&["X", "Y", "Z"]);
        let response = r.next(&req("commit"), &state);
        assert_eq!(
            response,
            InputResponse::CommitCards {
                indices: vec![0, 1],
            }
        );
    }

    #[test]
    fn commit_cards_duplicate_code_picks_distinct_indices_left_to_right() {
        let mut r = ScriptedResolver::new();
        r.commit_cards(&[CardCode::new("X"), CardCode::new("X")]);
        let state = state_with_in_flight_hand(&["A", "X", "B", "X"]);
        let response = r.next(&req("commit"), &state);
        assert_eq!(
            response,
            InputResponse::CommitCards {
                indices: vec![1, 3],
            }
        );
    }

    #[test]
    #[should_panic(expected = "no in-flight skill test")]
    fn commit_cards_panics_when_no_in_flight_skill_test() {
        let mut r = ScriptedResolver::new();
        r.commit_cards(&[CardCode::new("X")]);
        let _ = r.next(&req("commit"), &empty_state());
    }

    #[test]
    #[should_panic(expected = "not found")]
    fn commit_cards_panics_when_code_not_in_hand() {
        let mut r = ScriptedResolver::new();
        r.commit_cards(&[CardCode::new("MISSING")]);
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
