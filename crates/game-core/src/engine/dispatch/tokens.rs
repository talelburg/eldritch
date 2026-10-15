//! Moving tokens from one card to another: a move, neither a deal nor a
//! heal, so it is its own exit (ADR 0009). The Necronomicon 01009's
//! *"Move 1 horror from The Necronomicon to Daisy Walker."* is the first
//! caller, through [`NativeCx::move_horror`](crate::engine::NativeCx::move_horror).
//!
//! It sits in dispatch rather than [`board`](crate::engine::board) because a
//! horror move can defeat its destination, and defeat runs dispatch's
//! elimination and asset-defeat paths. `board` stays below dispatch and
//! imports nothing from it.

use card_dsl::card_data::CardKind;

use super::{combat, elimination};
use crate::engine::board::{find_instance, find_instance_mut, Placement};
use crate::engine::Cx;
use crate::event::Event;
use crate::state::{CardInstanceId, EliminationCause};

/// Why [`NativeCx::move_horror`](crate::engine::NativeCx::move_horror) moved
/// nothing. In every case nothing changed and no event was emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveHorrorError {
    /// The source or the destination instance is not on the board.
    NotInPlay,
    /// The destination has no sanity value. The official FAQ: *"An effect
    /// that can “move” damage/horror from one card to another—such as Wrong
    /// Place, Right Time (\[fhvp\] 115)—must move that damage/horror to a card
    /// that has a health or sanity value, respectively."*
    NoSanity,
}

/// Move up to `amount` horror from the card instance `from` onto the card
/// instance `to`, wherever each sits on the board (the [`find_instance`]
/// walk), and emit [`Event::HorrorMoved`] with the number moved. A source with
/// no horror moves none, which changes nothing and announces nothing.
///
/// A move, not a deal: no [`Event::HorrorTaken`] and no `DamagePlaced`
/// window, so nothing keyed on harm sees it (ADR 0009; the 01009 ruling,
/// *"Moving damage or horror is different from 'dealing' or 'taking' damage or
/// horror, so does not trigger effects […]"*).
///
/// The destination is defeated as horror placed any other way would defeat it
/// (`glossary/Sanity_and_Horror.md`), through the same helpers the deal
/// uses: an investigator card reaching its sanity is *"defeated"* and
/// eliminated with [`EliminationCause::Horror`] (`combat::sanity_reached`, so
/// an investigator no longer active is not defeated again), and an asset
/// reaching its sanity *"is defeated and placed on its owner's discard pile"*
/// (`combat::defeat_asset`). The Necronomicon 01009, the one corpus caller,
/// only ever moves onto Daisy Walker's investigator card; the asset branch is
/// the rule for a general exit, not a card's need.
pub(in crate::engine) fn move_horror(
    cx: &mut Cx,
    from: CardInstanceId,
    to: CardInstanceId,
    amount: u8,
) -> Result<(), MoveHorrorError> {
    let (source, _) = find_instance(cx.state, from).ok_or(MoveHorrorError::NotInPlay)?;
    let available = source.accumulated_horror;
    let (dest, placement) = find_instance(cx.state, to).ok_or(MoveHorrorError::NotInPlay)?;
    let has_sanity = match placement {
        Placement::InvestigatorCard(_) => true,
        _ => matches!(
            (cx.cards.metadata_for)(&dest.code).map(|meta| &meta.kind),
            Some(CardKind::Asset {
                sanity: Some(_),
                ..
            })
        ),
    };
    if !has_sanity {
        return Err(MoveHorrorError::NoSanity);
    }

    let moved = amount.min(available);
    if moved == 0 {
        return Ok(());
    }
    let (source, _) = find_instance_mut(cx.state, from).expect("source checked at entry");
    source.accumulated_horror -= moved;
    let (dest, _) = find_instance_mut(cx.state, to).expect("destination checked at entry");
    dest.accumulated_horror = dest.accumulated_horror.saturating_add(moved);
    cx.events.push(Event::HorrorMoved {
        from,
        to,
        amount: moved,
    });

    if let Placement::InvestigatorCard(id) = placement {
        if combat::sanity_reached(&cx.state.investigators[&id], cx.cards) {
            elimination::apply_investigator_elimination(cx, id, EliminationCause::Horror);
        }
    } else {
        let defeated = find_instance(cx.state, to)
            .is_some_and(|(dest, _)| combat::asset_overflowed(cx.cards, dest));
        if defeated {
            combat::defeat_asset(cx, to);
        }
    }
    Ok(())
}
