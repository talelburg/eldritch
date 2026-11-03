//! **A group's clue spend on an act advance**: each contributor settles how
//! many of their clues go towards the act's threshold.
//!
//! `glossary/Act_Deck_and_Agenda_Deck.md`: *"The act deck advances if the
//! investigators, as a group, spend the requisite number of clues (as indicated
//! by the act card). … Any or all investigators may contribute any number of
//! clues towards the total number of clues required to advance the act."* And
//! `glossary/Costs.md`: *"If the investigators are instructed to pay a cost as
//! a group, each investigator (or each investigator in the group defined by the
//! ability) may contribute to paying the cost."*
//!
//! Both spend sites — the `AdvanceAct` action and the round-end group spend
//! (The Barrier 01109) — validate affordability, then [`start`] a
//! [`ClueAllocationFrame`]. The contributors are asked in player order with the
//! investigator who started the advance (the **initiator**) last. Each is
//! offered only the amounts that keep the advance affordable for those asked
//! after them ([`range`]), and one with a single such amount is not asked. So
//! the initiator, with nobody after them, always covers the remainder unasked,
//! and a solo game or a group holding exactly the threshold asks nobody.
//! Nothing is spent until every share is settled; then the clues come off
//! together and the act advances.

use std::ops::RangeInclusive;

use crate::action::InputResponse;
use crate::card_registry::CardRegistry;
use crate::engine::dispatch::{act_agenda, choice, prompt};
use crate::engine::outcome::{ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget};
use crate::engine::Cx;
use crate::state::{AdvanceTrigger, ClueAllocationFrame, GameState, InvestigatorId};

/// Start the group's spend of `threshold` clues among `contributors` (in player
/// order) to advance the act: push the [`ClueAllocationFrame`] for the `drive`
/// loop, with `initiator` asked last.
///
/// The caller has validated that `contributors` hold at least `threshold`.
pub(super) fn start(
    cx: &mut Cx,
    initiator: InvestigatorId,
    contributors: &[InvestigatorId],
    threshold: u8,
) {
    let asking_order = contributors
        .iter()
        .copied()
        .filter(|id| *id != initiator)
        .chain(contributors.contains(&initiator).then_some(initiator))
        .collect();
    cx.state
        .continuations
        .push(ClueAllocationFrame::new(asking_order, threshold));
}

/// The `drive` loop's step for a [`ClueAllocationFrame`] on top: settle every
/// share that has only one amount, then ask the next contributor who has a
/// choice, or — once every share is settled — spend the clues and advance the
/// act.
pub(super) fn drive(cx: &mut Cx) -> EngineOutcome {
    loop {
        let frame = cx.state.continuations.top_expect::<ClueAllocationFrame>();
        let Some(range) = range(cx.state, frame) else {
            return finish(cx);
        };
        if range.start() < range.end() {
            return prompt::suspend(cx.state, cx.cards);
        }
        let forced = *range.start();
        cx.state
            .continuations
            .top_mut::<ClueAllocationFrame>()
            .shares
            .push(forced);
    }
}

/// What a [`ClueAllocationFrame`] asks: how many clues the next contributor
/// puts in, from the act card, or `None` when their share is forced or every
/// share is settled. One of the [prompt table](prompt::request)'s helpers; the
/// contributor is the one asked.
pub(super) fn request(
    state: &GameState,
    cards: &CardRegistry,
    frame: &ClueAllocationFrame,
) -> Option<(InputRequest, InvestigatorId)> {
    let range = range(state, frame).filter(|r| r.start() < r.end())?;
    let contributor = frame.contributors[frame.shares.len()];
    let act = choice::card_name(cards, &state.act_deck[state.act_index].code);
    let options = range
        .clone()
        .zip(0u32..)
        .map(|(amount, id)| ChoiceOption::new(OptionId(id), contribute_label(amount)))
        .collect();
    let request = InputRequest::pick_single(
        format!(
            "{act}: how many of your clues do you contribute? {} still needed.",
            remaining(frame)
        ),
        options,
    )
    .at(OptionTarget::Act)
    .deciding();
    Some((request, contributor))
}

/// Resume a [`ClueAllocationFrame`] with the asked contributor's
/// `PickSingle(OptionId)`, indexing the amounts [`request`] offered: record the
/// share, for [`drive`] to carry on from. A response of another kind, or an
/// amount that wasn't offered, rejects with nothing changed.
pub(super) fn resume(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let InputResponse::PickSingle(OptionId(i)) = response else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: a clue allocation expects InputResponse::PickSingle, got \
                 {response:?}"
            )
            .into(),
        };
    };
    let frame = cx.state.continuations.top_expect::<ClueAllocationFrame>();
    let Some(range) = range(cx.state, frame) else {
        unreachable!("a clue allocation with every share settled is never at rest");
    };
    let Some(amount) = u8::try_from(*i)
        .ok()
        .and_then(|i| range.start().checked_add(i))
        .filter(|amount| range.contains(amount))
    else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: clue allocation option {i} out of range (0..={})",
                range.end() - range.start()
            )
            .into(),
        };
    };
    cx.state
        .continuations
        .top_mut::<ClueAllocationFrame>()
        .shares
        .push(amount);
    EngineOutcome::Done
}

/// The amounts the next unsettled contributor may put in, or `None` once every
/// share is settled.
///
/// For the contributor at position *k* with *remaining* clues still to settle:
/// at most `min(their clues, remaining)`, and at least what those asked after
/// them can't cover, `remaining − their clues`, never below 0. The spend site
/// validated affordability, and no clue moves while the frame is open, so the
/// range is never empty.
fn range(state: &GameState, frame: &ClueAllocationFrame) -> Option<RangeInclusive<u8>> {
    let next = frame.shares.len();
    let contributor = *frame.contributors.get(next)?;
    let remaining = remaining(frame);
    let held = clues_of(state, contributor);
    let held_after: u32 = frame.contributors[next + 1..]
        .iter()
        .map(|id| u32::from(clues_of(state, *id)))
        .sum();
    let max = held.min(remaining);
    let min = u8::try_from(u32::from(remaining).saturating_sub(held_after))
        .expect("bounded by `remaining`, a u8");
    debug_assert!(
        min <= max,
        "a clue allocation the contributors can't afford: {frame:?}"
    );
    Some(min..=max)
}

/// The clues still to settle: the threshold less the shares settled so far.
fn remaining(frame: &ClueAllocationFrame) -> u8 {
    frame.threshold - frame.shares.iter().sum::<u8>()
}

/// The clues `investigator` holds, 0 for one not in the game.
fn clues_of(state: &GameState, investigator: InvestigatorId) -> u8 {
    state
        .investigators
        .get(&investigator)
        .map_or(0, |inv| inv.clues)
}

/// "Contribute N clues", one option's label.
fn contribute_label(amount: u8) -> String {
    if amount == 1 {
        "Contribute 1 clue".to_owned()
    } else {
        format!("Contribute {amount} clues")
    }
}

/// Every share is settled: pop the frame, take each contributor's share off
/// them at once, and advance the act. Both spend sites are the players'
/// deliberate advance, so the flip is not asked again.
fn finish(cx: &mut Cx) -> EngineOutcome {
    let ClueAllocationFrame {
        contributors,
        shares,
        ..
    } = cx.state.continuations.pop_expect::<ClueAllocationFrame>();
    for (id, share) in contributors.iter().zip(shares) {
        if let Some(inv) = cx.state.investigators.get_mut(id) {
            inv.clues -= share;
        }
    }
    act_agenda::advance_act(cx, AdvanceTrigger::Deliberate);
    EngineOutcome::Done
}
