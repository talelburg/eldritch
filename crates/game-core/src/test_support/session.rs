//! The step-wise [`TestSession`] driver (#938): settle a built state, then
//! apply one step at a time and drain to the next rest through the one drain
//! loop in [`resolver`](super::resolver).

use super::resolver::{
    drain_with_applier, is_turn_menu, sole_option_at, turn_action_option, ChoiceResolver,
    ScriptedResolver,
};
use crate::action::{Action, InputResponse, PlayerAction};
use crate::engine::enumerate::TurnAction;
use crate::engine::{
    self, ApplyResult, Cx, EngineOutcome, InputRequest, OptionId, OptionTarget, PromptNature,
    TimingEvent,
};
use crate::event::Event;
use crate::scenario_registry;
use crate::state::{EmitEventFrame, EmitStep, GameState, GameStateBuilder, InvestigatorId};

/// Step-wise test driver: drive the real engine one step at a time, reading the
/// prompt, state and events between steps.
///
/// One model, eager throughout (#938):
///
/// - **Construction settles** the built state through the production `apply`
///   scaffolding, so the session starts where the engine would rest. An open
///   turn yields the turn menu; an open Fast window re-surfaces its prompt. A
///   phase anchor staged at its phase end
///   ([`ending_enemy_phase`](crate::state::GameStateBuilder::ending_enemy_phase),
///   [`ending_upkeep_phase`](crate::state::GameStateBuilder::ending_upkeep_phase))
///   runs that phase end, and play goes on from it. A
///   fixture whose top frame cannot re-surface its prompt is a stack the engine
///   never rests at, and construction panics.
/// - **Every step applies once, then drains to the next rest.** The steps are
///   [`take`](Self::take), [`pick`](Self::pick), [`pick_nth`](Self::pick_nth),
///   [`confirm`](Self::confirm), [`skip`](Self::skip), the framework entry
///   points [`fire_at`](Self::fire_at) and [`take_damage`](Self::take_damage),
///   and the raw [`apply`](Self::apply). The drain answers prompts through the session's
///   reply policy and stops at the turn menu, at `Done` (the game is over), at
///   `Rejected`, or at the first prompt the policy has no answer for — which is
///   where the next step picks up.
/// - **The reply policy is a script set before the step it answers**, through
///   [`resolve_choices`](Self::resolve_choices). Its answers are consumed in
///   order across steps; once it runs out, the session stops at every prompt.
/// - **Readers** — [`prompt`](Self::prompt), [`expect_rejected`](Self::expect_rejected),
///   [`state`](Self::state), [`events`](Self::events) — look at the session
///   without moving it, and [`finish`](Self::finish) hands back the last step's
///   [`ApplyResult`].
///
/// Where the session stopped is read off the engine's outcome alone, never off
/// the continuation stack: the turn menu is the prompt anchored to
/// [`TurnControl`](OptionTarget::TurnControl), any other `AwaitingInput` is a
/// prompt, and `Done` is game over. No step matches on option labels — an
/// option is chosen by the board entity it anchors to, or
/// by printed position for a [`Decision`](PromptNature::Decision) alone
/// (ADR 0011, ADR 0015).
///
/// ```
/// # use game_core::engine::{enumerate::TurnAction, OptionTarget};
/// # use game_core::state::{GameStateBuilder, InvestigatorId};
/// # use game_core::test_support;
/// # test_support::install_test_registry();
/// let me = InvestigatorId(1);
/// let session = GameStateBuilder::new()
///     .with_investigator(test_support::test_investigator(1))
///     .open_turn(me)
///     .session()
///     .take(&TurnAction::Resource { investigator: me });
/// assert_eq!(session.state().investigators[&me].resources, 6);
/// assert_eq!(session.prompt().target, Some(OptionTarget::TurnControl(me)));
/// ```
///
/// Construct via [`GameStateBuilder::session`](crate::state::GameStateBuilder::session) or
/// [`TestSession::new`].
#[derive(Debug)]
#[must_use = "a TestSession step returns the advanced session"]
pub struct TestSession {
    state: GameState,
    /// Where the session rests: the outcome of the last step (or of settling).
    /// A rejected step rests at the last prompt it reached — the one whose
    /// scripted reply the engine rejected — or, if its first apply was
    /// rejected, where it already rested.
    rest: EngineOutcome,
    /// The last step's own outcome and events, rejection included.
    last_outcome: EngineOutcome,
    last_events: Vec<Event>,
    /// Every event since construction, settling included.
    events: Vec<Event>,
    script: ScriptedResolver,
}

impl GameStateBuilder {
    /// Build and settle into a [`TestSession`]. Equivalent to
    /// `TestSession::new(self.build())`. Lives here (test-only) rather than on
    /// the builder itself so the production builder in [`crate::state`] carries
    /// no test dependency.
    pub fn session(self) -> TestSession {
        TestSession::new(self.build())
    }
}

impl TestSession {
    /// Settle `state` to where the engine would rest, through the production
    /// `apply` scaffolding.
    ///
    /// # Panics
    ///
    /// Panics if the top frame cannot re-surface its prompt — a stack the
    /// engine never rests at. Build the state before the prompt and drive into
    /// it instead.
    pub fn new(state: GameState) -> Self {
        let settled = engine::apply_via(state, scenario_registry::current(), |cx| {
            let outcome = engine::drive(cx, EngineOutcome::Done);
            if matches!(outcome, EngineOutcome::Done) {
                if let Some(top) = cx.state.continuations.top() {
                    panic!(
                        "TestSession::new: the fixture's top frame cannot re-surface its \
                         prompt ({top:?}). The engine never rests at this stack; build the \
                         state before the prompt and drive into it instead",
                    );
                }
            }
            outcome
        });
        if let EngineOutcome::Rejected { reason } = &settled.outcome {
            panic!("TestSession::new: settling the fixture was rejected: {reason}");
        }
        let ApplyResult {
            state,
            events,
            outcome,
        } = settled;
        Self {
            state,
            rest: outcome.clone(),
            last_outcome: outcome,
            last_events: events.clone(),
            events,
            script: ScriptedResolver::new(),
        }
    }

    /// Extend the reply policy: the closure appends answers to the session's
    /// script, which the drain consumes in order across this and later steps.
    /// Call it **before** the step whose prompts it answers.
    ///
    /// ```
    /// # use game_core::state::GameStateBuilder;
    /// let _session = GameStateBuilder::new()
    ///     .session()
    ///     .resolve_choices(|c| {
    ///         c.commit_cards(&[]);
    ///         c.skip();
    ///     });
    /// ```
    pub fn resolve_choices(mut self, f: impl FnOnce(&mut ScriptedResolver)) -> Self {
        f(&mut self.script);
        self
    }

    /// Step: apply a raw `action`, then drain. The escape hatch for engine
    /// records and hand-built responses; prefer the named steps.
    pub fn apply(self, action: Action) -> Self {
        self.advance(|state| engine::apply(state, action))
    }

    /// Step: fire the timing point `event` from where the session rests, the
    /// way the engine fires it in play, then drain.
    ///
    /// The event's coordinator is pushed on top of the resting stack and driven
    /// through the production `apply` scaffolding, so a rejection restores the
    /// state and an ending latched along the way is finalized. The coordinator
    /// walks the condition's whole sequence, as `glossary/Nested_Sequences.md`
    /// puts it: *"1) execute “when...” effects that interrupt that triggering
    /// condition, (2) resolve the triggering condition, and then, (3) execute
    /// “after...” effects in response to that triggering condition."* — so
    /// firing [`EnemyAttacks`](TimingEvent::EnemyAttacks) deals the attack, and
    /// a caller-owned condition with a `when` ability declared on it rejects.
    /// Each cell resolves its forced abilities before it offers reactions
    /// (`glossary/Ability.md`: *"all forced abilities initiated in reference to
    /// that timing point must resolve before any \[reaction\] abilities …
    /// referencing the same timing point in the same manner may be
    /// initiated"*), and two or more simultaneous forced abilities become the
    /// lead's ordering prompt (`glossary/Priority_of_Simultaneous_Resolution.md`:
    /// *"the lead investigator determines the order in which the abilities
    /// resolve"*).
    ///
    /// When the sequence completes, the frame it was fired above is exposed
    /// again: at the turn menu the session comes back to the menu.
    pub fn fire_at(self, event: TimingEvent) -> Self {
        self.framework_step(|cx| {
            cx.state.continuations.push(EmitEventFrame {
                event,
                step: EmitStep::When,
            });
            engine::drive(cx, EngineOutcome::Done)
        })
    }

    /// Step: deal `amount` damage to `investigator` outside any attack, the
    /// way a card effect does, then drain. Lethal damage defeats them and runs
    /// `glossary/Elimination.md` to its end — including the scenario ending
    /// once no investigator is left.
    pub fn take_damage(self, investigator: InvestigatorId, amount: u8) -> Self {
        self.framework_step(|cx| {
            engine::take_damage(cx, investigator, amount);
            engine::drive(cx, EngineOutcome::Done)
        })
    }

    /// Change the resting state in place **without** settling, so the prompt
    /// the session rests at is not re-asked: it is now stale, like a prompt a
    /// client still holds after the board changed under it. The next step
    /// answers that prompt against the edited state.
    ///
    /// For a check the engine makes when an answer resolves rather than when
    /// the option was offered — a play-ban that arrives between a reaction
    /// window's offer and the pick (#917). A change that play can make is made
    /// by playing; this is for the change no step can make while a prompt is
    /// open. To have the prompt re-asked against the edit instead, settle the
    /// edited state with [`new`](Self::new).
    pub fn edit_state(mut self, edit: impl FnOnce(&mut GameState)) -> Self {
        edit(&mut self.state);
        self
    }

    /// Apply one framework entry point `dispatch` through the production
    /// `apply` scaffolding, then drive on and drain.
    fn framework_step(self, dispatch: impl FnOnce(&mut Cx) -> EngineOutcome) -> Self {
        self.advance(|state| engine::apply_via(state, scenario_registry::current(), dispatch))
    }

    /// The one step body: apply `first` to the resting state, then drain its
    /// prompts through the script to the next rest.
    fn advance(self, first: impl FnOnce(GameState) -> ApplyResult) -> Self {
        let Self {
            state,
            rest,
            last_outcome: _,
            last_events: _,
            mut events,
            mut script,
        } = self;
        // The last prompt the step reached. A reply the engine rejects restores
        // the state to that prompt, so the session rests there rather than where
        // the step started.
        let mut reached: Option<EngineOutcome> = None;
        let mut note = |result: ApplyResult| {
            if matches!(result.outcome, EngineOutcome::AwaitingInput { .. }) {
                reached = Some(result.outcome.clone());
            }
            result
        };
        let first = note(first(state));
        let result = drain_with_applier(
            first,
            |request, state| (script.remaining() > 0).then(|| script.next(request, state)),
            |state, action| note(engine::apply(state, action)),
        );
        let ApplyResult {
            state,
            events: step_events,
            outcome,
        } = result;
        events.extend(step_events.iter().cloned());
        Self {
            state,
            rest: match outcome {
                EngineOutcome::Rejected { .. } => reached.unwrap_or(rest),
                _ => outcome.clone(),
            },
            last_outcome: outcome,
            last_events: step_events,
            events,
            script,
        }
    }

    /// Step: take the open-turn action `action` from the turn menu, then drain.
    ///
    /// # Panics
    ///
    /// Panics if the session is not at the turn menu, or `action` is not on it.
    pub fn take(self, action: &TurnAction) -> Self {
        assert!(
            self.at_turn_menu(),
            "TestSession::take: the session is not at the turn menu; it rests at {:?}",
            self.rest,
        );
        let id = turn_action_option(&self.state, action, "TestSession::take");
        self.respond(InputResponse::PickSingle(id))
    }

    /// Step: choose the option of the current prompt anchored to `target`,
    /// whatever its position, then drain.
    ///
    /// # Panics
    ///
    /// Panics at the turn menu (take a turn action with [`take`](Self::take)),
    /// or unless exactly one option is anchored to `target`.
    // By value so a call reads `pick(OptionTarget::Enemy(id))`, the anchor named
    // inline like every other step's argument.
    #[allow(clippy::needless_pass_by_value)]
    pub fn pick(self, target: OptionTarget) -> Self {
        let id = self.sole_option_at(&target, "pick");
        self.respond(InputResponse::PickSingle(id))
    }

    /// The id of the one option of the current prompt whose anchor is
    /// `target`, or a panic naming why there isn't one.
    fn sole_option_at(&self, target: &OptionTarget, step: &str) -> OptionId {
        self.assert_off_turn_menu(step);
        sole_option_at(self.prompt(), target, &format!("TestSession::{step}"))
    }

    /// Panic if the session rests at the turn menu: a prompt-answering `step`
    /// there would be read as a turn action.
    fn assert_off_turn_menu(&self, step: &str) {
        assert!(
            !self.at_turn_menu(),
            "TestSession::{step}: the session is at the turn menu; take a turn action \
             with `take(&TurnAction)`",
        );
    }

    /// Step: choose the `n`th option (0-based) of the current prompt, then
    /// drain. Only for a [`Decision`](PromptNature::Decision), whose options are
    /// alternatives printed on one card, so printed order is the meaning
    /// (ADR 0015).
    ///
    /// # Panics
    ///
    /// Panics on a [`Selection`](PromptNature::Selection) — pick by target with
    /// [`pick`](Self::pick) — or if there is no `n`th option.
    pub fn pick_nth(self, n: usize) -> Self {
        let request = self.prompt();
        assert!(
            request.nature == PromptNature::Decision,
            "TestSession::pick_nth: prompt {:?} is a {:?}, not a Decision; pick by target \
             with `pick(OptionTarget)`",
            request.prompt,
            request.nature,
        );
        let id = request
            .options
            .get(n)
            .unwrap_or_else(|| {
                panic!(
                    "TestSession::pick_nth: no option {n}; prompt {:?} offers {:?}",
                    request.prompt, request.options,
                )
            })
            .id;
        self.respond(InputResponse::PickSingle(id))
    }

    /// Step: answer the current prompt with [`InputResponse::Confirm`], then
    /// drain.
    ///
    /// # Panics
    ///
    /// Panics at the turn menu (take a turn action with [`take`](Self::take)).
    pub fn confirm(self) -> Self {
        self.assert_off_turn_menu("confirm");
        self.respond(InputResponse::Confirm)
    }

    /// Step: answer the current prompt with [`InputResponse::Skip`], then drain.
    ///
    /// # Panics
    ///
    /// Panics at the turn menu (take a turn action with [`take`](Self::take)).
    pub fn skip(self) -> Self {
        self.assert_off_turn_menu("skip");
        self.respond(InputResponse::Skip)
    }

    /// The prompt the session rests at.
    ///
    /// # Panics
    ///
    /// Panics if the session rests at `Done` — the game is over.
    pub fn prompt(&self) -> &InputRequest {
        match &self.rest {
            EngineOutcome::AwaitingInput { request, .. } => request,
            other => panic!(
                "TestSession::prompt: no prompt is outstanding; the session rests at {other:?}"
            ),
        }
    }

    /// The reason the last step was rejected. A step rejected on its first
    /// apply leaves the state and the prompt as they were; one whose scripted
    /// reply is rejected partway through its drain rests at the prompt that
    /// reply answered, with the state as of that prompt.
    ///
    /// # Panics
    ///
    /// Panics if the last step was not rejected.
    pub fn expect_rejected(&self) -> &str {
        match &self.last_outcome {
            EngineOutcome::Rejected { reason } => reason,
            other => panic!(
                "TestSession::expect_rejected: the last step was not rejected; it returned \
                 {other:?}"
            ),
        }
    }

    /// The current state.
    pub fn state(&self) -> &GameState {
        &self.state
    }

    /// Every event since construction, in order, across all steps (settling
    /// included).
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// End the session, returning the last step's [`ApplyResult`]: the current
    /// state, the events of that step and its drain, and its outcome — the shape
    /// a one-shot test asserts on.
    ///
    /// # Panics
    ///
    /// Panics if the last step stopped at a prompt other than the turn menu: the
    /// script ran short of the prompts the step opened. Script the answers with
    /// [`resolve_choices`](Self::resolve_choices) before the step, or read the
    /// session with [`prompt`](Self::prompt) and [`state`](Self::state) instead.
    pub fn finish(self) -> ApplyResult {
        if let EngineOutcome::AwaitingInput { request, .. } = &self.last_outcome {
            assert!(
                is_turn_menu(request),
                "TestSession::finish: the last step stopped at an unanswered prompt {:?} \
                 (options {:?}); script its answer with `resolve_choices` before the step",
                request.prompt,
                request.options,
            );
        }
        ApplyResult {
            state: self.state,
            events: self.last_events,
            outcome: self.last_outcome,
        }
    }

    fn at_turn_menu(&self) -> bool {
        matches!(&self.rest, EngineOutcome::AwaitingInput { request, .. } if is_turn_menu(request))
    }

    fn respond(self, response: InputResponse) -> Self {
        self.apply(Action::Player(PlayerAction::ResolveInput { response }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn test_session_fluent_round_trip() {
        let id = InvestigatorId(1);
        // Two investigators so the first EndTurn is a mid-round rotation
        // (back to the next investigator's turn menu) rather than a
        // round-ending cascade.
        let result = GameStateBuilder::new()
            .with_investigator(test_support::test_investigator(1))
            .with_investigator(test_support::test_investigator(2))
            .with_location(test_support::test_location(10, "Study"))
            .with_turn_order([id, InvestigatorId(2)])
            .open_turn(id)
            .session()
            .resolve_choices(|c| {
                // Stale script: the engine reaches the next menu without prompting.
                c.confirm();
            })
            .take(&TurnAction::EndTurn)
            .finish();
        assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    }
}
