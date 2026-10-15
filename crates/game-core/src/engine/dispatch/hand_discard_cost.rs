//! **A hand-discard cost** — [`Cost::DiscardCardFromHand`], Wendy Adams
//! 01005's *"choose and discard 1 card from your hand"* (#1107).
//!
//! The cost needs a choice, so it cannot be paid inline with the others
//! (`abilities::pay_activation_costs`). Instead the ability's effect is pushed
//! first, with its event bindings already in its context, and a
//! [`HandDiscardCostFrame`] goes on top of it ([`park`]). The stack is LIFO, so
//! the cost is paid before the effect resolves, as Appendix I has it: step 2
//! *"Pay the cost(s)"*, the effect at step 4. A cost stays paid if the effect
//! is then cancelled — `glossary/Cancel.md`: *"Any time the effects of an
//! ability are canceled, the ability (apart from its effects) is still regarded
//! as initiated, and any costs have still been paid."*
//!
//! **A weakness can't be chosen.** Dark Memory 01013's ruling: *"You cannot
//! choose to discard a Weakness card from your hand, neither for exceeding the
//! max number of cards in hand, nor for card effects that make you 'choose and
//! discard'."* (<https://arkhamdb.com/card/01013>). So the cost is payable only
//! while the hand holds a card that is not one ([`payable`]), and only such
//! cards are offered ([`candidates`]).

use card_dsl::dsl::Cost;

use crate::action::InputResponse;
use crate::card_registry::CardRegistry;
use crate::engine::dispatch::{cards, choice};
use crate::engine::outcome::{EngineOutcome, InputRequest, OptionId, OptionTarget, ResumeToken};
use crate::engine::Cx;
use crate::state::{CardCode, GameState, HandDiscardCostFrame, Investigator, InvestigatorId};

/// Whether `inv` can pay a hand-discard cost: their hand holds a card that is
/// not a weakness. The reason it can't, otherwise.
pub(super) fn payable(cards: &CardRegistry, inv: &Investigator) -> Result<(), String> {
    if inv
        .hand
        .iter()
        .any(|code| !cards::is_weakness_code(cards, code))
    {
        return Ok(());
    }
    Err(format!(
        "ActivateAbility: the discard-a-card-from-hand cost needs a card in {:?}'s hand that is \
         not a weakness, and there is none",
        inv.id,
    ))
}

/// Park a hand-discard cost for each [`Cost::DiscardCardFromHand`] in `costs`
/// above whatever the caller has just pushed — the ability's effect, which
/// resolves once the cost is paid. `code` is the card the ability is on.
///
/// The caller has confirmed the cost payable ([`payable`]). No ability prints
/// the cost twice; one that did would pay it once per copy, each frame offering
/// the hand as the one above it left it.
pub(super) fn park(cx: &mut Cx, investigator: InvestigatorId, code: &CardCode, costs: &[Cost]) {
    for _ in costs
        .iter()
        .filter(|cost| matches!(cost, Cost::DiscardCardFromHand))
    {
        cx.state.continuations.push(HandDiscardCostFrame {
            investigator,
            code: code.clone(),
        });
    }
}

/// The `drive` loop's step for a [`HandDiscardCostFrame`] on top: offer the
/// hand cards the cost may discard, or — the resolve convention every choice
/// among board entities follows (`choice::resolve_grounded_choice`) — discard a
/// lone one with no prompt, unless the game is acknowledging every choice
/// (#466).
///
/// Rejects if nothing in hand may be discarded. The ability was refused at the
/// gate in that case, and nothing pays a cost between the gate and here, so the
/// rejection rolls back an engine fault rather than a player's choice.
pub(super) fn drive(cx: &mut Cx) -> EngineOutcome {
    let HandDiscardCostFrame { investigator, code } = cx
        .state
        .continuations
        .top_expect::<HandDiscardCostFrame>()
        .clone();
    let candidates = candidates(cx.state, cx.cards, investigator);
    match choice::resolve_choice_count(candidates.len(), cx.state.interactive_acknowledge) {
        choice::ChoiceResolution::Empty => EngineOutcome::Rejected {
            reason: format!(
                "{code}'s discard-a-card-from-hand cost: {investigator:?} has no card in hand \
                 that is not a weakness"
            )
            .into(),
        },
        choice::ChoiceResolution::Auto(i) => pay(cx, investigator, candidates[i]),
        choice::ChoiceResolution::Suspend => {
            let anchors: Vec<OptionTarget> = candidates
                .iter()
                .map(|&hand_index| hand_card(investigator, hand_index))
                .collect();
            EngineOutcome::AwaitingInput {
                request: InputRequest::pick_single(
                    format!(
                        "{}: choose a card from your hand to discard. Weaknesses can't be chosen.",
                        choice::card_name(cx.cards, &code),
                    ),
                    choice::candidate_options(cx.state, cx.cards, &anchors, OptionTarget::clone),
                ),
                resume_token: ResumeToken(0),
            }
        }
    }
}

/// Resume a [`HandDiscardCostFrame`] with the player's pick: a
/// `PickSingle(OptionId)` indexing the cards [`drive`] offered. Discards the
/// card and pops the frame, exposing the effect beneath. A pick out of range,
/// or a response of another kind, rejects with nothing changed.
pub(super) fn resume(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let investigator = cx
        .state
        .continuations
        .top_expect::<HandDiscardCostFrame>()
        .investigator;
    let InputResponse::PickSingle(OptionId(i)) = response else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: a discard-a-card-from-hand cost expects PickSingle, got {response:?}"
            )
            .into(),
        };
    };
    let candidates = candidates(cx.state, cx.cards, investigator);
    let Some(&hand_index) = candidates.get(*i as usize) else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: discard-a-card-from-hand option {i} out of range (0..{})",
                candidates.len()
            )
            .into(),
        };
    };
    pay(cx, investigator, hand_index)
}

/// Discard the chosen card and pop the frame.
fn pay(cx: &mut Cx, investigator: InvestigatorId, hand_index: u8) -> EngineOutcome {
    cx.state.continuations.pop_expect::<HandDiscardCostFrame>();
    cards::discard_from_hand(cx, investigator, usize::from(hand_index));
    EngineOutcome::Done
}

/// The hand indices of `investigator`'s cards the cost may discard — every one
/// that is not a weakness — in hand order, so a resumed frame re-reads the same
/// list its prompt offered.
fn candidates(state: &GameState, cards: &CardRegistry, investigator: InvestigatorId) -> Vec<u8> {
    state
        .investigators
        .get(&investigator)
        .map(|inv| {
            inv.hand
                .iter()
                .enumerate()
                .filter(|(_, code)| !cards::is_weakness_code(cards, code))
                .map(|(i, _)| u8::try_from(i).expect("a hand fits in u8"))
                .collect()
        })
        .unwrap_or_default()
}

fn hand_card(investigator: InvestigatorId, hand_index: u8) -> OptionTarget {
    OptionTarget::HandCard {
        investigator,
        hand_index,
    }
}
