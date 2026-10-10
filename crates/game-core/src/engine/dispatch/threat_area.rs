//! Threat-area and location zone helpers: placing a card into an
//! investigator's threat area, attaching it to a location, or putting it into
//! play at one. C4a (#233) ships the mechanism; which treacheries persist here
//! (and the Revelation routing that places them) is C4c (#235). A card leaves
//! these zones through the leave-play exits in
//! [`board::leave_play`](crate::engine::board::leave_play), never by hand.

use card_dsl::card_data::CardKind;

use crate::card_registry;
use crate::engine::Cx;
use crate::event::Event;
use crate::state::{CardCode, CardInPlay, CardInstanceId, InvestigatorId, LocationId, Owner};

/// Mint a fresh in-play instance of `code`: allocate its id, build the
/// `CardInPlay`, and seed the named-uses pool ("ammo") from the asset's
/// printed `uses` if any. Does **not** place it in a zone — the caller
/// pushes it into `cards_in_play` / `threat_area` / a location's
/// attachments and emits the zone-specific event.
///
/// The single construction point shared by `place_in_threat_area`,
/// `attach_to_location`, `put_into_play_at_location`, and `play_card`'s in-play
/// branch ([#296]).
///
/// `owner` is the card's [`Owner`], which every caller states (#977): the
/// player whose deck it came from, the encounter deck, or the scenario. **It is
/// not "who controls it"**: control is which collection the instance ends up
/// in, and it can change later while the owner does not
/// (`glossary/Ownership_and_Control.md`).
///
/// [#296]: https://github.com/talelburg/eldritch/issues/296
pub(super) fn new_in_play_instance(cx: &mut Cx, code: CardCode, owner: Owner) -> CardInPlay {
    let instance_id = cx.state.card_instance_ids.mint();
    let uses = card_registry::current()
        .and_then(|reg| (reg.metadata_for)(&code))
        .and_then(|m| match &m.kind {
            CardKind::Asset { uses, .. } => *uses,
            _ => None,
        });
    let mut card = CardInPlay::enter_play(code, instance_id, owner);
    if let Some(u) = uses {
        card.uses.insert(u.kind, u.count);
    }
    card
}

/// Place `code`, owned by `owner`, into `investigator`'s threat area as a fresh
/// in-play instance, minting an instance id from the per-state counter, and
/// emit [`Event::CardEnteredThreatArea`]. Returns the minted id.
///
/// The owner is the caller's to state, because a threat area holds both kinds:
/// an encounter treachery is the encounter deck's, and a weakness such as Cover
/// Up 01007 is its bearer's.
///
/// No-op (returns `None`) if the investigator isn't in state — callers
/// in dispatch have already validated the investigator exists, but the
/// helper stays total so a misuse can't panic.
///
/// # Panics
///
/// Does not panic in practice: the `get_mut` re-fetches the investigator just
/// confirmed present, with no intervening mutation, so the `expect` is a
/// state-corruption invariant guard.
pub fn place_in_threat_area(
    cx: &mut Cx,
    investigator: InvestigatorId,
    code: CardCode,
    owner: Owner,
) -> Option<CardInstanceId> {
    if !cx.state.investigators.contains_key(&investigator) {
        return None;
    }
    let card = new_in_play_instance(cx, code.clone(), owner);
    let instance_id = card.instance_id;
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("existence checked above");
    inv.threat_area.push(card);
    cx.events.push(Event::CardEnteredThreatArea {
        investigator,
        code,
        instance_id,
    });
    Some(instance_id)
}

/// Attach `code`, owned by `owner`, to `location` as a fresh in-play instance,
/// minting an instance id from the per-state counter, and emit
/// [`Event::CardAttachedToLocation`]. Returns the minted id, or `None`
/// if the location isn't in state.
///
/// The owner is the caller's to state: Obscuring Fog 01168 is the encounter
/// deck's, and a played Barricade 01038 is its player's.
///
/// **No limit enforcement** — "Limit 1 per location" is printed on
/// specific cards (Obscuring Fog 01168), not a property of all
/// attachments, so the limit lives in the card's Revelation, not here.
///
/// # Panics
///
/// Does not panic in practice: the `get_mut` re-fetches the location just
/// confirmed present, with no intervening mutation, so the `expect` is a
/// state-corruption invariant guard.
pub fn attach_to_location(
    cx: &mut Cx,
    location: LocationId,
    code: CardCode,
    owner: Owner,
) -> Option<CardInstanceId> {
    if !cx.state.locations.contains_key(&location) {
        return None;
    }
    let card = new_in_play_instance(cx, code.clone(), owner);
    let instance_id = card.instance_id;
    let loc = cx
        .state
        .locations
        .get_mut(&location)
        .expect("existence checked above");
    loc.attachments.push(card);
    cx.events.push(Event::CardAttachedToLocation {
        location,
        code,
        instance_id,
    });
    Some(instance_id)
}

/// Put `code`, owned by `owner`, into play **at** `location` as a fresh in-play
/// instance under no investigator's control, minting an instance id from the
/// per-state counter, and emit [`Event::CardPutIntoPlayAtLocation`].
/// Returns the minted id, or `None` if the location isn't in state.
///
/// The zone sibling of [`attach_to_location`], and deliberately not it:
/// `glossary/In_Play_and_Out_of_Play.md` counts *"each encounter card in a
/// investigator's threat area **or at a location**"* as in play, while
/// `glossary/Attach_To.md` gives an attachment a host-bound lifetime that a
/// card put into play in a place does not have. Act 01109b's *"Put the
/// set-aside Lita Chantler into play in the Parlor"* is the one caller
/// shape (#771).
///
/// # Panics
///
/// Does not panic in practice: the `get_mut` re-fetches the location just
/// confirmed present, with no intervening mutation, so the `expect` is a
/// state-corruption invariant guard.
pub(super) fn put_into_play_at_location(
    cx: &mut Cx,
    location: LocationId,
    code: CardCode,
    owner: Owner,
) -> Option<CardInstanceId> {
    if !cx.state.locations.contains_key(&location) {
        return None;
    }
    let card = new_in_play_instance(cx, code.clone(), owner);
    let instance_id = card.instance_id;
    let loc = cx
        .state
        .locations
        .get_mut(&location)
        .expect("existence checked above");
    loc.cards_at_location.push(card);
    cx.events.push(Event::CardPutIntoPlayAtLocation {
        location,
        code,
        instance_id,
    });
    Some(instance_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::GameStateBuilder;
    use crate::test_support;

    #[test]
    fn attach_mints_id_pushes_to_location_and_emits_event() {
        let mut state = GameStateBuilder::new()
            .with_location(test_support::test_location(7, "Study"))
            .build();
        let mut events = Vec::new();
        let id = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            attach_to_location(
                &mut cx,
                LocationId(7),
                CardCode::new("01168"),
                Owner::EncounterDeck,
            )
        };
        assert_eq!(id, Some(CardInstanceId(0)));
        let loc = &state.locations[&LocationId(7)];
        assert_eq!(loc.attachments.len(), 1);
        assert_eq!(loc.attachments[0].code.as_str(), "01168");
        assert!(events.iter().any(|e| matches!(
            e,
            Event::CardAttachedToLocation { code, location, .. }
                if code.as_str() == "01168" && *location == LocationId(7)
        )));
    }

    #[test]
    fn place_mints_id_pushes_instance_and_emits_event() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_support::test_investigator(1))
            .build();
        let mut events = Vec::new();
        let id = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            place_in_threat_area(
                &mut cx,
                InvestigatorId(1),
                CardCode::new("01164"),
                Owner::EncounterDeck,
            )
        };
        assert_eq!(id, Some(CardInstanceId(0)));
        let inv = &state.investigators[&InvestigatorId(1)];
        assert_eq!(inv.threat_area.len(), 1);
        assert_eq!(inv.threat_area[0].code.as_str(), "01164");
        assert!(events.iter().any(|e| matches!(
            e,
            Event::CardEnteredThreatArea { code, .. } if code.as_str() == "01164"
        )));
    }
}
