//! Spending a card's uses: the one path every spend takes, whether a
//! `Cost::SpendUses` pays it or a card's effect moves the uses off (#353).
//!
//! A card printing *"If First Aid has no supplies, discard it."* (First Aid
//! 01019) carries a **constant ability**, and
//! `data/rules-reference/rules/glossary/Ability.md` says of those: *"Constant
//! abilities are always interacting with the game state as long as the card is
//! in play. […] Constant abilities have no point of initiation."* With nothing
//! to schedule the discard after, it fires the moment the pool empties — in the
//! middle of paying a cost, or in the middle of an effect (Forbidden Knowledge
//! 01058's *"Move 1 secret from Forbidden Knowledge to your resource pool"*).
//! Routing every spend through [`spend_uses`] is what makes the check
//! continuous rather than a property of one cost arm.

use card_dsl::card_data::CardKind;

use crate::engine::{board, Cx};
use crate::event::Event;
use crate::state::{CardInstanceId, InvestigatorId, UseKind};

/// Why [`NativeCx::spend_uses`](crate::engine::NativeCx::spend_uses) could not
/// do its whole job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpendUsesError {
    /// The instance is not on the board, so it has no uses to spend. Nothing
    /// changed and no event was emitted.
    NotInPlay,
    /// The spend emptied a `discard_when_empty` pool, but the card is in no
    /// zone a discard can take it from (today only an investigator card, which
    /// never leaves play). The uses were spent and announced; the card stays.
    Undiscardable,
}

/// `investigator` spends `amount` uses of `kind` from the card instance
/// `instance`, wherever it sits on the board. A card-local native reaches it
/// as [`NativeCx::spend_uses`](crate::engine::NativeCx::spend_uses).
///
/// In order:
/// 1. decrements the pool, saturating at zero (a pool the card lacks counts as
///    empty);
/// 2. emits [`Event::UsesSpent`] naming `investigator` and the requested
///    `amount`;
/// 3. if the pool is now empty and the card's printed uses are
///    `discard_when_empty` of this `kind`, discards the card **immediately**
///    through [`board::discard_from_play`], which files it by its owner and
///    emits its own discard event. The depletion discard is a constant ability
///    (see the module docs), so nothing defers it.
///
/// `investigator` is whoever spends, not necessarily the card's controller or
/// owner: a cost passes the activator, a native whoever its text names.
///
/// # Errors
///
/// [`SpendUsesError::NotInPlay`] when `instance` is not on the board;
/// [`SpendUsesError::Undiscardable`] when the depletion discard has nowhere to
/// take the card from. The caller decides what either means for it.
pub fn spend_uses(
    cx: &mut Cx,
    investigator: InvestigatorId,
    instance: CardInstanceId,
    kind: UseKind,
    amount: u8,
) -> Result<(), SpendUsesError> {
    let (card, _) =
        board::find_instance_mut(cx.state, instance).ok_or(SpendUsesError::NotInPlay)?;
    let remaining = card.uses.entry(kind).or_insert(0);
    *remaining = remaining.saturating_sub(amount);
    let depleted = *remaining == 0;
    let code = card.code.clone();
    cx.events.push(Event::UsesSpent {
        investigator,
        instance_id: instance,
        kind,
        amount,
    });
    let discards_when_empty = (cx.cards.metadata_for)(&code)
        .and_then(|m| match m.kind {
            CardKind::Asset { uses, .. } => uses,
            _ => None,
        })
        .is_some_and(|u| u.discard_when_empty && u.kind == kind);
    if depleted && discards_when_empty && board::discard_from_play(cx, instance).is_none() {
        return Err(SpendUsesError::Undiscardable);
    }
    Ok(())
}
