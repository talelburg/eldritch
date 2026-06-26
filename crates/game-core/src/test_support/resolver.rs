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
//! investigator must reply with [`InputResponse::PickMultiple`] (each
//! `OptionId` a hand index). [`ScriptedResolver::commit_cards`] is the
//! ergonomic helper: tests pass card codes, the resolver translates them to
//! hand indices using [`GameState`] at resolve time.
//!
//! [`InputResponse::PickMultiple`]: crate::action::InputResponse::PickMultiple
//!
//! # Example
//!
//! ```
//! use game_core::action::{Action, InputResponse, PlayerAction};
//! use game_core::engine::EngineOutcome;
//! use game_core::test_support::GameStateBuilder;
//!
//! // A `ResolveInput` against a bare state with no outstanding prompt
//! // rejects — a tiny smoke test for the fluent API without needing a
//! // real chaos bag or any setup.
//! let result = GameStateBuilder::new()
//!     .session()
//!     .apply(Action::Player(PlayerAction::ResolveInput {
//!         response: InputResponse::Skip,
//!     }))
//!     .run();
//! assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
//! ```

use std::collections::VecDeque;

use crate::action::{Action, InputResponse, PlayerAction};
use crate::engine::{apply, ApplyResult, EngineOutcome, InputKind, InputRequest};
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
/// [`skip`](Self::skip), [`pick_single`](Self::pick_single),
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
    /// skill test. A by-`CardCode` convenience over the real index-based
    /// commit flow (resolves codes to hand indices at replay time).
    CommitCards(Vec<CardCode>),
    /// Pick the offered option whose label matches this string, resolved to
    /// `PickSingle(option.id)` at replay time. A by-id convenience for the
    /// location/investigator-pick windows, whose options are labeled with the
    /// candidate's debug repr (`pick_location` / `pick_investigator` store
    /// `format!("{id:?}")`).
    PickByLabel(String),
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
    pub fn pick_single(&mut self, id: crate::engine::OptionId) -> &mut Self {
        self.push(InputResponse::PickSingle(id))
    }

    /// Pick the investigator `id` from a structured-choice window by matching
    /// the offered option labeled `format!("{id:?}")` at replay time, yielding
    /// [`InputResponse::PickSingle`].
    pub fn pick_investigator(&mut self, id: InvestigatorId) -> &mut Self {
        self.steps
            .push_back(ScriptedStep::PickByLabel(format!("{id:?}")));
        self
    }

    /// Pick the location `id` from a structured-choice window by matching the
    /// offered option labeled `format!("{id:?}")` at replay time, yielding
    /// [`InputResponse::PickSingle`].
    pub fn pick_location(&mut self, id: LocationId) -> &mut Self {
        self.steps
            .push_back(ScriptedStep::PickByLabel(format!("{id:?}")));
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
            ScriptedStep::CommitCards(codes) => InputResponse::PickMultiple {
                selected: resolve_commit_codes(&codes, state, &request.prompt)
                    .into_iter()
                    .map(crate::engine::OptionId)
                    .collect(),
            },
            ScriptedStep::PickByLabel(label) => {
                let opt = request
                    .options
                    .iter()
                    .find(|o| o.label == label)
                    .unwrap_or_else(|| {
                        panic!(
                            "ScriptedResolver::pick_*: no offered option labeled {label:?}; \
                         prompt {:?}, options {:?}",
                            request.prompt, request.options,
                        )
                    });
                InputResponse::PickSingle(opt.id)
            }
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
    let in_flight = state.current_skill_test().unwrap_or_else(|| {
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
/// Like `drive(state, action, ScriptedResolver::commit_cards(&[]))`, with one
/// addition: it also `Skip`s any framework Fast player window the action *parks*.
/// The skill-test ST.1/ST.2 windows (#374) return `Done`-idle (no
/// `AwaitingInput`) with the window on the stack whenever a Fast card/ability is
/// available; a plain `drive` would mistake that idle for the terminal outcome,
/// so we decline (Skip) the window and continue. (For callers with no Fast
/// eligibility — the vast majority — the windows auto-skip and this is identical
/// to the plain commit-nothing drive.)
///
/// **Assumes every `AwaitingInput` is the commit prompt** (answers each with an
/// empty `PickMultiple`). A caller whose action drives a *reaction* window (a
/// real `PickSingle`) must script it via [`drive`] instead — here it would be
/// fed an empty `PickMultiple` and rejected. No current caller does this; the
/// skill-test ST.1/ST.2 windows never surface a reaction prompt (they carry no
/// `Trigger::OnEvent` candidates).
pub fn apply_no_commits(state: GameState, action: Action) -> ApplyResult {
    drive_to_terminal_no_commits(apply(state, action))
}

/// Whether `state` is paused at the open-turn action menu (2b, #447): an
/// [`InvestigatorTurn { ending: false }`](crate::state::Continuation::InvestigatorTurn)
/// frame on top, which the engine surfaces as the `AwaitingInput` action menu.
/// Test drivers treat it as a terminal stopping point — it is the *next*
/// action's prompt, not a window to resolve, so driving past it would silently
/// consume another turn action.
fn at_open_turn_menu(state: &GameState) -> bool {
    matches!(
        state.continuations.last(),
        Some(crate::state::Continuation::InvestigatorTurn { ending: false, .. })
    )
}

/// Start a plain skill test (the [`perform_skill_test`] synthetic entry point)
/// and drive it to a terminal outcome committing no cards and declining every
/// Fast window — the skill-test analogue of [`apply_no_commits`]. Replaces the
/// `apply_no_commits(state, Action::Player(PlayerAction::PerformSkillTest{..}))`
/// idiom (#447): a rejection at the start surfaces directly; a started test
/// resolves through its commit window with an empty commit list.
pub fn perform_skill_test_no_commits(
    state: GameState,
    investigator: InvestigatorId,
    skill: crate::state::SkillKind,
    difficulty: i8,
) -> ApplyResult {
    drive_to_terminal_no_commits(perform_skill_test(state, investigator, skill, difficulty))
}

/// Continue a no-commits drive from an already-applied [`ApplyResult`]: commit
/// no cards (empty `PickMultiple` at every commit window) and *decline* every
/// framework Fast player window (Skip). Most actions never open one, so this is
/// identical to a plain commit-nothing drive — but the skill-test ST.1/ST.2
/// player windows (#374) *park* (return `Done`-idle with the window on the
/// stack, no `AwaitingInput`) whenever a Fast card/ability is available, and a
/// plain `drive` would mistake that idle for the terminal outcome. So skip a
/// parked window explicitly and continue.
fn drive_to_terminal_no_commits(first: ApplyResult) -> ApplyResult {
    const MAX_ITERATIONS: u32 = 1024;
    let ApplyResult {
        mut state,
        mut events,
        mut outcome,
    } = first;
    let mut iterations = 0u32;
    loop {
        // The open-turn action menu (2b, #447) is a TERMINAL stopping point: it
        // is the next action's prompt, not a commit window, and resolving it
        // would consume another turn action. Stop here — the post-flip
        // equivalent of the old idle-`Done` open turn.
        if matches!(outcome, EngineOutcome::AwaitingInput { .. }) && at_open_turn_menu(&state) {
            return ApplyResult {
                state,
                events,
                outcome,
            };
        }
        // The only `AwaitingInput`s in a no-commits drive are the commit window
        // (PickMultiple) and the #478 acknowledge pause (Confirm); a `Done`-idle
        // with an open window is a parked Fast player window to decline. Anything
        // else is terminal.
        let next = if let EngineOutcome::AwaitingInput { request, .. } = &outcome {
            match request.kind {
                InputKind::Confirm => InputResponse::Confirm,
                _ => InputResponse::PickMultiple {
                    selected: Vec::new(),
                },
            }
        } else if matches!(outcome, EngineOutcome::Done) && !state.open_windows().is_empty() {
            InputResponse::Skip
        } else {
            return ApplyResult {
                state,
                events,
                outcome,
            };
        };
        iterations += 1;
        assert!(
            iterations <= MAX_ITERATIONS,
            "drive_to_terminal_no_commits: exceeded {MAX_ITERATIONS} iterations without a \
             terminal outcome; the engine appears to be cycling (re-parking a window?)",
        );
        let r = apply(
            state,
            Action::Player(PlayerAction::ResolveInput { response: next }),
        );
        state = r.state;
        events.extend(r.events);
        outcome = r.outcome;
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
    let first = applier(state, action);
    drain_with_applier(first, resolver, applier)
}

/// Start a plain skill test (the [`perform_skill_test`] synthetic entry point)
/// and drain its commit window — and any further `AwaitingInput` — through
/// `resolver`. The resolver/`commit_cards` analogue of [`drive`], replacing the
/// `drive(state, Action::Player(PlayerAction::PerformSkillTest{..}), resolver)`
/// idiom (#447).
pub fn drive_skill_test<R: ChoiceResolver>(
    state: GameState,
    investigator: InvestigatorId,
    skill: crate::state::SkillKind,
    difficulty: i8,
    mut resolver: R,
) -> ApplyResult {
    drain_with_applier(
        perform_skill_test(state, investigator, skill, difficulty),
        &mut resolver,
        apply,
    )
}

/// Continue a resolver-driven drive from an already-applied [`ApplyResult`]:
/// drain every [`AwaitingInput`](EngineOutcome::AwaitingInput) through
/// `resolver` (re-applying its `ResolveInput` responses via `applier`) until the
/// engine returns Done/Rejected. The shared tail of [`drive_with_applier`] and
/// [`drive_skill_test`], so neither re-implements the drain loop.
pub(crate) fn drain_with_applier<R, F>(
    first: ApplyResult,
    resolver: &mut R,
    mut applier: F,
) -> ApplyResult
where
    R: ChoiceResolver + ?Sized,
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
        // The open-turn action menu (2b, #447) is terminal for a resolver-driven
        // drive: it is the next action's prompt, not something the resolver
        // scripts, so stop and return it (the post-flip equivalent of the old
        // idle-`Done` open turn).
        if matches!(outcome, EngineOutcome::AwaitingInput { .. }) && at_open_turn_menu(&state) {
            return ApplyResult {
                state,
                events,
                outcome,
            };
        }
        let request = match outcome {
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
            } => request,
        };
        let response = resolver.next(&request, &state);
        iterations += 1;
        assert!(
            iterations <= MAX_ITERATIONS,
            "drive: exceeded {MAX_ITERATIONS} iterations without reaching Done/Rejected; \
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

/// Drive one open-turn action by enumerating the legal actions, finding the
/// `OptionId` whose `TurnAction` equals `action`, and submitting it as
/// `ResolveInput(PickSingle(..))`. Panics if `action` is not currently legal
/// (a test-authoring bug). Returns the raw `ApplyResult` — assert on the
/// resulting **state/events**, not on `outcome == Done` (post-flip the outcome
/// is the next open-turn menu's `AwaitingInput`).
pub fn take_turn_action(
    state: GameState,
    action: &crate::engine::enumerate::TurnAction,
) -> ApplyResult {
    let actions = crate::engine::enumerate::legal_actions(&state);
    let idx = actions.iter().position(|a| a == action).unwrap_or_else(|| {
        panic!("take_turn_action: {action:?} is not legal; offered: {actions:?}")
    });
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(crate::engine::OptionId(
                u32::try_from(idx).expect("action index fits u32"),
            )),
        }),
    )
}

/// Dispatch a [`TurnAction`](crate::engine::enumerate::TurnAction) straight to
/// its handler, **bypassing the enumeration gate** that
/// [`take_turn_action`] routes through.
///
/// [`take_turn_action`] calls `legal_actions` first and panics if the action
/// is not offered — so it cannot reach a handler against deliberately corrupt
/// state (the corrupt action is excluded from the enumeration). This seam runs
/// the action through the same `Cx` build, transactional restore, and
/// resolution-latch firing as [`apply`] (via the shared
/// `apply_via` scaffolding), but dispatches via the internal
/// `dispatch_turn_action` + `drive` instead of the enumeration round-trip. The
/// single legitimate use is the
/// `#[should_panic(expected = "state-corruption invariant violation")]` handler
/// tests that inject a dangling `current_location` / missing-from-map and expect
/// the handler — not the enumerator — to panic.
pub fn dispatch_turn_action_unchecked(
    state: GameState,
    action: &crate::engine::enumerate::TurnAction,
) -> ApplyResult {
    crate::engine::apply_via(state, crate::scenario_registry::current(), |cx| {
        let outcome = crate::engine::dispatch_turn_action(cx, action);
        crate::engine::drive(cx, outcome)
    })
}

/// Start a plain skill test directly: `investigator` tests `skill` against
/// `difficulty`, returning the [`ApplyResult`] (typically an `AwaitingInput`
/// pausing at the commit window).
///
/// The synthetic test entry point that replaced the retired
/// `PlayerAction::PerformSkillTest` wire variant (#447). Skill tests are
/// normally initiated by a real action (Investigate / Fight / Evade) or a card
/// effect; this lets a test exercise skill-test resolution in isolation with an
/// arbitrary skill + difficulty. Runs through the same `Cx` build / `drive` loop
/// as [`apply`], via the shared `apply_via` scaffolding.
pub fn perform_skill_test(
    state: GameState,
    investigator: InvestigatorId,
    skill: crate::state::SkillKind,
    difficulty: i8,
) -> ApplyResult {
    crate::engine::apply_via(state, crate::scenario_registry::current(), |cx| {
        let outcome = crate::engine::start_plain_skill_test(cx, investigator, skill, difficulty);
        crate::engine::drive(cx, outcome)
    })
}

/// Fluent test driver: pair a [`GameState`] with an initial action and
/// a scripted resolver, then [`run`](Self::run) the engine through to a
/// terminal outcome.
///
/// Construct via [`GameStateBuilder::session`](super::GameStateBuilder::session) or
/// [`TestSession::new`].
#[derive(Debug)]
#[must_use = "TestSession does nothing until you call .run()"]
pub struct TestSession {
    state: GameState,
    action: Option<Action>,
    resolver: ScriptedResolver,
}

impl crate::state::GameStateBuilder {
    /// Build into a [`TestSession`] for driving the engine with a
    /// scripted [`ChoiceResolver`].
    ///
    /// Equivalent to `TestSession::new(self.build())`; sugar so
    /// resolver-driven tests can write
    /// `GameStateBuilder::new()...session().apply(...).resolve_choices(...).run()`.
    /// Lives here (test-only) rather than on the builder itself so the
    /// production builder in [`crate::state`] carries no test dependency.
    pub fn session(self) -> TestSession {
        TestSession::new(self.build())
    }
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

    /// Fluent open-turn action: see [`take_turn_action`]. Threads the resulting
    /// state; drains any `AwaitingInput` the action itself opens via the session's
    /// resolver script, exactly like [`TestSession::apply`].
    pub fn take(self, action: &crate::engine::enumerate::TurnAction) -> Self {
        let idx = crate::engine::enumerate::legal_actions(&self.state)
            .iter()
            .position(|a| a == action)
            .unwrap_or_else(|| panic!("TestSession::take: {action:?} not legal"));
        self.apply(Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(crate::engine::OptionId(
                u32::try_from(idx).expect("action index fits u32"),
            )),
        }))
    }

    /// Record the resolver script. The closure receives `&mut
    /// ScriptedResolver`; chain calls inside to build up the response
    /// sequence:
    ///
    /// ```
    /// # use game_core::test_support::GameStateBuilder;
    /// # use game_core::action::{Action, PlayerAction};
    /// let _session = GameStateBuilder::new()
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
    use crate::engine::{EngineOutcome, InputRequest, OptionId, ResumeToken};
    use crate::event::Event;
    use crate::state::{CardCode, InvestigatorId, Phase};
    use crate::test_support::{test_investigator, test_location, GameStateBuilder};

    #[test]
    fn take_turn_action_resolves_end_turn_via_optionid() {
        use crate::engine::enumerate::TurnAction;
        // EndTurn reads max_health / max_sanity on the investigator card.
        crate::test_support::install_test_registry();
        let state = crate::test_support::GameStateBuilder::default()
            .with_investigator(crate::test_support::test_investigator(1))
            .with_phase(crate::state::Phase::Investigation)
            .with_active_investigator(crate::state::InvestigatorId(1))
            .with_turn_order([crate::state::InvestigatorId(1)])
            .with_chaos_bag(crate::state::ChaosBag::new([
                crate::state::ChaosToken::Numeric(0),
            ]))
            .with_phase_anchor(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(crate::state::InvestigatorId(1))
            .build();
        let result = take_turn_action(state, &TurnAction::EndTurn);
        assert!(
            !matches!(result.outcome, EngineOutcome::Rejected { .. }),
            "{:?}",
            result.outcome
        );
    }

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
        use crate::dsl::SkillTestKind;
        use crate::state::{InFlightSkillTest, SkillTestFollowUp};
        let id = InvestigatorId(1);
        let mut inv = crate::test_support::test_investigator(1);
        inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
        let mut state = GameStateBuilder::new().with_investigator(inv).build();
        state
            .continuations
            .push(crate::state::Continuation::SkillTest(InFlightSkillTest {
                investigator: id,
                skill: crate::state::SkillKind::Intellect,
                kind: SkillTestKind::Plain,
                difficulty: 1,
                committed_by_active: Vec::new(),
                tested_location: None,
                follow_up: SkillTestFollowUp::None,
                on_fail: None,
                on_success: None,
                source: None,
                continuation: crate::state::SkillTestStep::AwaitingCommit,
                test_modifier: 0,
                bonus_attack_damage: 0,
                resolved: None,
                symbol_on_fail: None,
            }));
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
        r.commit_cards(&[CardCode::new("X"), CardCode::new("Y")]);
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
        r.commit_cards(&[CardCode::new("X"), CardCode::new("X")]);
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
        use crate::engine::enumerate::TurnAction;
        let id = InvestigatorId(1);
        // Two investigators so the first EndTurn is a mid-round rotation
        // (reaches Done immediately) rather than a round-ending cascade — the
        // latter would now pause at the Mythos encounter-draw prompt (#348).
        let result = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_location(test_location(10, "Study"))
            .with_active_investigator(id)
            .with_turn_order([id, InvestigatorId(2)])
            .with_phase_anchor(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(id)
            .session()
            .take(&TurnAction::EndTurn)
            .resolve_choices(|c| {
                // Stale script: engine reaches Done without prompting.
                c.confirm();
            })
            .run();
        assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    }

    #[test]
    #[should_panic(expected = "call .apply(action) before .run()")]
    fn test_session_run_without_apply_panics() {
        let _ = GameStateBuilder::new().session().run();
    }

    /// Flag on: a no-commits drive auto-answers the acknowledge `Confirm` and
    /// resolves the skill test to teardown (exercises both the helper and the
    /// dispatch-level Confirm routing end-to-end through `apply`).
    #[test]
    fn flag_on_no_commits_drive_auto_confirms_acknowledge() {
        use crate::event::Event;
        use crate::state::{ChaosToken, InvestigatorId, SkillKind};
        use crate::test_support::{test_investigator, GameStateBuilder};

        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
        state.interactive_acknowledge = true;

        let result = perform_skill_test_no_commits(state, inv, SkillKind::Willpower, 2);

        assert!(
            matches!(result.outcome, EngineOutcome::Done),
            "drive auto-confirmed the acknowledge and reached a terminal outcome: {:?}",
            result.outcome
        );
        assert!(
            result
                .events
                .iter()
                .any(|e| matches!(e, Event::SkillTestEnded { .. })),
            "the test resolved to the end: {:?}",
            result.events
        );
    }
}
