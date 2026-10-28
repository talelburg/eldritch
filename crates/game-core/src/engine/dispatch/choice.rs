//! Interactive-choice resolution: how an effect node that needs a controller
//! pick suspends in place, builds its prompt, and resumes.
//!
//! The evaluator leaves the node's
//! [`EffectFrame::Leaf`](crate::state::EffectFrame::Leaf) on top of the
//! continuation stack as the prompt. [`resume_effect_choice`] sets the pick on
//! that frame and re-steps it. A choice among board entities goes through
//! [`resolve_grounded_choice`] (`0 ⇒ empty · 1 ⇒ auto · 2+ ⇒ suspend`, plus the
//! resume re-indexing), which the evaluator's `Chosen` arms and card-local
//! natives share. There is no separate choice frame.

use crate::action::InputResponse;
use crate::card_registry::CardRegistry;
use crate::engine::outcome::{
    ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget, ResumeToken,
};
use crate::engine::{board, Cx, EvalContext};
use crate::state::{CardCode, Continuation, EffectFrame, GameState};

/// Outcome of applying the uniform resolve convention to a count of legal
/// options. Crate-private: an entity selection goes
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
/// one-option pick (`Suspend`) instead of auto-binding silently.
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

/// Offer the controller a choice among board entities: the one way
/// an evaluator arm or a card-local native grounds a pick.
///
/// `candidates` must be enumerated from `state` in a deterministic order,
/// because a resumed node re-enumerates them and indexes the same list by the
/// pick. `anchor` names the board entity each candidate renders on, and each
/// option's label is derived from it (ADR 0011).
///
/// - A pick threaded in through
///   [`EvalContext::chosen_option`] is checked first. In range, it gives
///   [`Grounded::Picked`]. Out of range, it gives `Suspend(Rejected)`,
///   whatever the candidate count.
/// - Otherwise zero candidates give [`Grounded::Empty`], one gives `Picked`
///   with no prompt, and two or more give `Suspend` with the prompt. Under
///   `state.interactive_acknowledge` a lone candidate is prompted too.
///
/// It pushes nothing. The caller's suspending frame (the effect `Leaf`, or the
/// designated-action frame) stays on the stack as the prompt and is re-stepped
/// on resume. On `Picked`, the caller binds the candidate and clears the
/// transient pick (`set_chosen_option(None)`) if it threads the context onward.
pub fn resolve_grounded_choice<T: Clone>(
    state: &GameState,
    cards: &CardRegistry,
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
            Grounded::Suspend(awaiting_selection(state, cards, prompt, &anchors))
        }
    }
}

/// Offer the controller a choice among cards **in a deck** or a discard pile,
/// and hand back the picked card's index into `codes`.
///
/// It is the one way an evaluator arm or a card-local native picks a card that
/// has no board surface to anchor on, such as a searched card, a card looked at
/// on top of a deck, or Scavenging 01073's Item in the discard pile.
///
/// The rule is [`resolve_grounded_choice`]'s, so a threaded pick is checked
/// first and a lone card is prompted only under
/// `state.interactive_acknowledge`. The difference is the prompt: each
/// option is labelled with its card's name and left un-anchored, so the host
/// renders it in the prompt banner (ADR 0015 excludes deck cards from its two
/// natures).
///
/// `codes` must be read from `state` in a deterministic order, because a
/// resumed node reads them again and indexes the same list by the pick. Like
/// `resolve_grounded_choice`, it pushes nothing.
pub fn resolve_card_choice(
    state: &GameState,
    cards: &CardRegistry,
    ctx: &EvalContext,
    codes: &[CardCode],
    prompt: &str,
) -> Grounded<usize> {
    if let Some(OptionId(i)) = ctx.chosen_option() {
        let i = i as usize;
        return if i < codes.len() {
            Grounded::Picked(i)
        } else {
            Grounded::Suspend(EngineOutcome::Rejected {
                reason: format!("{prompt}: pick {i} out of range (0..{})", codes.len()).into(),
            })
        };
    }
    match resolve_choice_count(codes.len(), state.interactive_acknowledge) {
        ChoiceResolution::Empty => Grounded::Empty,
        ChoiceResolution::Auto(i) => Grounded::Picked(i),
        ChoiceResolution::Suspend => {
            let labels = codes.iter().map(|code| card_name(cards, code)).collect();
            Grounded::Suspend(awaiting_choice(prompt, labels))
        }
    }
}

/// Build the `AwaitingInput` for a controller pick among board entities, one
/// anchor per offered option in offered order (`OptionId(i)` is the index).
/// Each option renders on the entity its anchor names, and its label
/// is derived from that anchor by [`anchor_label`], so the two cannot disagree
/// and no caller can leave an option unlabelled. Pushes **nothing**: the
/// suspending effect node's own `Leaf` frame stays on the stack as the prompt,
/// and resume re-derives the option set and validates the pick by
/// checked indexing.
fn awaiting_selection(
    state: &GameState,
    cards: &CardRegistry,
    prompt: impl Into<String>,
    anchors: &[OptionTarget],
) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(
            prompt,
            candidate_options(state, cards, anchors, OptionTarget::clone),
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
/// [`anchor_label`]. Every candidate here is a board entity, so `anchor`
/// returns a bare [`OptionTarget`] rather than an `Option`: a caller cannot leave
/// one un-anchored and land it in the prompt banner (ADR 0011). It supplies
/// no label, so it cannot forget one or name a different entity than it anchors.
pub(super) fn candidate_options<T>(
    state: &GameState,
    cards: &CardRegistry,
    candidates: &[T],
    anchor: impl Fn(&T) -> OptionTarget,
) -> Vec<ChoiceOption> {
    candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let target = anchor(c);
            ChoiceOption::new(option_id(i), anchor_label(state, cards, &target)).at(target)
        })
        .collect()
}

/// The player-facing name of the board surface `anchor` names, for an option's
/// label. A location or enemy reads as its name. A card (in play, in a
/// threat area, an investigator card, in hand, the current act or agenda) reads
/// as its card's name via [`card_name`]. An investigator's panel affordances
/// read as the investigator's name.
///
/// Total and panic-free. An anchor naming an entity that isn't in `state` is an
/// engine bug, because every candidate is enumerated from `state`. It trips a
/// `debug_assert!` in tests and degrades to a neutral placeholder in a live
/// game, never to an id's `Debug` form.
fn anchor_label(state: &GameState, cards: &CardRegistry, anchor: &OptionTarget) -> String {
    let found = match anchor {
        OptionTarget::Location(id) => state.locations.get(id).map(|l| l.name.clone()),
        OptionTarget::Enemy(id) => state.enemies.get(id).map(|e| e.name.clone()),
        OptionTarget::CardInstance(id) => {
            board::find_instance(state, *id).map(|(card, _)| card_name(cards, &card.code))
        }
        OptionTarget::HandCard {
            investigator,
            hand_index,
        } => state
            .investigators
            .get(investigator)
            .and_then(|inv| inv.hand.get(usize::from(*hand_index)))
            .map(|code| card_name(cards, code)),
        OptionTarget::HandCardByCode { code, .. } => Some(card_name(cards, code)),
        OptionTarget::Act => state
            .act_deck
            .get(state.act_index)
            .map(|act| card_name(cards, &act.code)),
        OptionTarget::Agenda => state
            .agenda_deck
            .get(state.agenda_index)
            .map(|agenda| card_name(cards, &agenda.code)),
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

/// A card's printed name, looked up through the card registry's metadata.
/// Falls back to the bare code when the registry lacks the code, so a gap
/// degrades readably rather than crashing a live game.
pub(crate) fn card_name(cards: &CardRegistry, code: &CardCode) -> String {
    (cards.metadata_for)(code).map_or_else(|| code.0.clone(), |m| m.name.clone())
}

/// Build the `AwaitingInput` for a **decision** — a choice among alternatives
/// printed on one card, rather than among board entities (ADR 0015). Every
/// branch shares `anchor`, so it rides on the request as well as on each option:
/// the options keep it because that is what makes the card printing the choice
/// glow, and the request carries it because the surface presenting the choice
/// reads one anchor to name where the text is printed.
///
/// `anchor` is already **liveness-checked** by the caller — `None` means
/// the card left play during cost payment, and the prompt falls back to the
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
/// the prompt banner). Used by [`resolve_card_choice`], whose options are cards
/// in a deck with no board surface (ADR 0015 excludes them), labelled by
/// [`card_name`]. A choice among board entities goes the other way:
/// through [`resolve_grounded_choice`], with an anchor per option.
fn awaiting_choice(prompt: impl Into<String>, labels: Vec<String>) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(
            prompt,
            choice_options(labels.into_iter().map(|l| (l, None)).collect()),
        ),
        resume_token: ResumeToken(0),
    }
}

/// Resume an effect node suspended in place for a controller pick: the
/// top frame is the suspended [`EffectFrame::Leaf`] — or the
/// [`EffectFrame::Designated`] a designated **Fight** suspends on while the
/// controller picks which co-located enemy to attack. Set its
/// `chosen_option` and re-step it via the effect drive — the node grounds/picks
/// (checked indexing, validate-first) instead of suspending. On completion,
/// re-enter the enclosing driver (skill test / reaction window).
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
    // The pick is recorded on the suspended top `Effect` frame; returning
    // `Done` cedes to `apply_player_action`'s `drive(cx, outcome)`, whose
    // `Continuation::Effect` arm steps the parked frames via
    // `step_effect_frame`, then dispatches whatever frame the walk was nested
    // within (a `SkillTest` mid-resolution, a window with remaining candidates).
    // No bounded re-entry, no reach-down.
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
        CardInstanceId, EnemyId, GameState, GameStateBuilder, InvestigatorId, LocationId,
    };
    use crate::test_support::TEST_REGISTRY;
    use crate::test_support::{self, SeatLookup as _, TEST_REGISTRIES};

    /// A `ChooseOne` branch that is **live** — one `effect_can_change_state`
    /// cannot prove inert, so the mode filter keeps it in the offer. (An
    /// empty `Seq` is provably inert and would be filtered out.)
    fn live_branch() -> Effect {
        dsl::gain_resources(InvestigatorTarget::You, 1)
    }

    /// A state holding the investigator [`live_branch`] pays out to.
    fn state_with_investigator() -> GameState {
        GameStateBuilder::default()
            .with_registries(TEST_REGISTRIES)
            .with_seat(test_support::seat(1))
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
        // A lone option surfaces as a one-option pick in human play.
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
        // suspending, which is not the behaviour under test here.
        let effect = dsl::choose_one([("A", live_branch()), ("B", live_branch())]);
        let mut state = state_with_investigator();
        let mut events = Vec::new();
        // Push the effect root + drive it through the real global loop; a
        // 2-branch ChooseOne suspends in place for a pick.
        let out = {
            let mut cx = Cx::for_test(&mut state, &mut events);
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
        // One ChooseOne branch auto-binds by default. With interactive_acknowledge
        // on it must surface as a one-option pick.
        let effect = dsl::choose_one([("Only", live_branch())]);
        let ctx = EvalContext::for_controller(InvestigatorId(1));

        let mut state = state_with_investigator();
        state.interactive_acknowledge = true;
        let mut events = Vec::new();
        let out = {
            let mut cx = Cx::for_test(&mut state, &mut events);
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
            let mut cx = Cx::for_test(&mut state, &mut events);
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
            &TEST_REGISTRY,
            "Choose a target",
            &[
                OptionTarget::Enemy(EnemyId(1)),
                OptionTarget::Location(HERE),
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
            "the request carries the anchor so one surface can name the card printing the choice",
        );
        assert!(
            request
                .options
                .iter()
                .all(|o| o.target == Some(OptionTarget::Act)),
            "and each option keeps it, which is what makes the card glow",
        );
    }

    /// Regression for #845: a card discarded during cost payment leaves its
    /// decision un-anchored at both levels rather than pointing at a card no
    /// board surface renders.
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

    const HERE: LocationId = LocationId(10);

    /// A board carrying one of every entity an entity selection anchors to:
    /// investigator 1 in the Study ([`HERE`]), a Ghoul Minion (enemy 1) there,
    /// and a [`test_support::TEST_ASSET`] in play (find it with [`asset`]).
    fn labelled_board() -> GameState {
        GameStateBuilder::default()
            .with_registries(TEST_REGISTRIES)
            .with_location(test_support::test_location(HERE.0, "Study"))
            .with_seat(
                test_support::seat(1)
                    .at(HERE)
                    .in_play([test_support::TEST_ASSET]),
            )
            .with_enemy(test_support::test_enemy(1, "Ghoul Minion"))
            .build()
    }

    /// The instance id of [`labelled_board`]'s in-play asset.
    fn asset(state: &GameState) -> CardInstanceId {
        state.in_play(InvestigatorId(1), test_support::TEST_ASSET)
    }

    #[test]
    fn a_location_option_is_labelled_with_the_location_name() {
        let state = labelled_board();
        assert_eq!(
            anchor_label(&state, &TEST_REGISTRY, &OptionTarget::Location(HERE)),
            "Study"
        );
    }

    #[test]
    fn an_enemy_option_is_labelled_with_the_enemy_name() {
        let state = labelled_board();
        assert_eq!(
            anchor_label(&state, &TEST_REGISTRY, &OptionTarget::Enemy(EnemyId(1))),
            "Ghoul Minion"
        );
    }

    #[test]
    fn a_card_instance_option_is_labelled_with_the_card_name() {
        let state = labelled_board();
        assert_eq!(
            anchor_label(
                &state,
                &TEST_REGISTRY,
                &OptionTarget::CardInstance(asset(&state))
            ),
            "Test Asset"
        );
    }

    #[test]
    fn an_investigator_card_option_is_labelled_with_the_investigator_card_name() {
        let state = labelled_board();
        let me = &state.investigators[&InvestigatorId(1)];
        let anchor = me.card_anchor();
        assert_eq!(
            anchor,
            OptionTarget::CardInstance(me.investigator_card.instance_id)
        );
        // The registry's name for the investigator card, not the fixture's
        // `Investigator::name`.
        assert_eq!(
            anchor_label(&state, &TEST_REGISTRY, &anchor),
            "Test Investigator"
        );
    }

    #[test]
    fn a_card_the_registry_does_not_know_falls_back_to_its_code() {
        let mut state = labelled_board();
        let asset = asset(&state);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .cards_in_play[0]
            .code = CardCode::new("NOT_A_CARD");
        assert_eq!(
            anchor_label(&state, &TEST_REGISTRY, &OptionTarget::CardInstance(asset)),
            "NOT_A_CARD"
        );
        assert_eq!(
            card_name(&TEST_REGISTRY, &CardCode::new("NOT_A_CARD")),
            "NOT_A_CARD"
        );
    }

    /// `ctx` with `pick` threaded in, as resume re-steps a suspended node.
    fn picking(pick: u32) -> EvalContext {
        let mut ctx = EvalContext::for_controller(InvestigatorId(1));
        ctx.set_chosen_option(Some(OptionId(pick)));
        ctx
    }

    #[test]
    fn grounded_choice_auto_picks_a_lone_candidate_without_a_prompt() {
        let state = labelled_board();
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let out = resolve_grounded_choice(
            &state,
            &TEST_REGISTRY,
            &ctx,
            &[HERE],
            "Choose a location",
            |id| OptionTarget::Location(*id),
        );
        assert!(matches!(out, Grounded::Picked(HERE)));
    }

    #[test]
    fn grounded_choice_surfaces_a_lone_candidate_under_the_interactive_flag() {
        // In human play a lone candidate is a one-option pick.
        let mut state = labelled_board();
        state.interactive_acknowledge = true;
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let out = resolve_grounded_choice(
            &state,
            &TEST_REGISTRY,
            &ctx,
            &[HERE],
            "Choose a location",
            |id| OptionTarget::Location(*id),
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
            &TEST_REGISTRY,
            &ctx,
            &[] as &[LocationId],
            "Choose a location",
            |id| OptionTarget::Location(*id),
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
            &TEST_REGISTRY,
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

    /// Investigator candidates anchor to their investigator card and are
    /// labelled with that card's name.
    #[test]
    fn grounded_choice_anchors_investigators_to_their_card() {
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let state = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(test_support::seat(1))
            .with_seat(test_support::seat(2))
            .build();
        let cands = [InvestigatorId(1), InvestigatorId(2)];
        let out = resolve_grounded_choice(
            &state,
            &TEST_REGISTRY,
            &ctx,
            &cands,
            "Choose an investigator",
            |id| state.investigators[id].card_anchor(),
        );
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
            &TEST_REGISTRY,
            &picking(1),
            &[EnemyId(4), EnemyId(9)],
            "Choose an enemy",
            |id| OptionTarget::Enemy(*id),
        );
        assert!(matches!(out, Grounded::Picked(EnemyId(9))));
    }

    /// The threaded pick is checked before the candidates are counted, so an
    /// out-of-range pick is rejected whatever the list's length.
    #[test]
    fn grounded_choice_rejects_an_out_of_range_pick_at_any_candidate_count() {
        let state = labelled_board();
        let lists: [&[EnemyId]; 3] = [&[], &[EnemyId(4)], &[EnemyId(4), EnemyId(9)]];
        for candidates in lists {
            let out = resolve_grounded_choice(
                &state,
                &TEST_REGISTRY,
                &picking(2),
                candidates,
                "Choose an enemy",
                |id| OptionTarget::Enemy(*id),
            );
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
