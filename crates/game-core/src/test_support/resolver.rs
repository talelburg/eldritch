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
//! returns [`Done`](crate::engine::EngineOutcome::Done) or
//! [`Rejected`](crate::engine::EngineOutcome::Rejected). [`TestSession`](super::TestSession) is the
//! step-wise driver new tests use: it settles a built state, then applies one
//! step at a time and drains to the next rest through a resolver script.
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
    self, enumerate, ApplyResult, EngineOutcome, InputKind, InputRequest, OptionId, OptionTarget,
};
use crate::scenario_registry;
use crate::state::{CardCode, GameState, InvestigatorId, SkillKind};

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

/// A [`ChoiceResolver`] that takes one offered fast play at the **first**
/// Fast window it meets, then declines every later window and commits
/// nothing.
///
/// The counterpart to [`ScriptedResolver`] for the one prompt shape a script
/// cannot easily pin down: a framework Fast window (#476) re-opens after each
/// play it resolves, so the number of prompts depends on what stays eligible.
/// This resolver answers "buy exactly one thing, then get out of the way".
///
/// `option_index` selects among the window's offered plays, which are
/// enumerated in (investigator, hand-index / ability-index) order — so
/// Hyperawareness 01034's intellect ability is 0 and its agility ability 1.
///
/// It exists because a [`ThisSkillTest`](card_dsl::dsl::ModifierScope::ThisSkillTest)
/// modifier can only be bought from *inside* a test (#676), and a test's ST.1
/// player window is where a `[fast]` ability is offered.
#[derive(Debug, Clone, Copy)]
pub struct TakeOneFastPlay {
    option_index: usize,
    used: bool,
}

impl TakeOneFastPlay {
    /// Take the play at `option_index` at the first Fast window.
    #[must_use]
    pub fn at_index(option_index: usize) -> Self {
        Self {
            option_index,
            used: false,
        }
    }
}

impl ChoiceResolver for TakeOneFastPlay {
    fn next(&mut self, request: &InputRequest, state: &GameState) -> InputResponse {
        if request.skippable && !self.used && request.kind == InputKind::PickSingle {
            self.used = true;
            assert!(
                request.options.len() > self.option_index,
                "TakeOneFastPlay: option {} not offered; the window has {:?}",
                self.option_index,
                request.options,
            );
            return InputResponse::PickSingle(request.options[self.option_index].id);
        }
        NoCommits.next(request, state)
    }
}

/// The "commit nothing" reply policy: decline every skippable prompt (a #476
/// Fast window, a reaction window), acknowledge every `Confirm` (the #478
/// pause), and submit an empty `PickMultiple` to anything else (the skill-test
/// commit window).
///
/// It is a policy over the one drain loop rather than a loop of its own, so
/// [`apply_no_commits`] and [`perform_skill_test_no_commits`] stop exactly
/// where [`drive`] does: at the turn menu, at `Done`, or at `Rejected`.
#[derive(Debug, Clone, Copy, Default)]
struct NoCommits;

impl ChoiceResolver for NoCommits {
    fn next(&mut self, request: &InputRequest, _state: &GameState) -> InputResponse {
        if request.skippable {
            return InputResponse::Skip;
        }
        match request.kind {
            InputKind::Confirm => InputResponse::Confirm,
            _ => InputResponse::PickMultiple {
                selected: Vec::new(),
            },
        }
    }
}

/// Drive a single skill-test-initiating action through the engine
/// with an empty commit submitted to the commit window.
///
/// Tests that don't care about the commit window (they're exercising
/// the rest of skill-test resolution) call this instead of
/// [`apply`](crate::engine::apply) and treat the returned
/// [`ApplyResult`] exactly as they used to — `events` accumulates
/// across `SkillTestStarted`, the empty `ResolveInput`, and the
/// post-commit resolution chain; `outcome` is the terminal
/// [`Done`](EngineOutcome::Done) or [`Rejected`](EngineOutcome::Rejected),
/// or the open-turn menu the action returns to.
///
/// [`drive`] with the no-commits policy: every skippable prompt (a Fast player
/// window, a reaction window) is declined, every `Confirm` is acknowledged, and
/// every other prompt gets an empty `PickMultiple`. A caller whose action needs
/// a *non-skippable* `PickSingle` answered must script it via [`drive`] instead
/// — here it would be fed an empty `PickMultiple` and rejected.
pub fn apply_no_commits(state: GameState, action: Action) -> ApplyResult {
    drain_with_applier(
        engine::apply(state, action),
        answering(&mut NoCommits),
        engine::apply,
    )
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

/// The id of the one option of `request` anchored to `target`, or a panic
/// naming `caller` and why there isn't one. The anchor lookup behind
/// [`ScriptedResolver::pick`] and the session's `pick` step.
pub(super) fn sole_option_at(
    request: &InputRequest,
    target: &OptionTarget,
    caller: &str,
) -> OptionId {
    let mut matching = request
        .options
        .iter()
        .filter(|o| o.target.as_ref() == Some(target));
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
/// panic naming `caller` and listing what is offered. Shared by
/// [`take_turn_action`] and the session's `take` step.
pub(super) fn turn_action_option(state: &GameState, action: &TurnAction, caller: &str) -> OptionId {
    let actions = enumerate::legal_actions(state);
    let idx = actions
        .iter()
        .position(|a| a == action)
        .unwrap_or_else(|| panic!("{caller}: {action:?} is not legal; offered: {actions:?}"));
    OptionId(u32::try_from(idx).expect("action index fits u32"))
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
    skill: SkillKind,
    difficulty: i8,
) -> ApplyResult {
    drain_with_applier(
        perform_skill_test(state, investigator, skill, difficulty),
        answering(&mut NoCommits),
        engine::apply,
    )
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
    drive_with_applier(state, action, &mut resolver, engine::apply)
}

/// Loop body of [`drive`] with the engine entry point parameterized.
///
/// Tests in this module use this to substitute a fake `apply` that scripts
/// the `AwaitingInput` sequence, exercising the drain logic independently of
/// any particular engine prompt.
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
    drain_with_applier(first, answering(resolver), applier)
}

/// Start a plain skill test (the [`perform_skill_test`] synthetic entry point)
/// and drain its commit window — and any further `AwaitingInput` — through
/// `resolver`. The resolver/`commit_cards` analogue of [`drive`], replacing the
/// `drive(state, Action::Player(PlayerAction::PerformSkillTest{..}), resolver)`
/// idiom (#447).
pub fn drive_skill_test<R: ChoiceResolver>(
    state: GameState,
    investigator: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
    mut resolver: R,
) -> ApplyResult {
    drain_with_applier(
        perform_skill_test(state, investigator, skill, difficulty),
        answering(&mut resolver),
        engine::apply,
    )
}

/// A reply policy that answers every prompt through `resolver` — the policy
/// [`drive`] and its siblings run the drain loop under.
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
/// Every driver ([`drive`], [`drive_skill_test`], [`apply_no_commits`],
/// [`perform_skill_test_no_commits`], and each [`TestSession`](super::TestSession) step) is this
/// loop under a different reply policy; none re-implements it.
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
/// `ResolveInput(PickSingle(..))`. Returns the raw `ApplyResult` — assert on the
/// resulting **state/events**, not on `outcome == Done` (post-flip the outcome
/// is the next open-turn menu's `AwaitingInput`).
///
/// # Panics
///
/// Panics if `action` is not currently legal (a test-authoring bug) — the
/// rejection message lists the offered actions.
pub fn take_turn_action(state: GameState, action: &TurnAction) -> ApplyResult {
    let id = turn_action_option(&state, action, "take_turn_action");
    engine::apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(id),
        }),
    )
}

/// Dispatch a [`TurnAction`] straight to
/// its handler, **bypassing the enumeration gate** that
/// [`take_turn_action`] routes through.
///
/// [`take_turn_action`] calls `legal_actions` first and panics if the action
/// is not offered — so it cannot reach a handler against deliberately corrupt
/// state (the corrupt action is excluded from the enumeration). This seam runs
/// the action through the same `Cx` build, transactional restore, and
/// resolution-latch firing as [`apply`](crate::engine::apply) (via the shared
/// `apply_via` scaffolding), but dispatches via the internal
/// `dispatch_turn_action` + `drive` instead of the enumeration round-trip. Two
/// legitimate uses:
///
/// 1. The `#[should_panic(expected = "state-corruption invariant violation")]`
///    handler tests that inject a dangling `current_location` / missing-from-map
///    and expect the handler — not the enumerator — to panic.
/// 2. Proving a handler *itself* rejects an action the enumerator already
///    filters out — a real client can submit one over the wire without
///    consulting the menu (#639's activation initiation gate).
pub fn dispatch_turn_action_unchecked(state: GameState, action: &TurnAction) -> ApplyResult {
    engine::apply_via(state, scenario_registry::current(), |cx| {
        let outcome = engine::dispatch_turn_action(cx, action);
        engine::drive(cx, outcome)
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
/// as [`apply`](crate::engine::apply), via the shared `apply_via` scaffolding.
pub fn perform_skill_test(
    state: GameState,
    investigator: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
) -> ApplyResult {
    engine::apply_via(state, scenario_registry::current(), |cx| {
        let outcome = engine::start_plain_skill_test(cx, investigator, skill, difficulty);
        engine::drive(cx, outcome)
    })
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::SkillTestKind;

    use super::*;
    use crate::engine::ResumeToken;
    use crate::event::Event;
    use crate::state::{ChaosBag, ChaosToken, Continuation, GameStateBuilder, SkillTestId};
    use crate::test_support;

    #[test]
    fn take_turn_action_resolves_end_turn_via_optionid() {
        // EndTurn reads max_health / max_sanity on the investigator card.
        test_support::install_test_registry();
        let state = GameStateBuilder::default()
            .with_investigator(test_support::test_investigator(1))
            .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
            .open_turn(InvestigatorId(1))
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
        // A `ResolveInput` against a state with no outstanding prompt rejects;
        // this exercises the real engine's pass-through of that `Rejected`
        // outcome.
        let result = drive(
            empty_state(),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Confirm,
            }),
            ScriptedResolver::new(),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    }

    /// Flag on: a no-commits drive auto-answers the acknowledge `Confirm` and
    /// resolves the skill test to teardown (exercises both the helper and the
    /// dispatch-level Confirm routing end-to-end through `apply`).
    #[test]
    fn flag_on_no_commits_drive_auto_confirms_acknowledge() {
        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_support::test_investigator(1))
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
