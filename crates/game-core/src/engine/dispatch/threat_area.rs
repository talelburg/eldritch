//! Threat-area zone helpers: placing an encounter card into an
//! investigator's threat area and discarding it back to the encounter
//! discard pile. C4a (#233) ships the mechanism; which treacheries
//! persist here (and the Revelation routing that places them) is C4c
//! (#235).

use crate::card_data::CardKind;
use crate::event::Event;
use crate::state::{CardCode, CardInPlay, CardInstanceId, InvestigatorId, LocationId, Zone};

use super::Cx;

/// Mint a fresh in-play instance of `code`: allocate its id, build the
/// `CardInPlay`, and seed the named-uses pool ("ammo") from the asset's
/// printed `uses` if any. Does **not** place it in a zone — the caller
/// pushes it into `cards_in_play` / `threat_area` / a location's
/// attachments and emits the zone-specific event.
///
/// The single construction point shared by `place_in_threat_area`,
/// `attach_to_location`, and `play_card`'s in-play branch ([#296]) — and, since
/// #772, the single place a card's [`owner`](CardInPlay::owner) is written.
///
/// `owner` is the player whose deck the card came from, `None` for a
/// scenario-owned card. **It is not "who controls it"**: control is which
/// collection the instance ends up in, and it can change later while the owner
/// does not (`glossary/Ownership_and_Control.md`).
///
/// A card entering a *threat area* or a location's *attachment* zone is
/// scenario-owned here, which is right for the encounter cards that occupy both
/// zones in the corpus today and wrong for the two player cards that can —
/// Cover Up 01007 (a weakness, threat area) and Barricade 01038 (a location
/// attachment). Neither exit reads `owner`: only the `cards_in_play` exit
/// (`cards::discard_card_from_play`) does. `TODO(#829)`: thread the real owner
/// through those two zones when something reads it there.
///
/// [#296]: https://github.com/talelburg/eldritch/issues/296
pub(super) fn new_in_play_instance(
    cx: &mut Cx,
    code: CardCode,
    owner: Option<InvestigatorId>,
) -> CardInPlay {
    let instance_id = cx.state.card_instance_ids.mint();
    let uses = crate::card_registry::current()
        .and_then(|reg| (reg.metadata_for)(&code))
        .and_then(|m| match &m.kind {
            CardKind::Asset { uses, .. } => *uses,
            _ => None,
        });
    let mut card = CardInPlay::enter_play(code, instance_id).owned_by(owner);
    if let Some(u) = uses {
        card.uses.insert(u.kind, u.count);
    }
    card
}

/// Place `code` into `investigator`'s threat area as a fresh in-play
/// instance, minting an instance id from the per-state counter, and
/// emit [`Event::CardEnteredThreatArea`]. Returns the minted id.
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
) -> Option<CardInstanceId> {
    if !cx.state.investigators.contains_key(&investigator) {
        return None;
    }
    let card = new_in_play_instance(cx, code.clone(), None);
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

/// Attach `code` to `location` as a fresh in-play instance, minting an
/// instance id from the per-state counter, and emit
/// [`Event::CardAttachedToLocation`]. Returns the minted id, or `None`
/// if the location isn't in state.
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
) -> Option<CardInstanceId> {
    if !cx.state.locations.contains_key(&location) {
        return None;
    }
    let card = new_in_play_instance(cx, code.clone(), None);
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

/// Put `code` into play **at** `location` as a fresh in-play instance
/// under no investigator's control, minting an instance id from the
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
) -> Option<CardInstanceId> {
    if !cx.state.locations.contains_key(&location) {
        return None;
    }
    let card = new_in_play_instance(cx, code.clone(), None);
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

/// Remove the threat-area instance `instance_id` from `investigator`,
/// push its code onto the encounter discard pile, and emit
/// [`Event::CardDiscarded`] with `from: Zone::ThreatArea`. Returns
/// `true` if an instance was removed, `false` if none matched.
///
/// The encounter discard is unconditionally correct here: the only cards that
/// reach this helper are scenario-owned. A card's own Revelation-placed
/// self-discard (Frozen in Fear 01164, Dissonant Voices 01165) routes through
/// `Effect::DiscardSelf`, and Rules Reference p.10 Elimination step 4 sends an
/// eliminated investigator's *scenario-owned* threat-area cards here — their
/// **owned** weaknesses leave at step 1 instead, so they never arrive (#567).
pub(super) fn discard_from_threat_area(
    cx: &mut Cx,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
) -> bool {
    let Some(inv) = cx.state.investigators.get_mut(&investigator) else {
        return false;
    };
    let Some(pos) = inv
        .threat_area
        .iter()
        .position(|c| c.instance_id == instance_id)
    else {
        return false;
    };
    let card = inv.threat_area.remove(pos);
    cx.state.encounter_discard.push(card.code.clone());
    cx.events.push(Event::CardDiscarded {
        investigator,
        code: card.code,
        from: Zone::ThreatArea,
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_investigator, test_location, GameStateBuilder};

    #[test]
    fn attach_mints_id_pushes_to_location_and_emits_event() {
        let mut state = GameStateBuilder::new()
            .with_location(test_location(7, "Study"))
            .build();
        let mut events = Vec::new();
        let id = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            attach_to_location(&mut cx, LocationId(7), CardCode::new("01168"))
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
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let id = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            place_in_threat_area(&mut cx, InvestigatorId(1), CardCode::new("01164"))
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

    #[test]
    fn discard_removes_instance_pushes_to_encounter_discard_and_emits() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let id = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            place_in_threat_area(&mut cx, InvestigatorId(1), CardCode::new("01164"))
                .expect("placed")
        };
        events.clear();
        let removed = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            discard_from_threat_area(&mut cx, InvestigatorId(1), id)
        };
        assert!(removed);
        assert!(state.investigators[&InvestigatorId(1)]
            .threat_area
            .is_empty());
        assert_eq!(state.encounter_discard, vec![CardCode::new("01164")]);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::CardDiscarded { from: Zone::ThreatArea, code, .. } if code.as_str() == "01164"
        )));
    }

    #[test]
    fn discard_of_unknown_instance_is_a_no_op() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let removed = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            discard_from_threat_area(&mut cx, InvestigatorId(1), CardInstanceId(999))
        };
        assert!(!removed);
        assert!(events.is_empty());
        assert!(state.encounter_discard.is_empty());
    }
}
