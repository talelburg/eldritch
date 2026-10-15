//! Taking a card's uses off it: the one path every spend takes, whether a
//! `Cost::SpendUses` pays it or a card's effect spends them (#353), and the
//! move that turns uses into resources (Forbidden Knowledge 01058, #1050).
//!
//! A card printing *"If First Aid has no supplies, discard it."* (First Aid
//! 01019) carries a **constant ability**, and
//! `data/rules-reference/rules/glossary/Ability.md` says of those: *"Constant
//! abilities are always interacting with the game state as long as the card is
//! in play. […] Constant abilities have no point of initiation."* With nothing
//! to schedule the discard after, it fires the moment the pool empties — in the
//! middle of paying a cost, or in the middle of an effect (Forbidden Knowledge
//! 01058's *"Move 1 secret from Forbidden Knowledge to your resource pool"*).
//! Routing every spend through [`spend_uses`] and every move through
//! [`move_uses_to_resource_pool`] — both ending in the one depletion check — is
//! what makes the check continuous rather than a property of one cost arm.
//!
//! Spending and moving are announced differently. Spending a use is a cost
//! (`data/rules-reference/rules/glossary/Uses.md`: *"When such an ability
//! spends a use, a token of that type must be removed from the card bearing the
//! ability."*), and a move by an effect is not one; nor is the resource it
//! becomes gained (<https://arkhamdb.com/card/01058>: *"Moving secrets from
//! Forbidden Knowledge to your resource pool **does not** count as 'gaining'
//! resources"*). So a move emits [`Event::UsesMovedToResourcePool`], never
//! [`Event::UsesSpent`] or [`Event::ResourcesGained`].

use card_dsl::card_data::CardKind;

use crate::engine::{board, Cx};
use crate::event::Event;
use crate::state::{CardCode, CardInstanceId, InvestigatorId, UseKind};

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
    if depleted && !discard_if_printed(cx, instance, &code, kind) {
        return Err(SpendUsesError::Undiscardable);
    }
    Ok(())
}

/// Why [`NativeCx::move_uses_to_resource_pool`](crate::engine::NativeCx::move_uses_to_resource_pool)
/// could not do its whole job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveUsesError {
    /// The instance is not on the board, so it has no uses to move. Nothing
    /// changed and no event was emitted.
    NotInPlay,
    /// The receiving investigator is not in the game. Nothing changed and no
    /// event was emitted.
    NoSuchInvestigator,
    /// The move emptied a `discard_when_empty` pool, but the card is in no
    /// zone a discard can take it from (today only an investigator card, which
    /// never leaves play). The uses moved and were announced; the card stays.
    Undiscardable,
}

/// Move up to `amount` uses of `kind` off the card instance `instance` into
/// `investigator`'s resource pool, each becoming one resource. A card-local
/// native reaches it as
/// [`NativeCx::move_uses_to_resource_pool`](crate::engine::NativeCx::move_uses_to_resource_pool).
///
/// In order:
/// 1. takes `min(amount, uses left)` off the pool (a pool the card lacks counts
///    as empty) and adds as many to `investigator`'s resources. Moving none
///    changes nothing and announces nothing;
/// 2. emits [`Event::UsesMovedToResourcePool`] with the number moved — neither
///    [`Event::UsesSpent`] nor [`Event::ResourcesGained`] (see the module
///    docs);
/// 3. if the pool is now empty, runs the same depletion discard as
///    [`spend_uses`].
///
/// # Errors
///
/// [`MoveUsesError::NotInPlay`] when `instance` is not on the board;
/// [`MoveUsesError::NoSuchInvestigator`] when `investigator` is not in the
/// game; [`MoveUsesError::Undiscardable`] when the depletion discard has
/// nowhere to take the card from.
pub fn move_uses_to_resource_pool(
    cx: &mut Cx,
    investigator: InvestigatorId,
    instance: CardInstanceId,
    kind: UseKind,
    amount: u8,
) -> Result<(), MoveUsesError> {
    if !cx.state.investigators.contains_key(&investigator) {
        return Err(MoveUsesError::NoSuchInvestigator);
    }
    let (card, _) = board::find_instance_mut(cx.state, instance).ok_or(MoveUsesError::NotInPlay)?;
    let Some(remaining) = card.uses.get_mut(&kind) else {
        return Ok(());
    };
    let moved = amount.min(*remaining);
    if moved == 0 {
        return Ok(());
    }
    *remaining -= moved;
    let depleted = *remaining == 0;
    let code = card.code.clone();
    if let Some(receiver) = cx.state.investigators.get_mut(&investigator) {
        receiver.resources = receiver.resources.saturating_add(moved);
    }
    cx.events.push(Event::UsesMovedToResourcePool {
        investigator,
        instance_id: instance,
        kind,
        amount: moved,
    });
    if depleted && !discard_if_printed(cx, instance, &code, kind) {
        return Err(MoveUsesError::Undiscardable);
    }
    Ok(())
}

/// The depletion discard: `instance`'s pool of `kind` has just emptied, so
/// discard it **immediately** through [`board::discard_from_play`] if its
/// printed uses are `discard_when_empty` of that `kind`. Returns `false` only
/// when the card should go but no discard can take it.
fn discard_if_printed(
    cx: &mut Cx,
    instance: CardInstanceId,
    code: &CardCode,
    kind: UseKind,
) -> bool {
    let discards_when_empty = (cx.cards.metadata_for)(code)
        .and_then(|m| match m.kind {
            CardKind::Asset { uses, .. } => uses,
            _ => None,
        })
        .is_some_and(|u| u.discard_when_empty && u.kind == kind);
    !discards_when_empty || board::discard_from_play(cx, instance).is_some()
}
