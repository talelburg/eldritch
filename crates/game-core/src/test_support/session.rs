//! The step-wise [`TestSession`] driver: settle a built state, then
//! apply one step at a time and drain to the next rest through the one drain
//! loop in [`resolver`](super::resolver).

use super::resolver::{
    card_codes, drain_with_applier, hand_indices_of, is_turn_menu, options_at,
    resolve_commit_codes, sole_option_at, turn_action_option, ChoiceResolver, ScriptedResolver,
};
use crate::action::{Action, InputResponse, PlayerAction};
use crate::engine::enumerate::TurnAction;
use crate::engine::{
    self, ApplyResult, Cx, EngineOutcome, InputKind, InputRequest, OptionId, OptionTarget,
    PromptNature, Registries, TimingEvent,
};
use crate::event::Event;
use crate::state::{
    Continuation, DealDamageFrame, DealDamageStep, EmitEventFrame, EmitStep, GameState,
    GameStateBuilder, HandSizeDiscard, InvestigatorId, MulliganFrame, SkillKind,
};

/// Step-wise test driver: drive the real engine one step at a time, reading the
/// prompt, state and events between steps.
///
/// One model, eager throughout:
///
/// - **Construction settles** the built state through the production `apply`
///   scaffolding, so the session starts where the engine would rest. An open
///   turn yields the turn menu; an open Fast window re-surfaces its prompt. A
///   phase anchor staged at its phase end
///   ([`ending_enemy_phase`](crate::state::GameStateBuilder::ending_enemy_phase),
///   [`ending_upkeep_phase`](crate::state::GameStateBuilder::ending_upkeep_phase))
///   runs that phase end, and play goes on from it. A staged mulligan,
///   encounter draw or hand-size discard shows its prompt. A
///   fixture whose top frame cannot re-surface its prompt is a stack the engine
///   never rests at, and construction panics.
/// - **Every step applies once, then drains to the next rest.** The steps are:
///   - from the turn menu, [`take`](Self::take);
///   - at a prompt, [`pick`](Self::pick), [`pick_nth`](Self::pick_nth),
///     [`pick_nth_at`](Self::pick_nth_at), [`confirm`](Self::confirm),
///     [`skip`](Self::skip), [`commit`](Self::commit) at a skill test's
///     commit window, and [`choose_from_hand`](Self::choose_from_hand) at a
///     mulligan or hand-size discard;
///   - the framework entry points [`fire_at`](Self::fire_at),
///     [`take_damage`](Self::take_damage) and [`test_skill`](Self::test_skill);
///   - [`submit_off_menu`](Self::submit_off_menu), for a turn action the menu
///     does not offer, and the raw [`apply`](Self::apply).
///
///   The drain answers prompts through the session's
///   reply policy and stops at the turn menu, at `Done` (the game is over), at
///   `Rejected`, or at the first prompt the policy has no answer for — which is
///   where the next step picks up.
/// - **The reply policy is a script by default**, set before the step it
///   answers through [`resolve_choices`](Self::resolve_choices). Its answers
///   are consumed in order across steps; once it runs out, the session stops
///   at every prompt. [`answer_with`](Self::answer_with) installs a
///   [`ChoiceResolver`] instead, which answers every prompt until the policy
///   is changed again.
/// - **Readers** — [`prompt`](Self::prompt),
///   [`expect_rejected`](Self::expect_rejected), [`rejection`](Self::rejection),
///   [`state`](Self::state), [`events`](Self::events),
///   [`offered_at`](Self::offered_at),
///   [`assert_at_turn_menu`](Self::assert_at_turn_menu) and
///   [`assert_at_encounter_draw`](Self::assert_at_encounter_draw) — look at the
///   session without moving it. [`assert_rejects`](Self::assert_rejects) runs
///   one step and asserts the engine refused it with nothing moved, and
///   [`finish`](Self::finish) hands back the last step's [`ApplyResult`].
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
/// # let registries = test_support::TEST_REGISTRIES;
/// let me = InvestigatorId(1);
/// let session = GameStateBuilder::new()
///     .with_registries(registries)
///     .with_investigator(test_support::test_investigator(1))
///     .open_turn(me)
///     .session()
///     .take(&TurnAction::Resource { investigator: me });
/// assert_eq!(session.state().investigators[&me].resources, 6);
/// assert_eq!(session.prompt().target, Some(OptionTarget::TurnControl(me)));
/// ```
///
/// Construct via [`GameStateBuilder::session`](crate::state::GameStateBuilder::session),
/// or [`TestSession::new`].
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
    policy: Policy,
    /// The registries every step's `apply` runs against, fixed at
    /// construction.
    registries: Registries,
}

/// How the drain answers the prompts a step opens.
enum Policy {
    /// The default: answers consumed in order, stopping at the first prompt
    /// once they run out.
    Script(ScriptedResolver),
    /// A resolver installed by [`TestSession::answer_with`], which answers
    /// every prompt.
    Resolver(Box<dyn ChoiceResolver>),
}

impl Policy {
    /// The answer to `request`, or `None` to stop the drain there.
    fn answer(&mut self, request: &InputRequest, state: &GameState) -> Option<InputResponse> {
        match self {
            Self::Script(script) => (script.remaining() > 0).then(|| script.next(request, state)),
            Self::Resolver(resolver) => Some(resolver.next(request, state)),
        }
    }
}

// By hand: a boxed resolver has no `Debug`.
impl std::fmt::Debug for Policy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Script(script) => f.debug_tuple("Script").field(script).finish(),
            Self::Resolver(_) => f.write_str("Resolver(..)"),
        }
    }
}

impl GameStateBuilder {
    /// Build and settle into a [`TestSession`] driven with the registries named
    /// by [`with_registries`](GameStateBuilder::with_registries): equivalent to
    /// `TestSession::new(self.build(), regs)`. Lives here (test-only) rather
    /// than on the builder itself because [`TestSession`] is a test type: the
    /// production builder in [`crate::state`] holds only the optional
    /// registries slot this reads.
    ///
    /// # Panics
    ///
    /// Panics if the test never named its registries, and as
    /// [`TestSession::new`] does.
    pub fn session(self) -> TestSession {
        let registries = self.registries().unwrap_or_else(|| {
            panic!(
                "GameStateBuilder::session: name the registries first, with \
                 `.with_registries(..)` on the builder"
            )
        });
        TestSession::new(self.build(), registries)
    }
}

impl TestSession {
    /// Settle `state` to where the engine would rest, through the production
    /// `apply` scaffolding, driving it — and every later step — with
    /// `registries`. The way in for a hand-built state; a board names its
    /// registries on the builder instead
    /// ([`with_registries`](GameStateBuilder::with_registries)).
    ///
    /// # Panics
    ///
    /// Panics if the top frame cannot re-surface its prompt — a stack the
    /// engine never rests at. Build the state before the prompt and drive into
    /// it instead.
    pub fn new(state: GameState, registries: Registries) -> Self {
        let settled = engine::apply_via(state, registries, |cx| {
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
            policy: Policy::Script(ScriptedResolver::new()),
            registries,
        }
    }

    /// Extend the reply policy: the closure appends answers to the session's
    /// script, which the drain consumes in order across this and later steps.
    /// Call it **before** the step whose prompts it answers. After
    /// [`answer_with`](Self::answer_with), it puts a script back in the
    /// resolver's place, starting from the closure's answers.
    ///
    /// ```
    /// # use game_core::state::GameStateBuilder;
    /// # use game_core::test_support::TEST_REGISTRIES;
    /// let _session = GameStateBuilder::new()
    ///     .with_registries(TEST_REGISTRIES)
    ///     .session()
    ///     .resolve_choices(|c| {
    ///         c.commit_cards(&[]);
    ///         c.skip();
    ///     });
    /// ```
    pub fn resolve_choices(mut self, f: impl FnOnce(&mut ScriptedResolver)) -> Self {
        if let Policy::Resolver(_) = self.policy {
            self.policy = Policy::Script(ScriptedResolver::new());
        }
        let Policy::Script(script) = &mut self.policy else {
            unreachable!("a script was just installed");
        };
        f(script);
        self
    }

    /// Replace the reply policy with `resolver`, which answers every prompt
    /// the following steps open, until the policy is changed again — by
    /// another `answer_with`, or by [`resolve_choices`](Self::resolve_choices),
    /// which puts a script back. With a resolver installed the drain stops only
    /// at the turn menu, at `Done` or at `Rejected`.
    ///
    /// For a flow that needs a stateful reply rule; where the answers are
    /// known in advance, prefer the explicit steps or a script.
    pub fn answer_with(mut self, resolver: impl ChoiceResolver + 'static) -> Self {
        self.policy = Policy::Resolver(Box::new(resolver));
        self
    }

    /// Step: apply a raw `action`, then drain. The escape hatch for engine
    /// records and hand-built responses; prefer the named steps.
    pub fn apply(self, action: Action) -> Self {
        let registries = self.registries;
        self.advance(|state| engine::apply(state, action, registries))
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
    ///
    /// [`DamageDealt`](TimingEvent::DamageDealt) is only ever announced by a
    /// deal in progress, and its resolve step re-reads the assignment from that
    /// deal's frame (ADR 0009). So firing it parks the frame beneath first, at
    /// `Finish` and holding the event's own assignment, as the deal's `Announce`
    /// step leaves it. An assignment no distribution would make — to a
    /// threat-area card, say — places nothing on that card, since placement
    /// reaches only the dealt investigator's controlled assets.
    pub fn fire_at(self, event: TimingEvent) -> Self {
        self.framework_step(|cx| {
            if let TimingEvent::DamageDealt {
                source,
                investigator,
                assignment,
            } = &event
            {
                cx.state.continuations.push(DealDamageFrame {
                    investigator: *investigator,
                    source: *source,
                    direct: false,
                    assignment: assignment.clone(),
                    step: DealDamageStep::Finish,
                });
            }
            cx.state.continuations.push(EmitEventFrame {
                event,
                step: EmitStep::When,
                prevented: false,
            });
            engine::drive(cx, EngineOutcome::Done)
        })
    }

    /// Step: deal `amount` damage to `investigator` outside any attack, the
    /// way a card effect does, then drain. A soak-bearing asset is offered the
    /// soak, so the session may rest at the distribution prompt. Lethal damage
    /// defeats them and runs `glossary/Elimination.md` to its end — including
    /// the scenario ending once no investigator is left.
    pub fn take_damage(self, investigator: InvestigatorId, amount: u8) -> Self {
        self.framework_step(|cx| {
            let outcome = engine::deal_harm(cx, investigator, amount, 0);
            engine::drive(cx, outcome)
        })
    }

    /// Step: deal `amount` horror to `investigator` outside any attack — the
    /// horror twin of [`take_damage`](Self::take_damage), with the same soak
    /// offer, the same two conditions announced (so a reaction to the
    /// placement opens its window), and the same drain.
    pub fn take_horror(self, investigator: InvestigatorId, amount: u8) -> Self {
        self.framework_step(|cx| {
            let outcome = engine::deal_harm(cx, investigator, 0, amount);
            engine::drive(cx, outcome)
        })
    }

    /// Change the resting state in place **without** settling, so the prompt
    /// the session rests at is not re-asked: it is now stale, like a prompt a
    /// client still holds after the board changed under it. The next step
    /// answers that prompt against the edited state.
    ///
    /// For a check the engine makes when an answer resolves rather than when
    /// the option was offered — a play-ban that arrives between a reaction
    /// window's offer and the pick. A change that play can make is made
    /// by playing; this is for the change no step can make while a prompt is
    /// open. To have the prompt re-asked against the edit instead, settle the
    /// edited state with [`new`](Self::new).
    pub fn edit_state(mut self, edit: impl FnOnce(&mut GameState)) -> Self {
        edit(&mut self.state);
        self
    }

    /// Step: `investigator` starts a plain skill test of `skill` against
    /// `difficulty` on top of wherever the session rests, through the
    /// production `apply` scaffolding, then drain. Like every step it stops at
    /// the first prompt the reply policy has no answer for — here the test's
    /// commit window, which [`commit`](Self::commit) answers.
    ///
    /// When the test completes, the frame it was started above is exposed
    /// again: from the turn menu the session comes back to the menu.
    pub fn test_skill(
        self,
        investigator: InvestigatorId,
        skill: SkillKind,
        difficulty: i8,
    ) -> Self {
        self.framework_step(|cx| {
            let outcome = engine::start_plain_skill_test(cx, investigator, skill, difficulty);
            engine::drive(cx, outcome)
        })
    }

    /// Step: answer a skill test's commit window by committing the cards
    /// `codes` from the tested investigator's hand, then drain. Each code is
    /// matched to the first hand card with that code not already claimed, left
    /// to right — the translation
    /// [`ScriptedResolver::commit_cards`] makes. `commit(&[])` commits nothing.
    ///
    /// # Panics
    ///
    /// Panics at the turn menu, away from a skill test's commit window (a
    /// mulligan or hand-size discard is a `PickMultiple` too — answer it with
    /// [`choose_from_hand`](Self::choose_from_hand)), or if a code is not in
    /// the hand.
    pub fn commit(self, codes: &[&str]) -> Self {
        self.assert_off_turn_menu("commit");
        let request = self.prompt();
        assert!(
            request.kind == InputKind::PickMultiple && self.state.current_skill_test().is_some(),
            "TestSession::commit: the session is not at a skill test's commit window; it \
             rests at {:?}",
            request.prompt,
        );
        let selected = resolve_commit_codes(
            &card_codes(codes),
            &self.state,
            &request.prompt,
            "TestSession::commit",
        )
        .into_iter()
        .map(OptionId)
        .collect();
        self.respond(InputResponse::PickMultiple { selected })
    }

    /// Step: answer a mulligan or a hand-size discard — the hand prompts that
    /// are not a skill test's commit window — by choosing the cards `codes`
    /// from the prompted investigator's hand, then drain. A mulligan redraws
    /// the chosen cards, a hand-size discard discards them, and
    /// `choose_from_hand(&[])` chooses none (keep the hand). Codes translate to
    /// hand cards as [`commit`](Self::commit)'s do: each matches the first hand
    /// card with that code not already claimed, left to right, so two copies
    /// are named by repeating the code.
    ///
    /// # Panics
    ///
    /// Panics at the turn menu, at a skill test's commit window (commit cards
    /// with [`commit`](Self::commit)), at any other prompt, or if a code is not
    /// in the hand.
    pub fn choose_from_hand(self, codes: &[&str]) -> Self {
        self.assert_off_turn_menu("choose_from_hand");
        assert!(
            self.state.current_skill_test().is_none(),
            "TestSession::choose_from_hand: the session is at a skill test's commit window; \
             commit cards with `commit(&[code])`",
        );
        let request = self.prompt();
        // The prompt names no investigator; the frame that asks it does. This
        // reads the stack to translate codes, never to decide where the
        // session rests.
        let prompted = match self.state.continuations.top() {
            Some(
                Continuation::Mulligan(MulliganFrame { remaining })
                | Continuation::HandSizeDiscard(HandSizeDiscard { remaining }),
            ) => remaining.first().copied(),
            _ => None,
        }
        .unwrap_or_else(|| {
            panic!(
                "TestSession::choose_from_hand: the session is not at a mulligan or a \
                 hand-size discard; it rests at {:?}",
                request.prompt,
            )
        });
        let selected = hand_indices_of(
            &card_codes(codes),
            &self.state,
            prompted,
            &request.prompt,
            "TestSession::choose_from_hand",
        )
        .into_iter()
        .map(OptionId)
        .collect();
        self.respond(InputResponse::PickMultiple { selected })
    }

    /// Apply one framework entry point `dispatch` through the production
    /// `apply` scaffolding, then drive on and drain.
    fn framework_step(self, dispatch: impl FnOnce(&mut Cx) -> EngineOutcome) -> Self {
        let registries = self.registries;
        self.advance(|state| engine::apply_via(state, registries, dispatch))
    }

    /// The one step body: apply `first` to the resting state, then drain its
    /// prompts through the reply policy to the next rest.
    fn advance(self, first: impl FnOnce(GameState) -> ApplyResult) -> Self {
        let Self {
            state,
            rest,
            last_outcome: _,
            last_events: _,
            mut events,
            mut policy,
            registries,
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
            |request, state| policy.answer(request, state),
            |state, action| note(engine::apply(state, action, registries)),
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
            policy,
            registries,
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
        let id = turn_action_option(
            &self.state,
            self.registries.cards,
            action,
            "TestSession::take",
        );
        self.respond(InputResponse::PickSingle(id))
    }

    /// Step: dispatch `action` straight to its handler, **bypassing the turn
    /// menu**, through the production `apply` scaffolding, then drain.
    ///
    /// [`take`](Self::take) picks the action off the menu and panics if the
    /// menu does not offer it, so it cannot reach a handler with an action the
    /// enumerator filters out. This step can, for two uses only:
    ///
    /// 1. A `#[should_panic(expected = "state-corruption invariant
    ///    violation")]` handler test that injects corrupt state (a dangling
    ///    `current_location`, an investigator missing from the map) and expects
    ///    the handler — not the enumerator — to panic.
    /// 2. Proving a handler *itself* rejects an action the enumerator already
    ///    filters out, which a client can send over the wire without consulting
    ///    the menu (an activation's initiation gate, say).
    ///
    /// Everything else takes the action from the menu with
    /// [`take`](Self::take).
    pub fn submit_off_menu(self, action: &TurnAction) -> Self {
        self.framework_step(|cx| {
            let outcome = engine::dispatch_turn_action(cx, action);
            engine::drive(cx, outcome)
        })
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

    /// Step: choose the `n`th (0-based) of the current prompt's options
    /// anchored to `target`, in the order the prompt offers them, then drain.
    ///
    /// For several options sharing one anchor, which [`pick`](Self::pick)
    /// cannot tell apart:
    ///
    /// - several abilities printed on one card (two reactions, or two forced
    ///   abilities), where `n` names the ability in the order the card prints
    ///   them;
    /// - one ability offered to several investigators, where `n` follows the
    ///   prompt's order (assert first that each option's label names the
    ///   investigator it is offered to);
    /// - identical copies of one card in hand, which anchor to the same
    ///   `HandCardByCode`.
    ///
    /// # Panics
    ///
    /// Panics at the turn menu, unless at least two options are anchored to
    /// `target` (pick a lone one with [`pick`](Self::pick)), or if there is no
    /// `n`th.
    // By value, like `pick`.
    #[allow(clippy::needless_pass_by_value)]
    pub fn pick_nth_at(self, target: OptionTarget, n: usize) -> Self {
        self.assert_off_turn_menu("pick_nth_at");
        let request = self.prompt();
        let anchored: Vec<OptionId> = options_at(request, &target)
            .map(|option| option.id)
            .collect();
        assert!(
            anchored.len() >= 2,
            "TestSession::pick_nth_at: {} option(s) anchored to {target:?}, not several; pick \
             a lone one with `pick(OptionTarget)`. Prompt {:?} offers {:?}",
            anchored.len(),
            request.prompt,
            request.options,
        );
        let id = *anchored.get(n).unwrap_or_else(|| {
            panic!(
                "TestSession::pick_nth_at: no option {n} among the {} anchored to {target:?}",
                anchored.len(),
            )
        });
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

    /// The reason the last step was rejected, or `None` if it was accepted —
    /// the non-panicking [`expect_rejected`](Self::expect_rejected), for a
    /// sweep that submits many actions and sorts them by whether the engine
    /// accepts each.
    pub fn rejection(&self) -> Option<&str> {
        match &self.last_outcome {
            EngineOutcome::Rejected { reason } => Some(reason),
            _ => None,
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

    /// How many of the current prompt's options are anchored to `target`:
    /// `0` when it is not offered, `1` for a lone option [`pick`](Self::pick)
    /// can name, and more for several options sharing the anchor — several
    /// abilities printed on one card, one ability offered to several
    /// investigators, or identical copies in hand — which
    /// [`pick_nth_at`](Self::pick_nth_at) tells apart.
    ///
    /// # Panics
    ///
    /// Panics if no prompt is outstanding, as [`prompt`](Self::prompt) does.
    pub fn offered_at(&self, target: &OptionTarget) -> usize {
        options_at(self.prompt(), target).count()
    }

    /// Assert the session rests at `investigator`'s turn menu: the prompt
    /// anchored to their [`TurnControl`](OptionTarget::TurnControl). The state
    /// assertion for a step that returns to an open turn.
    ///
    /// # Panics
    ///
    /// Panics if the session rests anywhere else — another prompt, another
    /// investigator's menu, or `Done`.
    #[track_caller]
    pub fn assert_at_turn_menu(&self, investigator: InvestigatorId) {
        match &self.rest {
            EngineOutcome::AwaitingInput { request, .. }
                if request.target == Some(OptionTarget::TurnControl(investigator)) => {}
            other => panic!(
                "TestSession::assert_at_turn_menu: expected {investigator:?}'s turn menu; the \
                 session rests at {other:?}"
            ),
        }
    }

    /// Assert the session rests at a Mythos step-1.4 encounter-draw prompt:
    /// the prompt anchored to the [`EncounterDeck`](OptionTarget::EncounterDeck).
    /// Where a step that runs through a round end usually stops.
    ///
    /// # Panics
    ///
    /// Panics if the session rests anywhere else, or at `Done`.
    #[track_caller]
    pub fn assert_at_encounter_draw(&self) {
        match &self.rest {
            EngineOutcome::AwaitingInput { request, .. }
                if request.target == Some(OptionTarget::EncounterDeck) => {}
            other => panic!(
                "TestSession::assert_at_encounter_draw: expected the encounter-draw prompt; the \
                 session rests at {other:?}"
            ),
        }
    }

    /// Run `step` and assert the engine rejects it on its first apply with
    /// nothing moved: the state is byte-identical to before and the session
    /// still rests at the prompt it rested at. Returns the reason.
    ///
    /// The reader for a handler's own rejection — usually
    /// `assert_rejects(|s| s.submit_off_menu(&action))`, for an action the
    /// menu does not offer, or a `take` the handler refuses after the menu
    /// offered it.
    ///
    /// # Panics
    ///
    /// Panics if `step` is not rejected, or if the rejection left the state or
    /// the resting prompt changed.
    #[track_caller]
    pub fn assert_rejects(self, step: impl FnOnce(Self) -> Self) -> String {
        let state_before = self.state.clone();
        let rest_before = self.rest.clone();
        let session = step(self);
        let reason = session.expect_rejected().to_owned();
        assert_eq!(
            session.state, state_before,
            "TestSession::assert_rejects: a rejected step must leave state byte-identical",
        );
        assert_eq!(
            session.rest, rest_before,
            "TestSession::assert_rejects: a rejected step must rest where it started",
        );
        reason
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
    use crate::test_support::{self, TEST_REGISTRIES};

    #[test]
    fn test_session_fluent_round_trip() {
        let id = InvestigatorId(1);
        // Two investigators so the first EndTurn is a mid-round rotation
        // (back to the next investigator's turn menu) rather than a
        // round-ending cascade.
        let result = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
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
