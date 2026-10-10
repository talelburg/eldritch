//! Interactive-choice resolution (#422): effect nodes that need a controller
//! pick **suspend in place** — the evaluator leaves the node's
//! [`EffectFrame::Leaf`](crate::state::EffectFrame::Leaf) on top of the
//! continuation stack as the prompt. [`resume_effect_choice`] sets the pick on
//! that frame and re-steps it. A choice among board entities goes through
//! [`resolve_grounded_choice`] (`0 ⇒ empty · 1 ⇒ auto · 2+ ⇒ suspend`, plus the
//! resume re-indexing), which the evaluator's `Chosen` arms and card-local
//! natives share (#990). No replay, no separate choice frame (umbrella §3.4).

use crate::action::InputResponse;
use crate::card_registry;
use crate::engine::outcome::{
    ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget, ResumeToken,
};
use crate::engine::{board, Cx, EvalContext};
use crate::state::{CardCode, Continuation, EffectFrame, GameState, InvestigatorId};

/// Outcome of applying the uniform resolve convention to a count of legal
/// options (umbrella §3.4 / spec §5). Crate-private: an entity selection goes
/// through [`resolve_grounded_choice`], which owns the resume protocol on top of
/// this count; only `ChooseOne` and the search-deck pick count directly.
pub(crate) enum ChoiceResolution {
    /// Zero legal options — caller applies its printed fallback or rejects.
    Empty,
    /// Exactly one — auto-bind this index, no input.
    Auto(usize),
    /// Two or more — suspend for a controller pick.
    Suspend,
}

/// Map a legal-option count to the resolve convention. When `interactive` is set
/// (human play, `interactive_acknowledge`), a single option surfaces as a
/// one-option pick (`Suspend`) instead of auto-binding silently (#466).
pub(crate) fn resolve_choice_count(n: usize, interactive: bool) -> ChoiceResolution {
    match n {
        0 => ChoiceResolution::Empty,
        1 if interactive => ChoiceResolution::Suspend,
        1 => ChoiceResolution::Auto(0),
        _ => ChoiceResolution::Suspend,
    }
}

/// What [`resolve_grounded_choice`] hands back to its caller.
#[derive(Debug)]
pub enum Grounded<T> {
    /// The candidate to act on: the lone candidate auto-bound, or the
    /// controller's pick threaded in on resume.
    Picked(T),
    /// There were no candidates. The caller decides what that means: a printed
    /// fallback (Crypt Chill 01167 deals 2 damage), a skip, or a rejection.
    Empty,
    /// Return this outcome unchanged. It is the `AwaitingInput` prompt when the
    /// controller must pick, or `Rejected` when a threaded pick indexes past the
    /// candidate list.
    Suspend(EngineOutcome),
}

/// Offer the controller a choice among board entities (#422, #990): the one way
/// an evaluator arm or a card-local native grounds a pick.
///
/// `candidates` must be enumerated from `state` in a deterministic order,
/// because a resumed node re-enumerates them and indexes the same list by the
/// pick. `anchor` names the board entity each candidate renders on, and each
/// option's label is derived from it (ADR 0011, #989).
///
/// - A pick threaded in through
///   [`EvalContext::chosen_option`] is checked first. In range, it gives
///   [`Grounded::Picked`]. Out of range, it gives `Suspend(Rejected)`,
///   whatever the candidate count.
/// - Otherwise zero candidates give [`Grounded::Empty`], one gives `Picked`
///   with no prompt, and two or more give `Suspend` with the prompt. Under
///   `state.interactive_acknowledge` a lone candidate is prompted too (#466).
///
/// It pushes nothing. The caller's suspending frame (the effect `Leaf`, or the
/// designated-action frame) stays on the stack as the prompt and is re-stepped
/// on resume. On `Picked`, the caller binds the candidate and clears the
/// transient pick (`set_chosen_option(None)`) if it threads the context onward.
pub fn resolve_grounded_choice<T: Clone>(
    state: &GameState,
    ctx: &EvalContext,
    candidates: &[T],
    prompt: &str,
    anchor: impl Fn(&T) -> OptionTarget,
) -> Grounded<T> {
    if let Some(OptionId(i)) = ctx.chosen_option() {
        return match candidates.get(i as usize) {
            Some(c) => Grounded::Picked(c.clone()),
            None => Grounded::Suspend(EngineOutcome::Rejected {
                reason: format!("{prompt}: pick {i} out of range (0..{})", candidates.len()).into(),
            }),
        };
    }
    match resolve_choice_count(candidates.len(), state.interactive_acknowledge) {
        ChoiceResolution::Empty => Grounded::Empty,
        ChoiceResolution::Auto(i) => Grounded::Picked(candidates[i].clone()),
        ChoiceResolution::Suspend => {
            let anchors: Vec<_> = candidates.iter().map(anchor).collect();
            Grounded::Suspend(awaiting_selection(state, prompt, &anchors))
        }
    }
}

/// Build the `AwaitingInput` for a controller pick among board entities, one
/// anchor per offered option in offered order (`OptionId(i)` is the index).
/// Each option renders on the entity its anchor names (S5, #540), and its label
/// is derived from that anchor by [`anchor_label`], so the two cannot disagree
/// and no caller can leave an option unlabelled (#989). Pushes **nothing**: the
/// suspending effect node's own `Leaf` frame stays on the stack as the prompt
/// (#422), and resume re-derives the option set and validates the pick by
/// checked indexing.
pub(crate) fn awaiting_selection(
    state: &GameState,
    prompt: impl Into<String>,
    anchors: &[OptionTarget],
) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(
            prompt,
            candidate_options(state, anchors, OptionTarget::clone),
        ),
        resume_token: ResumeToken(0),
    }
}

/// One `(label, anchor)` per offered option to the `ChoiceOption` list a request
/// carries, `OptionId(i)` being the offered index. Shared by the un-anchored and
/// decision builders below.
fn choice_options(options: Vec<(String, Option<OptionTarget>)>) -> Vec<ChoiceOption> {
    options
        .into_iter()
        .enumerate()
        .map(|(i, (label, target))| ChoiceOption::new(option_id(i), label).maybe_at(target))
        .collect()
}

/// The [`OptionId`] of the option offered at index `i`: the index convention's
/// one home.
fn option_id(i: usize) -> OptionId {
    OptionId(u32::try_from(i).expect("offered option count fits in u32"))
}

/// Build the offered options for a candidate list: option `i` is
/// `candidates[i]`, anchored by `anchor` and labelled from that anchor by
/// [`anchor_label`] (#989). Every candidate here is a board entity, so `anchor`
/// returns a bare [`OptionTarget`] rather than an `Option`: a caller cannot leave
/// one un-anchored and land it in the prompt banner (ADR 0011, #950). It supplies
/// no label, so it cannot forget one or name a different entity than it anchors.
pub(crate) fn candidate_options<T>(
    state: &GameState,
    candidates: &[T],
    anchor: impl Fn(&T) -> OptionTarget,
) -> Vec<ChoiceOption> {
    candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let target = anchor(c);
            ChoiceOption::new(option_id(i), anchor_label(state, &target)).at(target)
        })
        .collect()
}

/// The anchor for one tied investigator: their investigator card (#950).
pub(super) fn investigator_anchor(state: &GameState, id: InvestigatorId) -> OptionTarget {
    state.investigators[&id].card_anchor()
}

/// The player-facing name of the board surface `anchor` names, for an option's
/// label (#989). A location or enemy reads as its name. A card (in play, in a
/// threat area, an investigator card, in hand, the current act or agenda) reads
/// as its card's name via [`card_name`]. An investigator's panel affordances
/// read as the investigator's name.
///
/// Total and panic-free. An anchor naming an entity that isn't in `state` is an
/// engine bug, because every candidate is enumerated from `state`. It trips a
/// `debug_assert!` in tests and degrades to a neutral placeholder in a live
/// game, never to an id's `Debug` form.
pub(crate) fn anchor_label(state: &GameState, anchor: &OptionTarget) -> String {
    let found = match anchor {
        OptionTarget::Location(id) => state.locations.get(id).map(|l| l.name.clone()),
        OptionTarget::Enemy(id) => state.enemies.get(id).map(|e| e.name.clone()),
        OptionTarget::CardInstance(id) => {
            board::find_instance(state, *id).map(|(card, _)| card_name(&card.code))
        }
        OptionTarget::HandCard {
            investigator,
            hand_index,
        } => state
            .investigators
            .get(investigator)
            .and_then(|inv| inv.hand.get(usize::from(*hand_index)))
            .map(card_name),
        OptionTarget::HandCardByCode { code, .. } => Some(card_name(code)),
        OptionTarget::Act => state
            .act_deck
            .get(state.act_index)
            .map(|act| card_name(&act.code)),
        OptionTarget::Agenda => state
            .agenda_deck
            .get(state.agenda_index)
            .map(|agenda| card_name(&agenda.code)),
        OptionTarget::TurnControl(id)
        | OptionTarget::ResourcePool(id)
        | OptionTarget::PlayerDeck(id) => state.investigators.get(id).map(|inv| inv.name.clone()),
        OptionTarget::EncounterDeck => Some("Encounter deck".to_owned()),
    };
    found.unwrap_or_else(|| {
        debug_assert!(
            false,
            "option anchor {anchor:?} names nothing in the game state"
        );
        "unknown card".to_owned()
    })
}

/// A card's printed name, looked up through the installed card registry's
/// metadata. Falls back to the bare code when no registry is installed or the
/// corpus lacks the code, so a gap degrades readably rather than crashing a live
/// game (#989).
pub(crate) fn card_name(code: &CardCode) -> String {
    card_registry::current()
        .and_then(|r| (r.metadata_for)(code))
        .map_or_else(|| code.0.clone(), |m| m.name.clone())
}

/// Build the `AwaitingInput` for a **decision** — a choice among alternatives
/// printed on one card, rather than among board entities (ADR 0015). Every
/// branch shares `anchor`, so it rides on the request as well as on each option:
/// the options keep it because that is what makes the source card glow, and the
/// request carries it because the surface presenting the choice reads one anchor
/// to name where the text is printed.
///
/// `anchor` is already **liveness-checked** by the caller (#845) — `None` means
/// the source left play during cost payment, and the prompt falls back to the
/// banner like any un-anchored one.
///
/// Two callers: the evaluator's `Effect::ChooseOne` step, and the skill-test
/// substitution offer (Mind over Matter 01036), whose source has left play and
/// so passes no anchor. Every other suspend keeps the
/// [`Selection`](crate::engine::PromptNature::Selection) default, whether it is
/// built by [`awaiting_selection`], [`awaiting_choice`], or a builder of its own.
pub(crate) fn awaiting_decision(
    prompt: impl Into<String>,
    labels: Vec<String>,
    anchor: Option<OptionTarget>,
) -> EngineOutcome {
    let options = labels
        .into_iter()
        .map(|label| (label, anchor.clone()))
        .collect();
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(prompt, choice_options(options))
            .maybe_at(anchor)
            .deciding(),
        resume_token: ResumeToken(0),
    }
}

/// Build the `AwaitingInput` for a controller choice from one render label per
/// offered option, each **un-anchored** (no board home, so the host renders it in
/// the prompt banner). Used by the search-deck pick, whose options are cards in a
/// deck with no board surface (ADR 0015 excludes it), labelled by [`card_name`].
/// Card-local native choices went the other way in #950: they are entity
/// selections, offered through [`resolve_grounded_choice`] with an anchor per
/// option.
pub(crate) fn awaiting_choice(prompt: impl Into<String>, labels: Vec<String>) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(
            prompt,
            choice_options(labels.into_iter().map(|l| (l, None)).collect()),
        ),
        resume_token: ResumeToken(0),
    }
}

/// Resume an effect node suspended in place for a controller pick (#422): the
/// top frame is the suspended [`EffectFrame::Leaf`] — or the
/// [`EffectFrame::Designated`] a designated **Fight** suspends on while the
/// controller picks which co-located enemy to attack (#805). Set its
/// `chosen_option` and re-step it via the effect drive — the node grounds/picks
/// (checked indexing, validate-first) instead of suspending. On completion,
/// re-enter the enclosing driver (skill test / reaction window), mirroring the
/// former replay resume.
pub(crate) fn resume_effect_choice(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let InputResponse::PickSingle(picked) = response else {
        return EngineOutcome::Rejected {
            reason: "ResolveInput: a choice is open; expected InputResponse::PickSingle".into(),
        };
    };
    match cx.state.continuations.top_frame_mut() {
        Some(Continuation::Effect(
            EffectFrame::Leaf { ctx, .. } | EffectFrame::Designated { ctx, .. },
        )) => {
            ctx.set_chosen_option(Some(*picked));
        }
        _ => {
            return EngineOutcome::Rejected {
                reason: "resume_effect_choice: top frame is not a suspended effect node".into(),
            }
        }
    }
    resume_effect_walk(cx)
}

/// Resume a parked effect walk after a player input by ceding to the global
/// `drive` loop (Slice D #423). The caller ([`resume_effect_choice`] / the
/// effect-path arm of the `DealDamage` frame's `Finish` step, K5b-2) has already recorded
/// the input on the suspended top `Effect` leaf; returning `Done` hands the
/// parked frames to `apply_player_action`'s `drive(cx, outcome)`, whose
/// `Continuation::Effect` arm steps them via the same `step_effect_frame` the
/// old bounded `drive_effect_to_base` used, then dispatches whatever frame the
/// walk was nested within (a `SkillTest` mid-resolution, a window with remaining
/// candidates). No bounded re-entry, no reach-down.
pub(crate) fn resume_effect_walk(_cx: &mut Cx) -> EngineOutcome {
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{self, Effect, InvestigatorTarget};

    use super::*;
    use crate::engine::dispatch;
    use crate::engine::evaluator::{self, EvalContext};
    use crate::engine::outcome::PromptNature;
    use crate::state::{
        CardInPlay, CardInstanceId, EnemyId, GameState, GameStateBuilder, InvestigatorId,
        LocationId, Owner,
    };
    use crate::test_support;

    /// A `ChooseOne` branch that is **live** — one `effect_can_change_state`
    /// cannot prove inert, so #664's mode filter keeps it in the offer. (An
    /// empty `Seq`, the old placeholder here, is provably inert and would be
    /// filtered out.)
    fn live_branch() -> Effect {
        dsl::gain_resources(InvestigatorTarget::You, 1)
    }

    /// A state holding the investigator [`live_branch`] pays out to.
    fn state_with_investigator() -> GameState {
        GameStateBuilder::default()
            .with_investigator(test_support::test_investigator(1))
            .build()
    }

    #[test]
    fn resolve_zero_options_is_reject() {
        assert!(matches!(
            resolve_choice_count(0, false),
            ChoiceResolution::Empty
        ));
        assert!(matches!(
            resolve_choice_count(0, true),
            ChoiceResolution::Empty
        ));
    }

    #[test]
    fn resolve_one_option_auto_binds_when_not_interactive() {
        assert!(matches!(
            resolve_choice_count(1, false),
            ChoiceResolution::Auto(0)
        ));
    }

    #[test]
    fn resolve_one_option_suspends_when_interactive() {
        // #466: a lone option surfaces as a one-option pick in human play.
        assert!(matches!(
            resolve_choice_count(1, true),
            ChoiceResolution::Suspend
        ));
    }

    #[test]
    fn resolve_two_options_suspends_regardless_of_flag() {
        assert!(matches!(
            resolve_choice_count(2, false),
            ChoiceResolution::Suspend
        ));
        assert!(matches!(
            resolve_choice_count(2, true),
            ChoiceResolution::Suspend
        ));
    }

    #[test]
    fn suspended_leaf_snapshots_active_skill_test_binding() {
        // A context carrying an active on_fail margin when a ChooseOne suspends.
        let mut ctx = EvalContext::for_controller(InvestigatorId(1));
        ctx.set_failed_by(2);

        // Two live branches — a filtered-empty ChooseOne skips instead of
        // suspending (#664), which is not the behaviour under test here.
        let effect = dsl::choose_one([("A", live_branch()), ("B", live_branch())]);
        let mut state = state_with_investigator();
        let mut events = Vec::new();
        // Push the effect root + drive it through the real global loop (the
        // deleted `apply_effect` bounded entry's test-only successor); a
        // 2-branch ChooseOne suspends in place for a pick.
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            evaluator::push_effect(&mut cx, &effect, ctx);
            dispatch::drive(&mut cx, EngineOutcome::Done)
        };
        assert!(
            matches!(out, EngineOutcome::AwaitingInput { .. }),
            "a 2-branch ChooseOne suspends for a pick",
        );

        let Some(Continuation::Effect(EffectFrame::Leaf { ctx, .. })) = state.continuations.top()
        else {
            panic!("expected a suspended effect Leaf frame on the stack");
        };
        assert_eq!(
            ctx.failed_by(),
            Some(2),
            "the active skill-test margin must ride the suspended Leaf frame's context, \
             not be dropped at suspend",
        );
    }

    #[test]
    fn single_branch_choose_one_surfaces_under_interactive_flag() {
        // One ChooseOne branch: today it auto-binds. With interactive_acknowledge
        // on it must surface as a one-option pick (#466).
        let effect = dsl::choose_one([("Only", live_branch())]);
        let ctx = EvalContext::for_controller(InvestigatorId(1));

        let mut state = state_with_investigator();
        state.interactive_acknowledge = true;
        let mut events = Vec::new();
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            evaluator::push_effect(&mut cx, &effect, ctx);
            dispatch::drive(&mut cx, EngineOutcome::Done)
        };
        match out {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(
                    request.options.len(),
                    1,
                    "lone branch surfaces as one option"
                );
            }
            other => panic!("expected a one-option suspend, got {other:?}"),
        }
    }

    #[test]
    fn single_branch_choose_one_auto_binds_when_flag_off() {
        let effect = dsl::choose_one([("Only", live_branch())]);
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let mut state = state_with_investigator(); // flag defaults false
        let mut events = Vec::new();
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            evaluator::push_effect(&mut cx, &effect, ctx);
            dispatch::drive(&mut cx, EngineOutcome::Done)
        };
        assert!(
            matches!(out, EngineOutcome::Done),
            "flag off: auto-binds, no suspend"
        );
    }

    #[test]
    fn awaiting_selection_anchors_and_names_each_option() {
        let state = labelled_board();
        let out = awaiting_selection(
            &state,
            "Choose a target",
            &[
                OptionTarget::Enemy(EnemyId(1)),
                OptionTarget::Location(LocationId(10)),
            ],
        );
        let EngineOutcome::AwaitingInput { request, .. } = out else {
            panic!("expected AwaitingInput");
        };
        assert_eq!(request.options[0].id, OptionId(0));
        assert_eq!(
            request.options[0].target,
            Some(OptionTarget::Enemy(EnemyId(1)))
        );
        assert_eq!(request.options[0].label, "Ghoul Minion");
        assert_eq!(request.options[1].id, OptionId(1));
        assert_eq!(request.options[1].label, "Study");
    }

    #[test]
    fn awaiting_decision_marks_the_request_and_anchors_both_levels() {
        let out = awaiting_decision(
            "Choose one",
            vec!["Burn it down".into(), "Do not".into()],
            Some(OptionTarget::Act),
        );
        let EngineOutcome::AwaitingInput { request, .. } = out else {
            panic!("expected AwaitingInput");
        };
        assert_eq!(request.nature, PromptNature::Decision);
        assert_eq!(
            request.target,
            Some(OptionTarget::Act),
            "the request carries the anchor so one surface can name the source card",
        );
        assert!(
            request
                .options
                .iter()
                .all(|o| o.target == Some(OptionTarget::Act)),
            "and each option keeps it, which is what makes the card glow",
        );
    }

    /// #845's degradation reaching the request: a source discarded during cost
    /// payment leaves the decision un-anchored at both levels rather than
    /// pointing at a card no board surface renders.
    #[test]
    fn awaiting_decision_with_no_live_source_is_unanchored_but_still_a_decision() {
        let out = awaiting_decision("Choose one", vec!["A".into(), "B".into()], None);
        let EngineOutcome::AwaitingInput { request, .. } = out else {
            panic!("expected AwaitingInput");
        };
        assert_eq!(request.nature, PromptNature::Decision);
        assert_eq!(request.target, None);
        assert!(request.options.iter().all(|o| o.target.is_none()));
    }

    /// A board carrying one of every entity an entity selection anchors to:
    /// investigator 1 (investigator card instance 100) in the Study (10), a
    /// Ghoul Minion (enemy 1) there, and a [`test_support::TEST_ASSET`] in play
    /// as instance 7.
    fn labelled_board() -> GameState {
        let mut me = test_support::test_investigator(1);
        me.investigator_card.instance_id = CardInstanceId(100);
        me.cards_in_play.push(CardInPlay::enter_play(
            CardCode::new(test_support::TEST_ASSET),
            CardInstanceId(7),
            Owner::Investigator(InvestigatorId(1)),
        ));
        GameStateBuilder::default()
            .with_location(test_support::test_location(10, "Study"))
            .with_investigator_at(me, LocationId(10))
            .with_enemy(test_support::test_enemy(1, "Ghoul Minion"))
            .build()
    }

    #[test]
    fn a_location_option_is_labelled_with_the_location_name() {
        let state = labelled_board();
        assert_eq!(
            anchor_label(&state, &OptionTarget::Location(LocationId(10))),
            "Study"
        );
    }

    #[test]
    fn an_enemy_option_is_labelled_with_the_enemy_name() {
        let state = labelled_board();
        assert_eq!(
            anchor_label(&state, &OptionTarget::Enemy(EnemyId(1))),
            "Ghoul Minion"
        );
    }

    #[test]
    fn a_card_instance_option_is_labelled_with_the_card_name() {
        let state = labelled_board();
        assert_eq!(
            anchor_label(&state, &OptionTarget::CardInstance(CardInstanceId(7))),
            "Test Asset"
        );
    }

    #[test]
    fn an_investigator_card_option_is_labelled_with_the_investigator_card_name() {
        let state = labelled_board();
        let anchor = state.investigators[&InvestigatorId(1)].card_anchor();
        assert_eq!(anchor, OptionTarget::CardInstance(CardInstanceId(100)));
        // The registry's name for the investigator card, not the fixture's
        // `Investigator::name`.
        assert_eq!(anchor_label(&state, &anchor), "Test Investigator");
    }

    #[test]
    fn a_card_the_registry_does_not_know_falls_back_to_its_code() {
        let mut state = labelled_board();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .cards_in_play[0]
            .code = CardCode::new("NOT_A_CARD");
        assert_eq!(
            anchor_label(&state, &OptionTarget::CardInstance(CardInstanceId(7))),
            "NOT_A_CARD"
        );
        assert_eq!(card_name(&CardCode::new("NOT_A_CARD")), "NOT_A_CARD");
    }

    /// `ctx` with `pick` threaded in, as resume re-steps a suspended node.
    fn picking(pick: u32) -> EvalContext {
        let mut ctx = EvalContext::for_controller(InvestigatorId(1));
        ctx.set_chosen_option(Some(OptionId(pick)));
        ctx
    }

    fn location_anchor(id: &LocationId) -> OptionTarget {
        OptionTarget::Location(*id)
    }

    #[test]
    fn grounded_choice_auto_picks_a_lone_candidate_without_a_prompt() {
        let state = labelled_board();
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let out = resolve_grounded_choice(
            &state,
            &ctx,
            &[LocationId(10)],
            "Choose a location",
            location_anchor,
        );
        assert!(matches!(out, Grounded::Picked(LocationId(10))));
    }

    #[test]
    fn grounded_choice_surfaces_a_lone_candidate_under_the_interactive_flag() {
        // #466: in human play a lone candidate is a one-option pick.
        let mut state = labelled_board();
        state.interactive_acknowledge = true;
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let out = resolve_grounded_choice(
            &state,
            &ctx,
            &[LocationId(10)],
            "Choose a location",
            location_anchor,
        );
        let Grounded::Suspend(EngineOutcome::AwaitingInput { request, .. }) = out else {
            panic!("expected a one-option suspend");
        };
        assert_eq!(request.options.len(), 1);
    }

    #[test]
    fn grounded_choice_hands_an_empty_candidate_list_back_to_the_caller() {
        let state = labelled_board();
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let out = resolve_grounded_choice(
            &state,
            &ctx,
            &[] as &[LocationId],
            "Choose a location",
            location_anchor,
        );
        assert!(matches!(out, Grounded::Empty));
    }

    #[test]
    fn grounded_choice_anchors_enemy_options() {
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let state = GameStateBuilder::new()
            .with_enemy(test_support::test_enemy(4, "Ghoul Minion"))
            .with_enemy(test_support::test_enemy(9, "Ravenous Ghoul"))
            .build();
        let out = resolve_grounded_choice(
            &state,
            &ctx,
            &[EnemyId(4), EnemyId(9)],
            "Choose an enemy",
            |id| OptionTarget::Enemy(*id),
        );
        let Grounded::Suspend(EngineOutcome::AwaitingInput { request, .. }) = out else {
            panic!("2 candidates suspend for a pick");
        };
        assert_eq!(request.prompt, "Choose an enemy");
        assert_eq!(request.options.len(), 2);
        assert_eq!(request.options[0].id, OptionId(0));
        assert_eq!(
            request.options[0].target,
            Some(OptionTarget::Enemy(EnemyId(4)))
        );
        assert_eq!(request.options[0].label, "Ghoul Minion");
        assert_eq!(request.options[1].id, OptionId(1));
        assert_eq!(
            request.options[1].target,
            Some(OptionTarget::Enemy(EnemyId(9)))
        );
        assert_eq!(request.options[1].label, "Ravenous Ghoul");
    }

    /// Investigator candidates anchor to their investigator card (#950) and are
    /// labelled with that card's name (#989).
    #[test]
    fn grounded_choice_anchors_investigators_to_their_card() {
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let state = GameStateBuilder::new()
            .with_investigator(test_support::test_investigator(1))
            .with_investigator(test_support::test_investigator(2))
            .build();
        let cands = [InvestigatorId(1), InvestigatorId(2)];
        let out = resolve_grounded_choice(&state, &ctx, &cands, "Choose an investigator", |id| {
            state.investigators[id].card_anchor()
        });
        let Grounded::Suspend(EngineOutcome::AwaitingInput { request, .. }) = out else {
            panic!("2 candidates suspend for a pick");
        };
        assert_eq!(request.options.len(), 2);
        for (option, id) in request.options.iter().zip(cands) {
            assert_eq!(option.target, Some(state.investigators[&id].card_anchor()));
            assert_eq!(option.label, "Test Investigator");
        }
    }

    #[test]
    fn grounded_choice_picks_the_threaded_candidate_on_resume() {
        let state = labelled_board();
        let out = resolve_grounded_choice(
            &state,
            &picking(1),
            &[EnemyId(4), EnemyId(9)],
            "Choose an enemy",
            |id| OptionTarget::Enemy(*id),
        );
        assert!(matches!(out, Grounded::Picked(EnemyId(9))));
    }

    /// The threaded pick is checked before the candidates are counted, so an
    /// out-of-range pick is rejected whatever the list's length (#990).
    #[test]
    fn grounded_choice_rejects_an_out_of_range_pick_at_any_candidate_count() {
        let state = labelled_board();
        let lists: [&[EnemyId]; 3] = [&[], &[EnemyId(4)], &[EnemyId(4), EnemyId(9)]];
        for candidates in lists {
            let out =
                resolve_grounded_choice(&state, &picking(2), candidates, "Choose an enemy", |id| {
                    OptionTarget::Enemy(*id)
                });
            assert!(
                matches!(out, Grounded::Suspend(EngineOutcome::Rejected { .. })),
                "pick 2 of {} candidates must be rejected",
                candidates.len(),
            );
        }
    }

    #[test]
    fn awaiting_choice_leaves_every_option_unanchored() {
        let out = awaiting_choice("Pick", vec!["x".into(), "y".into()]);
        let EngineOutcome::AwaitingInput { request, .. } = out else {
            panic!("expected AwaitingInput");
        };
        assert!(request.options.iter().all(|o| o.target.is_none()));
    }
}
